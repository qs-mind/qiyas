//! Draws a sheet into one PDF page as the screen draws it: for a layout, model space inside each
//! viewport first (clipped to its frame, without the layers it freezes), then the sheet's own
//! batch: pictures, fills, lines, wipeouts in the paper's white, the wipeouts' frames, text.
//! Each batch is drawn afresh for the plot, its curves flattened within `PLOT_TOL_MM` on paper.
//!
//! Two steps, so the session is held only while the drawing is read: `snapshot` gathers plain
//! data (the batches, what each entity plots with, font files, where the pictures go and what to
//! draw for them), then `write_pdf` draws the pictures and writes the PDF with no session.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, StringFormat};
use opencadcodec::objects::{Layout, ObjectType};
use opencadcodec::tables::normalize_name;
use opencadcodec::{EntityType, Handle};
use qiyas_core::{draw_picture, PaperView, Session, UnderlayImage, BATCH_TOL};
use qiyas_geom::Xform;
use qiyas_render::{
    arabic_stored_visually, Batch, ImagePlacement, LaidText, FILL_BACKGROUND, IMAGE_ADJUSTED,
    IMAGE_FADED, IMAGE_MONOCHROME, TEXT_BOX,
};

use crate::content::{matrix, utf16_hex, Content};
use crate::ctb::{object_pen, Pen};
use crate::font::{em_to_text, flate_stream, real, FontBytes, FontSet};
use crate::look::{Look, Looks};
use crate::page::{PageMap, PT_PER_MM};
use crate::{Area, PlotError, PlotJob, PlotOutput, PlotStyle};

const WHITE: [u8; 3] = [255, 255, 255];
/// Picture resolution on the page, in pixels per inch (a raster image never beyond its own).
const PICTURE_DPI: f64 = 300.0;
/// The longest side of a plotted picture, in pixels.
const MAX_PICTURE_PX: f64 = 8192.0;
/// How far a flattened curve may stray from the true one on the paper, in millimetres: a 1 m
/// radius at 1:100 (10 mm on paper) gets 18 segments a quarter turn.
pub const PLOT_TOL_MM: f64 = 0.01;

/// One batch to draw and how.
struct Layer<'b> {
    batch: &'b Batch,
    /// The pictures on the batch's sheet: handle, where, what to draw.
    pictures: &'b [Picture],
    /// Batch (drawing) coordinates to page points.
    xf: Xform,
    /// The viewport frame on the page, for model space inside it.
    clip: Option<[[f64; 2]; 4]>,
    /// The layers (normalised names) the viewport freezes.
    frozen: HashSet<String>,
}

/// What the plot draws with an entity, or `None` when it does not plot.
type Drawn = Option<(Pen, f64)>;

/// A picture of a sheet: its entity, where it goes, what to draw for it.
type Picture = (u64, ImagePlacement, UnderlayImage);

/// One batch of a snapshot to draw, and how.
#[derive(Debug, Clone)]
struct LayerPlan {
    batch: usize,
    xf: Xform,
    clip: Option<[[f64; 2]; 4]>,
    frozen: HashSet<String>,
    pictures: Vec<Picture>,
}

/// Everything a plot needs from the session, as plain data (`snapshot`): written to a PDF by
/// `write_pdf` without the session.
#[derive(Debug, Clone)]
pub struct PlotSnapshot {
    batches: Vec<Batch>,
    layers: Vec<LayerPlan>,
    /// What each drawn entity (by handle and chain) and picture (empty chain) plots with.
    looks: HashMap<(u64, Vec<u64>), Look>,
    /// The font files the texts use, by font id.
    fonts: HashMap<u16, (&'static [u8], u32)>,
    style: Option<PlotStyle>,
    lineweights: bool,
    arabic_reversed: bool,
    page: PageMap,
    title: String,
    /// What is already known to be left out.
    skipped: Vec<String>,
}

struct Plotter<'s> {
    style: Option<&'s PlotStyle>,
    lineweights: bool,
    looks: &'s HashMap<(u64, Vec<u64>), Look>,
    pens: HashMap<([u8; 3], String), Pen>,
    fonts: FontSet,
    out: Content,
    /// Image XObjects by resource name, and the pictures already drawn by handle and
    /// resolution.
    images: Vec<(String, ObjectId)>,
    pictures: HashMap<(u64, u64), Option<String>>,
    /// The drawing's "Arabic text is reversed" setting.
    arabic_reversed: bool,
    doc: Document,
    skipped: Vec<String>,
}

fn chain_at(batch: &Batch, chain: u32) -> &[u64] {
    batch.chains.get(chain as usize).map_or(&[], Vec::as_slice)
}

fn absolute(batch: &Batch, x: f32, y: f32) -> [f64; 2] {
    [
        batch.origin[0] + f64::from(x),
        batch.origin[1] + f64::from(y),
    ]
}

fn rgb_of(c: u32) -> [u8; 3] {
    let [_, r, g, b] = c.to_be_bytes();
    [r, g, b]
}

fn is_empty(batch: &Batch) -> bool {
    batch.ranges.is_empty()
        && batch.fill_ranges.is_empty()
        && batch.texts.is_empty()
        && batch.images.is_empty()
}

/// The layers (normalised names) viewport `view` freezes.
fn frozen_by(session: &Session, view: &PaperView) -> HashSet<String> {
    let doc = &session.drawing.doc;
    let Some(EntityType::Viewport(v)) = doc.get_entity(Handle::new(view.handle)) else {
        return HashSet::new();
    };
    v.frozen_layers
        .iter()
        .filter_map(|h| doc.layers.iter().find(|l| l.handle == *h))
        .map(|l| normalize_name(&l.name))
        .collect()
}

fn layout_of<'s>(session: &'s Session, block: &str) -> Option<&'s Layout> {
    let doc = &session.drawing.doc;
    let record = doc.block_records.get(block)?.handle;
    doc.objects.values().find_map(|o| match o {
        ObjectType::Layout(l) if l.block_record == record => Some(l),
        _ => None,
    })
}

/// Paper millimetres per layout unit: inches when the layout's page setup (with a paper size)
/// says so, else millimetres.
fn layout_mm_per_unit(layout: Option<&Layout>) -> f64 {
    match layout {
        Some(l) if l.paper_width > 0.0 && l.paper_height > 0.0 && l.plot_paper_units == 0 => 25.4,
        _ => 1.0,
    }
}

/// A layout's "layout" area in its own units: its whole paper when the page setup gives one
/// (the layout's origin at the printable area's lower left, inside the margins), else its
/// limits when they have a size.
fn layout_area(layout: &Layout, mm_per_unit: f64) -> Option<[[f64; 2]; 2]> {
    if layout.paper_width > 0.0 && layout.paper_height > 0.0 {
        let k = 1.0 / mm_per_unit;
        let (mut w, mut h) = (layout.paper_width * k, layout.paper_height * k);
        let (mut left, mut bottom) = (layout.plot_margin_left * k, layout.plot_margin_bottom * k);
        if layout.plot_rotation % 2 == 1 {
            (w, h) = (h, w);
            (left, bottom) = (bottom, left);
        }
        return Some([[-left, -bottom], [w - left, h - bottom]]);
    }
    let (lo, hi) = (layout.min_limits, layout.max_limits);
    (hi.0 > lo.0 && hi.1 > lo.1).then_some([[lo.0, lo.1], [hi.0, hi.1]])
}

/// Page points per drawing unit of a map with no shear.
fn pt_per_unit(xf: &Xform) -> f64 {
    let m = &xf.m;
    (m[0][0] * m[1][1] - m[0][1] * m[1][0]).abs().sqrt()
}

/// The flattening tolerance in drawing units for `pt` page points per drawing unit.
fn plot_tol(pt: f64) -> f64 {
    let tol = PLOT_TOL_MM * PT_PER_MM / pt;
    if tol > 0.0 && tol.is_finite() {
        tol.max(1e-9)
    } else {
        BATCH_TOL
    }
}

/// Reads what plotting `job` needs from `session` (the only step that needs it).
pub fn snapshot(session: &Session, job: &PlotJob) -> Result<PlotSnapshot, PlotError> {
    let sheet = session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.block.eq_ignore_ascii_case(&job.sheet))
        .ok_or_else(|| PlotError::NoSuchSheet(job.sheet.clone()))?;
    // As on screen: for the extents.
    let screen = session.sheet_batch(&sheet.block);
    let views = if sheet.is_model {
        Vec::new()
    } else {
        session.paper_views_of(&sheet.block)
    };
    let extents = || {
        if is_empty(&screen) && views.is_empty() {
            Err(PlotError::Nothing)
        } else {
            Ok(screen.bounds)
        }
    };
    let layout = layout_of(session, &sheet.block).filter(|_| !sheet.is_model);
    let mm_per_unit = if sheet.is_model {
        session.drawing.units.to_metres() * 1000.0
    } else {
        layout_mm_per_unit(layout)
    };
    let area = match job.area {
        Area::Extents => extents()?,
        Area::Window([a, b]) => [
            [a[0].min(b[0]), a[1].min(b[1])],
            [a[0].max(b[0]), a[1].max(b[1])],
        ],
        Area::Layout => match layout.and_then(|l| layout_area(l, mm_per_unit)) {
            Some(area) => area,
            None => extents()?,
        },
    };
    let page = PageMap::new(
        &job.paper,
        job.orientation,
        job.scale,
        job.centre,
        area,
        mm_per_unit,
    )?;

    // Each batch flattened for the paper: the sheet's, and model space's per viewport scale.
    const MODEL: &str = "*Model_Space";
    let mut batches: Vec<(u64, Batch)> = Vec::new();
    let mut batch_for = |sheet: &str, tol: f64| -> usize {
        let key = tol.to_bits();
        if sheet != MODEL || !batches.iter().any(|(k, _)| *k == key) {
            batches.push((key, session.sheet_batch_with_tol(sheet, tol)));
            return batches.len() - 1;
        }
        batches
            .iter()
            .position(|(k, _)| *k == key)
            .expect("found above")
    };
    // Each batch with the sheet whose pictures go with it.
    let mut plan: Vec<(&str, LayerPlan)> = Vec::new();
    let layer = |batch, xf, clip, frozen| LayerPlan {
        batch,
        xf,
        clip,
        frozen,
        pictures: Vec::new(),
    };
    for view in &views {
        let [lo, hi] = view.rect;
        let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let xf = Xform::translate(-view.centre[0], -view.centre[1])
            .then(&Xform::scale(view.scale, view.scale))
            .then(&Xform::translate(centre[0], centre[1]))
            .then(&page.xf);
        let clip = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]].map(|p| page.xf.apply(p));
        let at = batch_for(MODEL, plot_tol(pt_per_unit(&xf)));
        plan.push((MODEL, layer(at, xf, Some(clip), frozen_by(session, view))));
    }
    let at = batch_for(&sheet.block, plot_tol(pt_per_unit(&page.xf)));
    plan.push((
        sheet.block.as_str(),
        layer(at, page.xf, None, HashSet::new()),
    ));
    let mut skipped = Vec::new();
    let mut layers = Vec::new();
    for (sheet, mut plan) in plan {
        let px = pt_per_unit(&plan.xf) / 72.0 * PICTURE_DPI;
        let (pictures, problems) = session.plot_pictures(sheet, px, MAX_PICTURE_PX);
        for problem in problems {
            let why = format!("A picture is left out: {problem}");
            if !skipped.contains(&why) {
                skipped.push(why);
            }
        }
        plan.pictures = pictures;
        layers.push(plan);
    }
    let batches: Vec<Batch> = batches.into_iter().map(|(_, b)| b).collect();
    if batches.iter().any(|b| b.truncated) {
        skipped.push("Some blocks are too complex to draw: parts of them are left out".into());
    }

    let mut resolver = Looks::new(&session.drawing.doc, session.xrefs.docs());
    let mut looks = HashMap::new();
    let mut fonts = HashMap::new();
    let mut look = |entity: u64, chain: &[u64]| {
        looks
            .entry((entity, chain.to_vec()))
            .or_insert_with(|| resolver.of(entity, chain));
    };
    for batch in &batches {
        for r in batch.ranges.iter().chain(&batch.fill_ranges) {
            look(r.entity, chain_at(batch, r.chain));
        }
        for t in &batch.texts {
            look(t.entity, chain_at(batch, t.chain));
            for g in &t.glyphs {
                if let std::collections::hash_map::Entry::Vacant(slot) = fonts.entry(g.font) {
                    if let Some(bytes) = session.font_face(g.font) {
                        slot.insert(bytes);
                    }
                }
            }
        }
    }
    for layer in &layers {
        for (handle, _, _) in &layer.pictures {
            look(*handle, &[]);
        }
    }
    Ok(PlotSnapshot {
        batches,
        layers,
        looks,
        fonts,
        style: job.style.clone(),
        lineweights: job.lineweights,
        arabic_reversed: session.arabic_reversed,
        page,
        title: sheet.name,
        skipped,
    })
}

/// Writes `snapshot` to the PDF at `path` (beside it, then moved over it): draws its pictures,
/// embeds its fonts. Needs no session.
pub fn write_pdf(snapshot: &PlotSnapshot, path: &Path) -> Result<PlotOutput, PlotError> {
    let (mut doc, skipped) = pdf_document(snapshot);
    save_document(&mut doc, path)?;
    Ok(PlotOutput {
        path: path.to_path_buf(),
        skipped,
    })
}

fn pdf_document(snapshot: &PlotSnapshot) -> (Document, Vec<String>) {
    let layers: Vec<Layer> = snapshot
        .layers
        .iter()
        .map(|l| Layer {
            batch: &snapshot.batches[l.batch],
            pictures: &l.pictures,
            xf: l.xf,
            clip: l.clip,
            frozen: l.frozen.clone(),
        })
        .collect();
    let mut p = Plotter {
        style: snapshot.style.as_ref(),
        lineweights: snapshot.lineweights,
        looks: &snapshot.looks,
        pens: HashMap::new(),
        fonts: FontSet::default(),
        out: Content::default(),
        images: Vec::new(),
        pictures: HashMap::new(),
        arabic_reversed: snapshot.arabic_reversed,
        doc: Document::with_version("1.5"),
        skipped: snapshot.skipped.clone(),
    };
    let bytes = |id: u16| snapshot.fonts.get(&id).copied();
    for layer in &layers {
        p.prepare_text(layer, &bytes);
    }
    p.fonts.finish();
    // Round caps and joins, as the screen's lines end.
    p.out.op("1 J 1 j");
    for layer in &layers {
        p.draw(layer, &bytes);
    }
    p.finish(&snapshot.page, &snapshot.title);
    (p.doc, p.skipped)
}

impl Plotter<'_> {
    /// How `entity` (inside `chain`) drawn `rgb` plots: its pen and line width in points.
    fn drawn(&mut self, look: &Look, rgb: [u8; 3]) -> Drawn {
        if !look.plot {
            return None;
        }
        let pen = match self.style {
            None => object_pen(rgb),
            Some(style) => *self
                .pens
                .entry((rgb, look.style.clone()))
                .or_insert_with(|| style.apply(rgb, Some(&look.style))),
        };
        if !pen.plot {
            return None;
        }
        let object = if self.lineweights {
            look.weight_mm
        } else {
            0.0
        };
        Some((pen, pen.lineweight_mm.unwrap_or(object) * PT_PER_MM))
    }

    fn look(&mut self, batch: &Batch, entity: u64, chain: u32) -> Look {
        self.looks
            .get(&(entity, chain_at(batch, chain).to_vec()))
            .cloned()
            .unwrap_or_default()
    }

    /// The viewport freezes a layer `entity` (inside `chain`) is drawn on or inside of.
    fn is_frozen(&mut self, layer: &Layer, entity: u64, chain: u32) -> bool {
        !layer.frozen.is_empty()
            && self
                .look(layer.batch, entity, chain)
                .layers
                .iter()
                .any(|n| layer.frozen.contains(n))
    }

    /// Arabic stored in visual order (reversed for old CAD fonts, or in presentation forms) that
    /// Qiyas did not write: its shaped text is not the reading text, so it plots as outlines,
    /// never as PDF text with a wrong ToUnicode map.
    fn visual(&mut self, layer: &Layer, text: &LaidText) -> bool {
        let logical = self.look(layer.batch, text.entity, text.chain).logical;
        let force = self.arabic_reversed && !logical;
        text.text
            .split('\n')
            .any(|line| arabic_stored_visually(line, force))
    }

    fn prepare_text(&mut self, layer: &Layer, bytes: &FontBytes) {
        for text in &layer.batch.texts {
            if self.text_pen(layer, text).is_some() && !self.visual(layer, text) {
                self.fonts.prepare(text, bytes);
            }
        }
    }

    fn text_pen(&mut self, layer: &Layer, text: &LaidText) -> Option<Pen> {
        if self.is_frozen(layer, text.entity, text.chain) || text.glyphs.is_empty() {
            return None;
        }
        let look = self.look(layer.batch, text.entity, text.chain);
        self.drawn(&look, rgb_of(text.color)).map(|(pen, _)| pen)
    }

    fn draw(&mut self, layer: &Layer, bytes: &FontBytes) {
        self.out.save();
        if let Some(c) = layer.clip {
            self.out.point(c[0], "m");
            for p in &c[1..] {
                self.out.point(*p, "l");
            }
            self.out.op("h W n");
        }
        self.pictures(layer);
        let batch = layer.batch;
        let frames: HashSet<u64> = batch
            .fill_ranges
            .iter()
            .filter(|r| {
                batch
                    .fill_colors
                    .get(r.first as usize)
                    .is_some_and(|c| c & FILL_BACKGROUND != 0)
            })
            .map(|r| r.entity)
            .collect();
        self.fills(layer, false);
        self.lines(layer, &frames, false);
        if !frames.is_empty() {
            self.fills(layer, true);
            self.lines(layer, &frames, true);
        }
        for text in &batch.texts {
            self.text(layer, text, bytes);
        }
        self.out.restore();
    }

    /// The fill triangles: coloured ones, or (`background`) the wipeouts in the paper's white.
    fn fills(&mut self, layer: &Layer, background: bool) {
        let batch = layer.batch;
        let mut open: Option<[u8; 3]> = None;
        for r in &batch.fill_ranges {
            if r.count == 0 || self.is_frozen(layer, r.entity, r.chain) {
                continue;
            }
            let look = self.look(batch, r.entity, r.chain);
            for t in r.first..r.first + r.count {
                let Some(&c) = batch.fill_colors.get(t as usize) else {
                    break;
                };
                if (c & FILL_BACKGROUND != 0) != background {
                    continue;
                }
                let color = if background {
                    look.plot.then_some(WHITE)
                } else {
                    self.drawn(&look, rgb_of(c)).map(|(pen, _)| pen.rgb)
                };
                let Some(color) = color else {
                    continue;
                };
                if open != Some(color) {
                    if open.is_some() {
                        self.out.op("B");
                    }
                    self.out.fill(color);
                    self.out.stroke(color);
                    self.out.width(0.0);
                    open = Some(color);
                }
                let at = 6 * t as usize;
                let Some(v) = batch.fills.get(at..at + 6) else {
                    break;
                };
                for (k, op) in ["m", "l", "l"].iter().enumerate() {
                    let p = absolute(batch, v[2 * k], v[2 * k + 1]);
                    self.out.point(layer.xf.apply(p), op);
                }
                self.out.op("h");
            }
        }
        if open.is_some() {
            self.out.op("B");
        }
    }

    /// The line segments: all but the wipeouts' frames, or (`frames`) only those.
    fn lines(&mut self, layer: &Layer, frames: &HashSet<u64>, only_frames: bool) {
        let batch = layer.batch;
        let mut open: Option<([u8; 3], f64)> = None;
        let mut end: Option<[f32; 2]> = None;
        for r in &batch.ranges {
            if r.flags & TEXT_BOX != 0
                || r.count == 0
                || frames.contains(&r.entity) != only_frames
                || self.is_frozen(layer, r.entity, r.chain)
            {
                continue;
            }
            let look = self.look(batch, r.entity, r.chain);
            for i in r.first..r.first + r.count {
                let i = i as usize;
                let (Some(&c), Some(s)) =
                    (batch.colors.get(i), batch.segments.get(4 * i..4 * i + 4))
                else {
                    break;
                };
                let Some((pen, width)) = self.drawn(&look, rgb_of(c)) else {
                    continue;
                };
                if open != Some((pen.rgb, width)) {
                    if open.is_some() {
                        self.out.op("S");
                    }
                    self.out.stroke(pen.rgb);
                    self.out.width(width);
                    open = Some((pen.rgb, width));
                    end = None;
                }
                if end != Some([s[0], s[1]]) {
                    self.out
                        .point(layer.xf.apply(absolute(batch, s[0], s[1])), "m");
                }
                self.out
                    .point(layer.xf.apply(absolute(batch, s[2], s[3])), "l");
                end = Some([s[2], s[3]]);
            }
        }
        if open.is_some() {
            self.out.op("S");
        }
    }

    /// A text: real PDF text in its embedded font when it maps back to its characters, else
    /// its glyphs' outlines filled. A glyph the ToUnicode map gives back as another letter
    /// (`FontSet::actual_text`) is shown inside a marked-content span whose `ActualText` holds
    /// its own characters, which copy and search use. One span per glyph, not per text: a
    /// viewer then orders its characters by where the glyph is, as any other (poppler reads a
    /// whole RTL text's `ActualText` as left to right and turns its words round).
    fn text(&mut self, layer: &Layer, text: &LaidText, bytes: &FontBytes) {
        let Some(pen) = self.text_pen(layer, text) else {
            return;
        };
        let to_page = text.xf.then(&layer.xf);
        if !self.visual(layer, text) && self.fonts.as_text(text) {
            let actual = self.fonts.actual_text(text);
            self.out.op("BT");
            self.out.fill(pen.rgb);
            let mut font = None;
            let em = 1.0 / f64::from(qiyas_render::CAP_EM);
            for (i, g) in text.glyphs.iter().enumerate() {
                if font != Some(g.font) {
                    self.out.op(&format!("/F{} 1 Tf", g.font));
                    font = Some(g.font);
                }
                let tm = Xform::scale(em, em)
                    .then(&Xform::translate(f64::from(g.x), f64::from(g.y)))
                    .then(&to_page);
                self.out.matrix(matrix(&tm), "Tm");
                let gid = self.fonts.subset_gid(g.font, g.glyph);
                match actual.get(i).and_then(Option::as_ref) {
                    Some(chars) => self.out.op(&format!(
                        "/Span <</ActualText {}>> BDC <{gid:04X}> Tj EMC",
                        utf16_hex(chars)
                    )),
                    None => self.out.op(&format!("<{gid:04X}> Tj")),
                }
            }
            self.out.op("ET");
        } else {
            self.out.fill(pen.rgb);
            let start = self.out.buf.len();
            let mut drew = false;
            for g in &text.glyphs {
                let upem = self.fonts.units_per_em(g.font);
                let xf = em_to_text(upem)
                    .then(&Xform::translate(f64::from(g.x), f64::from(g.y)))
                    .then(&to_page);
                drew |= self
                    .fonts
                    .outline(g.font, g.glyph, &xf, bytes, &mut self.out);
            }
            if drew {
                self.out.op("f");
            } else if self.out.buf.len() > start {
                self.out.op("n");
            }
        }
    }

    /// The pictures (PDF underlays and raster images) on the layer's sheet at 300 pixels per
    /// inch on the page (a raster image never beyond its own pixels), clipped as the layer is;
    /// each one left out is noted in `skipped` with why.
    fn pictures(&mut self, layer: &Layer) {
        for (handle, placement, image) in layer.pictures {
            let handle = *handle;
            let look = self
                .looks
                .get(&(handle, Vec::new()))
                .cloned()
                .unwrap_or_default();
            if !look.plot || look.layers.iter().any(|n| layer.frozen.contains(n)) {
                continue;
            }
            let key = (handle, image.resolution());
            let name = match self.pictures.get(&key) {
                Some(name) => name.clone(),
                None => {
                    let name = self.picture(image, placement.flags);
                    self.pictures.insert(key, name.clone());
                    name
                }
            };
            let Some(name) = name else {
                continue;
            };
            let [tl, _, br, bl] = placement.corners.map(|p| layer.xf.apply(p));
            self.out.save();
            self.out.matrix(
                [
                    br[0] - bl[0],
                    br[1] - bl[1],
                    tl[0] - bl[0],
                    tl[1] - bl[1],
                    bl[0],
                    bl[1],
                ],
                "cm",
            );
            self.out.op(&format!("/{name} Do"));
            self.out.restore();
        }
    }

    /// Notes something left out, once.
    fn skip(&mut self, why: String) {
        if !self.skipped.contains(&why) {
            self.skipped.push(why);
        }
    }

    /// Draws `picture` and adds it as an image XObject; its resource name, or `None` (noted in
    /// `skipped`) when it can't be drawn.
    fn picture(&mut self, picture: &UnderlayImage, flags: u32) -> Option<String> {
        let image = match draw_picture(picture, |_| {}) {
            Ok(image) => image,
            Err(e) => {
                let file = picture.path.display();
                self.skip(format!("A picture is left out: {file}: {e}"));
                return None;
            }
        };
        let (w, h) = (image.width(), image.height());
        let raw = image.into_raw();
        let (mono, brightness, contrast, fade) = adjustments(flags);
        let mut rgb = Vec::with_capacity(raw.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(raw.len() / 4);
        for px in raw.as_chunks::<4>().0 {
            let mut c = [px[0], px[1], px[2]].map(|v| f64::from(v) / 255.0);
            if mono {
                c = [0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]; 3];
            }
            for v in &mut c {
                *v = ((*v - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);
                *v += (1.0 - *v) * fade;
            }
            rgb.extend(c.map(|v| (v * 255.0).round() as u8));
            alpha.push(px[3]);
        }
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(w),
            "Height" => i64::from(h),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        };
        if alpha.iter().any(|a| *a < 255) {
            let mask = self.doc.add_object(flate_stream(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => i64::from(w),
                    "Height" => i64::from(h),
                    "ColorSpace" => "DeviceGray",
                    "BitsPerComponent" => 8,
                },
                &alpha,
            ));
            dict.set("SMask", mask);
        }
        let object = self.doc.add_object(flate_stream(dict, &rgb));
        let name = format!("Im{}", self.images.len());
        self.images.push((name.clone(), object));
        Some(name)
    }

    fn finish(&mut self, page: &PageMap, title: &str) {
        let doc = &mut self.doc;
        let pages = doc.new_object_id();
        let fonts = self.fonts.write(doc);
        let mut xobjects = Dictionary::new();
        for (name, id) in &self.images {
            xobjects.set(name.as_str(), *id);
        }
        let content = doc.add_object(flate_stream(Dictionary::new(), self.out.buf.as_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), real(page.size[0]), real(page.size[1])],
            "Contents" => content,
            "Resources" => dictionary! { "Font" => fonts, "XObject" => xobjects },
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        let mut utf16 = vec![0xFE, 0xFF];
        utf16.extend(title.encode_utf16().flat_map(u16::to_be_bytes));
        let info = doc.add_object(dictionary! {
            "Producer" => Object::string_literal("Qiyas"),
            "Title" => Object::String(utf16, StringFormat::Hexadecimal),
        });
        doc.trailer.set("Root", catalog);
        doc.trailer.set("Info", info);
    }
}

fn save_document(doc: &mut Document, path: &Path) -> Result<(), PlotError> {
    let name = path
        .file_name()
        .ok_or_else(|| PlotError::Output("Give the PDF a file name".into()))?;
    let tmp = path.with_file_name(format!(".{}.qiyas-tmp", name.to_string_lossy()));
    let written = (|| -> Result<(), PlotError> {
        let file = std::fs::File::create(&tmp).map_err(write_error)?;
        let mut w = std::io::BufWriter::new(file);
        doc.save_to(&mut w)
            .map_err(|e| PlotError::Write(e.to_string()))?;
        w.flush().map_err(write_error)?;
        drop(w);
        std::fs::rename(&tmp, path).map_err(write_error)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

fn write_error(e: std::io::Error) -> PlotError {
    PlotError::Write(e.to_string())
}

/// A picture's look from its quad flags, as the screen draws it: grey, brightness added (-1 to
/// 1), contrast about mid grey (0 to 2) and the fade into the paper (0 to 1).
fn adjustments(flags: u32) -> (bool, f64, f64, f64) {
    let mono = flags & IMAGE_MONOCHROME != 0;
    if flags & IMAGE_ADJUSTED == 0 {
        let fade = if flags & IMAGE_FADED != 0 { 0.5 } else { 0.0 };
        return (mono, 0.0, 1.0, fade);
    }
    let byte = |shift: u32| f64::from(((flags >> shift) & 0xFF).min(100));
    (
        mono,
        (byte(8) - 50.0) / 50.0,
        byte(16) / 50.0,
        byte(24) / 100.0,
    )
}

#[path = "evidence.rs"]
pub mod evidence;
