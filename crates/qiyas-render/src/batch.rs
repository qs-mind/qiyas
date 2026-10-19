use std::collections::{HashMap, HashSet};

use opencadcodec::{CadDocument, Color, Handle};
use opencadkernel::geom2d::triangulate_rings;
use qiyas_geom::{flatten_within, Paint, Path, Xform, XrefDocs};

use crate::color::resolve_color;
use crate::text::{LaidText, Quad, TextEngine};
use crate::visibility::layer_shown_with;

/// First four bytes of a batch: "BYIQ" read as bytes, `0x51495942` as a little-endian `u32`.
pub const MAGIC: u32 = 0x5149_5942;
/// Layout version of `Batch::to_bytes`.
pub const VERSION: u32 = 2;
/// `Range::chain` of an entity drawn at the top level, outside any block.
pub const NO_CHAIN: u32 = u32::MAX;

/// Most block instances plus paths one sheet's batch flattens; past it the batch is cut and
/// marked `truncated` ("Drawing too complex"). The biggest real drawing tried (1.4 M segments)
/// spends about 0.4 M.
pub const MAX_SHEET_WORK: usize = 5 * qiyas_geom::MAX_WORK;

/// Section kinds of `Batch::to_bytes`. Later layouts add kinds; readers skip unknown ones.
/// Kinds 1 and 2 (per-segment handles) are not used: the ranges carry the handles.
pub mod section {
    pub const RANGES: u32 = 3;
    pub const CHAINS: u32 = 4;
    pub const SEGMENTS: u32 = 5;
    pub const COLORS: u32 = 6;
    pub const QUADS: u32 = 7;
    pub const ATLAS: u32 = 8;
    pub const TEXT_RANGES: u32 = 9;
    pub const IMAGES: u32 = 10;
    pub const IMAGE_TABLE: u32 = 11;
    pub const FILLS: u32 = 12;
    pub const FILL_RANGES: u32 = 13;
}

/// `Batch::fill_colors` bit: fill with the background colour (a wipeout), not the RGB below it.
pub const FILL_BACKGROUND: u32 = 0x0100_0000;

/// `ImageQuad::flags` bit: draw the image in grey.
pub const IMAGE_MONOCHROME: u32 = 1;
/// `ImageQuad::flags` bit: draw the image half faded into the background.
pub const IMAGE_FADED: u32 = 2;
/// `ImageQuad::flags` bit: the upper bytes hold the image's own brightness (bits 8 to 15),
/// contrast (16 to 23) and fade (24 to 31), each 0 to 100 as AutoCAD keeps them (50, 50 and 0
/// leave the picture as it is); the fade then replaces `IMAGE_FADED`'s half.
pub const IMAGE_ADJUSTED: u32 = 4;

/// The `ImageQuad::flags` of a raster image with AutoCAD's `brightness`, `contrast` and `fade`
/// (each cut to 0..100).
pub fn image_flags(brightness: u8, contrast: u8, fade: u8) -> u32 {
    let [b, c, f] = [brightness, contrast, fade].map(|v| u32::from(v.min(100)));
    IMAGE_ADJUSTED | b << 8 | c << 16 | f << 24
}

/// A picture to draw under the lines (a PDF underlay's page): where its corners go in drawing
/// coordinates, in the order top-left, top-right, bottom-right, bottom-left of the picture as it
/// stands upright; `id` and `version` name its pixels, which the window asks for separately.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagePlacement {
    pub id: u32,
    pub version: u64,
    pub corners: [[f64; 2]; 4],
    /// `IMAGE_MONOCHROME`, `IMAGE_FADED`, or `image_flags`.
    pub flags: u32,
}

/// An image quad for the GPU: `corners` x y relative to the batch origin, in `ImagePlacement`'s
/// order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageQuad {
    pub corners: [f32; 8],
    pub id: u32,
    pub flags: u32,
}

/// The pixels an image quad needs: image `id` as of `version` (the window fetches it again when
/// the version changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageEntry {
    pub id: u32,
    pub version: u64,
}

/// `Range::flags` bit: the segments are a text's box, picked and highlighted but not drawn (the
/// text's glyph quads are).
pub const TEXT_BOX: u32 = 1;
/// `Range::flags` bit: the segments are a hatch's pattern lines, drawn and picked as the hatch
/// but never part of a boundary.
pub const PATTERN: u32 = 2;

/// The segments (or, in `Batch::text_ranges`, the glyph quads) of one drawn entity:
/// `first..first + count`. For an entity inside blocks, `top` is the outermost block reference on
/// the sheet, `entity` the entity itself and `chain` an index into `Batch::chains`; at the top
/// level `top == entity` and `chain == NO_CHAIN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub top: u64,
    pub entity: u64,
    pub chain: u32,
    pub first: u32,
    pub count: u32,
    /// `TEXT_BOX` or 0; always 0 in `text_ranges`.
    pub flags: u32,
}

/// How texts are drawn: the drawing's "Arabic text is reversed" setting, and the zoom the glyphs
/// are drawn for (screen pixels per drawing unit; `None` picks one that fits the sheet on about
/// 1600 pixels).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextOptions {
    pub arabic_reversed: bool,
    pub px_per_unit: Option<f64>,
}

/// Pixels the default zoom fits the sheet's longer side on.
const DEFAULT_FIT_PX: f64 = 1600.0;

/// Line segments of one sheet, ready for the GPU.
///
/// Segment coordinates are relative to `origin`, so drawings far from (0, 0), such as
/// survey grids, keep their precision in `f32`. Add `origin` back for drawing coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    /// `x0 y0 x1 y1` per segment, relative to `origin`.
    pub segments: Vec<f32>,
    /// `0xRRGGBB` per segment.
    pub colors: Vec<u32>,
    /// One per drawn entity, in segment order; every segment is in exactly one.
    pub ranges: Vec<Range>,
    /// The block-reference handles leading to each nested entity, outermost first.
    pub chains: Vec<Vec<u64>>,
    /// `[[min_x, min_y], [max_x, max_y]]` of all segments and fills in drawing coordinates (rays'
    /// and construction lines' left out: they cross it); zeros when empty.
    pub bounds: [[f64; 2]; 2],
    /// Centre of `bounds`; zeros when empty.
    pub origin: [f64; 2],
    /// Some block content was too deep or too large to draw (see `qiyas_geom::FlattenOutcome`).
    /// Not part of `to_bytes`.
    pub truncated: bool,
    /// Glyph quads of the texts, relative to `origin`; empty without a text engine.
    pub quads: Vec<Quad>,
    /// One per drawn text, over `quads`, in quad order.
    pub text_ranges: Vec<Range>,
    /// The atlas version and page count the quads need.
    pub atlas_version: u64,
    pub atlas_pages: u32,
    /// The zoom the glyphs were drawn for, in screen pixels per drawing unit; 0 without texts.
    pub px_per_unit: f64,
    /// The tolerance curves were flattened within, and the drawing's own screen tolerance (0.05
    /// mm in its units), in drawing units: the window asks for finer curves when zoomed in past
    /// what `curve_tol` draws cleanly. Set by the session; 0 when not known.
    pub curve_tol: f64,
    pub base_tol: f64,
    /// The texts laid out, to place their glyphs again at another zoom (`place_text`). Not part
    /// of `to_bytes`.
    pub texts: Vec<LaidText>,
    /// Pictures drawn under the lines (see `add_images`), and the pixels they need.
    pub images: Vec<ImageQuad>,
    pub image_table: Vec<ImageEntry>,
    /// Filled triangles (solid hatches, 2D solids, traces, wipeouts), drawn under the lines:
    /// `x0 y0 x1 y1 x2 y2` per triangle, relative to `origin`.
    pub fills: Vec<f32>,
    /// `0xRRGGBB` per triangle, or `FILL_BACKGROUND` for the background colour.
    pub fill_colors: Vec<u32>,
    /// One per filled entity, over the triangles, in triangle order (flags 0).
    pub fill_ranges: Vec<Range>,
}

/// What the user has hidden on top of the drawing's own layer state.
/// Layer names match case-insensitively, as in DXF.
#[derive(Debug, Clone, Default)]
pub struct Visibility {
    pub hidden_layers: HashSet<String>,
}

/// Flattens the entities of `sheet_block` (e.g. `*Model_Space`) into line segments.
/// Skips invisible entities and entities on hidden, off or frozen layers; inside blocks this
/// goes by the inner entity's own layer (layer "0" meaning its insert's), so hiding a layer
/// hides only those parts of a block. Colours are the inner entity's, ByBlock and ByLayer
/// resolved as AutoCAD does.
///
/// Texts are kept as their boxes only (flagged `TEXT_BOX`); `build_batch_with_text` draws them.
pub fn build_batch(doc: &CadDocument, sheet_block: &str, vis: &Visibility, tol: f64) -> Batch {
    build(doc, &XrefDocs::new(), sheet_block, vis, tol, None)
}

/// `build_batch`, with every text shaped and laid out by `engine` and drawn as glyph quads, its
/// box made to fit what is drawn.
pub fn build_batch_with_text(
    doc: &CadDocument,
    sheet_block: &str,
    vis: &Visibility,
    tol: f64,
    engine: &mut TextEngine,
    options: TextOptions,
) -> Batch {
    build_batch_with_xrefs(
        doc,
        &XrefDocs::new(),
        sheet_block,
        vis,
        tol,
        engine,
        options,
    )
}

/// `build_batch_with_text`, with the references to the loaded `xrefs` drawn (see
/// `qiyas_geom::flatten_with`): their paths sit under the xref reference's chain, on layers named
/// `XREF|LAYER`, and their texts are shaped with the xref's text styles into the same atlas.
pub fn build_batch_with_xrefs(
    doc: &CadDocument,
    xrefs: &XrefDocs,
    sheet_block: &str,
    vis: &Visibility,
    tol: f64,
    engine: &mut TextEngine,
    options: TextOptions,
) -> Batch {
    engine.start_build();
    let mut batch = build(doc, xrefs, sheet_block, vis, tol, Some((engine, options)));
    if !batch.texts.is_empty() {
        let [[x0, y0], [x1, y1]] = batch.bounds;
        let fit = DEFAULT_FIT_PX / (x1 - x0).max(y1 - y0);
        let px = options
            .px_per_unit
            .filter(|p| *p > 0.0 && p.is_finite())
            .unwrap_or(if fit.is_finite() { fit } else { 1.0 });
        place_text(&mut batch, engine, px);
    }
    batch
}

/// Draws the batch's texts again for a zoom of `px_per_unit` screen pixels per drawing unit:
/// new quads, text ranges and atlas fields; the lines stay.
pub fn place_text(batch: &mut Batch, engine: &mut TextEngine, px_per_unit: f64) {
    let (quads, ranges) = engine.place(&batch.texts, batch.origin, px_per_unit);
    batch.quads = quads;
    batch.text_ranges = ranges;
    batch.atlas_version = engine.atlas().version();
    batch.atlas_pages = engine.atlas().page_count();
    batch.px_per_unit = px_per_unit;
}

/// Adds pictures to draw under the lines: their quads, relative to the batch origin, and one
/// table entry per image id. The bounds grow to hold them; a batch with nothing else in it takes
/// their centre as its origin.
pub fn add_images(batch: &mut Batch, images: &[ImagePlacement]) {
    let corners = images.iter().flat_map(|i| i.corners);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for [x, y] in corners {
        lo = [lo[0].min(x), lo[1].min(y)];
        hi = [hi[0].max(x), hi[1].max(y)];
    }
    if !lo[0].is_finite() || !hi[0].is_finite() {
        return;
    }
    let empty = batch.segments.is_empty()
        && batch.quads.is_empty()
        && batch.images.is_empty()
        && batch.fills.is_empty();
    if empty {
        batch.bounds = [lo, hi];
        batch.origin = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    } else {
        let [b0, b1] = batch.bounds;
        batch.bounds = [
            [b0[0].min(lo[0]), b0[1].min(lo[1])],
            [b1[0].max(hi[0]), b1[1].max(hi[1])],
        ];
    }
    let origin = batch.origin;
    for image in images {
        let mut corners = [0f32; 8];
        for (k, [x, y]) in image.corners.iter().enumerate() {
            corners[2 * k] = (x - origin[0]) as f32;
            corners[2 * k + 1] = (y - origin[1]) as f32;
        }
        batch.images.push(ImageQuad {
            corners,
            id: image.id,
            flags: image.flags,
        });
        if !batch.image_table.iter().any(|e| e.id == image.id) {
            batch.image_table.push(ImageEntry {
                id: image.id,
                version: image.version,
            });
        }
    }
}

/// Makes pictures pickable: per `(handle, corners)` (drawing coordinates, any order around), the
/// picture's outline as four segments in a range of its own flagged `TEXT_BOX` (picked and
/// highlighted, never drawn), after the segments already there. Call after `add_images`, which
/// sets the origin of a batch with nothing else in it.
pub fn add_pick_outlines(batch: &mut Batch, outlines: &[(u64, [[f64; 2]; 4])]) {
    let origin = batch.origin;
    for (handle, corners) in outlines {
        let first = (batch.segments.len() / 4) as u32;
        for k in 0..4 {
            let [a, b] = [corners[k], corners[(k + 1) % 4]];
            batch.segments.extend([
                (a[0] - origin[0]) as f32,
                (a[1] - origin[1]) as f32,
                (b[0] - origin[0]) as f32,
                (b[1] - origin[1]) as f32,
            ]);
            batch.colors.push(0x80_80_80);
        }
        batch.ranges.push(Range {
            top: *handle,
            entity: *handle,
            chain: NO_CHAIN,
            first,
            count: 4,
            flags: TEXT_BOX,
        });
    }
}

/// A ray or construction line met while building, drawn once the sheet's bounds are known.
struct Infinite {
    base: [f64; 2],
    dir: [f64; 2],
    both_ways: bool,
    rgb: u32,
    top: u64,
    entity: u64,
    chain: u32,
}

/// A POINT met while building, drawn as a cross once the sheet's bounds are known.
struct Mark {
    at: [f64; 2],
    /// Across, in drawing units; 0 sizes it from the sheet.
    size: f64,
    rgb: u32,
    top: u64,
    entity: u64,
    chain: u32,
}

/// The index of `chain` in `chains`, adding it when new; `NO_CHAIN` for an empty one.
fn chain_id(chains: &mut Vec<Vec<u64>>, index: &mut HashMap<Vec<u64>, u32>, chain: &[u64]) -> u32 {
    if chain.is_empty() {
        return NO_CHAIN;
    }
    if let Some(&i) = index.get(chain) {
        return i;
    }
    let i = chains.len() as u32;
    index.insert(chain.to_vec(), i);
    chains.push(chain.to_vec());
    i
}

/// Adds `count` records at `first` for an entity to `ranges`, extending the last range when it
/// is the same entity's and ends at `first`.
fn extend_ranges(ranges: &mut Vec<Range>, at: Range) {
    match ranges.last_mut() {
        Some(last)
            if last.top == at.top
                && last.entity == at.entity
                && last.chain == at.chain
                && last.flags == at.flags
                && last.first + last.count == at.first =>
        {
            last.count += at.count;
        }
        _ => ranges.push(at),
    }
}

fn build(
    doc: &CadDocument,
    xrefs: &XrefDocs,
    sheet_block: &str,
    vis: &Visibility,
    tol: f64,
    mut text: Option<(&mut TextEngine, TextOptions)>,
) -> Batch {
    // By layer name as written, so the many paths of a block look their layer up once.
    let mut shown_layers: HashMap<String, bool> = HashMap::new();
    let mut shown = |layer: &str| match shown_layers.get(layer) {
        Some(&shown) => shown,
        None => {
            let shown = layer_shown_with(doc, xrefs, vis, layer);
            shown_layers.insert(layer.to_string(), shown);
            shown
        }
    };
    let mut batch = Batch::default();
    let mut chain_index: HashMap<Vec<u64>, u32> = HashMap::new();
    let mut chain_shown: HashMap<Vec<u64>, bool> = HashMap::new();
    // Absolute `x0 y0 x1 y1` per segment and `x0 y0 x1 y1 x2 y2` per fill triangle; made
    // relative once the bounds are known.
    let mut absolute: Vec<[f64; 4]> = Vec::new();
    let mut triangles: Vec<[f64; 6]> = Vec::new();
    let mut infinite: Vec<Infinite> = Vec::new();
    let mut marks: Vec<Mark> = Vec::new();
    // Each fill entity's rings (own coordinates) and their triangulation, by xref and handle.
    type Tessellated = (Vec<Vec<[f64; 2]>>, Vec<[f64; 2]>, Vec<[usize; 3]>);
    let mut tessellated: HashMap<(Option<String>, u64), Tessellated> = HashMap::new();
    // One budget for the whole sheet, so a big block inserted many times stops too.
    let mut budget = MAX_SHEET_WORK;
    for entity in doc.entities_in_block(sheet_block) {
        let common = entity.common();
        if common.invisible || !shown(&common.layer) {
            continue;
        }
        let top = common.handle.value();
        let outcome = flatten_within(doc, xrefs, entity, &Xform::identity(), tol, &mut budget);
        batch.truncated |= outcome.truncated;
        for flat in &outcome.paths {
            if !shown(&flat.layer) {
                continue;
            }
            // A nested block reference on a hidden layer hides everything inside it.
            let chain_shown = match chain_shown.get(flat.chain.as_slice()) {
                Some(&v) => v,
                None => {
                    let v = nested_inserts_shown(doc, &flat.chain, &common.layer, &mut shown);
                    chain_shown.insert(flat.chain.clone(), v);
                    v
                }
            };
            if !chain_shown {
                continue;
            }
            // Inside an xref: a block or nested xref reference on a hidden layer hides its content.
            if !flat.xref_insert_layers.iter().all(|l| shown(l)) {
                continue;
            }
            let [r, g, b] = resolve_color(&flat.color, &Color::ByLayer);
            let rgb = u32::from_be_bytes([0, r, g, b]);
            match &flat.paint {
                Paint::Fill(fill) => {
                    // Ear clipping slows down with the square of the corners: a fill that big
                    // keeps its outline only.
                    if fill.rings.iter().map(Vec::len).sum::<usize>() > MAX_FILL_CORNERS {
                        continue;
                    }
                    // Once per fill entity in its own coordinates: a block inserted many times
                    // maps the same triangles through each reference (an affine map keeps them).
                    let key = (flat.xref.clone(), fill.source);
                    if tessellated.get(&key).is_none_or(|t| t.0 != fill.rings) {
                        let (points, tris) = triangulate_rings(&fill.rings);
                        tessellated.insert(key.clone(), (fill.rings.clone(), points, tris));
                    }
                    let (_, points, tris) = &tessellated[&key];
                    if tris.is_empty() {
                        continue;
                    }
                    let points: Vec<[f64; 2]> = points.iter().map(|p| fill.xf.apply(*p)).collect();
                    let first = triangles.len() as u32;
                    for [i, j, k] in tris {
                        let (a, b, c) = (points[*i], points[*j], points[*k]);
                        triangles.push([a[0], a[1], b[0], b[1], c[0], c[1]]);
                    }
                    let color = if fill.background {
                        FILL_BACKGROUND
                    } else {
                        rgb
                    };
                    batch
                        .fill_colors
                        .extend(std::iter::repeat_n(color, tris.len()));
                    let chain = chain_id(&mut batch.chains, &mut chain_index, &flat.chain);
                    extend_ranges(
                        &mut batch.fill_ranges,
                        Range {
                            top,
                            entity: flat.entity,
                            chain,
                            first,
                            count: tris.len() as u32,
                            flags: 0,
                        },
                    );
                    continue;
                }
                Paint::Infinite { both_ways } => {
                    let [a, b] = [flat.path.points[0], flat.path.points[1]];
                    infinite.push(Infinite {
                        base: a,
                        dir: [b[0] - a[0], b[1] - a[1]],
                        both_ways: *both_ways,
                        rgb,
                        top,
                        entity: flat.entity,
                        chain: chain_id(&mut batch.chains, &mut chain_index, &flat.chain),
                    });
                    continue;
                }
                Paint::Point { size } => {
                    marks.push(Mark {
                        at: flat.path.points[0],
                        size: *size,
                        rgb,
                        top,
                        entity: flat.entity,
                        chain: chain_id(&mut batch.chains, &mut chain_index, &flat.chain),
                    });
                    continue;
                }
                Paint::Outline | Paint::Pattern => {}
            }
            // A laid-out text's box is the box of what it draws.
            let laid = match (&flat.text, text.as_mut()) {
                (Some(t), Some((engine, options))) => {
                    engine.lay_out_text(doc, xrefs, t, options.arabic_reversed)
                }
                _ => None,
            };
            let first = absolute.len();
            match &laid {
                Some(l) => {
                    let c = &l.corners;
                    for i in 0..4 {
                        let (a, b) = (c[i], c[(i + 1) % 4]);
                        absolute.push([a[0], a[1], b[0], b[1]]);
                        batch.colors.push(rgb);
                    }
                }
                None => {
                    for (a, b) in segments(&flat.path) {
                        absolute.push([a[0], a[1], b[0], b[1]]);
                        batch.colors.push(rgb);
                    }
                }
            }
            let count = absolute.len() - first;
            if count == 0 {
                continue;
            }
            let chain = chain_id(&mut batch.chains, &mut chain_index, &flat.chain);
            if let Some(l) = laid {
                let xf = l.xf;
                let along = xf.m[0][0].hypot(xf.m[1][0]);
                let up = xf.m[0][1].hypot(xf.m[1][1]);
                batch.texts.push(LaidText {
                    top,
                    entity: flat.entity,
                    chain,
                    color: rgb,
                    world_size: along.max(up),
                    xf,
                    glyphs: l.glyphs,
                    text: l.text,
                });
            }
            let flags = if flat.text.is_some() {
                TEXT_BOX
            } else if flat.paint == Paint::Pattern {
                PATTERN
            } else {
                0
            };
            // A path of the same entity right after its last one extends that range.
            extend_ranges(
                &mut batch.ranges,
                Range {
                    top,
                    entity: flat.entity,
                    chain,
                    first: first as u32,
                    count: count as u32,
                    flags,
                },
            );
        }
    }
    if absolute.is_empty() && triangles.is_empty() && infinite.is_empty() && marks.is_empty() {
        return batch;
    }
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    let grow = |bounds: &mut [[f64; 2]; 2], x: f64, y: f64| {
        let [lo, hi] = bounds;
        *lo = [lo[0].min(x), lo[1].min(y)];
        *hi = [hi[0].max(x), hi[1].max(y)];
    };
    for s in &absolute {
        grow(&mut bounds, s[0], s[1]);
        grow(&mut bounds, s[2], s[3]);
    }
    for t in &triangles {
        grow(&mut bounds, t[0], t[1]);
        grow(&mut bounds, t[2], t[3]);
        grow(&mut bounds, t[4], t[5]);
    }
    for m in &marks {
        grow(&mut bounds, m.at[0], m.at[1]);
    }
    if !bounds[0][0].is_finite() {
        // Only rays and construction lines: the sheet is around their base points.
        for line in &infinite {
            grow(&mut bounds, line.base[0], line.base[1]);
        }
    }
    let [mut lo, mut hi] = bounds;
    let side = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let side = if side > 1e-9 { side } else { INFINITE_REACH };
    add_marks(&mut batch, &mut absolute, &marks, side);
    if !infinite.is_empty() {
        add_infinite(&mut batch, &mut absolute, &infinite, [lo, hi], side);
        if absolute.len() == infinite.len() && triangles.is_empty() {
            // Nothing else on the sheet: show the lines as drawn.
            lo = [lo[0] - side, lo[1] - side];
            hi = [hi[0] + side, hi[1] + side];
        }
    }
    let origin = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    batch.segments = absolute
        .iter()
        .flat_map(|s| {
            [
                s[0] - origin[0],
                s[1] - origin[1],
                s[2] - origin[0],
                s[3] - origin[1],
            ]
            .map(|v| v as f32)
        })
        .collect();
    batch.fills = triangles
        .iter()
        .flat_map(|t| {
            [
                t[0] - origin[0],
                t[1] - origin[1],
                t[2] - origin[0],
                t[3] - origin[1],
                t[4] - origin[0],
                t[5] - origin[1],
            ]
            .map(|v| v as f32)
        })
        .collect();
    batch.bounds = [lo, hi];
    batch.origin = origin;
    batch
}

/// A point's cross spans this share of the sheet's larger side when the drawing gives it no size
/// (PDSIZE 0): about as big as the snap marker with the whole sheet in view.
const POINT_SHARE: f64 = 0.01;

/// Draws each point as a cross of two segments (AutoCAD's PDMODE 0 drawn big enough to see),
/// `size` across or `side * POINT_SHARE` without one, after the segments already there.
fn add_marks(batch: &mut Batch, absolute: &mut Vec<[f64; 4]>, marks: &[Mark], side: f64) {
    for m in marks {
        let half = if m.size > 0.0 {
            m.size
        } else {
            side * POINT_SHARE
        } / 2.0;
        let [x, y] = m.at;
        let first = absolute.len() as u32;
        absolute.push([x - half, y, x + half, y]);
        absolute.push([x, y - half, x, y + half]);
        batch.colors.extend([m.rgb, m.rgb]);
        extend_ranges(
            &mut batch.ranges,
            Range {
                top: m.top,
                entity: m.entity,
                chain: m.chain,
                first,
                count: 2,
                flags: 0,
            },
        );
    }
}

/// Most corners one fill is triangulated with; a bigger one is drawn as its outline only. The
/// kernel's ear clipping slows down with the square of the corners or worse.
const MAX_FILL_CORNERS: usize = 2_000;

/// How far rays and construction lines reach past an empty sheet, in drawing units.
const INFINITE_REACH: f64 = 100.0;

/// Draws the rays and construction lines across the box `bounds` widened by `side` each way
/// (and around their base points), one segment each, after the segments already there.
fn add_infinite(
    batch: &mut Batch,
    absolute: &mut Vec<[f64; 4]>,
    lines: &[Infinite],
    bounds: [[f64; 2]; 2],
    side: f64,
) {
    let [lo, hi] = bounds;
    for line in lines {
        let (b, d) = (line.base, line.dir);
        let lo = [lo[0].min(b[0]) - side, lo[1].min(b[1]) - side];
        let hi = [hi[0].max(b[0]) + side, hi[1].max(b[1]) + side];
        // Liang-Barsky: the stretch of `b + t d` inside the box.
        let (mut t0, mut t1) = (
            if line.both_ways {
                f64::NEG_INFINITY
            } else {
                0.0
            },
            f64::INFINITY,
        );
        for axis in 0..2 {
            if d[axis].abs() < 1e-15 {
                continue;
            }
            let a = (lo[axis] - b[axis]) / d[axis];
            let z = (hi[axis] - b[axis]) / d[axis];
            t0 = t0.max(a.min(z));
            t1 = t1.min(a.max(z));
        }
        if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
            continue;
        }
        let first = absolute.len() as u32;
        absolute.push([
            b[0] + t0 * d[0],
            b[1] + t0 * d[1],
            b[0] + t1 * d[0],
            b[1] + t1 * d[1],
        ]);
        batch.colors.push(line.rgb);
        extend_ranges(
            &mut batch.ranges,
            Range {
                top: line.top,
                entity: line.entity,
                chain: line.chain,
                first,
                count: 1,
                flags: 0,
            },
        );
    }
}

/// Whether every block reference of `chain` after the first (the top-level one, already checked)
/// sits on a shown layer. A reference on layer "0" is drawn on the layer of the one around it.
fn nested_inserts_shown(
    doc: &CadDocument,
    chain: &[u64],
    top_layer: &str,
    shown: &mut impl FnMut(&str) -> bool,
) -> bool {
    let mut layer = top_layer;
    for &h in chain.iter().skip(1) {
        let Some(insert) = doc.get_entity(Handle::new(h)) else {
            continue;
        };
        let own = insert.common().layer.as_str();
        if own != "0" {
            layer = own;
        }
        if !shown(layer) {
            return false;
        }
    }
    true
}

/// A path of n points gives n - 1 segments, plus the closing one when closed
/// (same rule as `Path::length`, so a closed two-point path is not drawn twice).
fn segments(path: &Path) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    let n = path.points.len();
    let count = if path.closed && n > 2 {
        n
    } else {
        n.saturating_sub(1)
    };
    (0..count).map(move |i| (path.points[i], path.points[(i + 1) % n]))
}

impl Batch {
    /// The range holding segment `i`.
    pub fn range_of(&self, i: usize) -> Option<&Range> {
        let at = self
            .ranges
            .partition_point(|r| (r.first + r.count) as usize <= i);
        self.ranges.get(at).filter(|r| r.first as usize <= i)
    }

    /// The block-reference chain of a range; empty at the top level.
    pub fn chain_of(&self, range: &Range) -> &[u64] {
        self.chains
            .get(range.chain as usize)
            .map_or(&[], Vec::as_slice)
    }

    /// Encodes the batch for the frontend: layout version 2. Little-endian; every section
    /// starts on an 8-byte boundary (zero padding before it). With k = sections (7, plus 2 with
    /// images, plus 2 with fills):
    ///
    /// | offset   | size  | contents                                                     |
    /// |----------|-------|--------------------------------------------------------------|
    /// | 0        | 4     | `u32` magic `0x51495942`                                     |
    /// | 4        | 4     | `u32` version, 2                                             |
    /// | 8        | 32    | `f64` x4 bounds: min x, min y, max x, max y (absolute)       |
    /// | 40       | 16    | `f64` x2 origin: x, y                                        |
    /// | 56       | 4     | `u32` k, the number of sections                              |
    /// | 60       | 12k   | per section: `u32` kind, `u32` offset, `u32` byte length     |
    ///
    /// Sections, in this order:
    ///
    /// | kind | name           | contents                                                    |
    /// |------|----------------|-------------------------------------------------------------|
    /// | 3    | ranges         | 32 bytes per range: `u64` top, `u64` entity, `u32` chain    |
    /// |      |                | index (`0xFFFFFFFF` = none), `u32` first, `u32` count,      |
    /// |      |                | `u32` flags (bit 0 `TEXT_BOX`: picked, not drawn; bit 1     |
    /// |      |                | `PATTERN`: a hatch's pattern lines)                         |
    /// | 4    | chains         | per chain: `u32` length m, then m `u64` handles, outermost  |
    /// |      |                | first; chains are numbered from 0 in order                  |
    /// | 5    | segments       | `f32` x0 y0 x1 y1 per segment, relative to origin           |
    /// | 6    | colours        | `u32` 0xRRGGBB per segment                                  |
    /// | 7    | glyph quads    | 48 bytes per quad: `f32` x8 corners x y relative to origin  |
    /// |      |                | (top-left, top-right, bottom-right, bottom-left of the      |
    /// |      |                | glyph), `u16` x4 texels u0 v0 u1 v1, `u32` 0xRRGGBB,        |
    /// |      |                | `u16` atlas page, `u16` flags (bit 0: inside a block)       |
    /// | 8    | atlas          | 24 bytes: `u64` atlas version, `u32` page count, 4 pad,     |
    /// |      |                | `f64` the zoom the glyphs are drawn for (screen pixels per  |
    /// |      |                | drawing unit; 0 without texts)                              |
    /// | 9    | text ranges    | 32 bytes per text, as kind 3 but over the quads (flags 0)   |
    /// | 10   | image quads    | 40 bytes per quad: `f32` x8 corners x y relative to origin  |
    /// |      |                | (top-left, top-right, bottom-right, bottom-left of the      |
    /// |      |                | picture), `u32` image id, `u32` flags (bit 0 monochrome,    |
    /// |      |                | bit 1 faded by half, bit 2 adjusted: bits 8-15 brightness,  |
    /// |      |                | 16-23 contrast, 24-31 fade, each 0-100); under the lines    |
    /// | 11   | image table    | 16 bytes per image: `u32` id, 4 pad, `u64` version (below   |
    /// |      |                | 2^53)                                                       |
    /// | 12   | fill triangles | 28 bytes per triangle: `f32` x6 corners x y relative to     |
    /// |      |                | origin, `u32` 0xRRGGBB (bit 24 `FILL_BACKGROUND`: the       |
    /// |      |                | background colour instead); under the lines                 |
    /// | 13   | fill ranges    | 32 bytes per filled entity, as kind 3 but over the          |
    /// |      |                | triangles (flags 0)                                         |
    ///
    /// Kinds 12 and 13 are written only when the batch has fills, after the images'. Range flags
    /// bit 1 (`PATTERN`) marks a hatch's pattern lines.
    ///
    /// Kinds 10 and 11 are written only when the batch has images. Their pixels are not in the
    /// batch either: the window asks for an image (IPC `underlay_image`) when it doesn't hold that
    /// id at that version, or holds it drawn for a zoom more than twice off.
    ///
    /// Every segment belongs to exactly one range, so the ranges give each segment's handles;
    /// likewise every quad belongs to one text range. The atlas pages themselves are not in the
    /// batch: the window asks for them (`Atlas::to_bytes`) when the version differs from its own.
    /// Kinds 1 and 2 are unused. Readers find sections by kind and skip kinds they don't know, so
    /// a later layout can add sections (images) without a new version.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut chains = Vec::new();
        for chain in &self.chains {
            chains.extend((chain.len() as u32).to_le_bytes());
            for h in chain {
                chains.extend(h.to_le_bytes());
            }
        }
        let mut quads = Vec::with_capacity(self.quads.len() * 48);
        for q in &self.quads {
            quads.extend(le_bytes(&q.corners, f32::to_le_bytes));
            quads.extend(le_bytes(&q.uv, u16::to_le_bytes));
            quads.extend(q.color.to_le_bytes());
            quads.extend(q.page.to_le_bytes());
            quads.extend(q.flags.to_le_bytes());
        }
        let mut atlas = Vec::with_capacity(40);
        atlas.extend(self.atlas_version.to_le_bytes());
        atlas.extend(self.atlas_pages.to_le_bytes());
        atlas.extend([0u8; 4]);
        atlas.extend(self.px_per_unit.to_le_bytes());
        atlas.extend(self.curve_tol.to_le_bytes());
        atlas.extend(self.base_tol.to_le_bytes());
        let mut sections: Vec<(u32, Vec<u8>)> = vec![
            (section::RANGES, ranges_bytes(&self.ranges)),
            (section::CHAINS, chains),
            (
                section::SEGMENTS,
                le_bytes(&self.segments, f32::to_le_bytes),
            ),
            (section::COLORS, le_bytes(&self.colors, u32::to_le_bytes)),
            (section::QUADS, quads),
            (section::ATLAS, atlas),
            (section::TEXT_RANGES, ranges_bytes(&self.text_ranges)),
        ];
        if !self.images.is_empty() || !self.image_table.is_empty() {
            let mut images = Vec::with_capacity(self.images.len() * 40);
            for q in &self.images {
                images.extend(le_bytes(&q.corners, f32::to_le_bytes));
                images.extend(q.id.to_le_bytes());
                images.extend(q.flags.to_le_bytes());
            }
            let mut table = Vec::with_capacity(self.image_table.len() * 16);
            for e in &self.image_table {
                table.extend(e.id.to_le_bytes());
                table.extend([0u8; 4]);
                table.extend(e.version.to_le_bytes());
            }
            sections.push((section::IMAGES, images));
            sections.push((section::IMAGE_TABLE, table));
        }
        if !self.fill_colors.is_empty() {
            let mut fills = Vec::with_capacity(self.fill_colors.len() * 28);
            for (t, color) in self.fills.as_chunks::<6>().0.iter().zip(&self.fill_colors) {
                fills.extend(le_bytes(t, f32::to_le_bytes));
                fills.extend(color.to_le_bytes());
            }
            sections.push((section::FILLS, fills));
            sections.push((section::FILL_RANGES, ranges_bytes(&self.fill_ranges)));
        }

        let mut out = Vec::new();
        out.extend(MAGIC.to_le_bytes());
        out.extend(VERSION.to_le_bytes());
        for v in self.bounds.as_flattened().iter().chain(&self.origin) {
            out.extend(v.to_le_bytes());
        }
        out.extend((sections.len() as u32).to_le_bytes());
        let mut at = align8(60 + 12 * sections.len());
        for (kind, bytes) in &sections {
            out.extend(kind.to_le_bytes());
            out.extend((at as u32).to_le_bytes());
            out.extend((bytes.len() as u32).to_le_bytes());
            at = align8(at + bytes.len());
        }
        for (_, bytes) in &sections {
            out.resize(align8(out.len()), 0);
            out.extend(bytes);
        }
        out
    }
}

fn ranges_bytes(ranges: &[Range]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ranges.len() * 32);
    for r in ranges {
        out.extend(r.top.to_le_bytes());
        out.extend(r.entity.to_le_bytes());
        out.extend(r.chain.to_le_bytes());
        out.extend(r.first.to_le_bytes());
        out.extend(r.count.to_le_bytes());
        out.extend(r.flags.to_le_bytes());
    }
    out
}

fn le_bytes<T: Copy, const N: usize>(values: &[T], f: fn(T) -> [u8; N]) -> Vec<u8> {
    values.iter().flat_map(|v| f(*v)).collect()
}

fn align8(n: usize) -> usize {
    n.next_multiple_of(8)
}
