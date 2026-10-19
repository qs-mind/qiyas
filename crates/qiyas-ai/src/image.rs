//! A picture of the drawing for the model to look at, drawn as the engineer sees it on white:
//! model space inside a layout's viewports, pictures (raster images and PDF underlays), then
//! fills, lines and text in their colours; over them the take-off and highlighted entities in
//! violet, a region the user marked dashed blue, and numbered marks on entities.

use std::collections::{HashMap, HashSet};

use qiyas_core::{Kind, Measurement, PaperView};
use qiyas_render::{Batch, FILL_BACKGROUND, PAGE_SIZE, TEXT_BOX};
use tiny_skia::{
    Color, FillRule, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash,
    Transform,
};

use crate::marks::{self, Mark};

/// Largest picture side, in pixels.
pub(crate) const MAX_SIDE: u32 = 1600;
/// The take-off overlay and highlighted entities: Qiyas violet `#6E4A9E`.
const VIOLET: [u8; 3] = [0x6E, 0x4A, 0x9E];
/// A region the user marked.
const REGION: [u8; 3] = [0x1C, 0x7E, 0xD6];
/// Radius of the dot marking a counted point or a one-point markup, in pixels.
const DOT: f32 = 4.0;

/// A picture drawn under the lines: its pixels (premultiplied) and where its corners go in
/// drawing coordinates (top left, top right, bottom right, bottom left).
pub(crate) struct Raster {
    pub corners: [[f64; 2]; 4],
    pub pixmap: Pixmap,
}

impl Raster {
    /// `rgba` (straight alpha, rows from the top) as a raster; `None` when the sizes disagree.
    pub fn from_rgba(
        corners: [[f64; 2]; 4],
        width: u32,
        height: u32,
        mut rgba: Vec<u8>,
    ) -> Option<Raster> {
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            if a < 255 {
                for c in &mut px[..3] {
                    *c = (u16::from(*c) * a / 255) as u8;
                }
            }
        }
        let size = tiny_skia::IntSize::from_wh(width, height)?;
        Some(Raster {
            corners,
            pixmap: Pixmap::from_vec(rgba, size)?,
        })
    }
}

/// Model space drawn inside one of a layout's viewports.
pub(crate) struct Inset<'a> {
    pub batch: &'a Batch,
    pub pages: &'a [Vec<u8>],
    pub view: &'a PaperView,
}

/// What a picture shows.
pub(crate) struct Scene<'a> {
    /// The sheet's own drawing, and the glyph atlas pages its quads draw from.
    pub batch: &'a Batch,
    pub pages: &'a [Vec<u8>],
    pub rasters: &'a [Raster],
    pub insets: &'a [Inset<'a>],
    pub measurements: &'a [Measurement],
    /// Entities at the top of the sheet drawn thick violet, and marked first.
    pub highlight: &'a [u64],
    /// A region the user marked, drawing coordinates; empty for none.
    pub region: &'a [[f64; 2]],
    /// Most entities to mark with numbers; 0 for none.
    pub marks: usize,
}

/// A drawn picture.
pub(crate) struct Drawn {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub marks: Vec<Mark>,
    pub text: Option<TextSize>,
}

/// How tall the text in a picture is, in pixels (the tallest glyph of each text).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextSize {
    /// A tenth of the texts are smaller.
    pub smallest: f32,
    /// Half of them are smaller.
    pub typical: f32,
}

/// Drawing coordinates `bounds` onto a `width` x `height` picture, y down.
#[derive(Debug, Clone, Copy)]
pub(crate) struct View {
    lo: [f64; 2],
    hi: [f64; 2],
    sx: f64,
    sy: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    /// `bounds` on a picture `width` pixels wide (1 to 1600), its height following their
    /// aspect (1 to 1600).
    pub fn new(bounds: [[f64; 2]; 2], width: u32) -> View {
        let width = width.clamp(1, MAX_SIDE);
        let [lo, hi] = normalized(bounds);
        let height = height_for(hi[0] - lo[0], hi[1] - lo[1], width);
        View::sized([lo, hi], width, height)
    }

    /// `bounds` stretched onto `width` x `height` pixels (a picture of the screen).
    pub fn sized(bounds: [[f64; 2]; 2], width: u32, height: u32) -> View {
        let [lo, hi] = normalized(bounds);
        let dx = (hi[0] - lo[0]).max(f64::MIN_POSITIVE);
        let dy = (hi[1] - lo[1]).max(f64::MIN_POSITIVE);
        View {
            lo,
            hi,
            sx: f64::from(width) / dx,
            sy: f64::from(height) / dy,
            width,
            height,
        }
    }

    /// Where the drawing point (`x`, `y`) falls on the picture.
    pub fn px(&self, x: f64, y: f64) -> (f32, f32) {
        (
            ((x - self.lo[0]) * self.sx) as f32,
            ((self.hi[1] - y) * self.sy) as f32,
        )
    }

    /// The drawing point under the pixel corner (`px`, `py`).
    fn point(&self, px: f64, py: f64) -> [f64; 2] {
        [self.lo[0] + px / self.sx, self.hi[1] - py / self.sy]
    }

    /// Pixels per drawing unit across.
    pub fn px_per_unit(&self) -> f64 {
        self.sx
    }
}

/// Renders `bounds` (`[[min_x, min_y], [max_x, max_y]]`, drawing coordinates) of `batch` as a
/// PNG `width` pixels wide (1 to 1600), its height following the bounds' aspect (1 to 1600).
/// White background; each segment 1 px in its colour, white drawn black; then every
/// measurement in violet, 2 px: its points as a ring or polyline (dots for counts), or, with no
/// points, the segments of its handles.
pub fn render_png(
    batch: &Batch,
    measurements: &[Measurement],
    bounds: [[f64; 2]; 2],
    width: u32,
) -> Vec<u8> {
    render_png_with_text(batch, &[], measurements, bounds, width)
}

/// `render_png` with the drawing's text as it reads on screen: `pages` are the glyph atlas pages
/// the batch's quads draw from (`Session::picture_batch`). Solid fills (hatches, solids,
/// wipeouts) are drawn under the lines; with glyphs, the text's outline boxes are left out.
pub fn render_png_with_text(
    batch: &Batch,
    pages: &[Vec<u8>],
    measurements: &[Measurement],
    bounds: [[f64; 2]; 2],
    width: u32,
) -> Vec<u8> {
    draw(
        &Scene {
            batch,
            pages,
            rasters: &[],
            insets: &[],
            measurements,
            highlight: &[],
            region: &[],
            marks: 0,
        },
        View::new(bounds, width),
    )
    .png
}

/// Draws `scene` onto a picture of `view`.
pub(crate) fn draw(scene: &Scene, view: View) -> Drawn {
    let mut pixmap = Pixmap::new(view.width, view.height).expect("1 to 1600 px each way");
    pixmap.fill(Color::WHITE);
    for inset in scene.insets {
        draw_inset(&mut pixmap, &view, inset);
    }
    for raster in scene.rasters {
        draw_raster(&mut pixmap, &view, raster);
    }
    paint(&mut pixmap, &view, scene.batch, scene.pages, None);
    highlight(&mut pixmap, &view, scene.batch, scene.highlight);
    overlay(&mut pixmap, &view, scene.batch, scene.measurements);
    region(&mut pixmap, &view, scene.region);
    let marks = if scene.marks > 0 {
        let chosen = marks::choose(scene.batch, &view, scene.highlight, scene.marks);
        marks::draw(&mut pixmap, &chosen);
        chosen
    } else {
        Vec::new()
    };
    Drawn {
        png: pixmap
            .encode_png()
            .expect("a pixmap in memory always encodes"),
        width: view.width,
        height: view.height,
        marks,
        text: text_size(scene.batch, &view),
    }
}

/// A picture of the screen (`png`, showing `bounds` of the sheet `batch` draws) with up to `max`
/// marks on it, those in `first` first: the picture, its marks, and how tall its text is.
pub(crate) fn mark_screen(
    png: &[u8],
    batch: &Batch,
    bounds: [[f64; 2]; 2],
    first: &[u64],
    max: usize,
) -> Result<Drawn, String> {
    let mut pixmap =
        Pixmap::decode_png(png).map_err(|e| format!("The window sent no picture: {e}"))?;
    let view = View::sized(bounds, pixmap.width(), pixmap.height());
    let marks = marks::choose(batch, &view, first, max);
    marks::draw(&mut pixmap, &marks);
    let png = pixmap
        .encode_png()
        .map_err(|e| format!("The picture could not be made: {e}"))?;
    Ok(Drawn {
        png,
        width: view.width,
        height: view.height,
        marks,
        text: text_size(batch, &view),
    })
}

/// The height of a picture `width` pixels wide of a `dx` by `dy` box: 1 to 1600 pixels.
pub(crate) fn height_for(dx: f64, dy: f64, width: u32) -> u32 {
    let height = (f64::from(width) * dy / dx).round();
    if height.is_finite() {
        (height as u32).clamp(1, MAX_SIDE)
    } else {
        MAX_SIDE
    }
}

/// The box with its corners in order, whichever way round they were given.
pub(crate) fn normalized([a, b]: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [
        [a[0].min(b[0]), a[1].min(b[1])],
        [a[0].max(b[0]), a[1].max(b[1])],
    ]
}

/// The segment `i` of `batch` in drawing coordinates.
pub(crate) fn segment(batch: &Batch, i: usize) -> [f64; 4] {
    let s = &batch.segments[i * 4..i * 4 + 4];
    let [ox, oy] = batch.origin;
    [
        f64::from(s[0]) + ox,
        f64::from(s[1]) + oy,
        f64::from(s[2]) + ox,
        f64::from(s[3]) + oy,
    ]
}

/// Which items `spans` (`[first, count]` each) take, of `len`; `None` takes them all.
fn allowed(spans: Option<&Vec<[u32; 2]>>, len: usize) -> Option<Vec<bool>> {
    let spans = spans?;
    let mut keep = vec![false; len];
    for &[first, count] in spans {
        for k in first as usize..(first + count) as usize {
            if let Some(slot) = keep.get_mut(k) {
                *slot = true;
            }
        }
    }
    Some(keep)
}

/// Fills, lines and text of `batch` on `pixmap`; with `view_of`, only what that viewport draws
/// (its spans).
fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    batch: &Batch,
    pages: &[Vec<u8>],
    view_of: Option<&PaperView>,
) {
    let (lo, hi) = (
        view.point(0.0, f64::from(view.height)),
        view.point(f64::from(view.width), 0.0),
    );
    let outside = |s: [f64; 4]| {
        s[0].max(s[2]) < lo[0]
            || s[0].min(s[2]) > hi[0]
            || s[1].max(s[3]) < lo[1]
            || s[1].min(s[3]) > hi[1]
    };
    let fills_kept = allowed(
        view_of.and_then(|v| v.fills.as_ref()),
        batch.fill_colors.len(),
    );
    let lines_kept = allowed(view_of.and_then(|v| v.lines.as_ref()), batch.colors.len());
    let quads_kept = allowed(view_of.and_then(|v| v.texts.as_ref()), batch.quads.len());

    // Fills first, one path per colour; a wipeout's background is the white.
    let [ox, oy] = batch.origin;
    let mut fills: HashMap<u32, PathBuilder> = HashMap::new();
    for (t, rgb) in batch.fill_colors.iter().enumerate() {
        if fills_kept.as_ref().is_some_and(|k| !k[t]) {
            continue;
        }
        let Some(tri) = batch.fills.get(t * 6..t * 6 + 6) else {
            break;
        };
        let rgb = if *rgb & FILL_BACKGROUND != 0 {
            0xFF_FFFF
        } else if *rgb == 0xFF_FFFF {
            0
        } else {
            *rgb
        };
        let path = fills.entry(rgb).or_default();
        let (x0, y0) = view.px(f64::from(tri[0]) + ox, f64::from(tri[1]) + oy);
        path.move_to(x0, y0);
        for k in 1..3 {
            let (x, y) = view.px(f64::from(tri[2 * k]) + ox, f64::from(tri[2 * k + 1]) + oy);
            path.line_to(x, y);
        }
        path.close();
    }
    for (rgb, path) in fills {
        let [_, r, g, b] = rgb.to_be_bytes();
        fill(pixmap, path, [r, g, b]);
    }

    // The text's outline boxes are what the window draws while its glyphs load: with the
    // glyphs here, they would only box every word.
    let mut boxed = vec![false; batch.colors.len()];
    if !pages.is_empty() {
        for r in batch.ranges.iter().filter(|r| r.flags & TEXT_BOX != 0) {
            for i in r.first as usize..(r.first + r.count) as usize {
                if let Some(b) = boxed.get_mut(i) {
                    *b = true;
                }
            }
        }
    }

    // One path per colour, so a big drawing is a handful of strokes.
    let mut by_color: HashMap<u32, PathBuilder> = HashMap::new();
    for (i, rgb) in batch.colors.iter().enumerate() {
        if boxed[i] || lines_kept.as_ref().is_some_and(|k| !k[i]) {
            continue;
        }
        let s = segment(batch, i);
        if outside(s) {
            continue;
        }
        let rgb = if *rgb == 0xFF_FFFF { 0 } else { *rgb };
        let path = by_color.entry(rgb).or_default();
        let (x0, y0) = view.px(s[0], s[1]);
        let (x1, y1) = view.px(s[2], s[3]);
        path.move_to(x0, y0);
        path.line_to(x1, y1);
    }
    for (rgb, path) in by_color {
        let [_, r, g, b] = rgb.to_be_bytes();
        stroke(pixmap, path, [r, g, b], 1.0);
    }
    if !pages.is_empty() {
        glyphs(pixmap, view, batch, pages, quads_kept.as_deref());
    }
}

/// Model space in `inset`'s viewport, clipped to the part of its frame in the picture.
fn draw_inset(pixmap: &mut Pixmap, view: &View, inset: &Inset) {
    let v = inset.view;
    if !(v.scale > 0.0 && v.scale.is_finite()) {
        return;
    }
    // The frame in whole pixels, within the picture.
    let (fx0, fy0) = view.px(v.rect[0][0], v.rect[1][1]);
    let (fx1, fy1) = view.px(v.rect[1][0], v.rect[0][1]);
    let x0 = fx0.floor().max(0.0);
    let y0 = fy0.floor().max(0.0);
    let x1 = fx1.ceil().min(view.width as f32);
    let y1 = fy1.ceil().min(view.height as f32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let Some(mut sub) = Pixmap::new(w, h) else {
        return;
    };
    // The layout point at the frame's centre shows the model point `centre`.
    let frame = [
        (v.rect[0][0] + v.rect[1][0]) / 2.0,
        (v.rect[0][1] + v.rect[1][1]) / 2.0,
    ];
    let to_model = |[x, y]: [f64; 2]| {
        [
            v.centre[0] + (x - frame[0]) / v.scale,
            v.centre[1] + (y - frame[1]) / v.scale,
        ]
    };
    let lo = to_model(view.point(f64::from(x0), f64::from(y1)));
    let hi = to_model(view.point(f64::from(x1), f64::from(y0)));
    let model = View::sized([lo, hi], w, h);
    paint(&mut sub, &model, inset.batch, inset.pages, Some(v));
    pixmap.draw_pixmap(
        x0 as i32,
        y0 as i32,
        sub.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// A picture under the lines, its pixels mapped onto its corners.
fn draw_raster(pixmap: &mut Pixmap, view: &View, raster: &Raster) {
    let [tl, tr, _, bl] = raster.corners.map(|p| view.px(p[0], p[1]));
    let (w, h) = (raster.pixmap.width() as f32, raster.pixmap.height() as f32);
    let to_picture = Transform::from_row(
        (tr.0 - tl.0) / w,
        (tr.1 - tl.1) / w,
        (bl.0 - tl.0) / h,
        (bl.1 - tl.1) / h,
        tl.0,
        tl.1,
    );
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    pixmap.draw_pixmap(0, 0, raster.pixmap.as_ref(), &paint, to_picture, None);
}

/// The entities at the top of the sheet in `handles`, thick violet.
fn highlight(pixmap: &mut Pixmap, view: &View, batch: &Batch, handles: &[u64]) {
    if handles.is_empty() {
        return;
    }
    let wanted: HashSet<u64> = handles.iter().copied().collect();
    let mut path = PathBuilder::new();
    for r in batch.ranges.iter().filter(|r| wanted.contains(&r.top)) {
        for i in r.first as usize..(r.first + r.count) as usize {
            let s = segment(batch, i);
            let (x0, y0) = view.px(s[0], s[1]);
            let (x1, y1) = view.px(s[2], s[3]);
            path.move_to(x0, y0);
            path.line_to(x1, y1);
        }
    }
    stroke(pixmap, path, VIOLET, 3.0);
}

/// Every measurement in violet, 2 px: its points as a ring or polyline (dots for counts), or,
/// with no points, the segments of its handles.
fn overlay(pixmap: &mut Pixmap, view: &View, batch: &Batch, measurements: &[Measurement]) {
    let mut overlay = PathBuilder::new();
    for m in measurements {
        let points: Vec<(f32, f32)> = m.points.iter().map(|p| view.px(p[0], p[1])).collect();
        match points.as_slice() {
            [] => {
                // An entity on the sheet by its top handle; one inside blocks by handle and chain.
                let drawn = batch.ranges.iter().filter(|r| {
                    m.handles.iter().any(|&h| {
                        if m.chain.is_empty() {
                            r.top == h
                        } else {
                            r.entity == h && batch.chain_of(r) == m.chain.as_slice()
                        }
                    })
                });
                for r in drawn {
                    for i in r.first as usize..(r.first + r.count) as usize {
                        let s = segment(batch, i);
                        let (x0, y0) = view.px(s[0], s[1]);
                        let (x1, y1) = view.px(s[2], s[3]);
                        overlay.move_to(x0, y0);
                        overlay.line_to(x1, y1);
                    }
                }
            }
            [only] => overlay.push_circle(only.0, only.1, DOT),
            _ if m.kind == Kind::Count => {
                for (x, y) in &points {
                    overlay.push_circle(*x, *y, DOT);
                }
            }
            [first, rest @ ..] => {
                overlay.move_to(first.0, first.1);
                for (x, y) in rest {
                    overlay.line_to(*x, *y);
                }
                if matches!(m.kind, Kind::Area | Kind::Volume) || m.unit == "markup" {
                    overlay.close();
                }
            }
        }
    }
    stroke(pixmap, overlay, VIOLET, 2.0);
}

/// The region the user marked, dashed.
fn region(pixmap: &mut Pixmap, view: &View, points: &[[f64; 2]]) {
    let [first, rest @ ..] = points else {
        return;
    };
    let mut path = PathBuilder::new();
    let (x, y) = view.px(first[0], first[1]);
    path.move_to(x, y);
    for p in rest {
        let (x, y) = view.px(p[0], p[1]);
        path.line_to(x, y);
    }
    path.close();
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    let [r, g, b] = REGION;
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: 2.0,
        dash: StrokeDash::new(vec![8.0, 5.0], 0.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// How tall the texts drawn in the picture are: each text's tallest glyph, in pixels; `None`
/// when no text is in view.
fn text_size(batch: &Batch, view: &View) -> Option<TextSize> {
    let [ox, oy] = batch.origin;
    let (w, h) = (view.width as f32, view.height as f32);
    let mut sizes: Vec<f32> = batch
        .text_ranges
        .iter()
        .filter_map(|r| {
            let quads = batch
                .quads
                .get(r.first as usize..(r.first + r.count) as usize)?;
            let mut tallest: Option<f32> = None;
            for q in quads {
                let c = |k: usize| {
                    view.px(
                        f64::from(q.corners[2 * k]) + ox,
                        f64::from(q.corners[2 * k + 1]) + oy,
                    )
                };
                let (top, bottom) = (c(0), c(3));
                let mid = ((top.0 + bottom.0) / 2.0, (top.1 + bottom.1) / 2.0);
                if !(0.0..=w).contains(&mid.0) || !(0.0..=h).contains(&mid.1) {
                    continue;
                }
                let tall = (bottom.0 - top.0).hypot(bottom.1 - top.1);
                tallest = Some(tallest.map_or(tall, |t| t.max(tall)));
            }
            tallest
        })
        .collect();
    if sizes.is_empty() {
        return None;
    }
    sizes.sort_by(f32::total_cmp);
    Some(TextSize {
        smallest: sizes[sizes.len() / 10],
        typical: sizes[sizes.len() / 2],
    })
}

fn fill(pixmap: &mut Pixmap, path: PathBuilder, [r, g, b]: [u8; 3]) {
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::EvenOdd,
        Transform::identity(),
        None,
    );
}

/// Draws the batch's glyph quads from the atlas `pages` (R8 coverage, `PAGE_SIZE` square): each
/// quad is a turned rectangle, corners top left, top right, bottom right, bottom left, its
/// texels `uv` `u0 v0 u1 v1`; every pixel inside it takes the coverage of the texel under it.
/// With `kept`, only the quads it marks.
fn glyphs(
    pixmap: &mut Pixmap,
    view: &View,
    batch: &Batch,
    pages: &[Vec<u8>],
    kept: Option<&[bool]>,
) {
    let (w, h) = (pixmap.width() as i64, pixmap.height() as i64);
    let side = PAGE_SIZE as usize;
    let [ox, oy] = batch.origin;
    let data = pixmap.data_mut();
    for (n, q) in batch.quads.iter().enumerate() {
        if kept.is_some_and(|k| !k[n]) {
            continue;
        }
        let Some(page) = pages.get(usize::from(q.page)) else {
            continue;
        };
        let c: Vec<(f32, f32)> = (0..4)
            .map(|k| {
                view.px(
                    f64::from(q.corners[2 * k]) + ox,
                    f64::from(q.corners[2 * k + 1]) + oy,
                )
            })
            .collect();
        let (e1, e2) = (
            (c[1].0 - c[0].0, c[1].1 - c[0].1),
            (c[3].0 - c[0].0, c[3].1 - c[0].1),
        );
        let det = e1.0 * e2.1 - e1.1 * e2.0;
        if det.abs() < 1e-6 {
            continue;
        }
        let xs = c.iter().map(|p| p.0);
        let ys = c.iter().map(|p| p.1);
        let (x0, x1) = (
            xs.clone().fold(f32::MAX, f32::min),
            xs.fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f32::MAX, f32::min),
            ys.fold(f32::MIN, f32::max),
        );
        let [u0, v0, u1, v1] = q.uv.map(f32::from);
        let [_, r, g, b] = q.color.to_be_bytes();
        let [r, g, b] = if [r, g, b] == [255, 255, 255] {
            [0, 0, 0]
        } else {
            [r, g, b]
        };
        for py in (y0.floor() as i64).max(0)..=(y1.ceil() as i64).min(h - 1) {
            for px in (x0.floor() as i64).max(0)..=(x1.ceil() as i64).min(w - 1) {
                // Where the pixel's centre falls in the quad, 0 to 1 along each side.
                let (dx, dy) = (px as f32 + 0.5 - c[0].0, py as f32 + 0.5 - c[0].1);
                let s = (dx * e2.1 - dy * e2.0) / det;
                let t = (e1.0 * dy - e1.1 * dx) / det;
                if !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&t) {
                    continue;
                }
                let u = (u0 + s * (u1 - u0)) as usize;
                let v = (v0 + t * (v1 - v0)) as usize;
                let Some(&cover) = page.get(v.min(side - 1) * side + u.min(side - 1)) else {
                    continue;
                };
                if cover == 0 {
                    continue;
                }
                let a = u32::from(cover);
                let at = ((py * w + px) * 4) as usize;
                for (k, channel) in [r, g, b].into_iter().enumerate() {
                    let under = u32::from(data[at + k]);
                    data[at + k] = ((u32::from(channel) * a + under * (255 - a)) / 255) as u8;
                }
                // An inset starts transparent: what a glyph covers becomes opaque.
                let under = u32::from(data[at + 3]);
                data[at + 3] = (a + under * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}

pub(crate) fn stroke(pixmap: &mut Pixmap, path: PathBuilder, [r, g, b]: [u8; 3], width: f32) {
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}
