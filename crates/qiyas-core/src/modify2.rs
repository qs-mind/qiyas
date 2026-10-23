//! Reshaping entities already on the sheet on screen (`Command::Edit`): trim, extend, break,
//! join, lengthen, reverse, offset, fillet, chamfer, blend, pedit, overkill and flatten. Each is
//! one undo step, on the drawing's own entities at the top of the sheet (see
//! `Session::sheet_targets`); a command that can't do what it is asked changes nothing.
//!
//! An entity reshaped in place keeps its handle. When a trim or a break leaves several pieces,
//! the longest keeps the handle and the others are new entities; a join keeps the first
//! entity's. The take-off follows in the same step (`Session::follow_edit`): rows on a cut
//! entity measure all its pieces, rows on joined entities the joined polyline, and a row that
//! can't be measured any more loses its link (the outcome counts them).
//!
//! The geometry is opencadkernel's `geom2d`, in world coordinates on plan: lines, arcs, circles
//! and polylines lying flat (their normal up) are reshaped; anything drawn (block references,
//! splines, ellipses...) can be a cutting edge or a boundary.

use std::collections::HashSet;
use std::f64::consts::{PI, TAU};

use opencadcodec::entities::LwVertex;
use opencadcodec::types::Matrix3;
use opencadcodec::{EntityType, Handle, LwPolyline, Spline, Vector2, Vector3};
use opencadkernel::geom2d::{
    break_spans, fillet_between_rays, fit_arc_chain, intersect, line_line, normalize_angle,
    offset_polyline, trim_spans, Arc as KArc, BulgeArc, Circle as KCircle, Curve, Line as KLine,
    NurbsCurve, Parameterization, Polyline, PolylineVertex, Ray, Tolerance,
};
use qiyas_geom::{flatten_with, snap_shapes, Xform};
use qiyas_render::layer_shown;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::session::variant_name;
use crate::{CoreError, Outcome, Session, Target};

/// Points closer than this, in drawing units, are the same point; crossings closer than it
/// to an end are at that end. Far from the origin it grows with the coordinates (`tol_near`).
const TOL: f64 = 1e-6;
/// Past a million or so, `TOL` is a few float steps of the coordinates (a UTM northing in
/// millimetres, 3.3e9, has steps of 5e-7): two ends that meet may differ by more. There the
/// tolerance is this share of the coordinates' size instead (3 µm at 3.3e9 mm), still far below
/// anything drawn.
const REL_TOL: f64 = 1e-12;

/// `TOL` near the point `p`: never less than `REL_TOL` of its coordinates' size.
fn tol_near(p: [f64; 2]) -> f64 {
    TOL.max(p[0].abs().max(p[1].abs()) * REL_TOL)
}

/// What `Command::Edit` does. Points are in drawing coordinates; a pick point (`at_point`,
/// `at_end`, `at_a`...) is where the entity was clicked, which says which piece, end or side is
/// meant.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditSpec {
    /// Cuts away the piece of `target` around `at_point` between the crossings of the `cutters`
    /// (none: everything drawn on the sheet cuts).
    #[schemars(example = json!({"kind": "trim", "cutters": [{"handle": 1235}], "target": {"handle": 1234}, "at_point": [6, 0]}))]
    #[schemars(extend("x-words" = ["TRIM", "TR"]))]
    Trim {
        #[serde(default)]
        cutters: Vec<Target>,
        target: Target,
        at_point: [f64; 2],
    },
    /// Runs the end of `target` nearest `at_end` on to the first of the `boundaries` it meets
    /// (none: everything drawn on the sheet). Lines, arcs and open polylines.
    #[schemars(example = json!({"kind": "extend", "boundaries": [{"handle": 1235}], "target": {"handle": 1234}, "at_end": [4.9, 0]}))]
    #[schemars(extend("x-words" = ["EXTEND", "EX"]))]
    Extend {
        #[serde(default)]
        boundaries: Vec<Target>,
        target: Target,
        at_end: [f64; 2],
    },
    /// Removes the part of `target` between `p1` and `p2` (the same point splits it there). A
    /// circle keeps the part counter-clockwise from `p2` round to `p1`.
    #[schemars(example = json!({"kind": "break", "target": {"handle": 1234}, "p1": [2, 0], "p2": [3, 0]}))]
    #[schemars(extend("x-words" = ["BREAK", "BR"]))]
    Break {
        target: Target,
        p1: [f64; 2],
        p2: [f64; 2],
    },
    /// Joins lines, arcs and open polylines whose ends meet, starting from the first, into one
    /// polyline under the first's handle (closed when the chain closes). The others that don't
    /// meet the chain are left as they are.
    #[schemars(example = json!({"kind": "join", "targets": [{"handle": 1234}, {"handle": 1235}]}))]
    #[schemars(extend("x-words" = ["JOIN", "J"]))]
    Join { targets: Vec<Target> },
    /// Lengthens (or with a negative change, shortens) the end of `target` nearest `at_end`.
    /// Lines, arcs and open polylines (their end side).
    #[schemars(example = json!({"kind": "lengthen", "target": {"handle": 1234}, "at_end": [5, 0], "by": {"delta": 0.5}}))]
    #[schemars(extend("x-words" = ["LENGTHEN", "LEN"]))]
    Lengthen {
        target: Target,
        at_end: [f64; 2],
        by: LengthenBy,
    },
    /// Turns lines and polylines to run the other way; anything else is left as it is.
    #[schemars(example = json!({"kind": "reverse", "targets": [{"handle": 1234}]}))]
    #[schemars(extend("x-words" = ["REVERSE"]))]
    Reverse { targets: Vec<Target> },
    /// A copy of `target` `distance` away (above 0) on the side of `side_point`: lines, arcs,
    /// circles and polylines (arcs stay arcs; straight corners stay sharp).
    #[schemars(example = json!({"kind": "offset", "target": {"handle": 1234}, "distance": 0.2, "side_point": [1, 1]}))]
    #[schemars(extend("x-words" = ["OFFSET", "O"]))]
    Offset {
        target: Target,
        distance: f64,
        side_point: [f64; 2],
    },
    /// Rounds the corner where two lines meet (each kept on its picked side) with an arc of
    /// `radius`; 0 meets them at the corner. `a` and `b` may be one polyline: then its two
    /// straight sides picked, which must meet at a vertex.
    #[schemars(example = json!({"kind": "fillet", "a": {"handle": 1234}, "b": {"handle": 1235}, "at_a": [4, 0], "at_b": [5, 1], "radius": 0.5}))]
    #[schemars(extend("x-words" = ["FILLET", "F"]))]
    Fillet {
        a: Target,
        b: Target,
        at_a: [f64; 2],
        at_b: [f64; 2],
        radius: f64,
    },
    /// Cuts the corner as `Fillet` rounds it: `d1` back along `a`, `d2` back along `b`, and a
    /// line between.
    #[schemars(example = json!({"kind": "chamfer", "a": {"handle": 1234}, "b": {"handle": 1235}, "at_a": [4, 0], "at_b": [5, 1], "d1": 0.3, "d2": 0.3}))]
    #[schemars(extend("x-words" = ["CHAMFER", "CHA"]))]
    Chamfer {
        a: Target,
        b: Target,
        at_a: [f64; 2],
        at_b: [f64; 2],
        d1: f64,
        d2: f64,
    },
    /// A spline from the end of `a` nearest `at_a` to the end of `b` nearest `at_b`, tangent to
    /// both. Lines, arcs and open polylines.
    #[schemars(example = json!({"kind": "blend", "a": {"handle": 1234}, "b": {"handle": 1235}, "at_a": [4, 0], "at_b": [6, 1]}))]
    #[schemars(extend("x-words" = ["BLEND"]))]
    Blend {
        a: Target,
        b: Target,
        at_a: [f64; 2],
        at_b: [f64; 2],
    },
    /// Edits a polyline (or turns a line or an arc into one).
    #[serde(rename = "pedit")]
    #[schemars(example = json!({"kind": "pedit", "target": {"handle": 1234}, "op": "close"}))]
    #[schemars(example = json!({"kind": "pedit", "target": {"handle": 1234}, "op": {"width": 0.05}}))]
    #[schemars(extend("x-words" = ["PEDIT", "PE"]))]
    PEdit { target: Target, op: PEditOp },
    /// Removes duplicates among `targets`: lines lying on one another within `tol` (drawing
    /// units) become one line covering them all; circles, arcs, points and polylines drawn twice
    /// go. Only entities with the same layer, colour, linetype and lineweight count as the same.
    #[schemars(example = json!({"kind": "overkill", "targets": [{"handle": 1234}, {"handle": 1235}]}))]
    #[schemars(extend("x-words" = ["OVERKILL"]))]
    Overkill {
        targets: Vec<Target>,
        /// Default: 0.000001 mm converted to drawing units. Explicit zero is allowed.
        #[serde(default)]
        tol: Option<f64>,
    },
    /// Puts `targets` on the plan: every height to 0 and normals up, looking the same from above.
    /// What lies tilted (or faces down, for texts and block references) is left as it is.
    #[schemars(example = json!({"kind": "flatten", "targets": [{"handle": 1234}]}))]
    #[schemars(extend("x-words" = ["FLATTEN"]))]
    Flatten { targets: Vec<Target> },
}

pub(crate) fn default_overkill_tol(units: crate::Units) -> f64 {
    1e-9 / units.to_metres()
}

/// How `Lengthen` changes the length: by `Delta` drawing units, to a `Total`, or to a `Percent`
/// of what it is.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LengthenBy {
    Delta(f64),
    Total(f64),
    Percent(f64),
}

/// A `PEdit` change. `AddVertex` puts a vertex on the polyline at the point nearest the one
/// given; `DeleteVertex` removes the vertex nearest it (the side across is straight);
/// `FitCurve` runs tangent arcs through the vertices; `Decurve` straightens every side;
/// `ConvertToPolyline` turns a line or an arc into a polyline under its own handle.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PEditOp {
    Close,
    Open,
    Width(f64),
    AddVertex([f64; 2]),
    DeleteVertex([f64; 2]),
    FitCurve,
    Decurve,
    ConvertToPolyline,
}

impl EditSpec {
    /// Every entity the command names, cutters and boundaries too.
    pub(crate) fn targets(&self) -> Vec<Target> {
        match self {
            EditSpec::Trim {
                cutters: more,
                target,
                ..
            }
            | EditSpec::Extend {
                boundaries: more,
                target,
                ..
            } => std::iter::once(target).chain(more).cloned().collect(),
            EditSpec::Break { target, .. }
            | EditSpec::Lengthen { target, .. }
            | EditSpec::Offset { target, .. }
            | EditSpec::PEdit { target, .. } => vec![target.clone()],
            EditSpec::Fillet { a, b, .. }
            | EditSpec::Chamfer { a, b, .. }
            | EditSpec::Blend { a, b, .. } => vec![a.clone(), b.clone()],
            EditSpec::Join { targets }
            | EditSpec::Reverse { targets }
            | EditSpec::Overkill { targets, .. }
            | EditSpec::Flatten { targets } => targets.clone(),
        }
    }
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn finite(values: &[f64]) -> Result<(), CoreError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("Points and distances must be finite numbers"))
    }
}

/// The kernel's tolerance for curves near `p` (`tol_near`).
fn tol(p: [f64; 2]) -> Tolerance {
    Tolerance::new(tol_near(p))
}

/// "Line 2A": how a message names an entity.
fn named(e: &EntityType) -> String {
    format!("{} {:X}", variant_name(e), e.common().handle.value())
}

fn xy(v: Vector3) -> [f64; 2] {
    [v.x, v.y]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: [f64; 2], b: [f64; 2], s: f64) -> [f64; 2] {
    [a[0] + b[0] * s, a[1] + b[1] * s]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn unit(v: [f64; 2]) -> Option<[f64; 2]> {
    let l = v[0].hypot(v[1]);
    (l > 1e-12).then(|| [v[0] / l, v[1] / l])
}

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    dist(a, b) <= tol_near(a)
}

/// The normal points straight up (+Z): object coordinates are world coordinates on plan.
fn up(n: &Vector3) -> bool {
    n.x.abs() < 1e-9 && n.y.abs() < 1e-9 && n.z > 0.0
}

/// The curve `e` is drawn along, in world coordinates on plan: lines, arcs, circles and
/// polylines lying flat; why not for anything else.
fn curve_of(e: &EntityType) -> Result<Curve, CoreError> {
    let off_plan = || {
        invalid(format!(
            "{} doesn't lie flat on the plan: flatten it first",
            named(e)
        ))
    };
    match e {
        EntityType::Line(l) => {
            if same(xy(l.start), xy(l.end)) {
                return Err(invalid(format!("{} has no length", named(e))));
            }
            Ok(Curve::Line(KLine {
                start: xy(l.start),
                end: xy(l.end),
            }))
        }
        EntityType::Arc(a) if up(&a.normal) => Ok(Curve::Arc(KArc {
            centre: xy(a.center),
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
        })),
        EntityType::Circle(c) if up(&c.normal) => Ok(Curve::Circle(KCircle {
            centre: xy(c.center),
            radius: c.radius,
        })),
        EntityType::LwPolyline(pl) if up(&pl.normal) => {
            if pl.vertices.len() < 2 {
                return Err(invalid(format!("{} has fewer than 2 vertices", named(e))));
            }
            Ok(Curve::Polyline(kernel_polyline(pl)))
        }
        EntityType::Arc(_) | EntityType::Circle(_) | EntityType::LwPolyline(_) => Err(off_plan()),
        _ => Err(invalid(format!(
            "{} can't be changed this way: lines, arcs, circles and polylines can",
            named(e)
        ))),
    }
}

fn kernel_polyline(pl: &LwPolyline) -> Polyline {
    Polyline {
        vertices: pl
            .vertices
            .iter()
            .map(|v| PolylineVertex::curved([v.location.x, v.location.y], v.bulge))
            .collect(),
        closed: pl.is_closed,
    }
}

/// The polyline's sides: start, end and bulge of each.
fn sides(k: &Polyline) -> Vec<([f64; 2], [f64; 2], f64)> {
    let n = k.vertices.len();
    let count = if k.closed { n } else { n.saturating_sub(1) };
    (0..count)
        .map(|i| {
            let (a, b) = (k.vertices[i], k.vertices[(i + 1) % n]);
            (a.position, b.position, a.bulge)
        })
        .collect()
}

/// The point a fraction `f` along the side from `a` to `b` with `bulge` (by angle on an arc).
fn side_point(a: [f64; 2], b: [f64; 2], bulge: f64, f: f64) -> [f64; 2] {
    match BulgeArc::from_bulge(a, b, bulge) {
        Some(arc) => arc.sample(f),
        None => add(a, sub(b, a), f),
    }
}

fn side_length(a: [f64; 2], b: [f64; 2], bulge: f64) -> f64 {
    match BulgeArc::from_bulge(a, b, bulge) {
        Some(arc) => arc.radius * arc.sweep.abs(),
        None => dist(a, b),
    }
}

/// The bulge of the part of a side with `bulge` that covers a fraction `f` of it.
fn part_bulge(bulge: f64, f: f64) -> f64 {
    if bulge == 0.0 {
        0.0
    } else {
        (bulge.atan() * f).tan()
    }
}

/// A part of a polyline side: where it starts, its bulge, and the side it is part of.
type SidePart = ([f64; 2], f64, usize);

/// The parts of the polyline's sides between curve parameters `t0` and `t1` (`t1` past 1 runs
/// on round a closed polyline): each part's start, its bulge, and the side it comes from; then
/// the end point.
fn sub_polyline(k: &Polyline, t0: f64, t1: f64) -> (Vec<SidePart>, [f64; 2]) {
    let sides = sides(k);
    let n = sides.len();
    let (s0, s1) = (t0 * n as f64, t1 * n as f64);
    let mut out = Vec::new();
    let mut end = k.vertices[0].position;
    let mut i = s0.floor().max(0.0) as usize;
    while (i as f64) < s1 {
        let (lo, hi) = (
            s0.max(i as f64) - i as f64,
            s1.min(i as f64 + 1.0) - i as f64,
        );
        let (a, b, bulge) = sides[i % n];
        if hi - lo > 1e-12 {
            out.push((
                side_point(a, b, bulge, lo),
                part_bulge(bulge, hi - lo),
                i % n,
            ));
            end = side_point(a, b, bulge, hi);
        }
        i += 1;
    }
    (out, end)
}

/// How long the part of `curve` between parameters `span` is.
fn span_length(curve: &Curve, [t0, t1]: [f64; 2]) -> f64 {
    match curve {
        Curve::Line(l) => l.length() * (t1 - t0),
        Curve::Arc(a) => a.radius * a.sweep() * (t1 - t0),
        Curve::Circle(c) => TAU * c.radius * (t1 - t0),
        Curve::Polyline(k) => {
            let sides = sides(k);
            let n = sides.len() as f64;
            let (s0, s1) = (t0 * n, t1 * n);
            let mut total = 0.0;
            let mut i = s0.floor().max(0.0);
            while i < s1 {
                let part = s1.min(i + 1.0) - s0.max(i);
                let (a, b, bulge) = sides[i as usize % sides.len()];
                total += side_length(a, b, bulge) * part.max(0.0);
                i += 1.0;
            }
            total
        }
        _ => 0.0,
    }
}

/// The part of `e` (drawn along `curve`) between parameters `span`, as an entity like it: same
/// layer, colour and handle. A circle's part is an arc; a polyline's is open.
fn piece(e: &EntityType, curve: &Curve, [t0, t1]: [f64; 2]) -> EntityType {
    let mut out = match (e, curve) {
        (EntityType::Line(l), _) => {
            let mut l = l.clone();
            let (a, b) = (curve.point_at(t0), curve.point_at(t1));
            let z = |t: f64| l.start.z + (l.end.z - l.start.z) * t;
            let (z0, z1) = (z(t0), z(t1));
            l.start = Vector3::new(a[0], a[1], z0);
            l.end = Vector3::new(b[0], b[1], z1);
            EntityType::Line(l)
        }
        (EntityType::Arc(a), Curve::Arc(k)) => {
            let mut a = a.clone();
            let (from, sweep) = (normalize_angle(k.start_angle), k.sweep());
            a.start_angle = normalize_angle(from + t0 * sweep);
            a.end_angle = normalize_angle(from + t1 * sweep);
            EntityType::Arc(a)
        }
        (EntityType::Circle(c), _) => {
            let mut a = opencadcodec::Arc::new();
            a.common = c.common.clone();
            a.center = c.center;
            a.radius = c.radius;
            a.thickness = c.thickness;
            a.normal = c.normal;
            a.start_angle = normalize_angle(t0 * TAU);
            a.end_angle = normalize_angle(t1 * TAU);
            EntityType::Arc(a)
        }
        (EntityType::LwPolyline(pl), Curve::Polyline(k)) => {
            let (parts, end) = sub_polyline(k, t0, t1);
            let mut out = pl.clone();
            out.vertices = parts
                .into_iter()
                .map(|(p, bulge, i)| {
                    let mut v = pl.vertices[i];
                    v.location = Vector2::new(p[0], p[1]);
                    v.bulge = bulge;
                    v
                })
                .collect();
            out.vertices
                .push(LwVertex::new(Vector2::new(end[0], end[1])));
            out.is_closed = false;
            EntityType::LwPolyline(out)
        }
        _ => unreachable!("curve_of made the curve of this entity"),
    };
    out.common_mut().graphic_data = None;
    out
}

/// Which end of an open curve, and how it runs on past it.
#[derive(Clone, Copy)]
struct End {
    /// The curve's start (else its end).
    at_start: bool,
    way: Way,
}

#[derive(Clone, Copy)]
enum Way {
    /// A straight end side: its end point, the unit direction out past it, and its length.
    Straight {
        from: [f64; 2],
        dir: [f64; 2],
        length: f64,
    },
    /// A round end side: its circle, the angle at the end, which way round is out past it
    /// (1 counter-clockwise, -1 clockwise), and how far it sweeps.
    Round {
        centre: [f64; 2],
        radius: f64,
        angle: f64,
        sign: f64,
        sweep: f64,
    },
}

impl Way {
    /// The end point moved `d` on past the end (a distance, or an angle on a round end).
    fn moved(&self, d: f64) -> [f64; 2] {
        match *self {
            Way::Straight { from, dir, .. } => add(from, dir, d),
            Way::Round {
                centre,
                radius,
                angle,
                sign,
                ..
            } => {
                let a = angle + sign * d;
                [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
            }
        }
    }

    /// The end point and the unit direction out past it.
    fn out(&self) -> ([f64; 2], [f64; 2]) {
        match *self {
            Way::Straight { from, dir, .. } => (from, dir),
            Way::Round { angle, sign, .. } => {
                (self.moved(0.0), [-sign * angle.sin(), sign * angle.cos()])
            }
        }
    }
}

/// The end of the open curve `curve` nearest `at`.
fn end_of(e: &EntityType, curve: &Curve, at: [f64; 2]) -> Result<End, CoreError> {
    if curve.is_closed() {
        return Err(invalid(format!("{} is closed: it has no end", named(e))));
    }
    let at_start = dist(curve.point_at(0.0), at) <= dist(curve.point_at(1.0), at);
    let straight = |from: [f64; 2], other: [f64; 2]| Way::Straight {
        from,
        dir: unit(sub(from, other)).unwrap_or([1.0, 0.0]),
        length: dist(from, other),
    };
    let way = match curve {
        Curve::Line(l) => {
            if at_start {
                straight(l.start, l.end)
            } else {
                straight(l.end, l.start)
            }
        }
        Curve::Arc(a) => Way::Round {
            centre: a.centre,
            radius: a.radius,
            angle: if at_start { a.start_angle } else { a.end_angle },
            sign: if at_start { -1.0 } else { 1.0 },
            sweep: a.sweep(),
        },
        Curve::Polyline(k) => {
            let sides = sides(k);
            let (a, b, bulge) = if at_start {
                sides[0]
            } else {
                sides[sides.len() - 1]
            };
            match BulgeArc::from_bulge(a, b, bulge) {
                Some(arc) => Way::Round {
                    centre: arc.center,
                    radius: arc.radius,
                    angle: if at_start {
                        arc.start_angle
                    } else {
                        arc.end_angle
                    },
                    sign: if at_start {
                        -arc.sweep.signum()
                    } else {
                        arc.sweep.signum()
                    },
                    sweep: arc.sweep.abs(),
                },
                None if at_start => straight(a, b),
                None => straight(b, a),
            }
        }
        _ => unreachable!("closed curves have no end"),
    };
    Ok(End { at_start, way })
}

/// `e` with its end `end` moved `d` on (negative: back).
fn move_end(e: &mut EntityType, end: &End, d: f64) {
    let p = end.way.moved(d);
    match e {
        EntityType::Line(l) => {
            let v = if end.at_start {
                &mut l.start
            } else {
                &mut l.end
            };
            v.x = p[0];
            v.y = p[1];
        }
        EntityType::Arc(a) => {
            if end.at_start {
                a.start_angle = normalize_angle(a.start_angle - d);
            } else {
                a.end_angle = normalize_angle(a.end_angle + d);
            }
        }
        EntityType::LwPolyline(pl) => {
            let n = pl.vertices.len();
            let (vertex, side) = if end.at_start { (0, 0) } else { (n - 1, n - 2) };
            pl.vertices[vertex].location = Vector2::new(p[0], p[1]);
            if let Way::Round { sweep, .. } = end.way {
                let b = pl.vertices[side].bulge;
                pl.vertices[side].bulge = b.signum() * ((sweep + d) / 4.0).tan();
            }
        }
        _ => unreachable!("only lines, arcs and polylines have ends here"),
    }
    e.common_mut().graphic_data = None;
}

/// A line, arc or open polyline as a chain of vertices, each with the bulge of the side leaving
/// it (the last's is 0); `None` for anything else.
fn chain_of(e: &EntityType) -> Option<Vec<([f64; 2], f64)>> {
    match e {
        EntityType::Line(l) if !same(xy(l.start), xy(l.end)) => {
            Some(vec![(xy(l.start), 0.0), (xy(l.end), 0.0)])
        }
        EntityType::Arc(a) if up(&a.normal) => {
            let k = KArc {
                centre: xy(a.center),
                radius: a.radius,
                start_angle: a.start_angle,
                end_angle: a.end_angle,
            };
            let curve = Curve::Arc(k);
            Some(vec![
                (curve.point_at(0.0), (k.sweep() / 4.0).tan()),
                (curve.point_at(1.0), 0.0),
            ])
        }
        EntityType::LwPolyline(pl) if up(&pl.normal) && !pl.is_closed && pl.vertices.len() > 1 => {
            let mut out: Vec<_> = pl
                .vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], v.bulge))
                .collect();
            out.last_mut().expect("2 vertices").1 = 0.0;
            Some(out)
        }
        _ => None,
    }
}

/// The chain run the other way.
fn reversed(chain: &[([f64; 2], f64)]) -> Vec<([f64; 2], f64)> {
    let n = chain.len();
    (0..n)
        .map(|k| {
            let bulge = if k + 1 < n { -chain[n - 2 - k].1 } else { 0.0 };
            (chain[n - 1 - k].0, bulge)
        })
        .collect()
}

/// What fillet and chamfer do to a corner.
#[derive(Clone, Copy)]
enum Corner {
    Round(f64),
    Bevel(f64, f64),
}

impl Session {
    /// `Command::Edit`: see `EditSpec`. `Edited` with the new and removed handles for the
    /// changes in place; `Added` for offset and blend, which only add.
    pub(crate) fn edit_entities(&mut self, spec: EditSpec) -> Result<Outcome, CoreError> {
        // Every entity named must be the sheet's own, cutters and boundaries too (they may be
        // locked: only what changes may not).
        self.sheet_targets_any(&spec.targets())?;
        let one = |s: &Session, t: &Target| -> Result<Handle, CoreError> {
            Ok(s.sheet_targets(std::slice::from_ref(t))?[0])
        };
        match spec {
            EditSpec::Trim {
                cutters,
                target,
                at_point,
            } => {
                finite(&at_point)?;
                let h = one(self, &target)?;
                let edges = self.edges(&cutters, h, true)?;
                self.trim(h, &edges, at_point)
            }
            EditSpec::Extend {
                boundaries,
                target,
                at_end,
            } => {
                finite(&at_end)?;
                let h = one(self, &target)?;
                let edges = self.edges(&boundaries, h, false)?;
                self.extend(h, &edges, at_end)
            }
            EditSpec::Break { target, p1, p2 } => {
                finite(&[p1[0], p1[1], p2[0], p2[1]])?;
                let h = one(self, &target)?;
                self.break_at(h, p1, p2)
            }
            EditSpec::Join { targets } => {
                let handles = self.sheet_targets(&targets)?;
                self.join(&handles)
            }
            EditSpec::Lengthen { target, at_end, by } => {
                let (LengthenBy::Delta(v) | LengthenBy::Total(v) | LengthenBy::Percent(v)) = by;
                finite(&[at_end[0], at_end[1], v])?;
                let h = one(self, &target)?;
                self.lengthen(h, at_end, by)
            }
            EditSpec::Reverse { targets } => {
                let handles = self.sheet_targets(&targets)?;
                self.reverse(&handles)
            }
            EditSpec::Offset {
                target,
                distance,
                side_point,
            } => {
                finite(&[distance, side_point[0], side_point[1]])?;
                if distance <= 0.0 {
                    return Err(invalid("The offset distance must be more than 0"));
                }
                let h = one(self, &target)?;
                self.offset(h, distance, side_point)
            }
            EditSpec::Fillet {
                a,
                b,
                at_a,
                at_b,
                radius,
            } => {
                finite(&[at_a[0], at_a[1], at_b[0], at_b[1], radius])?;
                if radius < 0.0 {
                    return Err(invalid("The fillet radius must be 0 or more"));
                }
                let (a, b) = (one(self, &a)?, one(self, &b)?);
                self.corner(a, b, at_a, at_b, Corner::Round(radius))
            }
            EditSpec::Chamfer {
                a,
                b,
                at_a,
                at_b,
                d1,
                d2,
            } => {
                finite(&[at_a[0], at_a[1], at_b[0], at_b[1], d1, d2])?;
                if d1 < 0.0 || d2 < 0.0 {
                    return Err(invalid("The chamfer distances must be 0 or more"));
                }
                let (a, b) = (one(self, &a)?, one(self, &b)?);
                self.corner(a, b, at_a, at_b, Corner::Bevel(d1, d2))
            }
            EditSpec::Blend { a, b, at_a, at_b } => {
                finite(&[at_a[0], at_a[1], at_b[0], at_b[1]])?;
                let (a, b) = (one(self, &a)?, one(self, &b)?);
                self.blend(a, b, at_a, at_b)
            }
            EditSpec::PEdit { target, op } => {
                let h = one(self, &target)?;
                self.pedit(h, op)
            }
            EditSpec::Overkill { targets, tol } => {
                let tol = tol.unwrap_or_else(|| default_overkill_tol(self.drawing.units));
                finite(&[tol])?;
                if tol < 0.0 {
                    return Err(invalid("The tolerance must be 0 or more"));
                }
                let handles = self.sheet_targets(&targets)?;
                self.overkill(&handles, tol.max(1e-15 / self.drawing.units.to_metres()))
            }
            EditSpec::Flatten { targets } => {
                let handles = self.sheet_targets(&targets)?;
                self.flatten_all(&handles)
            }
        }
    }

    /// Replaces the entity `h` with `e` (made from it: same handle), in place.
    fn replace(&mut self, h: Handle, mut e: EntityType) {
        e.common_mut().handle = h;
        e.common_mut().graphic_data = None;
        *self.drawing.doc.get_entity_mut(h).expect("a sheet target") = e;
    }

    /// The handles whose curves cut or bound `h`: `named` (less `h`), or with none, what is
    /// drawn on the sheet on screen (for a trim, only what is drawn across `h`'s box).
    fn edges(&self, named: &[Target], h: Handle, near: bool) -> Result<Vec<Handle>, CoreError> {
        if !named.is_empty() {
            let mut handles = self.sheet_targets_any(named)?;
            handles.retain(|&c| c != h);
            return Ok(handles);
        }
        if near {
            if let Ok(curve) = curve_of(self.entity(h)) {
                let points = curve.tessellate(8.0);
                let lo = points
                    .iter()
                    .fold([f64::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
                let hi = points
                    .iter()
                    .fold([f64::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
                let pad = TOL.max(1e-9 * (hi[0] - lo[0]).abs().max((hi[1] - lo[1]).abs()));
                let bounds = [[lo[0] - pad, lo[1] - pad], [hi[0] + pad, hi[1] + pad]];
                return Ok(self
                    .pick_window(bounds, true)
                    .into_iter()
                    .map(|p| Handle::new(p.top))
                    .filter(|&c| c != h)
                    .collect());
            }
        }
        let doc = &self.drawing.doc;
        Ok(doc
            .entities_in_block(&self.sheet)
            .filter(|e| {
                let c = e.common();
                !c.invisible && c.handle != h && layer_shown(doc, &self.visibility, &c.layer)
            })
            .map(|e| e.common().handle)
            .collect())
    }

    /// The curves `handles` are drawn with, in world coordinates: exact lines, arcs, circles,
    /// ellipses and polyline sides where they have them; the drawn outline otherwise (block and
    /// xref references, splines...). Texts and points give none.
    fn edge_curves(&self, handles: &[Handle]) -> Vec<Curve> {
        let doc = &self.drawing.doc;
        let xrefs = self.xref_docs();
        let mut out = Vec::new();
        for &h in handles {
            let Some(e) = doc.get_entity(h) else {
                continue;
            };
            let shapes = snap_shapes(doc, e, &Xform::identity(), self.measure_tol());
            if !shapes.is_empty() {
                out.extend(shapes.into_iter().flat_map(|s| s.pieces));
            } else if matches!(e, EntityType::Insert(_)) {
                let drawn = flatten_with(doc, &xrefs, e, &Xform::identity(), self.measure_tol());
                out.extend(drawn.into_paths().into_iter().map(|path| {
                    Curve::Polyline(Polyline {
                        vertices: path
                            .points
                            .iter()
                            .map(|&p| PolylineVertex::straight(p))
                            .collect(),
                        closed: path.closed,
                    })
                }));
            }
        }
        out
    }

    /// Puts `pieces` (parts of the entity `h`, with their lengths) in its place: the longest
    /// keeps its handle, the others are new. The new handles, and how many take-off rows lost
    /// their link (see `Session::follow_edit`).
    fn place_pieces(
        &mut self,
        h: Handle,
        mut pieces: Vec<(f64, EntityType)>,
    ) -> Result<(Vec<u64>, usize), CoreError> {
        pieces.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut pieces = pieces.into_iter();
        let (_, longest) = pieces.next().expect("a piece");
        self.replace(h, longest);
        let owner = self.sheet_owner();
        let added = pieces
            .map(|(_, e)| self.add_fresh(e, owner))
            .collect::<Result<Vec<_>, _>>()?;
        // Rows measuring the entity measure every piece of it.
        let unlinked = self.follow_edit(&[], &[], &[(h, added.clone())], &[]);
        Ok((added, unlinked))
    }

    /// `EditSpec::Trim` against the curves of `edges`.
    fn trim(&mut self, h: Handle, edges: &[Handle], at: [f64; 2]) -> Result<Outcome, CoreError> {
        let e = self.entity(h).clone();
        let curve = curve_of(&e)?;
        let length = span_length(&curve, [0.0, 1.0]);
        let eps = TOL / length;
        let mut cuts: Vec<f64> = self
            .edge_curves(edges)
            .iter()
            .flat_map(|c| intersect(&curve, c, tol(at)))
            .map(|x| x.t_a)
            .collect();
        let picked = curve.parameter_at(at);
        let spans = if curve.is_closed() {
            // Round a closed curve the crossings are cyclic: the piece clicked runs between two
            // of them, and what is left is the rest, one piece across the seam.
            cuts = cuts.into_iter().map(|t| t.rem_euclid(1.0)).collect();
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() <= eps);
            if cuts.len() > 1 && cuts[0] + 1.0 - cuts[cuts.len() - 1] <= eps {
                cuts.pop();
            }
            if cuts.len() < 2 {
                return Err(invalid(format!(
                    "{} needs two cutting edges across it to trim",
                    named(&e)
                )));
            }
            let next = cuts.iter().position(|&c| c > picked);
            let span = match next {
                Some(j) if j > 0 => [cuts[j], cuts[j - 1] + 1.0],
                _ => [cuts[0], cuts[cuts.len() - 1]],
            };
            vec![span]
        } else {
            cuts.retain(|&t| t > eps && t < 1.0 - eps);
            if cuts.is_empty() {
                return Err(invalid(format!(
                    "No cutting edge crosses {} there",
                    named(&e)
                )));
            }
            trim_spans(&curve, &cuts, picked, tol(at))
        };
        if spans.is_empty() {
            return Err(invalid(format!(
                "Trimming would leave nothing of {}",
                named(&e)
            )));
        }
        let pieces = spans
            .iter()
            .map(|&s| (span_length(&curve, s), piece(&e, &curve, s)))
            .collect();
        let (added, unlinked) = self.place_pieces(h, pieces)?;
        Ok(edited(added, Vec::new(), 0, unlinked))
    }

    /// `EditSpec::Extend` to the curves of `edges`.
    fn extend(&mut self, h: Handle, edges: &[Handle], at: [f64; 2]) -> Result<Outcome, CoreError> {
        let mut e = self.entity(h).clone();
        let curve = curve_of(&e)?;
        let end = end_of(&e, &curve, at)?;
        let (carrier, room) = match end.way {
            Way::Straight { from, dir, .. } => (
                Curve::Ray(Ray {
                    origin: from,
                    direction: dir,
                }),
                f64::INFINITY,
            ),
            Way::Round {
                centre,
                radius,
                sweep,
                ..
            } => (
                Curve::Circle(KCircle { centre, radius }),
                TAU - sweep - TOL / radius,
            ),
        };
        let reach = self
            .edge_curves(edges)
            .iter()
            .flat_map(|c| intersect(&carrier, c, tol(at)))
            .map(|x| match end.way {
                Way::Straight { .. } => x.t_a,
                Way::Round {
                    centre,
                    angle,
                    sign,
                    ..
                } => {
                    let at = (x.point[1] - centre[1]).atan2(x.point[0] - centre[0]);
                    (sign * (at - angle)).rem_euclid(TAU)
                }
            })
            .filter(|&d| {
                let floor = match end.way {
                    Way::Straight { .. } => TOL,
                    Way::Round { radius, .. } => TOL / radius,
                };
                d > floor && d < room
            })
            .min_by(f64::total_cmp)
            .ok_or_else(|| invalid(format!("{} doesn't reach a boundary that way", named(&e))))?;
        move_end(&mut e, &end, reach);
        self.replace(h, e);
        let unlinked = self.follow_takeoff(&[h], &[]);
        Ok(edited(Vec::new(), Vec::new(), 0, unlinked))
    }

    /// `EditSpec::Break`.
    fn break_at(&mut self, h: Handle, p1: [f64; 2], p2: [f64; 2]) -> Result<Outcome, CoreError> {
        let e = self.entity(h).clone();
        let curve = curve_of(&e)?;
        let (t1, t2) = (curve.parameter_at(p1), curve.parameter_at(p2));
        let spans = break_spans(&curve, t1, t2, tol(p1))
            .ok_or_else(|| invalid(format!("{} can't be broken there", named(&e))))?;
        if spans.is_empty() {
            return Err(invalid(format!(
                "That breaks away all of {}: erase it instead",
                named(&e)
            )));
        }
        if spans == [[0.0, 1.0]] {
            return Err(invalid(format!(
                "Nothing of {} lies between those points",
                named(&e)
            )));
        }
        let pieces = spans
            .iter()
            .map(|&s| (span_length(&curve, s), piece(&e, &curve, s)))
            .collect();
        let (added, unlinked) = self.place_pieces(h, pieces)?;
        Ok(edited(added, Vec::new(), 0, unlinked))
    }

    /// `EditSpec::Join`.
    fn join(&mut self, handles: &[Handle]) -> Result<Outcome, CoreError> {
        let first = self.entity(handles[0]).clone();
        let chains: Vec<_> = handles.iter().map(|&h| chain_of(self.entity(h))).collect();
        let Some(mut chain) = chains[0].clone() else {
            return Err(invalid(format!(
                "{} can't be joined: lines, arcs and open polylines can",
                named(&first)
            )));
        };
        let mut used = vec![false; handles.len()];
        used[0] = true;
        loop {
            let mut grew = false;
            for i in 1..handles.len() {
                let closed = chain.len() > 2 && same(chain[0].0, chain[chain.len() - 1].0);
                let (false, Some(more)) = (used[i] || closed, &chains[i]) else {
                    continue;
                };
                let (start, end) = (chain[0].0, chain[chain.len() - 1].0);
                let (m_start, m_end) = (more[0].0, more[more.len() - 1].0);
                let (more, at_end) = if same(end, m_start) {
                    (more.clone(), true)
                } else if same(end, m_end) {
                    (reversed(more), true)
                } else if same(start, m_end) {
                    (more.clone(), false)
                } else if same(start, m_start) {
                    (reversed(more), false)
                } else {
                    continue;
                };
                if at_end {
                    chain.pop();
                    chain.extend(more);
                } else {
                    chain = more[..more.len() - 1]
                        .iter()
                        .copied()
                        .chain(chain)
                        .collect();
                }
                used[i] = true;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        let joined = used.iter().filter(|&&u| u).count();
        if joined < 2 {
            return Err(invalid(format!(
                "Nothing joins {}: the ends must meet",
                named(&first)
            )));
        }
        let closed = chain.len() > 2 && same(chain[0].0, chain[chain.len() - 1].0);
        if closed {
            chain.pop();
        }
        let mut pl = match &first {
            EntityType::LwPolyline(pl) => pl.clone(),
            other => {
                let mut pl = LwPolyline::new();
                pl.common = other.common().clone();
                pl.elevation = match other {
                    EntityType::Line(l) => l.start.z,
                    EntityType::Arc(a) => a.center.z,
                    _ => 0.0,
                };
                pl
            }
        };
        pl.vertices = chain
            .into_iter()
            .map(|(p, bulge)| LwVertex::with_bulge(Vector2::new(p[0], p[1]), bulge))
            .collect();
        pl.is_closed = closed;
        let h = handles[0];
        self.replace(h, EntityType::LwPolyline(pl));
        let removed: Vec<Handle> = (1..handles.len())
            .filter(|&i| used[i])
            .map(|i| handles[i])
            .collect();
        for &r in &removed {
            self.drawing.doc.remove_entity_arc(r);
        }
        // Rows on the entities joined in measure the joined polyline instead.
        let merged: Vec<(Handle, Handle)> = removed.iter().map(|&r| (r, h)).collect();
        let unlinked = self.follow_edit(&[h], &[], &[], &merged);
        Ok(Outcome::Edited {
            added: Vec::new(),
            removed: removed.iter().map(|h| h.value()).collect(),
            left_alone: handles.len() - joined,
            unlinked,
            joined_rows: self.rows_measuring(h),
            constraints_removed: 0,
        })
    }

    /// `EditSpec::Lengthen`.
    fn lengthen(&mut self, h: Handle, at: [f64; 2], by: LengthenBy) -> Result<Outcome, CoreError> {
        let mut e = self.entity(h).clone();
        let curve = curve_of(&e)?;
        let end = end_of(&e, &curve, at)?;
        let length = span_length(&curve, [0.0, 1.0]);
        let change = match by {
            LengthenBy::Delta(d) => d,
            LengthenBy::Total(total) if total > 0.0 => total - length,
            LengthenBy::Percent(p) if p > 0.0 => length * p / 100.0 - length,
            _ => return Err(invalid("The new length must be more than 0")),
        };
        let d = match end.way {
            Way::Straight { length: side, .. } => {
                if side + change <= TOL {
                    return Err(invalid(format!("That leaves {} too short", named(&e))));
                }
                change
            }
            Way::Round { radius, sweep, .. } => {
                let d = change / radius;
                if sweep + d <= TOL / radius {
                    return Err(invalid(format!("That leaves {} too short", named(&e))));
                }
                if sweep + d >= TAU {
                    return Err(invalid(format!(
                        "That makes {} go round more than a full circle",
                        named(&e)
                    )));
                }
                d
            }
        };
        move_end(&mut e, &end, d);
        self.replace(h, e);
        let unlinked = self.follow_takeoff(&[h], &[]);
        Ok(edited(Vec::new(), Vec::new(), 0, unlinked))
    }

    /// `EditSpec::Reverse`.
    fn reverse(&mut self, handles: &[Handle]) -> Result<Outcome, CoreError> {
        let mut changed = Vec::new();
        for &h in handles {
            let mut e = self.entity(h).clone();
            match &mut e {
                EntityType::Line(l) => std::mem::swap(&mut l.start, &mut l.end),
                EntityType::LwPolyline(pl) if pl.vertices.len() > 1 => {
                    let n = pl.vertices.len();
                    let old = pl.vertices.clone();
                    for (k, v) in pl.vertices.iter_mut().enumerate() {
                        // The side leaving the k-th vertex now is side n - 2 - k run backwards.
                        let side = &old[(2 * n - 2 - k) % n];
                        *v = old[n - 1 - k];
                        v.bulge = -side.bulge;
                        v.start_width = side.end_width;
                        v.end_width = side.start_width;
                    }
                }
                _ => continue,
            }
            self.replace(h, e);
            changed.push(h);
        }
        if changed.is_empty() {
            return Err(invalid(
                "Nothing picked can be reversed: lines and polylines can",
            ));
        }
        let unlinked = self.follow_takeoff(&changed, &[]);
        Ok(edited(
            Vec::new(),
            Vec::new(),
            handles.len() - changed.len(),
            unlinked,
        ))
    }

    /// `EditSpec::Offset`.
    fn offset(&mut self, h: Handle, d: f64, side: [f64; 2]) -> Result<Outcome, CoreError> {
        let e = self.entity(h).clone();
        let curve = curve_of(&e)?;
        let nothing_left = || invalid(format!("Nothing of {} is left at that distance", named(&e)));
        let round = |centre: [f64; 2], r: f64| -> Result<f64, CoreError> {
            let r = if dist(side, centre) > r { r + d } else { r - d };
            if r <= TOL {
                return Err(nothing_left());
            }
            Ok(r)
        };
        let copies: Vec<EntityType> = match (&e, &curve) {
            (EntityType::Line(l), Curve::Line(k)) => {
                let along = unit(k.direction()).expect("a line with length");
                let s = cross(along, sub(side, k.start));
                if s.abs() <= TOL {
                    return Err(invalid("Pick the side away from the line"));
                }
                let shift = [-along[1] * s.signum() * d, along[0] * s.signum() * d];
                let mut l = l.clone();
                l.start.x += shift[0];
                l.start.y += shift[1];
                l.end.x += shift[0];
                l.end.y += shift[1];
                vec![EntityType::Line(l)]
            }
            (EntityType::Circle(c), Curve::Circle(k)) => {
                let mut c = c.clone();
                c.radius = round(k.centre, k.radius)?;
                vec![EntityType::Circle(c)]
            }
            (EntityType::Arc(a), Curve::Arc(k)) => {
                let mut a = a.clone();
                a.radius = round(k.centre, k.radius)?;
                vec![EntityType::Arc(a)]
            }
            (EntityType::LwPolyline(pl), Curve::Polyline(k)) => {
                let found = offset_polyline(k, d, side);
                if found.is_empty() {
                    return Err(nothing_left());
                }
                found
                    .into_iter()
                    .map(|o| {
                        let mut copy = pl.clone();
                        copy.vertices = o
                            .vertices
                            .iter()
                            .map(|v| {
                                let mut lw = LwVertex::with_bulge(
                                    Vector2::new(v.position[0], v.position[1]),
                                    v.bulge,
                                );
                                lw.start_width = pl.constant_width;
                                lw.end_width = pl.constant_width;
                                lw
                            })
                            .collect();
                        copy.is_closed = o.closed;
                        EntityType::LwPolyline(copy)
                    })
                    .collect()
            }
            _ => unreachable!("curve_of made the curve of this entity"),
        };
        let owner = self.sheet_owner();
        let handles = copies
            .into_iter()
            .map(|c| self.add_fresh(c, owner))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Outcome::Added { handles })
    }

    /// `EditSpec::Fillet` and `EditSpec::Chamfer`.
    fn corner(
        &mut self,
        a: Handle,
        b: Handle,
        at_a: [f64; 2],
        at_b: [f64; 2],
        how: Corner,
    ) -> Result<Outcome, CoreError> {
        if a == b {
            return self.polyline_corner(a, at_a, at_b, how);
        }
        let (ea, eb) = (self.entity(a).clone(), self.entity(b).clone());
        let (EntityType::Line(la), EntityType::Line(lb)) = (&ea, &eb) else {
            return Err(invalid(
                "Fillet and chamfer take two lines, or two straight sides of one polyline",
            ));
        };
        let (pa, pb) = ([xy(la.start), xy(la.end)], [xy(lb.start), xy(lb.end)]);
        let (da, db) = (sub(pa[1], pa[0]), sub(pb[1], pb[0]));
        let apex = match line_line(pa[0], da, pb[0], db) {
            Some((t, _)) if unit(da).is_some() && unit(db).is_some() => add(pa[0], da, t),
            _ => return Err(invalid("The lines are parallel: they have no corner")),
        };
        // Each line keeps the side of the corner it was picked on.
        let keep = |ends: [[f64; 2]; 2], at: [f64; 2]| -> [f64; 2] {
            let u = unit(sub(ends[1], ends[0])).expect("checked above");
            let mut s = dot(sub(at, apex), u);
            if s.abs() <= TOL {
                let far = if dist(ends[0], apex) > dist(ends[1], apex) {
                    ends[0]
                } else {
                    ends[1]
                };
                s = dot(sub(far, apex), u);
            }
            [u[0] * s.signum(), u[1] * s.signum()]
        };
        let (ka, kb) = (keep(pa, at_a), keep(pb, at_b));
        let (ta, tb, arc) = match how {
            Corner::Round(r) if r > 0.0 => {
                let f = fillet_between_rays(apex, ka, kb, r)
                    .ok_or_else(|| invalid("The lines are in line: they have no corner"))?;
                (f.tangent1, f.tangent2, Some(f))
            }
            Corner::Round(_) => (apex, apex, None),
            Corner::Bevel(d1, d2) => (add(apex, ka, d1), add(apex, kb, d2), None),
        };
        let mut new_a = ea.clone();
        let mut new_b = eb.clone();
        for (line, way, to) in [(&mut new_a, ka, ta), (&mut new_b, kb, tb)] {
            let EntityType::Line(l) = line else {
                unreachable!("lines")
            };
            let from = dot(sub(to, apex), way);
            let (s0, s1) = (
                dot(sub(xy(l.start), apex), way),
                dot(sub(xy(l.end), apex), way),
            );
            if s0.max(s1) <= from + TOL {
                return Err(invalid(match how {
                    Corner::Round(_) => "The radius is too large for these lines",
                    Corner::Bevel(..) => "The distances are too large for these lines",
                }));
            }
            let v = if s0 < s1 { &mut l.start } else { &mut l.end };
            v.x = to[0];
            v.y = to[1];
        }
        self.replace(a, new_a);
        self.replace(b, new_b);
        let owner = self.sheet_owner();
        let mut added = Vec::new();
        if let Some(f) = arc {
            let mut arc = opencadcodec::Arc::new();
            arc.common = la.common.clone();
            arc.center = Vector3::new(f.centre[0], f.centre[1], la.start.z);
            arc.radius = match how {
                Corner::Round(r) => r,
                Corner::Bevel(..) => unreachable!("a fillet"),
            };
            arc.start_angle = f.start_angle;
            arc.end_angle = f.end_angle;
            added.push(self.add_fresh(EntityType::Arc(arc), owner)?);
        } else if !same(ta, tb) {
            let mut across = la.clone();
            across.start = Vector3::new(ta[0], ta[1], la.start.z);
            across.end = Vector3::new(tb[0], tb[1], la.start.z);
            added.push(self.add_fresh(EntityType::Line(across), owner)?);
        }
        let unlinked = self.follow_takeoff(&[a, b], &[]);
        Ok(edited(added, Vec::new(), 0, unlinked))
    }

    /// A fillet or chamfer on the corner between two straight sides of one polyline.
    fn polyline_corner(
        &mut self,
        h: Handle,
        at_a: [f64; 2],
        at_b: [f64; 2],
        how: Corner,
    ) -> Result<Outcome, CoreError> {
        let mut e = self.entity(h).clone();
        let curve = curve_of(&e)?;
        let (EntityType::LwPolyline(pl), Curve::Polyline(k)) = (&mut e, &curve) else {
            return Err(invalid(
                "Fillet and chamfer take two lines, or two straight sides of one polyline",
            ));
        };
        let n = pl.vertices.len();
        let count = sides(k).len();
        let side_at =
            |p: [f64; 2]| ((curve.parameter_at(p) * count as f64).floor() as usize).min(count - 1);
        let (i, j) = (side_at(at_a), side_at(at_b));
        let (first, corner, how) = if (i + 1) % n == j && (i + 1 < n || pl.is_closed) {
            (i, (i + 1) % n, how)
        } else if (j + 1) % n == i && (j + 1 < n || pl.is_closed) {
            // The side picked first comes after the corner: its distance goes there.
            let how = match how {
                Corner::Bevel(d1, d2) => Corner::Bevel(d2, d1),
                round => round,
            };
            (j, (j + 1) % n, how)
        } else {
            return Err(invalid("Pick two sides that meet at a corner"));
        };
        if pl.vertices[first].bulge != 0.0 || pl.vertices[corner].bulge != 0.0 {
            return Err(invalid(
                "Only straight sides of a polyline take a fillet or a chamfer",
            ));
        }
        let at = |v: usize| {
            let p = pl.vertices[v].location;
            [p.x, p.y]
        };
        let (prev, apex, next) = (at(first), at(corner), at((corner + 1) % n));
        let (Some(d1), Some(d2)) = (unit(sub(prev, apex)), unit(sub(next, apex))) else {
            return Err(invalid("Those sides have no length"));
        };
        let (t1, t2, bulge) = match how {
            Corner::Round(r) if r > 0.0 => {
                let f = fillet_between_rays(apex, d1, d2, r)
                    .ok_or_else(|| invalid("Those sides are in line: they have no corner"))?;
                let turn = cross(sub(apex, prev), sub(next, apex)).signum();
                let sweep = PI - dot(d1, d2).clamp(-1.0, 1.0).acos();
                (f.tangent1, f.tangent2, turn * (sweep / 4.0).tan())
            }
            Corner::Round(_) => return Ok(Outcome::Nothing),
            Corner::Bevel(a, b) => (add(apex, d1, a), add(apex, d2, b), 0.0),
        };
        if dist(t1, apex) > dist(prev, apex) + TOL || dist(t2, apex) > dist(next, apex) + TOL {
            return Err(invalid(match how {
                Corner::Round(_) => "The radius is too large for those sides",
                Corner::Bevel(..) => "The distances are too large for those sides",
            }));
        }
        let mut second = pl.vertices[corner];
        second.location = Vector2::new(t2[0], t2[1]);
        let v = &mut pl.vertices[corner];
        v.location = Vector2::new(t1[0], t1[1]);
        v.bulge = bulge;
        pl.vertices.insert(corner + 1, second);
        self.replace(h, e);
        let unlinked = self.follow_takeoff(&[h], &[]);
        Ok(edited(Vec::new(), Vec::new(), 0, unlinked))
    }

    /// `EditSpec::Blend`.
    fn blend(
        &mut self,
        a: Handle,
        b: Handle,
        at_a: [f64; 2],
        at_b: [f64; 2],
    ) -> Result<Outcome, CoreError> {
        if a == b {
            return Err(invalid("Blend joins two different entities"));
        }
        let (ea, eb) = (self.entity(a).clone(), self.entity(b).clone());
        let end_a = end_of(&ea, &curve_of(&ea)?, at_a)?;
        let end_b = end_of(&eb, &curve_of(&eb)?, at_b)?;
        let ((p0, t0), (p1, t1)) = (end_a.way.out(), end_b.way.out());
        if same(p0, p1) {
            return Err(invalid("Those ends meet already"));
        }
        // Out of the first, and into the second against its own way out.
        let curve = NurbsCurve::interpolate(
            &[p0, p1],
            Some(t0),
            Some([-t1[0], -t1[1]]),
            Parameterization::Chord,
        )
        .ok_or_else(|| invalid("Those ends can't be blended"))?;
        let v3 = |p: [f64; 2]| Vector3::new(p[0], p[1], 0.0);
        let mut spline = Spline::from_fit_points(vec![v3(p0), v3(p1)]);
        spline.common = ea.common().clone();
        spline.degree = curve.degree() as i32;
        spline.control_points = curve.control_points().iter().map(|&p| v3(p)).collect();
        spline.knots = curve.knots().to_vec();
        spline.begin_tangent = v3(t0);
        spline.end_tangent = v3([-t1[0], -t1[1]]);
        spline.flags.planar = true;
        let owner = self.sheet_owner();
        let handle = self.add_fresh(EntityType::Spline(spline), owner)?;
        Ok(Outcome::Added {
            handles: vec![handle],
        })
    }

    /// `EditSpec::PEdit`.
    fn pedit(&mut self, h: Handle, op: PEditOp) -> Result<Outcome, CoreError> {
        let mut e = self.entity(h).clone();
        let name = named(&e);
        if let PEditOp::ConvertToPolyline = op {
            let chain = match &e {
                EntityType::LwPolyline(_) => return Ok(Outcome::Nothing),
                EntityType::Line(_) | EntityType::Arc(_) => chain_of(&e),
                _ => None,
            }
            .ok_or_else(|| {
                invalid(format!(
                    "{} can't become a polyline: lines and arcs lying flat can",
                    named(&e)
                ))
            })?;
            let mut pl = LwPolyline::new();
            pl.common = e.common().clone();
            pl.elevation = match &e {
                EntityType::Line(l) => l.start.z,
                EntityType::Arc(a) => a.center.z,
                _ => 0.0,
            };
            pl.vertices = chain
                .into_iter()
                .map(|(p, bulge)| LwVertex::with_bulge(Vector2::new(p[0], p[1]), bulge))
                .collect();
            self.replace(h, EntityType::LwPolyline(pl));
            let unlinked = self.follow_takeoff(&[h], &[]);
            return Ok(edited(Vec::new(), Vec::new(), 0, unlinked));
        }
        let curve = match &e {
            EntityType::LwPolyline(_) => curve_of(&e)?,
            _ => {
                return Err(invalid(format!(
                    "{} is not a polyline: turn it into one first",
                    named(&e)
                )))
            }
        };
        let Curve::Polyline(k) = &curve else {
            unreachable!("a polyline's curve")
        };
        let EntityType::LwPolyline(pl) = &mut e else {
            unreachable!("matched above")
        };
        let n = pl.vertices.len();
        match op {
            PEditOp::Close => pl.is_closed = true,
            PEditOp::Open => pl.is_closed = false,
            PEditOp::Width(w) => {
                finite(&[w])?;
                if w < 0.0 {
                    return Err(invalid("The width must be 0 or more"));
                }
                pl.constant_width = w;
                for v in &mut pl.vertices {
                    v.start_width = w;
                    v.end_width = w;
                }
            }
            PEditOp::AddVertex(at) => {
                finite(&at)?;
                let count = sides(k).len();
                let s = curve.parameter_at(at) * count as f64;
                let i = (s.floor() as usize).min(count - 1);
                let f = s - i as f64;
                let (a, b, bulge) = sides(k)[i];
                if f * side_length(a, b, bulge) <= TOL
                    || (1.0 - f) * side_length(a, b, bulge) <= TOL
                {
                    return Err(invalid("There is a vertex there already"));
                }
                let p = side_point(a, b, bulge, f);
                let mut v = pl.vertices[i];
                v.location = Vector2::new(p[0], p[1]);
                v.bulge = part_bulge(bulge, 1.0 - f);
                pl.vertices[i].bulge = part_bulge(bulge, f);
                pl.vertices.insert(i + 1, v);
            }
            PEditOp::DeleteVertex(at) => {
                finite(&at)?;
                if n <= if pl.is_closed { 3 } else { 2 } {
                    return Err(invalid(format!("{name} has too few vertices to lose one")));
                }
                let c = (0..n)
                    .min_by(|&x, &y| {
                        let d = |i: usize| {
                            let p = pl.vertices[i].location;
                            dist([p.x, p.y], at)
                        };
                        d(x).total_cmp(&d(y))
                    })
                    .expect("vertices");
                pl.vertices.remove(c);
                // The side now running across the vertex removed is straight.
                if c > 0 {
                    pl.vertices[c - 1].bulge = 0.0;
                } else if pl.is_closed {
                    pl.vertices.last_mut().expect("vertices").bulge = 0.0;
                }
            }
            PEditOp::FitCurve => {
                let points: Vec<[f64; 2]> = k.vertices.iter().map(|v| v.position).collect();
                let fitted = fit_arc_chain(&points, pl.is_closed, &vec![None; n])
                    .ok_or_else(|| invalid("Those vertices can't be fitted with arcs"))?;
                let old = pl.vertices.clone();
                pl.vertices = fitted
                    .into_iter()
                    .map(|f| {
                        let mut v = old[f.source.min(n - 1)];
                        v.location = Vector2::new(f.point[0], f.point[1]);
                        v.bulge = f.bulge;
                        v
                    })
                    .collect();
            }
            PEditOp::Decurve => {
                for v in &mut pl.vertices {
                    v.bulge = 0.0;
                }
            }
            PEditOp::ConvertToPolyline => unreachable!("handled above"),
        }
        self.replace(h, e);
        let unlinked = self.follow_takeoff(&[h], &[]);
        Ok(edited(Vec::new(), Vec::new(), 0, unlinked))
    }

    /// `EditSpec::Overkill`.
    fn overkill(&mut self, handles: &[Handle], tol: f64) -> Result<Outcome, CoreError> {
        let order = |h: Handle| handles.iter().position(|&x| x == h).expect("a target");
        let mut removed: HashSet<Handle> = HashSet::new();
        let mut changed: HashSet<Handle> = HashSet::new();
        // Lines: one on another become one. Swept along x (then y), repeated until no merge.
        let mut lines: Vec<(Handle, [[f64; 2]; 2])> = handles
            .iter()
            .filter_map(|&h| match self.entity(h) {
                EntityType::Line(l) if dist(xy(l.start), xy(l.end)) > tol => {
                    Some((h, [xy(l.start), xy(l.end)]))
                }
                _ => None,
            })
            .collect();
        loop {
            lines.retain(|(h, _)| !removed.contains(h));
            let lo = |s: &[[f64; 2]; 2]| [s[0][0].min(s[1][0]), s[0][1].min(s[1][1])];
            lines.sort_by(|a, b| {
                let (p, q) = (lo(&a.1), lo(&b.1));
                p[0].total_cmp(&q[0]).then(p[1].total_cmp(&q[1]))
            });
            let mut merged = false;
            for i in 0..lines.len() {
                if removed.contains(&lines[i].0) {
                    continue;
                }
                for j in i + 1..lines.len() {
                    let x_max = lines[i].1[0][0].max(lines[i].1[1][0]);
                    if lo(&lines[j].1)[0] > x_max + tol {
                        break;
                    }
                    if removed.contains(&lines[j].0)
                        || removed.contains(&lines[i].0)
                        || !self.same_style(lines[i].0, lines[j].0)
                    {
                        continue;
                    }
                    let Some(union) = overlap(lines[i].1, lines[j].1, tol) else {
                        continue;
                    };
                    // The one picked first stays, running its own way, covering both.
                    let (keep, drop) = if order(lines[i].0) < order(lines[j].0) {
                        (i, j)
                    } else {
                        (j, i)
                    };
                    let way = unit(sub(lines[keep].1[1], lines[keep].1[0])).expect("length");
                    let union = if dot(sub(union[1], union[0]), way) >= 0.0 {
                        union
                    } else {
                        [union[1], union[0]]
                    };
                    lines[keep].1 = union;
                    changed.insert(lines[keep].0);
                    removed.insert(lines[drop].0);
                    merged = true;
                }
            }
            if !merged {
                break;
            }
        }
        // Circles, arcs, points and polylines drawn twice.
        let mut signed: Vec<(Handle, Vec<f64>)> = handles
            .iter()
            .filter_map(|&h| signature(self.entity(h)).map(|s| (h, s)))
            .collect();
        signed.sort_by(|a, b| a.1[1].total_cmp(&b.1[1]));
        for i in 0..signed.len() {
            for j in i + 1..signed.len() {
                if signed[j].1[1] > signed[i].1[1] + tol {
                    break;
                }
                let (a, b) = (&signed[i], &signed[j]);
                if removed.contains(&a.0) || removed.contains(&b.0) {
                    continue;
                }
                let alike = a.1.len() == b.1.len()
                    && a.1[0] == b.1[0]
                    && a.1.iter().zip(&b.1).all(|(x, y)| (x - y).abs() <= tol)
                    && self.same_style(a.0, b.0);
                if alike {
                    removed.insert(if order(a.0) < order(b.0) { b.0 } else { a.0 });
                }
            }
        }
        for (h, ends) in &lines {
            if changed.contains(h) && !removed.contains(h) {
                let mut e = self.entity(*h).clone();
                if let EntityType::Line(l) = &mut e {
                    l.start.x = ends[0][0];
                    l.start.y = ends[0][1];
                    l.end.x = ends[1][0];
                    l.end.y = ends[1][1];
                }
                self.replace(*h, e);
            }
        }
        let mut removed: Vec<Handle> = removed.into_iter().collect();
        removed.sort_by_key(|&h| order(h));
        for &h in &removed {
            self.drawing.doc.remove_entity_arc(h);
        }
        let changed: Vec<Handle> = changed
            .into_iter()
            .filter(|h| !removed.contains(h))
            .collect();
        let unlinked = self.follow_takeoff(&changed, &removed);
        Ok(edited(
            Vec::new(),
            removed.iter().map(|h| h.value()).collect(),
            0,
            unlinked,
        ))
    }

    /// The two entities share their layer, colour, linetype and lineweight.
    fn same_style(&self, a: Handle, b: Handle) -> bool {
        let (a, b) = (self.entity(a).common(), self.entity(b).common());
        a.layer == b.layer
            && a.color == b.color
            && a.linetype.eq_ignore_ascii_case(&b.linetype)
            && a.line_weight == b.line_weight
    }

    /// `EditSpec::Flatten`.
    fn flatten_all(&mut self, handles: &[Handle]) -> Result<Outcome, CoreError> {
        let mut changed = Vec::new();
        let mut left_alone = 0;
        for &h in handles {
            let mut e = self.entity(h).clone();
            if !flattened(&mut e) {
                left_alone += 1;
                continue;
            }
            if &e != self.entity(h) {
                self.replace(h, e);
                changed.push(h);
            }
        }
        let unlinked = self.follow_takeoff(&changed, &[]);
        Ok(edited(Vec::new(), Vec::new(), left_alone, unlinked))
    }
}

fn edited(added: Vec<u64>, removed: Vec<u64>, left_alone: usize, unlinked: usize) -> Outcome {
    Outcome::Edited {
        added,
        removed,
        left_alone,
        unlinked,
        joined_rows: 0,
        constraints_removed: 0,
    }
}

/// When the segments `a` and `b` lie on one line (each end of `b` within `tol` of `a`'s line)
/// and overlap by more than `tol`, the segment covering both, along `a`.
fn overlap(a: [[f64; 2]; 2], b: [[f64; 2]; 2], tol: f64) -> Option<[[f64; 2]; 2]> {
    let u = unit(sub(a[1], a[0]))?;
    let off = |p: [f64; 2]| cross(u, sub(p, a[0])).abs();
    if off(b[0]) > tol || off(b[1]) > tol {
        return None;
    }
    let along = |p: [f64; 2]| dot(sub(p, a[0]), u);
    let la = dist(a[0], a[1]);
    let (b0, b1) = (along(b[0]), along(b[1]));
    let (lo, hi) = (b0.min(b1), b0.max(b1));
    if la.min(hi) - lo.max(0.0) <= tol {
        return None;
    }
    let (from, to) = (lo.min(0.0), hi.max(la));
    Some([add(a[0], u, from), add(a[0], u, to)])
}

/// What makes two circles, arcs, points or polylines the same: a kind number, then numbers that
/// must agree within the tolerance (the second is an x, for sorting).
fn signature(e: &EntityType) -> Option<Vec<f64>> {
    match e {
        EntityType::Circle(c) => Some(vec![1.0, c.center.x, c.center.y, c.center.z, c.radius]),
        EntityType::Arc(a) => Some(vec![
            2.0,
            a.center.x,
            a.center.y,
            a.center.z,
            a.radius,
            normalize_angle(a.start_angle) * a.radius,
            normalize_angle(a.end_angle) * a.radius,
        ]),
        EntityType::Point(p) => Some(vec![3.0, p.location.x, p.location.y, p.location.z]),
        EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            let mut s =
                vec![4.0 + pl.vertices.len() as f64 * 2.0 + f64::from(u8::from(pl.is_closed))];
            for v in &pl.vertices {
                s.extend([v.location.x, v.location.y, v.bulge]);
            }
            s.push(pl.elevation);
            Some(s)
        }
        _ => None,
    }
}

/// `e` put on the plan facing up, looking the same from above; false (and `e` as it was) when
/// it can't be: tilted, or a text or block reference facing down, or not a kind flattened here.
fn flattened(e: &mut EntityType) -> bool {
    let z_up = Vector3::new(0.0, 0.0, 1.0);
    let down = |n: &Vector3| n.x.abs() < 1e-9 && n.y.abs() < 1e-9 && n.z < 0.0;
    // A point stored in the object coordinates of `n`, on plan (x mirrored when facing down).
    let on_plan = |n: Vector3, p: Vector3| {
        let w = Matrix3::arbitrary_axis(n) * p;
        Vector3::new(w.x, w.y, 0.0)
    };
    let done = match e {
        EntityType::Line(l) => {
            l.start.z = 0.0;
            l.end.z = 0.0;
            l.normal = z_up;
            true
        }
        EntityType::Point(p) => {
            p.location.z = 0.0;
            true
        }
        EntityType::Circle(c) if up(&c.normal) || down(&c.normal) => {
            c.center = on_plan(c.normal, c.center);
            c.normal = z_up;
            true
        }
        EntityType::Arc(a) if up(&a.normal) || down(&a.normal) => {
            if down(&a.normal) {
                let (s, t) = (a.start_angle, a.end_angle);
                a.start_angle = normalize_angle(PI - t);
                a.end_angle = normalize_angle(PI - s);
            }
            a.center = on_plan(a.normal, a.center);
            a.normal = z_up;
            true
        }
        EntityType::Ellipse(el) if up(&el.normal) || down(&el.normal) => {
            if down(&el.normal) {
                let (s, t) = (el.start_parameter, el.end_parameter);
                el.start_parameter = normalize_angle(-t);
                el.end_parameter = normalize_angle(-s);
                if (t - s - TAU).abs() < 1e-12 {
                    el.start_parameter = 0.0;
                    el.end_parameter = TAU;
                }
            }
            el.center.z = 0.0;
            el.major_axis.z = 0.0;
            el.normal = z_up;
            true
        }
        EntityType::LwPolyline(pl) if up(&pl.normal) || down(&pl.normal) => {
            if down(&pl.normal) {
                for v in &mut pl.vertices {
                    v.location.x = -v.location.x;
                    v.bulge = -v.bulge;
                }
            }
            pl.elevation = 0.0;
            pl.normal = z_up;
            true
        }
        EntityType::Polyline3D(pl) => {
            for v in &mut pl.vertices {
                v.position.z = 0.0;
            }
            true
        }
        EntityType::Spline(s) => {
            for p in s.control_points.iter_mut().chain(s.fit_points.iter_mut()) {
                p.z = 0.0;
            }
            s.begin_tangent.z = 0.0;
            s.end_tangent.z = 0.0;
            s.normal = z_up;
            true
        }
        EntityType::Text(t) if up(&t.normal) => {
            t.insertion_point.z = 0.0;
            if let Some(p) = &mut t.alignment_point {
                p.z = 0.0;
            }
            true
        }
        EntityType::MText(t) if up(&t.normal) => {
            t.insertion_point.z = 0.0;
            true
        }
        EntityType::Insert(i) if up(&i.normal) => {
            i.insert_point.z = 0.0;
            for a in &mut i.attributes {
                a.insertion_point.z = 0.0;
                a.alignment_point.z = 0.0;
            }
            true
        }
        _ => false,
    };
    if done {
        e.common_mut().graphic_data = None;
    }
    done
}
