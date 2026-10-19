//! A picture of the drawing for the model to look at: the sheet's fills, lines and text on
//! white, with the take-off drawn over them in violet.

use std::collections::HashMap;

use qiyas_core::{Kind, Measurement};
use qiyas_render::{Batch, FILL_BACKGROUND, PAGE_SIZE, TEXT_BOX};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Largest picture side, in pixels.
pub(crate) const MAX_SIDE: u32 = 1600;
/// The take-off overlay: Qiyas violet `#6E4A9E`.
const VIOLET: [u8; 3] = [0x6E, 0x4A, 0x9E];
/// Radius of the dot marking a counted point or a one-point markup, in pixels.
const DOT: f32 = 4.0;

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
    let width = width.clamp(1, MAX_SIDE);
    let [lo, hi] = normalized(bounds);
    let dx = (hi[0] - lo[0]).max(f64::MIN_POSITIVE);
    let dy = (hi[1] - lo[1]).max(f64::MIN_POSITIVE);
    let height = height_for(dx, dy, width);
    let (sx, sy) = (f64::from(width) / dx, f64::from(height) / dy);
    // Pixels have y down; the drawing has y up.
    let to_px = |x: f64, y: f64| (((x - lo[0]) * sx) as f32, ((hi[1] - y) * sy) as f32);
    let segment = |i: usize| {
        let s = &batch.segments[i * 4..i * 4 + 4];
        let [ox, oy] = batch.origin;
        [
            f64::from(s[0]) + ox,
            f64::from(s[1]) + oy,
            f64::from(s[2]) + ox,
            f64::from(s[3]) + oy,
        ]
    };
    let outside = |s: [f64; 4]| {
        s[0].max(s[2]) < lo[0]
            || s[0].min(s[2]) > hi[0]
            || s[1].max(s[3]) < lo[1]
            || s[1].min(s[3]) > hi[1]
    };

    let mut pixmap = Pixmap::new(width, height).expect("1 to 1600 px each way");
    pixmap.fill(Color::WHITE);

    // Fills first, one path per colour; a wipeout's background is the white.
    let [ox, oy] = batch.origin;
    let mut fills: HashMap<u32, PathBuilder> = HashMap::new();
    for (t, rgb) in batch.fill_colors.iter().enumerate() {
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
        let (x0, y0) = to_px(f64::from(tri[0]) + ox, f64::from(tri[1]) + oy);
        path.move_to(x0, y0);
        for k in 1..3 {
            let (x, y) = to_px(f64::from(tri[2 * k]) + ox, f64::from(tri[2 * k + 1]) + oy);
            path.line_to(x, y);
        }
        path.close();
    }
    for (rgb, path) in fills {
        let [_, r, g, b] = rgb.to_be_bytes();
        fill(&mut pixmap, path, [r, g, b]);
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
        let s = segment(i);
        if outside(s) || boxed[i] {
            continue;
        }
        let rgb = if *rgb == 0xFF_FFFF { 0 } else { *rgb };
        let path = by_color.entry(rgb).or_default();
        let (x0, y0) = to_px(s[0], s[1]);
        let (x1, y1) = to_px(s[2], s[3]);
        path.move_to(x0, y0);
        path.line_to(x1, y1);
    }
    for (rgb, path) in by_color {
        let [_, r, g, b] = rgb.to_be_bytes();
        stroke(&mut pixmap, path, [r, g, b], 1.0);
    }
    if !pages.is_empty() {
        glyphs(&mut pixmap, batch, pages, &to_px);
    }

    let mut overlay = PathBuilder::new();
    for m in measurements {
        let points: Vec<(f32, f32)> = m.points.iter().map(|p| to_px(p[0], p[1])).collect();
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
                        let s = segment(i);
                        if !outside(s) {
                            let (x0, y0) = to_px(s[0], s[1]);
                            let (x1, y1) = to_px(s[2], s[3]);
                            overlay.move_to(x0, y0);
                            overlay.line_to(x1, y1);
                        }
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
    stroke(&mut pixmap, overlay, VIOLET, 2.0);

    pixmap
        .encode_png()
        .expect("a pixmap in memory always encodes")
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
fn glyphs(
    pixmap: &mut Pixmap,
    batch: &Batch,
    pages: &[Vec<u8>],
    to_px: &dyn Fn(f64, f64) -> (f32, f32),
) {
    let (w, h) = (pixmap.width() as i64, pixmap.height() as i64);
    let side = PAGE_SIZE as usize;
    let [ox, oy] = batch.origin;
    let data = pixmap.data_mut();
    for q in &batch.quads {
        let Some(page) = pages.get(usize::from(q.page)) else {
            continue;
        };
        let c: Vec<(f32, f32)> = (0..4)
            .map(|k| {
                to_px(
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
            }
        }
    }
}

fn stroke(pixmap: &mut Pixmap, path: PathBuilder, [r, g, b]: [u8; 3], width: f32) {
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
