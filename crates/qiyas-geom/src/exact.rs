//! Exact lengths and areas of the entities whose geometry has a closed form, so an arc or a
//! circle measures its true length instead of the length of its sampled chords.

use std::f64::consts::{PI, TAU};

use opencadcodec::entities::BoundaryEdge;
use opencadcodec::{EntityType, Vector3};

use crate::flatten::sweep;
use crate::Xform;

/// Bulges smaller than this are straight segments, as in `flatten`.
const MIN_BULGE: f64 = 1e-12;
/// Relative tolerance for telling a uniform scale from a stretch.
const UNIFORM: f64 = 1e-9;

/// The plan length of `entity` after `xf`, worked out exactly: Line, LwPolyline and Polyline2D
/// (bulged segments as arcs), Polyline, Arc, Circle and Ellipse (numerical integration for
/// arcs, Ramanujan's second approximation for full ellipses). `None` for any other entity,
/// a stretching `xf`, a tilted extrusion normal or
/// a closed polyline of two straight segments or fewer: measure the flattened paths then.
pub fn exact_length(entity: &EntityType, xf: &Xform) -> Option<f64> {
    let s = uniform_scale(xf)?;
    let local = match entity {
        EntityType::Line(e) => (e.end.x - e.start.x).hypot(e.end.y - e.start.y),
        EntityType::LwPolyline(e) => {
            flat(&e.normal)?;
            let vertices: Vec<_> = e
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], v.bulge))
                .collect();
            bulge_length(&vertices, e.is_closed)?
        }
        EntityType::Polyline2D(e) => {
            flat(&e.normal)?;
            let vertices: Vec<_> = e
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], v.bulge))
                .collect();
            bulge_length(&vertices, e.is_closed())?
        }
        EntityType::Polyline(e) => {
            let vertices: Vec<_> = e
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], 0.0))
                .collect();
            bulge_length(&vertices, e.is_closed())?
        }
        EntityType::Arc(e) => {
            flat(&e.normal)?;
            e.radius * sweep(e.start_angle, e.end_angle)
        }
        EntityType::Circle(e) => {
            flat(&e.normal)?;
            TAU * e.radius
        }
        EntityType::Ellipse(e) => {
            flat(&e.normal)?;
            if e.major_axis.z.abs() > 1e-12 {
                return None;
            }
            let a = e.major_axis.x.hypot(e.major_axis.y);
            let b = a * e.minor_axis_ratio.abs();
            let span = sweep(e.start_parameter, e.end_parameter);
            if span != TAU {
                // Integrate speed, independent of display tolerance and drawing scale.
                // Translation does not enter the derivative, avoiding cancellation.
                const N: usize = 1024;
                let step = span / N as f64;
                let speed = |t: f64| (a * t.sin()).hypot(b * t.cos());
                let mut sum = speed(e.start_parameter) + speed(e.start_parameter + span);
                for i in 1..N {
                    sum += if i % 2 == 0 { 2.0 } else { 4.0 }
                        * speed(e.start_parameter + i as f64 * step);
                }
                return Some(s * sum * step / 3.0);
            }
            let h = ((a - b) / (a + b)).powi(2);
            PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()))
        }
        _ => return None,
    };
    Some(local * s)
}

/// The plan area of each boundary path of a hatch after `xf`, in path order, worked out exactly
/// for polyline boundaries and boundaries made of lines, circular or elliptic arcs. `None`
/// for any other hatch or entity, a stretching `xf` or a tilted normal: measure the flattened
/// paths then. Each area is positive; the caller takes islands away.
pub fn exact_hatch_areas(entity: &EntityType, xf: &Xform) -> Option<Vec<f64>> {
    let EntityType::Hatch(h) = entity else {
        return None;
    };
    flat(&h.normal)?;
    let s = uniform_scale(xf)?;
    h.paths
        .iter()
        .map(|path| match path.edges.as_slice() {
            [BoundaryEdge::Polyline(e)] if e.vertices.len() >= 2 => {
                let vertices: Vec<_> = e.vertices.iter().map(|v| ([v.x, v.y], v.z)).collect();
                Some(bulge_area(&vertices) * s * s)
            }
            edges if !edges.is_empty() => {
                // Green's theorem: sum integral(x dy - y dx), then halve it. Keeping the
                // signed integral until the whole ring is summed preserves concave boundaries.
                let origin = match &edges[0] {
                    BoundaryEdge::Line(e) => [e.start.x, e.start.y],
                    BoundaryEdge::CircularArc(e) => [e.center.x, e.center.y],
                    BoundaryEdge::EllipticArc(e) => [e.center.x, e.center.y],
                    _ => return None,
                };
                let twice = edges
                    .iter()
                    .map(|edge| edge_integral(edge, origin))
                    .sum::<Option<f64>>()?;
                Some(twice.abs() * 0.5 * s * s)
            }
            _ => None,
        })
        .collect()
}

/// Signed boundary integral in a translated frame, to avoid cancellation far from (0, 0).
fn edge_integral(edge: &BoundaryEdge, origin: [f64; 2]) -> Option<f64> {
    let (center, major, minor, start, end, ccw) = match edge {
        BoundaryEdge::Line(e) => {
            return Some(
                (e.start.x - origin[0]) * (e.end.y - origin[1])
                    - (e.end.x - origin[0]) * (e.start.y - origin[1]),
            );
        }
        BoundaryEdge::CircularArc(e) => (
            [e.center.x, e.center.y],
            [e.radius, 0.0],
            [0.0, e.radius],
            e.start_angle,
            e.end_angle,
            e.counter_clockwise,
        ),
        BoundaryEdge::EllipticArc(e) => {
            let u = [e.major_axis_endpoint.x, e.major_axis_endpoint.y];
            (
                [e.center.x, e.center.y],
                u,
                [-u[1] * e.minor_axis_ratio, u[0] * e.minor_axis_ratio],
                e.start_angle,
                e.end_angle,
                e.counter_clockwise,
            )
        }
        _ => return None,
    };
    // Clockwise DXF hatch edges store mirrored angles, as in flatten::edge_points.
    let (a, delta) = if ccw {
        (start, sweep(start, end))
    } else {
        (-start, -sweep(-end, -start))
    };
    let b = a + delta;
    let dx = major[0] * (b.cos() - a.cos()) + minor[0] * (b.sin() - a.sin());
    let dy = major[1] * (b.cos() - a.cos()) + minor[1] * (b.sin() - a.sin());
    Some(
        (center[0] - origin[0]) * dy - (center[1] - origin[1]) * dx
            + (major[0] * minor[1] - major[1] * minor[0]) * delta,
    )
}

/// The plan area of a closed `entity` after `xf`, worked out exactly: a closed LwPolyline or
/// Polyline2D (the vertex polygon plus or minus each bulge's circular segment), a closed
/// Polyline, a Circle and a full Ellipse. `None` otherwise, as for `exact_length`. The caller
/// must not use it for a self-crossing outline, whose lobes would cancel here.
pub fn exact_area(entity: &EntityType, xf: &Xform) -> Option<f64> {
    let s = uniform_scale(xf)?;
    let local = match entity {
        EntityType::LwPolyline(e) if e.is_closed => {
            flat(&e.normal)?;
            let vertices: Vec<_> = e
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], v.bulge))
                .collect();
            bulge_area(&vertices)
        }
        EntityType::Polyline2D(e) if e.is_closed() => {
            flat(&e.normal)?;
            let vertices: Vec<_> = e
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], v.bulge))
                .collect();
            bulge_area(&vertices)
        }
        EntityType::Polyline(e) if e.is_closed() => {
            let vertices: Vec<_> = e
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], 0.0))
                .collect();
            bulge_area(&vertices)
        }
        EntityType::Circle(e) => {
            flat(&e.normal)?;
            PI * e.radius * e.radius
        }
        EntityType::Ellipse(e) => {
            let (a, b) = full_ellipse(e)?;
            PI * a * b
        }
        _ => return None,
    };
    Some(local * s * s)
}

/// The scale factor of `xf` when it scales both axes alike (a mirror is fine), else `None`.
pub(crate) fn uniform_scale(xf: &Xform) -> Option<f64> {
    let [[a, b, _], [c, d, _], _] = xf.m;
    let (sx, sy) = (a.hypot(c), b.hypot(d));
    let uniform = sx > 0.0
        && (sx - sy).abs() <= UNIFORM * sx.max(sy)
        && (a * b + c * d).abs() <= UNIFORM * sx * sy;
    uniform.then_some(sx)
}

/// `Some` when the extrusion normal is along Z (either way), so plan lengths are true lengths.
fn flat(normal: &Vector3) -> Option<()> {
    (normal.x.abs() < 1e-12 && normal.y.abs() < 1e-12 && normal.z != 0.0).then_some(())
}

/// The semi-axes of a full ellipse lying flat; `None` for an elliptical arc.
fn full_ellipse(e: &opencadcodec::Ellipse) -> Option<(f64, f64)> {
    flat(&e.normal)?;
    if e.major_axis.z.abs() > 1e-12 || sweep(e.start_parameter, e.end_parameter) != TAU {
        return None;
    }
    let a = e.major_axis.x.hypot(e.major_axis.y);
    Some((a, a * e.minor_axis_ratio.abs()))
}

/// Length along vertices whose bulges turn segments into arcs. A closed outline adds the
/// segment back to the first vertex. `None` for a closed outline of fewer than three vertices
/// and no bulge, which the flattened path measures as one segment (two bulged vertices, the usual
/// way to draw a circle, are measured).
fn bulge_length(vertices: &[([f64; 2], f64)], closed: bool) -> Option<f64> {
    let n = vertices.len();
    let bulged = vertices.iter().any(|(_, b)| b.abs() >= MIN_BULGE);
    if closed && (n < 2 || (n == 2 && !bulged)) {
        return None;
    }
    let segments = if closed { n } else { n.saturating_sub(1) };
    Some(
        (0..segments)
            .map(|i| {
                let (p0, b) = vertices[i];
                let p1 = vertices[(i + 1) % n].0;
                let d = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
                if b.abs() < MIN_BULGE || d == 0.0 {
                    d
                } else {
                    // Included angle 4·atan(b); radius from the chord.
                    let theta = 4.0 * b.abs().atan();
                    let r = d * (1.0 + b * b) / (4.0 * b.abs());
                    r * theta
                }
            })
            .sum(),
    )
}

/// Area of a closed outline of bulged segments: the signed vertex polygon plus, for each bulge,
/// its signed circular segment (a counter-clockwise arc adds on a counter-clockwise outline).
fn bulge_area(vertices: &[([f64; 2], f64)]) -> f64 {
    let n = vertices.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (p0, b) = vertices[i];
            let p1 = vertices[(i + 1) % n].0;
            let mut cross = p0[0] * p1[1] - p1[0] * p0[1];
            let d = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
            if b.abs() >= MIN_BULGE && d > 0.0 {
                let theta = 4.0 * b.abs().atan();
                let r = d * (1.0 + b * b) / (4.0 * b.abs());
                // Twice the segment area, signed like the bulge.
                cross += b.signum() * r * r * (theta - theta.sin());
            }
            cross
        })
        .sum();
    (twice / 2.0).abs()
}
