//! Text in the viewport: shaping (rustybuzz; Latin left to right, Arabic with its joined forms
//! right to left), bidi order (unicode-bidi), font fallback, layout of TEXT, MTEXT and attributes,
//! and glyph quads drawn from the atlas.

use std::borrow::Cow;
use std::collections::HashMap;

use ab_glyph::{Font as _, GlyphId, Point};
use opencadcodec::entities::mtext_format::parse_mtext;
use opencadcodec::CadDocument;
use qiyas_geom::{xref_symbol, FlatText, HAlign, TextLayout, VAlign, Xform, XrefDocs};
use rustybuzz::{Direction, ShapePlan, UnicodeBuffer};
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

use crate::atlas::{Atlas, PageView, PAD, PAGE_SIZE};
use crate::fonts::Fonts;

/// Glyph sizes kept in the atlas, in pixels per text height. A text is drawn from the smallest
/// size at least as big as it shows on screen (the largest when it shows bigger).
pub const SIZE_BUCKETS: [u16; 9] = [8, 12, 16, 24, 32, 48, 64, 96, 128];
/// Text height as a share of the em: CAD text height is the height of a capital letter. One em
/// is `1 / CAP_EM` text units.
pub const CAP_EM: f32 = 0.7;
/// Default MTEXT line pitch, in text heights, at line spacing factor 1.
const LINE_PITCH: f64 = 5.0 / 3.0;
/// Where a text's bottom sits under its baseline, in text heights (bottom alignment).
const DESCENT: f64 = 1.0 / 3.0;

/// One shaped glyph: font, glyph id, and pen position in text heights (x along the baseline from
/// the line's left end, y up from the baseline).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub font: u16,
    pub glyph: u16,
    pub x: f32,
    pub y: f32,
    /// Byte offset in the shaped text of the first character the glyph draws.
    pub cluster: u32,
}

/// One line of text in visual order (left to right on the page).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapedLine {
    pub glyphs: Vec<ShapedGlyph>,
    /// Advance width in text heights.
    pub width: f32,
}

/// A text laid out, ready for placing its glyphs at any zoom. Not sent to the window.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidText {
    /// As in `Range`: the top-level entity, the text entity (or the insert, for an attribute),
    /// and the chain index (`NO_CHAIN` at the top level).
    pub top: u64,
    pub entity: u64,
    pub chain: u32,
    /// `0xRRGGBB`.
    pub color: u32,
    /// Text units (x along the baseline, y up, 1 = the text height) to drawing coordinates.
    pub xf: Xform,
    /// How long one text unit is in drawing units along the more stretched of the text's x and
    /// y axes (a wide width factor or fit stretches x), for the glyph size.
    pub world_size: f64,
    pub glyphs: Vec<ShapedGlyph>,
    /// The text as shaped (an MText's lines joined with `\n`): `ShapedGlyph::cluster` is a
    /// byte offset into it, so a glyph can be mapped back to the characters it draws.
    pub text: String,
}

/// A text laid out (`TextEngine::lay_out_text`): its glyphs in text units, the transform placing
/// them, its box (four corners in drawing coordinates, for picking) and the text as shaped (see
/// `LaidText::text`).
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub xf: Xform,
    pub glyphs: Vec<ShapedGlyph>,
    pub corners: [[f64; 2]; 4],
    pub text: String,
}

/// A glyph quad for the GPU: `corners` relative to the batch origin in the order top-left,
/// top-right, bottom-right, bottom-left of the glyph's bitmap (as it stands upright), `uv` the
/// texel rectangle `u0 v0 u1 v1` on atlas page `page`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub corners: [f32; 8],
    pub uv: [u16; 4],
    /// `0xRRGGBB`.
    pub color: u32,
    pub page: u16,
    /// Bit 0 (`QUAD_IN_BLOCK`): the text sits inside a block.
    pub flags: u16,
}

pub const QUAD_IN_BLOCK: u16 = 1;

/// How a glyph's bitmap sits in the atlas and against the pen, in pixels at its size bucket.
#[derive(Debug, Clone, Copy)]
struct GlyphSlot {
    page: u16,
    rect: [u16; 4],
    /// Top-left of the padded bitmap from the pen position, y down.
    left: f32,
    top: f32,
}

/// The fonts, the shaping plans, the glyph atlas and its glyph cache of one session. The atlas
/// lives as long as the engine, so its pages stay valid across batches.
pub struct TextEngine {
    fonts: Fonts,
    atlas: Atlas,
    /// Keyed by font, glyph and size bucket; `None` for glyphs with nothing to draw (spaces).
    glyphs: HashMap<(u16, u16, u8), Option<GlyphSlot>>,
    plans: HashMap<(u16, bool, u32), ShapePlan>,
    buffer: Option<UnicodeBuffer>,
    /// Fallback chain and height and width factor of each text style, by style name as written.
    styles: HashMap<String, StyleFont>,
    /// Lines shaped during the current build, by chain, text and reversal.
    shaped: HashMap<(Vec<u16>, String, bool), ShapedLine>,
}

#[derive(Debug, Clone)]
struct StyleFont {
    chain: Vec<u16>,
    height: f64,
    width_factor: f64,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// The bundled fonts and the computer's TrueType fonts.
    pub fn new() -> Self {
        Self::with_fonts(Fonts::new(true))
    }

    /// The bundled fonts only, so results don't depend on the computer (tests).
    pub fn bundled_only() -> Self {
        Self::with_fonts(Fonts::new(false))
    }

    fn with_fonts(fonts: Fonts) -> Self {
        TextEngine {
            fonts,
            atlas: Atlas::new(),
            glyphs: HashMap::new(),
            plans: HashMap::new(),
            buffer: None,
            styles: HashMap::new(),
            shaped: HashMap::new(),
        }
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The atlas pages changed after `since` (all when `None`), as `Atlas::to_bytes` lays them out.
    pub fn atlas_bytes(&self, since: Option<u64>) -> Vec<u8> {
        self.atlas.to_bytes(since)
    }

    /// The atlas pages changed after `since`.
    pub fn atlas_pages(&self, since: Option<u64>) -> impl Iterator<Item = PageView<'_>> {
        self.atlas.pages_since(since)
    }

    /// The whole font file and selected collection face, for matching PDF output to shaping.
    pub fn font_face(&self, id: u16) -> Option<(&'static [u8], u32)> {
        self.fonts
            .bytes(id)
            .map(|data| (data, self.fonts.get(id).index))
    }

    /// The font file of font `id` (as `ShapedGlyph::font` names it), whole, for embedding its
    /// glyphs elsewhere (the PDF plot); `None` for an id this engine never gave out.
    pub fn font_bytes(&self, id: u16) -> Option<&'static [u8]> {
        self.fonts.bytes(id)
    }

    /// Forgets the styles and lines of the last build (a new drawing or sheet may differ).
    pub(crate) fn start_build(&mut self) {
        self.styles.clear();
        self.shaped.clear();
    }

    /// The fallback chain for the font file `file` (see `Fonts::chain`).
    pub fn chain(&mut self, file: &str) -> Vec<u16> {
        self.fonts.chain(file)
    }

    /// Shapes one line with the fonts of `chain`, in visual order. Text in reading order goes
    /// through bidi; Arabic stored in visual order (`arabic_stored_visually`, which
    /// `arabic_reversed` forces when its letters can't tell) is drawn as stored, each Arabic word
    /// joined in place (`visual_pieces`).
    pub fn shape_line(&mut self, chain: &[u16], text: &str, arabic_reversed: bool) -> ShapedLine {
        let key = (chain.to_vec(), text.to_string(), arabic_reversed);
        if let Some(line) = self.shaped.get(&key) {
            return line.clone();
        }
        let line = self.shape_uncached(chain, text, arabic_reversed);
        self.shaped.insert(key, line.clone());
        line
    }

    fn shape_uncached(&mut self, chain: &[u16], text: &str, arabic_reversed: bool) -> ShapedLine {
        let mut line = ShapedLine::default();
        if text.is_empty() {
            return line;
        }
        let mut pen = 0.0;
        if arabic_stored_visually(text, arabic_reversed) {
            // Left to right as stored; each Arabic word joined and drawn right to left in place.
            for piece in visual_pieces(text) {
                let before = line.glyphs.last().map(|g| g.font);
                let mut fonts = self.itemize(chain, &piece.text, before);
                if piece.rtl {
                    fonts.reverse();
                }
                for (font, range) in fonts {
                    let at = piece.at + range.start;
                    let part = &piece.text[range];
                    pen = self.shape_piece(font, part, at, piece.rtl, pen, &mut line.glyphs);
                }
            }
        } else if !text.chars().any(is_rtl_char) {
            for (font, range) in self.itemize(chain, text, None) {
                let at = range.start;
                pen = self.shape_piece(font, &text[range], at, false, pen, &mut line.glyphs);
            }
        } else {
            let bidi = unicode_bidi::ParagraphBidiInfo::new(text, None);
            let (levels, runs) = bidi.visual_runs(0..text.len());
            for run in runs {
                let rtl = levels[run.start].is_rtl();
                let part = &text[run.clone()];
                let mut pieces = self.itemize(chain, part, None);
                // Right to left, the piece read last is drawn first.
                if rtl {
                    pieces.reverse();
                }
                for (font, range) in pieces {
                    let at = run.start + range.start;
                    pen = self.shape_piece(font, &part[range], at, rtl, pen, &mut line.glyphs);
                }
            }
        }
        line.width = pen;
        line
    }

    /// Splits `text` into pieces drawn by one font each: a letter takes the first font of
    /// `chain` that has it; a space, digit or sign stays with the font before it when that font
    /// has it. A character no font has takes the first font (its missing-glyph box).
    fn itemize(
        &self,
        chain: &[u16],
        text: &str,
        before: Option<u16>,
    ) -> Vec<(u16, std::ops::Range<usize>)> {
        let mut pieces: Vec<(u16, std::ops::Range<usize>)> = Vec::new();
        for (i, cluster) in text.grapheme_indices(true) {
            let current = pieces.last().map(|p| p.0).or(before);
            let c = cluster.chars().next().expect("nonempty grapheme");
            // Keep a base and its marks in one shaping face. If no face covers the whole
            // grapheme, prefer its base instead of detaching the mark into another run.
            let covers = |f| cluster.chars().all(|c| self.fonts.get(f).has(c));
            let keep = current.filter(|&f| !is_letter(c) && covers(f));
            let font = keep
                .or_else(|| chain.iter().copied().find(|&f| covers(f)))
                .or_else(|| chain.iter().copied().find(|&f| self.fonts.get(f).has(c)))
                .or(current)
                .unwrap_or(chain[0]);
            match pieces.last_mut() {
                Some(p) if p.0 == font => p.1.end = i + cluster.len(),
                _ => pieces.push((font, i..i + cluster.len())),
            }
        }
        pieces
    }

    /// Shapes `text` (at byte `at` of the line) with one font and direction, appending its glyphs
    /// from `pen` (text heights); returns the pen after it.
    fn shape_piece(
        &mut self,
        font: u16,
        text: &str,
        at: usize,
        rtl: bool,
        mut pen: f32,
        out: &mut Vec<ShapedGlyph>,
    ) -> f32 {
        let Self {
            fonts,
            plans,
            buffer,
            ..
        } = self;
        let f = fonts.get(font);
        let direction = if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };
        let mut buf = buffer.take().unwrap_or_default();
        buf.push_str(text);
        buf.set_direction(direction);
        buf.guess_segment_properties();
        let script = buf.script();
        let plan = plans
            .entry((font, rtl, script.tag().0))
            .or_insert_with(|| ShapePlan::new(&f.face, direction, Some(script), None, &[]));
        let shaped = rustybuzz::shape_with_plan(&f.face, plan, buf);
        let k = 1.0 / (f.upem * CAP_EM);
        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            out.push(ShapedGlyph {
                font,
                glyph: info.glyph_id as u16,
                x: pen + pos.x_offset as f32 * k,
                y: pos.y_offset as f32 * k,
                cluster: at as u32 + info.cluster,
            });
            pen += pos.x_advance as f32 * k;
        }
        *buffer = Some(shaped.clear());
        pen
    }

    fn style(&mut self, doc: &CadDocument, xrefs: &XrefDocs, name: &str) -> StyleFont {
        if let Some(style) = self.styles.get(name) {
            return style.clone();
        }
        // An xref's style `XREF|STYLE`: the host's copy when it keeps one, else the xref's own.
        let found = doc.text_styles.get(name).or_else(|| {
            let (xdoc, inner) = xref_symbol(xrefs, name)?;
            xdoc.text_styles.get(inner)
        });
        let style = StyleFont {
            chain: self.fonts.chain(found.map_or("", |s| s.font_file.as_str())),
            height: found.map_or(0.0, |s| s.height),
            width_factor: found.map_or(0.0, |s| s.width_factor),
        };
        self.styles.insert(name.to_string(), style.clone());
        style
    }

    /// Lays out `text`: its glyphs in text units, the transform placing them, and its box (four
    /// corners in drawing coordinates, for picking). `None` when it has no height to draw at.
    pub fn lay_out(
        &mut self,
        doc: &CadDocument,
        text: &FlatText,
        arabic_reversed: bool,
    ) -> Option<(Xform, Vec<ShapedGlyph>, [[f64; 2]; 4])> {
        self.lay_out_with(doc, &XrefDocs::new(), text, arabic_reversed)
    }

    /// `lay_out`, with a style named `XREF|STYLE` looked up in that loaded xref. A text Qiyas
    /// wrote (`FlatText::logical`) is in reading order: `arabic_reversed` leaves it alone.
    pub fn lay_out_with(
        &mut self,
        doc: &CadDocument,
        xrefs: &XrefDocs,
        text: &FlatText,
        arabic_reversed: bool,
    ) -> Option<(Xform, Vec<ShapedGlyph>, [[f64; 2]; 4])> {
        self.lay_out_text(doc, xrefs, text, arabic_reversed)
            .map(|l| (l.xf, l.glyphs, l.corners))
    }

    /// `lay_out_with`, keeping the text as shaped beside the glyphs (`Laid::text`).
    pub fn lay_out_text(
        &mut self,
        doc: &CadDocument,
        xrefs: &XrefDocs,
        text: &FlatText,
        arabic_reversed: bool,
    ) -> Option<Laid> {
        let arabic_reversed = arabic_reversed && !text.logical;
        let style = self.style(doc, xrefs, &text.style);
        let height = [text.height, style.height]
            .into_iter()
            .find(|h| *h > 0.0 && h.is_finite())?;
        let (local, glyphs, [x0, y0, x1, y1], shaped) = match text.layout {
            TextLayout::Line {
                insertion,
                alignment,
                rotation,
                width_factor,
                horizontal,
                vertical,
                backward,
                upside_down,
            } => {
                let value = decode_codes(&text.value);
                let line = self.shape_line(&style.chain, &value, arabic_reversed);
                let w = f64::from(line.width);
                let wf = [width_factor, style.width_factor, 1.0]
                    .into_iter()
                    .find(|f| *f > 0.0 && f.is_finite())
                    .unwrap_or(1.0);
                let (mut sx, mut sy) = (height * wf, height);
                let (mut dx, mut dy) = (0.0, 0.0);
                let (mut at, mut angle) = (insertion, rotation);
                let fitted = matches!(horizontal, HAlign::Aligned | HAlign::Fit);
                match alignment {
                    Some(end) if fitted && w > 0.0 => {
                        let (ex, ey) = (end[0] - insertion[0], end[1] - insertion[1]);
                        let length = ex.hypot(ey);
                        if length > 0.0 {
                            angle = ey.atan2(ex);
                            sx = length / w;
                            if horizontal == HAlign::Aligned {
                                sy = sx / wf;
                            }
                        }
                    }
                    Some(end) if horizontal != HAlign::Left || vertical != VAlign::Baseline => {
                        at = end;
                        dx = match horizontal {
                            HAlign::Center | HAlign::Middle => -w / 2.0,
                            HAlign::Right => -w,
                            _ => 0.0,
                        };
                        dy = match (horizontal, vertical) {
                            (HAlign::Middle, _) | (_, VAlign::Middle) => -0.5,
                            (_, VAlign::Top) => -1.0,
                            (_, VAlign::Bottom) => DESCENT,
                            _ => 0.0,
                        };
                    }
                    _ => {}
                }
                if backward {
                    sx = -sx;
                }
                if upside_down {
                    sy = -sy;
                }
                let local = Xform::translate(dx, dy)
                    .then(&Xform::scale(sx, sy))
                    .then(&Xform::rotate(angle))
                    .then(&Xform::translate(at[0], at[1]));
                (local, line.glyphs, [0.0, 0.0, w, 1.0], value.into_owned())
            }
            TextLayout::Block {
                insertion,
                rotation,
                width,
                attachment,
                line_spacing,
            } => {
                let plain = parse_mtext(&text.value, true).to_plain_text();
                let wrap = (width > 0.0).then_some(width / height);
                let mut lines = Vec::new();
                for paragraph in plain.split('\n') {
                    for line in self.wrap(&style.chain, paragraph, wrap, arabic_reversed) {
                        let shaped = self.shape_line(&style.chain, &line, arabic_reversed);
                        lines.push((line, shaped));
                    }
                }
                let factor = if line_spacing > 0.0 {
                    line_spacing
                } else {
                    1.0
                };
                let pitch = LINE_PITCH * factor;
                let attachment = attachment.clamp(1, 9) - 1;
                let (col, row) = (f64::from(attachment % 3), f64::from(attachment / 3));
                let total = 1.0 + pitch * (lines.len().max(1) - 1) as f64;
                let mut glyphs = Vec::new();
                let mut shaped = String::new();
                let (mut x0, mut x1) = (f64::INFINITY, f64::NEG_INFINITY);
                let mut last_baseline = -1.0;
                for (i, (text, line)) in lines.iter().enumerate() {
                    let w = f64::from(line.width);
                    let x = -col * w / 2.0;
                    let y = -1.0 - pitch * i as f64 + row * total / 2.0;
                    x0 = x0.min(x);
                    x1 = x1.max(x + w);
                    last_baseline = y;
                    // Clusters count from the line's start: move them to the joined text.
                    if i > 0 {
                        shaped.push('\n');
                    }
                    let base = shaped.len() as u32;
                    shaped.push_str(text);
                    glyphs.extend(line.glyphs.iter().map(|g| ShapedGlyph {
                        x: g.x + x as f32,
                        y: g.y + y as f32,
                        cluster: g.cluster + base,
                        ..*g
                    }));
                }
                if lines.is_empty() {
                    (x0, x1) = (0.0, 0.0);
                }
                let top = -1.0 + row * total / 2.0 + 1.0;
                let local = Xform::scale(height, height)
                    .then(&Xform::rotate(rotation))
                    .then(&Xform::translate(insertion[0], insertion[1]));
                (local, glyphs, [x0, last_baseline, x1, top], shaped)
            }
        };
        let xf = local.then(&text.xf);
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| xf.apply(p));
        Some(Laid {
            xf,
            glyphs,
            corners,
            text: shaped,
        })
    }

    /// `paragraph` broken into lines no wider than `width` text heights (a word wider than that
    /// gets a line of its own); one line when `width` is `None`.
    fn wrap(
        &mut self,
        chain: &[u16],
        paragraph: &str,
        width: Option<f64>,
        arabic_reversed: bool,
    ) -> Vec<String> {
        let Some(width) = width else {
            return vec![paragraph.to_string()];
        };
        let space = f64::from(self.shape_line(chain, " ", false).width);
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut used = 0.0;
        let mut start = 0;
        let mut separator = "";
        for (end, opportunity) in unicode_linebreak::linebreaks(paragraph) {
            let span = &paragraph[start..end];
            start = end;
            let hard = span.ends_with([
                '\n', '\r', '\u{000b}', '\u{000c}', '\u{0085}', '\u{2028}', '\u{2029}',
            ]);
            let span = span.trim_end_matches([
                '\n', '\r', '\u{000b}', '\u{000c}', '\u{0085}', '\u{2028}', '\u{2029}',
            ]);
            let word = span.trim_end_matches(' ');
            let spaces = span.len() - word.len();
            let w = f64::from(self.shape_line(chain, word, arabic_reversed).width);
            let gap = if separator.is_empty() { 0.0 } else { space };
            if !current.is_empty() && used + gap + w > width {
                lines.push(std::mem::take(&mut current));
                used = 0.0;
            }
            if !current.is_empty() {
                current.push_str(separator);
                used += gap;
            }
            current.push_str(word);
            used += w;
            // Retain CAD's ASCII-space advances, including repeated and trailing spaces.
            for _ in 0..spaces.saturating_sub(usize::from(end < paragraph.len() && !hard)) {
                if !current.is_empty() && used + space > width {
                    lines.push(std::mem::take(&mut current));
                    used = 0.0;
                }
                if !current.is_empty() {
                    current.push(' ');
                    used += space;
                }
            }
            separator = if spaces > 0 { " " } else { "" };
            if hard && opportunity == unicode_linebreak::BreakOpportunity::Mandatory {
                lines.push(std::mem::take(&mut current));
                used = 0.0;
                separator = "";
            }
        }
        lines.push(current);
        lines
    }

    /// The glyph quads of `texts` at `px_per_unit` screen pixels per drawing unit, relative to
    /// `origin`, with one range per text over them (same shape as the segment ranges). Draws the
    /// glyphs not in the atlas yet and commits the atlas.
    pub fn place(
        &mut self,
        texts: &[LaidText],
        origin: [f64; 2],
        px_per_unit: f64,
    ) -> (Vec<Quad>, Vec<crate::Range>) {
        let mut quads = Vec::new();
        let mut ranges: Vec<crate::Range> = Vec::new();
        for text in texts {
            let first = quads.len();
            let bucket = size_bucket(text.world_size * px_per_unit);
            let px = f32::from(SIZE_BUCKETS[usize::from(bucket)]);
            let flags = if text.chain == crate::NO_CHAIN {
                0
            } else {
                QUAD_IN_BLOCK
            };
            for g in &text.glyphs {
                let Some(slot) = self.glyph(g.font, g.glyph, bucket) else {
                    continue;
                };
                let [u0, v0, u1, v1] = slot.rect;
                let (w, h) = (f32::from(u1 - u0), f32::from(v1 - v0));
                let corner = |cx: f32, cy: f32| {
                    let p = [
                        f64::from(g.x + (slot.left + cx) / px),
                        f64::from(g.y - (slot.top + cy) / px),
                    ];
                    let [x, y] = text.xf.apply(p);
                    [(x - origin[0]) as f32, (y - origin[1]) as f32]
                };
                let [a, b, c, d] = [
                    corner(0.0, 0.0),
                    corner(w, 0.0),
                    corner(w, h),
                    corner(0.0, h),
                ];
                quads.push(Quad {
                    corners: [a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1]],
                    uv: slot.rect,
                    color: text.color,
                    page: slot.page,
                    flags,
                });
            }
            let count = (quads.len() - first) as u32;
            match ranges.last_mut() {
                Some(last)
                    if last.top == text.top
                        && last.entity == text.entity
                        && last.chain == text.chain
                        && (last.first + last.count) as usize == first =>
                {
                    last.count += count;
                }
                _ if count == 0 => {}
                _ => ranges.push(crate::Range {
                    top: text.top,
                    entity: text.entity,
                    chain: text.chain,
                    first: first as u32,
                    count,
                    flags: 0,
                }),
            }
        }
        self.atlas.commit();
        (quads, ranges)
    }

    /// Where glyph `glyph` of `font` at size bucket `bucket` sits in the atlas, drawing it there
    /// first if needed; `None` when it has nothing to draw.
    fn glyph(&mut self, font: u16, glyph: u16, bucket: u8) -> Option<GlyphSlot> {
        if let Some(slot) = self.glyphs.get(&(font, glyph, bucket)) {
            return *slot;
        }
        let slot = self.rasterize(font, glyph, bucket);
        self.glyphs.insert((font, glyph, bucket), slot);
        slot
    }

    fn rasterize(&mut self, font: u16, glyph: u16, bucket: u8) -> Option<GlyphSlot> {
        let f = self.fonts.get(font);
        let em_px = f32::from(SIZE_BUCKETS[usize::from(bucket)]) / CAP_EM;
        let scale = em_px * f.raster.height_unscaled() / f.upem;
        let outlined = f.raster.outline_glyph(
            GlyphId(glyph).with_scale_and_position(scale, Point { x: 0.0, y: 0.0 }),
        )?;
        let bounds = outlined.px_bounds();
        let (w, h) = (bounds.width() as u32, bounds.height() as u32);
        if w == 0 || h == 0 || w + 2 * PAD > PAGE_SIZE || h + 2 * PAD > PAGE_SIZE {
            return None;
        }
        let mut pixels = vec![0u8; (w * h) as usize];
        outlined.draw(|x, y, c| {
            if x < w && y < h {
                pixels[(y * w + x) as usize] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        let slot = self.atlas.insert(w, h, &pixels);
        Some(GlyphSlot {
            page: slot.page,
            rect: slot.rect,
            left: bounds.min.x - PAD as f32,
            top: bounds.min.y - PAD as f32,
        })
    }
}

/// The index into `SIZE_BUCKETS` for a text `px` pixels tall on screen.
pub fn size_bucket(px: f64) -> u8 {
    SIZE_BUCKETS
        .iter()
        .position(|&b| f64::from(b) >= px)
        .unwrap_or(SIZE_BUCKETS.len() - 1) as u8
}

/// TEXT control codes: `%%d` degree, `%%p` plus-minus, `%%c` diameter, `%%%` percent, `%%nnn`
/// the character with that code, `%%u`/`%%o`/`%%k` (underline, overline, strike) dropped, and
/// `\U+XXXX` the Unicode character.
pub fn decode_codes(value: &str) -> Cow<'_, str> {
    if !value.contains("%%") && !value.contains("\\U+") && !value.contains("\\u+") {
        return Cow::Borrowed(value);
    }
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < chars.len() {
        let rest = &chars[i..];
        if rest.len() >= 3 && rest[0] == '%' && rest[1] == '%' {
            let digits: String = rest[2..].iter().take(3).collect();
            match rest[2].to_ascii_lowercase() {
                'd' => out.push('°'),
                'p' => out.push('±'),
                'c' => out.push('Ø'),
                '%' => out.push('%'),
                'u' | 'o' | 'k' => {}
                _ if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()) => {
                    if let Some(c) = digits.parse().ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                    i += 5;
                    continue;
                }
                _ => {
                    out.push('%');
                    i += 1;
                    continue;
                }
            }
            i += 3;
        } else if rest.len() >= 7
            && rest[0] == '\\'
            && matches!(rest[1], 'U' | 'u')
            && rest[2] == '+'
        {
            let hex: String = rest[3..7].iter().collect();
            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                Some(c) => {
                    out.push(c);
                    i += 7;
                }
                None => {
                    out.push('\\');
                    i += 1;
                }
            }
        } else {
            out.push(rest[0]);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// Arabic script and its explicitly shared marks and signs.
fn is_arabic(c: char) -> bool {
    c.script() == Script::Arabic
        || c.script_extension()
            .iter()
            .any(|script| script == Script::Arabic)
}

fn is_rtl_char(c: char) -> bool {
    use unicode_bidi::BidiClass::{AL, AN, R, RLE, RLI, RLO};
    matches!(unicode_bidi::bidi_class(c), AL | AN | R | RLE | RLI | RLO)
}

fn is_letter(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

fn is_digit(c: char) -> bool {
    get_general_category(c) == GeneralCategory::DecimalNumber
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Isolated,
    Final,
    Initial,
    Medial,
}

/// The joining form of an Arabic Presentation Forms-B letter (U+FE80 to U+FEFC).
fn presentation_form(c: char) -> Option<Form> {
    // First code point of each letter and how many forms it has (isolated, final, initial,
    // medial, in that order).
    const LETTERS: [(u32, u32); 40] = [
        (0xFE80, 1),
        (0xFE81, 2),
        (0xFE83, 2),
        (0xFE85, 2),
        (0xFE87, 2),
        (0xFE89, 4),
        (0xFE8D, 2),
        (0xFE8F, 4),
        (0xFE93, 2),
        (0xFE95, 4),
        (0xFE99, 4),
        (0xFE9D, 4),
        (0xFEA1, 4),
        (0xFEA5, 4),
        (0xFEA9, 2),
        (0xFEAB, 2),
        (0xFEAD, 2),
        (0xFEAF, 2),
        (0xFEB1, 4),
        (0xFEB5, 4),
        (0xFEB9, 4),
        (0xFEBD, 4),
        (0xFEC1, 4),
        (0xFEC5, 4),
        (0xFEC9, 4),
        (0xFECD, 4),
        (0xFED1, 4),
        (0xFED5, 4),
        (0xFED9, 4),
        (0xFEDD, 4),
        (0xFEE1, 4),
        (0xFEE5, 4),
        (0xFEE9, 4),
        (0xFEED, 2),
        (0xFEEF, 2),
        (0xFEF1, 4),
        (0xFEF5, 2),
        (0xFEF7, 2),
        (0xFEF9, 2),
        (0xFEFB, 2),
    ];
    let code = c as u32;
    let (start, forms) = LETTERS
        .iter()
        .rev()
        .find(|(start, _)| *start <= code)
        .copied()?;
    let at = code.checked_sub(start)?;
    (at < forms).then(|| [Form::Isolated, Form::Final, Form::Initial, Form::Medial][at as usize])
}

/// Whether `text` holds its Arabic in visual order: typed left to right as it looks on the
/// page, as old CAD fonts that could not join or order Arabic needed it.
///
/// Presentation forms (ready-joined letters) decide by their joining: read in order, a letter
/// that joins forward (initial, medial) must be followed by one that joins back (medial, final),
/// a word starts with an isolated or initial form and ends with an isolated or final one. The
/// reading (as stored, or reversed) with fewer breaks of that wins. Text the forms can't decide
/// (no presentation forms, or both readings fit, as with words of non-joining letters only) is
/// visual only when the drawing says so (`force`, "Arabic text is reversed").
pub fn arabic_stored_visually(text: &str, force: bool) -> bool {
    if !text.chars().any(|c| is_arabic(c) && is_letter(c)) {
        return false;
    }
    presentation_order(text).unwrap_or(force)
}

/// `Some(true)` when the presentation forms of `text` fit a visual reading better than the
/// stored order, `Some(false)` when the stored order fits better, `None` when undecided.
fn presentation_order(text: &str) -> Option<bool> {
    let chars: Vec<char> = text.chars().collect();
    let (mut stored, mut reversed, mut any) = (0, 0, false);
    for word in chars.split(|&c| !is_arabic(c)) {
        let forms: Vec<Option<Form>> = word
            .iter()
            .filter(|&&c| is_letter(c))
            .map(|&c| presentation_form(c))
            .collect();
        if forms.iter().all(Option::is_none) {
            continue;
        }
        any = true;
        stored += join_breaks(forms.iter().copied());
        reversed += join_breaks(forms.iter().rev().copied());
    }
    (any && stored != reversed).then_some(reversed < stored)
}

/// How many times the joining forms of one word, in the order given, break the joining rules.
/// Letters of unknown form (not presentation forms) are not judged.
fn join_breaks(forms: impl Iterator<Item = Option<Form>>) -> usize {
    let forward = |f: Form| matches!(f, Form::Initial | Form::Medial);
    let back = |f: Form| matches!(f, Form::Medial | Form::Final);
    let forms: Vec<Option<Form>> = forms.collect();
    let mut breaks = 0;
    if let Some(Some(f)) = forms.first() {
        breaks += usize::from(back(*f));
    }
    if let Some(Some(f)) = forms.last() {
        breaks += usize::from(forward(*f));
    }
    for pair in forms.windows(2) {
        if let [Some(a), Some(b)] = pair {
            breaks += usize::from(forward(*a) != back(*b));
        }
    }
    breaks
}

/// One piece of a text stored in visual order: where it starts in the text (bytes), its
/// characters, and whether it is an Arabic word (then in reading order, to shape right to left).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualPiece {
    pub at: usize,
    pub text: String,
    pub rtl: bool,
}

/// `text`, stored in visual order, as pieces left to right on the page: each Arabic word
/// (letters, marks, signs; not digits) put back in reading order with every mark kept on its
/// letter, everything else as stored. Drawing the pieces in this order shows the text where its
/// author saw it.
pub fn visual_pieces(text: &str) -> Vec<VisualPiece> {
    let arabic_word = |c: char| is_arabic(c) && !is_digit(c);
    let mut pieces: Vec<VisualPiece> = Vec::new();
    for (at, cluster) in text.grapheme_indices(true) {
        let rtl = cluster.chars().any(arabic_word);
        match pieces.last_mut() {
            Some(p) if p.rtl == rtl => p.text.push_str(cluster),
            _ => pieces.push(VisualPiece {
                at,
                text: cluster.to_string(),
                rtl,
            }),
        }
    }
    for piece in pieces.iter_mut().filter(|p| p.rtl) {
        piece.text = piece.text.graphemes(true).rev().collect();
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{DEFAULT_CHAIN, NOTO_ARABIC, NOTO_SANS};

    fn ids(line: &ShapedLine) -> Vec<(u16, u16)> {
        line.glyphs.iter().map(|g| (g.font, g.glyph)).collect()
    }

    /// The glyphs of `line` that are not marks (Noto Sans Arabic draws the dots of qaf and yeh
    /// as separate mark glyphs).
    fn bases(engine: &TextEngine, line: &ShapedLine) -> Vec<ShapedGlyph> {
        line.glyphs
            .iter()
            .filter(|g| {
                let gdef = engine.fonts.get(g.font).face.tables().gdef;
                gdef.and_then(|t| t.glyph_class(rustybuzz::ttf_parser::GlyphId(g.glyph)))
                    != Some(rustybuzz::ttf_parser::gdef::GlyphClass::Mark)
            })
            .copied()
            .collect()
    }

    #[test]
    fn qiyas_shapes_to_four_joined_arabic_glyphs() {
        let mut engine = TextEngine::bundled_only();
        let word = engine.shape_line(&DEFAULT_CHAIN, "قياس", false);
        assert!(word.glyphs.iter().all(|g| g.font == NOTO_ARABIC));
        let letters = bases(&engine, &word);
        assert_eq!(letters.len(), 4);
        let mut clusters: Vec<u32> = word.glyphs.iter().map(|g| g.cluster).collect();
        clusters.dedup();
        assert_eq!(clusters.len(), 4);
        // Visual order, left to right: س ا ي ق. Qaf starts the word (initial), yeh sits in it
        // (medial), alef ends a join (final); seen follows alef, which never joins forward, so
        // it stands alone (isolated).
        let isolated = |c: &str, engine: &mut TextEngine| {
            let line = engine.shape_line(&DEFAULT_CHAIN, c, false);
            bases(engine, &line)[0].glyph
        };
        let [seen, alef, yeh, qaf] = ["س", "ا", "ي", "ق"].map(|c| isolated(c, &mut engine));
        let got: Vec<u16> = letters.iter().map(|g| g.glyph).collect();
        assert_eq!(got[0], seen);
        assert_ne!(got[1], alef);
        assert_ne!(got[2], yeh);
        assert_ne!(got[3], qaf);
        // Left to right on the page: seen at the left end, qaf at the right.
        assert!(letters.windows(2).all(|w| w[0].x <= w[1].x));
        assert!(word.width > 0.0);
    }

    /// Visual order: every glyph starts at or right of the one before.
    fn left_to_right(line: &ShapedLine) -> bool {
        let pens: Vec<f32> = line.glyphs.iter().map(|g| g.x).collect();
        // Marks sit over their letter, so allow them back a little.
        pens.windows(2).all(|w| w[1] >= w[0] - 0.6)
    }

    #[test]
    fn reversed_arabic_draws_the_same_glyphs_once_the_drawing_says_so() {
        let mut engine = TextEngine::bundled_only();
        let right = engine.shape_line(&DEFAULT_CHAIN, "قياس", false);
        let stored_backwards: String = "قياس".chars().rev().collect();
        let as_is = engine.shape_line(&DEFAULT_CHAIN, &stored_backwards, false);
        assert_ne!(ids(&as_is), ids(&right));
        let fixed = engine.shape_line(&DEFAULT_CHAIN, &stored_backwards, true);
        assert_eq!(ids(&fixed), ids(&right));
        let xs = |l: &ShapedLine| l.glyphs.iter().map(|g| g.x).collect::<Vec<_>>();
        assert_eq!(xs(&fixed), xs(&right));
        // Text in reading order is left alone without the toggle.
        assert!(!arabic_stored_visually("قياس", false));
    }

    /// `(font, glyph)` of the glyphs that draw something (spaces left out: which font a space
    /// comes from doesn't show).
    fn ink(engine: &TextEngine, lines: &[ShapedLine]) -> Vec<(u16, u16)> {
        let space = |f: u16| engine.fonts.get(f).face.glyph_index(' ').map(|g| g.0);
        lines
            .iter()
            .flat_map(|l| l.glyphs.iter())
            .filter(|g| space(g.font) != Some(g.glyph))
            .map(|g| (g.font, g.glyph))
            .collect()
    }

    #[test]
    fn reversed_mixed_text_keeps_every_run_where_its_author_saw_it() {
        let mut engine = TextEngine::bundled_only();
        let mut check = |stored: &str, as_seen: &[&str]| {
            let line = engine.shape_line(&DEFAULT_CHAIN, stored, true);
            let parts: Vec<ShapedLine> = as_seen
                .iter()
                .map(|t| engine.shape_line(&DEFAULT_CHAIN, t, false))
                .collect();
            assert_eq!(
                ink(&engine, std::slice::from_ref(&line)),
                ink(&engine, &parts),
                "{stored}"
            );
            assert!(left_to_right(&line), "{stored}");
            line
        };
        // "غرفة D1" seen left to right as ةفرغ D1: the room word on the left, D1 on the right.
        let line = check("ةفرغ D1", &["غرفة", "D1"]);
        let last = line.glyphs.last().unwrap();
        assert_eq!(last.font, NOTO_SANS);
        assert!(line
            .glyphs
            .iter()
            .filter(|g| g.font == NOTO_ARABIC)
            .all(|g| g.x < last.x));
        // Two Arabic words around a Latin one keep their places.
        check("ريبك D1 ةفرغ", &["كبير", "D1", "غرفة"]);
        // A number on the left stays on the left; digits read as typed.
        check("12 ةفرغ", &["12", "غرفة"]);
        check("12.5 ةفرغ", &["12.5", "غرفة"]);
        // Two Arabic words, stored visually, draw as the phrase in reading order does.
        check("ةشيعملا ةفرغ", &["غرفة المعيشة"]);
    }

    #[test]
    fn marks_stay_on_their_letter_when_a_word_is_put_back() {
        // "مَطبخ" with a fatha on meem, stored visually: خ ب ط م + fatha (typed after its letter).
        let pieces = visual_pieces("خبطمَ");
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].text, "مَطبخ");
        assert!(pieces[0].rtl);
        let mut engine = TextEngine::bundled_only();
        let fixed = engine.shape_line(&DEFAULT_CHAIN, "خبطمَ", true);
        let right = engine.shape_line(&DEFAULT_CHAIN, "مَطبخ", false);
        assert_eq!(ids(&fixed), ids(&right));
    }

    #[test]
    fn presentation_forms_tell_their_order_by_how_they_join() {
        // "قياس" in reading order as presentation forms: qaf initial, yeh medial, alef final,
        // seen isolated.
        let reading = "\u{FED7}\u{FEF4}\u{FE8E}\u{FEB1}";
        let visual: String = reading.chars().rev().collect();
        assert_eq!(presentation_form('\u{FEB1}'), Some(Form::Isolated));
        assert_eq!(presentation_form('\u{FE8E}'), Some(Form::Final));
        assert_eq!(presentation_form('\u{FED7}'), Some(Form::Initial));
        assert_eq!(presentation_form('\u{FEF4}'), Some(Form::Medial));
        assert!(arabic_stored_visually(&visual, false));
        assert!(!arabic_stored_visually(reading, false));
        // Even with the toggle, joining that reads right stays as stored.
        assert!(!arabic_stored_visually(reading, true));
        let mut engine = TextEngine::bundled_only();
        let fixed = engine.shape_line(&DEFAULT_CHAIN, &visual, false);
        let right = engine.shape_line(&DEFAULT_CHAIN, reading, false);
        assert_eq!(ids(&fixed), ids(&right));
        assert!(fixed.glyphs.iter().all(|g| g.font == NOTO_ARABIC));
        // Drawn joined: not the isolated forms of its letters.
        let isolated = engine.shape_line(&DEFAULT_CHAIN, "\u{FED5}\u{FEEF}\u{FE8D}\u{FEB1}", false);
        assert_ne!(ids(&fixed), ids(&isolated));
        // "دور" (dal, waw, reh never join): every form isolated, both readings fit; as stored.
        let dor = "\u{FEA9}\u{FEED}\u{FEAD}";
        assert!(!arabic_stored_visually(dor, false));
        // The drawing's toggle decides what the forms can't.
        assert!(arabic_stored_visually(dor, true));
        // A word in base letters is reversed only by the toggle.
        assert!(!arabic_stored_visually("سايق", false));
        assert!(arabic_stored_visually("سايق", true));
    }

    #[test]
    fn mixed_text_comes_out_in_visual_order() {
        let mut engine = TextEngine::bundled_only();
        let line = engine.shape_line(&DEFAULT_CHAIN, "Door D1 باب", false);
        let latin = engine.shape_line(&DEFAULT_CHAIN, "Door D1 ", false);
        let arabic = engine.shape_line(&DEFAULT_CHAIN, "باب", false);
        // A left-to-right paragraph: the Latin run first, then the Arabic word after it.
        assert_eq!(line.glyphs.len(), latin.glyphs.len() + arabic.glyphs.len());
        let (head, tail) = line.glyphs.split_at(latin.glyphs.len());
        assert!(head.iter().all(|g| g.font == NOTO_SANS));
        assert!(tail.iter().all(|g| g.font == NOTO_ARABIC));
        let tail_ids: Vec<(u16, u16)> = tail.iter().map(|g| (g.font, g.glyph)).collect();
        assert_eq!(tail_ids, ids(&arabic));
        assert!(tail[0].x >= latin.width - 1e-4);
        // An Arabic paragraph with a Latin word: the Arabic first word sits on the right.
        let rtl = engine.shape_line(&DEFAULT_CHAIN, "باب D1", false);
        assert_eq!(rtl.glyphs[0].font, NOTO_SANS);
        assert_eq!(rtl.glyphs.last().unwrap().font, NOTO_ARABIC);
    }

    #[test]
    fn codes_decode() {
        assert_eq!(decode_codes("45%%d"), "45°");
        assert_eq!(decode_codes("%%c100 %%p5 50%%%"), "Ø100 ±5 50%");
        assert_eq!(decode_codes("%%uA%%u"), "A");
        assert_eq!(decode_codes("%%065"), "A");
        assert_eq!(decode_codes("\\U+0642"), "ق");
        assert_eq!(decode_codes("plain"), "plain");
        assert!(matches!(decode_codes("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn size_buckets_round_up() {
        assert_eq!(size_bucket(0.1), 0);
        assert_eq!(size_bucket(8.0), 0);
        assert_eq!(size_bucket(9.0), 1);
        assert_eq!(size_bucket(100.0), 8);
        assert_eq!(size_bucket(1000.0), 8);
    }
}
