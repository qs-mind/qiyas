//! Numbered marks on a picture of the drawing: a badge on each of the entities in view, so the
//! model can name what it sees by handle (the caption lists each number's handle). Entities at
//! the top of the sheet only: a block reference is marked whole.

use std::collections::HashMap;

use qiyas_render::Batch;
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Transform};

use crate::image::{segment, stroke, View};

/// A badge: number `n` (from 1) on the entity `top`, centred at `at` (pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mark {
    pub n: usize,
    pub top: u64,
    pub at: (f32, f32),
}

/// Entities smaller than this on the picture (pixels across) are not marked.
const MIN_PX: f32 = 6.0;
/// Badges closer than this (pixels, centre to centre) would cover each other.
const APART: f32 = 26.0;
/// The badge colour, apart from anything a drawing or the take-off uses.
const BADGE: [u8; 3] = [0xE8, 0x59, 0x0C];
/// Each digit is 3 x 5 cells of `CELL` pixels.
const CELL: f32 = 3.0;
/// The digits 0 to 9, five rows of three cells each (bit 2 is the left cell).
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// What an entity draws inside the picture.
#[derive(Default)]
struct Seen {
    lo: (f32, f32),
    hi: (f32, f32),
    /// Its three longest visible pieces: length and middle.
    anchors: Vec<(f32, (f32, f32))>,
    /// Where it first appears in the batch, for a steady order.
    order: usize,
}

impl Seen {
    fn add(&mut self, a: (f32, f32), b: (f32, f32)) {
        if self.anchors.is_empty() {
            (self.lo, self.hi) = (a, a);
        }
        for p in [a, b] {
            self.lo = (self.lo.0.min(p.0), self.lo.1.min(p.1));
            self.hi = (self.hi.0.max(p.0), self.hi.1.max(p.1));
        }
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        self.anchors.push((length, middle));
        self.anchors.sort_by(|x, y| y.0.total_cmp(&x.0));
        self.anchors.truncate(3);
    }

    fn across(&self) -> f32 {
        (self.hi.0 - self.lo.0).hypot(self.hi.1 - self.lo.1)
    }
}

/// Up to `max` marks on the entities `batch` draws inside `view`: those in `first` (in their
/// order) first, then the largest on the picture. A badge sits on the middle of one of the
/// entity's longest visible pieces, clear of the badges already placed (always for `first`).
pub(crate) fn choose(batch: &Batch, view: &View, first: &[u64], max: usize) -> Vec<Mark> {
    let (w, h) = (view.width as f32, view.height as f32);
    let mut seen: HashMap<u64, Seen> = HashMap::new();
    for r in &batch.ranges {
        for i in r.first as usize..(r.first + r.count) as usize {
            let s = segment(batch, i);
            let Some((a, b)) = clip(view.px(s[0], s[1]), view.px(s[2], s[3]), w, h) else {
                continue;
            };
            let next = seen.len();
            let entry = seen.entry(r.top).or_insert_with(|| Seen {
                order: next,
                ..Seen::default()
            });
            entry.add(a, b);
        }
    }
    // Solid fills with no outline (a hatch without its boundary) are marked by their triangles.
    let [ox, oy] = batch.origin;
    for r in &batch.fill_ranges {
        for t in r.first as usize..(r.first + r.count) as usize {
            let Some(tri) = batch.fills.get(t * 6..t * 6 + 6) else {
                break;
            };
            let p = |k: usize| view.px(f64::from(tri[2 * k]) + ox, f64::from(tri[2 * k + 1]) + oy);
            for (a, b) in [(p(0), p(1)), (p(1), p(2))] {
                let Some((a, b)) = clip(a, b, w, h) else {
                    continue;
                };
                let next = seen.len();
                seen.entry(r.top)
                    .or_insert_with(|| Seen {
                        order: next,
                        ..Seen::default()
                    })
                    .add(a, b);
            }
        }
    }

    let mut rest: Vec<(&u64, &Seen)> = seen
        .iter()
        .filter(|(top, s)| !first.contains(top) && s.across() >= MIN_PX)
        .collect();
    rest.sort_by(|a, b| {
        b.1.across()
            .total_cmp(&a.1.across())
            .then(a.1.order.cmp(&b.1.order))
    });
    let wanted = first
        .iter()
        .filter_map(|top| seen.get(top).map(|s| (top, s, true)))
        .chain(rest.into_iter().map(|(top, s)| (top, s, false)));

    let mut marks: Vec<Mark> = Vec::new();
    for (top, s, must) in wanted {
        if marks.len() == max {
            break;
        }
        let clear = |at: (f32, f32)| {
            marks
                .iter()
                .all(|m| (m.at.0 - at.0).hypot(m.at.1 - at.1) >= APART)
        };
        let at = s
            .anchors
            .iter()
            .map(|(_, at)| *at)
            .find(|at| clear(*at))
            .or_else(|| must.then(|| s.anchors.first().map(|(_, at)| *at)).flatten());
        if let Some(at) = at {
            marks.push(Mark {
                n: marks.len() + 1,
                top: *top,
                at,
            });
        }
    }
    marks
}

/// The part of the segment `a`–`b` inside the picture `w` x `h`; `None` when it is outside.
fn clip(a: (f32, f32), b: (f32, f32), w: f32, h: f32) -> Option<((f32, f32), (f32, f32))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (p, q) in [(-dx, a.0), (dx, w - a.0), (-dy, a.1), (dy, h - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((
        (a.0 + t0 * dx, a.1 + t0 * dy),
        (a.0 + t1 * dx, a.1 + t1 * dy),
    ))
}

/// Draws each mark: an orange badge, white edged, its number in white.
pub(crate) fn draw(pixmap: &mut Pixmap, marks: &[Mark]) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    for m in marks {
        let digits: Vec<usize> =
            m.n.to_string()
                .bytes()
                .map(|d| usize::from(d - b'0'))
                .collect();
        let text_w = digits.len() as f32 * 4.0 * CELL - CELL;
        let (bw, bh) = (text_w + 3.0 * CELL, 5.0 * CELL + 3.0 * CELL);
        // Kept whole inside the picture.
        let x = (m.at.0 - bw / 2.0).clamp(0.0, (w - bw).max(0.0));
        let y = (m.at.1 - bh / 2.0).clamp(0.0, (h - bh).max(0.0));
        let Some(rect) = Rect::from_xywh(x, y, bw, bh) else {
            continue;
        };
        let mut paint = Paint::default();
        let [r, g, b] = BADGE;
        paint.set_color_rgba8(r, g, b, 255);
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        let mut edge = PathBuilder::new();
        edge.push_rect(rect);
        stroke(pixmap, edge, [255, 255, 255], 1.0);
        paint.set_color_rgba8(255, 255, 255, 255);
        for (k, d) in digits.iter().enumerate() {
            let left = x + 1.5 * CELL + k as f32 * 4.0 * CELL;
            for (row, bits) in DIGITS[*d].iter().enumerate() {
                for col in 0..3 {
                    if bits & (0b100 >> col) == 0 {
                        continue;
                    }
                    let cell = Rect::from_xywh(
                        left + col as f32 * CELL,
                        y + 1.5 * CELL + row as f32 * CELL,
                        CELL,
                        CELL,
                    );
                    if let Some(cell) = cell {
                        pixmap.fill_rect(cell, &paint, Transform::identity(), None);
                    }
                }
            }
        }
    }
}
