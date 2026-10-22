//! What a sheet draws besides outlines (see `Paint`): hatch pattern lines clipped to their
//! boundary, and the shapes of 2D solids, wipeouts and multilines. All in the entity's own
//! (object) coordinates; the walk maps them to the world.

use opencadcodec::entities::{HatchPatternLine, MLine, MLineJustification, Solid, Wipeout};
use opencadcodec::objects::ObjectType;
use opencadcodec::{CadDocument, Handle};

use crate::transform::Xform;

/// What a flattened path draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Paint {
    /// The entity's outline: drawn, picked and measured.
    #[default]
    Outline,
    /// One span of a hatch's pattern line (two points): drawn and picked as the hatch, never
    /// measured, never part of a hatch boundary.
    Pattern,
    /// A filled area; the path itself is empty. Drawn under the lines, never measured.
    Fill(Box<FlatFill>),
    /// A ray (one way) or a construction line (both ways): the path holds a point on it and a
    /// second one along it. Drawn across the sheet, never measured.
    Infinite { both_ways: bool },
    /// A POINT: the path holds its position. Drawn as a small cross, `size` across (drawing
    /// units), or sized from the sheet when 0. Picked as the point, never measured.
    Point { size: f64 },
}

/// A filled area: `rings` in the filled entity's own coordinates, mapped to drawing coordinates
/// by `xf`, filled even-odd (a ring inside another is a hole, one inside that an island again).
/// `background` fills with the background colour instead of the path's (a wipeout). `source` is
/// the filled entity's own handle (the path's `entity` may be its dimension's): a block inserted
/// many times gives the same `rings` under each reference's `xf`, so they are triangulated once.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatFill {
    pub rings: Vec<Vec<[f64; 2]>>,
    pub xf: Xform,
    pub source: u64,
    pub background: bool,
}

impl FlatFill {
    /// `rings` in drawing coordinates.
    pub fn world_rings(&self) -> Vec<Vec<[f64; 2]>> {
        self.rings
            .iter()
            .map(|r| r.iter().map(|p| self.xf.apply(*p)).collect())
            .collect()
    }
}

/// Most pattern lines one hatch draws; a pattern denser than that is left out (too fine to see).
const MAX_PATTERN_LINES: f64 = 20_000.0;
/// Most pattern spans (dashes) one hatch draws; past it the rest are left out.
const MAX_PATTERN_SPANS: usize = 50_000;

/// The spans of the pattern `lines` inside `rings` (even-odd), as point pairs, in the hatch's own
/// coordinates. Each line family runs through its base point and every offset from it, its
/// dashes (positive drawn, negative gaps, zero dots left out) measured from there. Empty when a
/// family would need more than `MAX_PATTERN_LINES` lines.
pub(crate) fn pattern_spans(
    lines: &[HatchPatternLine],
    rings: &[Vec<[f64; 2]>],
) -> Vec<[[f64; 2]; 2]> {
    let mut out = Vec::new();
    let corners = rings.iter().flatten();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in corners {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if !(lo[0].is_finite() && hi[0].is_finite()) {
        return out;
    }
    let box_corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
    for line in lines {
        let (s, c) = line.angle.sin_cos();
        let (d, n) = ([c, s], [-s, c]);
        let base = [line.base_point.x, line.base_point.y];
        let offset = [line.offset.x, line.offset.y];
        let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
        let spacing = dot(offset, n);
        if !spacing.is_finite() || spacing.abs() < 1e-12 {
            continue;
        }
        // Which lines k (through base + k * offset) cross the boundary's box.
        let across: Vec<f64> = box_corners
            .iter()
            .map(|p| dot([p[0] - base[0], p[1] - base[1]], n) / spacing)
            .collect();
        let k0 = across.iter().copied().fold(f64::INFINITY, f64::min).ceil();
        let k1 = across
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            .floor();
        if k1 - k0 > MAX_PATTERN_LINES {
            continue;
        }
        let period: f64 = line.dash_lengths.iter().map(|v| v.abs()).sum();
        let mut k = k0;
        while k <= k1 {
            if out.len() >= MAX_PATTERN_SPANS {
                return out;
            }
            let origin = [base[0] + k * offset[0], base[1] + k * offset[1]];
            for [t0, t1] in inside_spans(rings, origin, d, n) {
                if line.dash_lengths.is_empty() || period < 1e-12 {
                    push_span(&mut out, origin, d, t0, t1);
                    continue;
                }
                let mut at = (t0 / period).floor() * period;
                while at < t1 && out.len() < MAX_PATTERN_SPANS {
                    for &dash in &line.dash_lengths {
                        let end = at + dash.abs();
                        if dash > 0.0 {
                            push_span(&mut out, origin, d, at.max(t0), end.min(t1));
                        }
                        at = end;
                    }
                }
            }
            k += 1.0;
        }
    }
    out
}

/// Adds the stretch `a` to `b` of the line `origin + t * d`, unless it is empty or `out` is full.
fn push_span(out: &mut Vec<[[f64; 2]; 2]>, origin: [f64; 2], d: [f64; 2], a: f64, b: f64) {
    if b - a > 1e-12 && out.len() < MAX_PATTERN_SPANS {
        out.push([
            [origin[0] + a * d[0], origin[1] + a * d[1]],
            [origin[0] + b * d[0], origin[1] + b * d[1]],
        ]);
    }
}

/// The stretches `[t0, t1]` of the line `origin + t * d` inside `rings` (even-odd); `n` is the
/// line's normal. A crossing is counted once at a shared vertex (half-open rule).
fn inside_spans(
    rings: &[Vec<[f64; 2]>],
    origin: [f64; 2],
    d: [f64; 2],
    n: [f64; 2],
) -> Vec<[f64; 2]> {
    let mut cuts = Vec::new();
    for ring in rings {
        let m = ring.len();
        for i in 0..m {
            let (a, b) = (ring[i], ring[(i + 1) % m]);
            let side = |p: [f64; 2]| (p[0] - origin[0]) * n[0] + (p[1] - origin[1]) * n[1];
            let (sa, sb) = (side(a), side(b));
            if (sa > 0.0) != (sb > 0.0) {
                let f = sa / (sa - sb);
                let p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                cuts.push((p[0] - origin[0]) * d[0] + (p[1] - origin[1]) * d[1]);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.as_chunks::<2>().0.to_vec()
}

/// A 2D solid's (or trace's) outline in its object coordinates: 1, 2, 4, 3.
pub(crate) fn solid_outline(s: &Solid) -> Vec<[f64; 2]> {
    s.boundary_corners().iter().map(|v| [v.x, v.y]).collect()
}

/// A wipeout's frame in drawing coordinates: its clip polygon (the closing point dropped), its
/// clip rectangle's four corners, or the whole frame when it clips nothing.
pub(crate) fn wipeout_outline(w: &Wipeout) -> Vec<[f64; 2]> {
    let half = [w.size.x * 0.5, w.size.y * 0.5];
    let frame = vec![
        [-half[0], -half[1]],
        [half[0], -half[1]],
        [half[0], half[1]],
        [-half[0], half[1]],
    ];
    let clip: Vec<[f64; 2]> = w
        .clip_boundary_vertices
        .iter()
        .map(|v| [v.x, v.y])
        .collect();
    let mut corners = match clip.len() {
        _ if !w.clipping_enabled => frame,
        2 => {
            let ([x0, y0], [x1, y1]) = (clip[0], clip[1]);
            vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
        }
        n if n >= 3 => clip,
        _ => frame,
    };
    if corners.len() > 3 && corners.first() == corners.last() {
        corners.pop();
    }
    // Clip coordinates are centred on the picture, y down.
    let (p, u, v) = (w.insertion_point, w.u_vector, w.v_vector);
    corners
        .into_iter()
        .map(|[x, y]| {
            let (a, b) = (x + half[0], half[1] - y);
            [p.x + u.x * a + v.x * b, p.y + u.y * a + v.y * b]
        })
        .collect()
}

/// A multiline's element lines in drawing coordinates, and whether they close: each element's
/// offset (from its style, found by handle then by name; Standard's ±0.5 when neither is
/// there) times the scale, moved for the justification, along each vertex's miter.
pub(crate) fn mline_paths(doc: &CadDocument, m: &MLine) -> (Vec<Vec<[f64; 2]>>, bool) {
    let style = doc
        .objects
        .get(&m.style_handle.unwrap_or(Handle::NULL))
        .and_then(|o| match o {
            ObjectType::MLineStyle(s) => Some(s),
            _ => None,
        })
        .or_else(|| {
            doc.objects.values().find_map(|o| match o {
                ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(&m.style_name) => Some(s),
                _ => None,
            })
        });
    let mut offsets: Vec<f64> = match style {
        Some(s) if !s.elements.is_empty() => s.elements.iter().map(|e| e.offset).collect(),
        _ => vec![0.5, -0.5],
    };
    let top = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let bottom = offsets.iter().copied().fold(f64::INFINITY, f64::min);
    let shift = match m.justification {
        MLineJustification::Zero => 0.0,
        MLineJustification::Top => -top,
        MLineJustification::Bottom => -bottom,
    };
    for o in &mut offsets {
        *o = (*o + shift) * m.scale_factor;
    }
    let paths = offsets
        .iter()
        .map(|o| {
            m.vertices
                .iter()
                .map(|v| {
                    let across = [-v.direction.y, v.direction.x];
                    let cos = v.miter.x * across[0] + v.miter.y * across[1];
                    let along = if cos.abs() < 1e-9 { *o } else { o / cos };
                    [
                        v.position.x + v.miter.x * along,
                        v.position.y + v.miter.y * along,
                    ]
                })
                .collect()
        })
        .collect();
    (paths, m.is_closed())
}

#[cfg(test)]
mod tests {
    use crate::performance;
    use opencadcodec::Vector2;

    use super::*;

    #[test]
    fn pattern_spans_stop_once_the_cap_is_full() {
        // A 40,000-corner ring 100 across, crossed by about 19,600 lines of 0.01 dashes: the cap
        // is full after the first few lines, and the rest are never looked at.
        let ring: Vec<[f64; 2]> = (0..40_000)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 40_000.0;
                [50.0 * a.cos(), 50.0 * a.sin()]
            })
            .collect();
        let line = HatchPatternLine {
            angle: 0.0,
            base_point: Vector2::new(0.0, 0.0),
            offset: Vector2::new(0.0, 0.0051),
            dash_lengths: vec![0.01, -0.01],
        };
        let reference = performance::reference();
        let t = std::time::Instant::now();
        let spans = pattern_spans(&[line], &[ring]);
        let elapsed = t.elapsed();
        assert_eq!(spans.len(), MAX_PATTERN_SPANS);
        performance::assert_relative(elapsed, reference, 0.625, "capped hatch spans");
    }
}
