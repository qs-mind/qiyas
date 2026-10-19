//! Parametric constraints on the sheet on screen: geometric ones (`Command::AddConstraint`,
//! `AutoConstrain`) and dimensional ones (`AddDimConstraint`) with their parameters
//! (`SetParameter`), solved by `qiyas_param`.
//!
//! A constraint holds parts of entities (`Ref`: a vertex, a centre or an edge of a line, a
//! lightweight polyline, an arc or a circle); other entities are refused. It is stored on an
//! entity as `PARAM_APP` xdata (JSON strings, one constraint after an `Integer16(1)` each), so it
//! goes with the drawing through DWG and DXF: a geometric one on the entity it holds that holds
//! the fewest, a dimensional one on its own Dimension, which draws it like any dimension with the
//! text `[d1=<>]` (`[fx: d1=<>]` when its expression uses other parameters): the brackets are the
//! lock mark. A stored constraint names its holder; a copy of the holder (Qiyas's or AutoCAD's)
//! keeps stale xdata that is never read.
//!
//! Every drawing command keeps them (`Session::run_as`): an entity a command erased, or whose
//! vertices or kind it changed, loses its constraints (a dimensional one its Dimension too, and
//! the parameters that used it keep their values); one it moved is held where it went while the
//! constraints it is part of are solved again in the same undo step, and a conflict (a fixed point
//! moved, a dimension scaled) refuses the whole command.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use opencadcodec::entities::{Dimension, EntityCommon};
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, EntityType, Handle, Vector3};
use opencadkernel::geom2d::constraint_inference::{
    infer_constraints_with_settings, Endpoint, InferenceKind, InferenceSettings,
    InferredConstraint, ParametricPrimitive,
};
use opencadkernel::geom2d::{Arc as KArc, Circle as KCircle, Line as KLine};
use qiyas_param::expr::normalize;
use qiyas_param::{evaluate_each, Constraint, Curve, Expr, ExprError, PointId, Sketch, SolveError};
use serde::{Deserialize, Serialize};

use crate::session::variant_name;
use crate::{CoreError, DimSpec, Session, Target};

/// The xdata application holding constraints (never `QIYAS`: `embed_measurements` deletes those).
pub const PARAM_APP: &str = "QIYAS-PARAM";
/// Longest xdata string written (DXF allows 255 bytes).
const CHUNK: usize = 250;
/// Most constraints one AUTOCONSTRAIN adds: more is a whole plan in one solve.
const MAX_AUTO: usize = 2000;
/// Moves smaller than this, relative to how far the point is from the origin (float noise on a
/// site grid), are no moves.
const SAME: f64 = 1e-12;

/// `a` and `b` are one point, within float noise at their distance from the origin.
fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    dist(a, b) <= SAME * (1.0 + a[0].abs().max(a[1].abs()))
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

/// What a constraint keeps: the geometric kinds, then the dimensional ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Coincident,
    Collinear,
    Concentric,
    Fixed,
    Parallel,
    Perpendicular,
    Horizontal,
    Vertical,
    Tangent,
    Smooth,
    Symmetric,
    Equal,
    Linear,
    Aligned,
    Angular,
    Radius,
    Diameter,
}

impl ConstraintKind {
    pub fn is_dimensional(self) -> bool {
        matches!(
            self,
            ConstraintKind::Linear
                | ConstraintKind::Aligned
                | ConstraintKind::Angular
                | ConstraintKind::Radius
                | ConstraintKind::Diameter
        )
    }

    /// A new parameter's name starts with this (as AutoCAD's: d1, ang1, rad1, dia1).
    fn prefix(self) -> &'static str {
        match self {
            ConstraintKind::Angular => "ang",
            ConstraintKind::Radius => "rad",
            ConstraintKind::Diameter => "dia",
            _ => "d",
        }
    }

    fn inference(self) -> Option<InferenceKind> {
        Some(match self {
            ConstraintKind::Coincident => InferenceKind::Coincident,
            ConstraintKind::Collinear => InferenceKind::Collinear,
            ConstraintKind::Parallel => InferenceKind::Parallel,
            ConstraintKind::Perpendicular => InferenceKind::Perpendicular,
            ConstraintKind::Tangent => InferenceKind::Tangent,
            ConstraintKind::Concentric => InferenceKind::Concentric,
            ConstraintKind::Horizontal => InferenceKind::Horizontal,
            ConstraintKind::Vertical => InferenceKind::Vertical,
            ConstraintKind::Equal => InferenceKind::Equal,
            _ => return None,
        })
    }
}

/// The part of an entity a constraint holds: a vertex (a line's start 0 and end 1, a polyline's
/// vertex, an arc's start 0 and end 1), a circle's or an arc's centre, or an edge (a line 0, a
/// polyline's straight segment from vertex `index`, a circle or an arc 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum Part {
    Point { index: usize },
    Centre,
    Edge { index: usize },
}

/// A part of the entity `handle` (at the top of the sheet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ref {
    pub handle: u64,
    pub part: Part,
}

/// What `Command::AutoConstrain` looks for, and how close the drawing must be already.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoSettings {
    /// The kinds to apply, first first. Without Equal by default (opposite sides of a rectangle
    /// are not meant to stay equal).
    #[serde(default = "AutoSettings::default_kinds")]
    pub kinds: Vec<ConstraintKind>,
    /// Drawing units; left out, 0.05 mm in the drawing's units (as AutoCAD's 0.05 in a millimetre
    /// drawing), so ends 5 cm apart in a metre plan are never welded.
    #[serde(default)]
    pub distance_tolerance: Option<f64>,
    /// Degrees.
    #[serde(default = "AutoSettings::default_angle")]
    pub angle_tolerance: f64,
}

impl AutoSettings {
    fn default_kinds() -> Vec<ConstraintKind> {
        use ConstraintKind::*;
        vec![
            Coincident,
            Collinear,
            Parallel,
            Perpendicular,
            Tangent,
            Concentric,
            Horizontal,
            Vertical,
        ]
    }
    fn default_angle() -> f64 {
        1.0
    }
}

impl Default for AutoSettings {
    fn default() -> Self {
        AutoSettings {
            kinds: AutoSettings::default_kinds(),
            distance_tolerance: None,
            angle_tolerance: AutoSettings::default_angle(),
        }
    }
}

/// A constraint on the sheet on screen (`Command::ListConstraints`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConstraintInfo {
    pub id: u32,
    pub kind: ConstraintKind,
    /// The entities it holds, each once, in order.
    pub handles: Vec<u64>,
    /// Where its first part is, in drawing coordinates.
    pub at: [f64; 2],
    /// A dimensional constraint's parameter, expression, value (drawing units or degrees) and
    /// Dimension.
    pub name: Option<String>,
    pub expression: Option<String>,
    pub value: Option<f64>,
    pub dimension: Option<u64>,
}

/// A dimensional constraint's parameter (`Command::ListParameters`), on any sheet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParameterInfo {
    pub id: u32,
    pub name: String,
    pub kind: ConstraintKind,
    pub expression: String,
    /// Drawing units, or degrees for an angle; none when the expression can't be worked out.
    pub value: Option<f64>,
    pub dimension: u64,
    /// Why the expression can't be worked out (its constraint is left out of every solve until
    /// it is set again or deleted).
    pub error: Option<String>,
}

/// A constraint as stored in the drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Stored {
    id: u32,
    holder: u64,
    kind: ConstraintKind,
    refs: Vec<Ref>,
    /// Where a fixed point stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dim: Option<DimData>,
}

/// A dimensional constraint's parameter and how it measures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DimData {
    name: String,
    expression: String,
    /// A linear one's direction (radians).
    #[serde(default)]
    rotation: f64,
    /// An angular one: which way along each line its rays run (+1 or -1).
    #[serde(default = "DimData::forward")]
    sides: [f64; 2],
}

impl DimData {
    fn forward() -> [f64; 2] {
        [1.0, 1.0]
    }
}

/// The constraints an entity's xdata holds (any holder named).
fn read_record(common: &EntityCommon) -> Vec<Stored> {
    let Some(record) = common.extended_data.get_record(PARAM_APP) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut text: Option<String> = None;
    let mut flush = |text: &mut Option<String>| {
        if let Some(t) = text.take() {
            if let Ok(s) = serde_json::from_str::<Stored>(&t) {
                out.push(s);
            }
        }
    };
    for v in &record.values {
        match v {
            XDataValue::Integer16(1) => {
                flush(&mut text);
                text = Some(String::new());
            }
            XDataValue::String(s) => {
                if let Some(t) = text.as_mut() {
                    t.push_str(s);
                }
            }
            _ => {}
        }
    }
    flush(&mut text);
    out
}

/// Keeps `list` as the entity's constraints (none: the record goes).
fn write_record(common: &mut EntityCommon, list: &[Stored]) {
    if list.is_empty() {
        common.extended_data.remove_record(PARAM_APP);
        return;
    }
    let mut record = ExtendedDataRecord::new(PARAM_APP);
    for s in list {
        record.add_value(XDataValue::Integer16(1));
        let json = serde_json::to_string(s).expect("plain data");
        let mut rest = json.as_str();
        while !rest.is_empty() {
            let mut cut = rest.len().min(CHUNK);
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            let (head, tail) = rest.split_at(cut);
            record.add_value(XDataValue::String(head.to_string()));
            rest = tail;
        }
    }
    common.extended_data.upsert_record(record);
}

/// An entity's shape as constraints see it, in drawing coordinates.
#[derive(Debug, Clone, PartialEq)]
enum Geo {
    Line([f64; 2], [f64; 2]),
    Poly {
        verts: Vec<[f64; 2]>,
        bulges: Vec<f64>,
        closed: bool,
    },
    Circle([f64; 2], f64),
    /// Centre, radius, start and end angles (radians).
    Arc([f64; 2], f64, f64, f64),
}

fn xy(v: &Vector3) -> [f64; 2] {
    [v.x, v.y]
}

fn flat(normal: &Vector3) -> bool {
    normal.x.abs() < 1e-9 && normal.y.abs() < 1e-9 && normal.z > 0.0
}

/// Its shape, or why constraints can't hold it.
fn geo_of(e: &EntityType) -> Result<Geo, String> {
    let not_flat = || "Constraints hold entities drawn flat in plan only".to_string();
    match e {
        EntityType::Line(l) => Ok(Geo::Line(xy(&l.start), xy(&l.end))),
        EntityType::LwPolyline(p) => {
            if !flat(&p.normal) {
                return Err(not_flat());
            }
            Ok(Geo::Poly {
                verts: p
                    .vertices
                    .iter()
                    .map(|v| [v.location.x, v.location.y])
                    .collect(),
                bulges: p.vertices.iter().map(|v| v.bulge).collect(),
                closed: p.is_closed,
            })
        }
        EntityType::Circle(c) if flat(&c.normal) => Ok(Geo::Circle(xy(&c.center), c.radius)),
        EntityType::Arc(a) if flat(&a.normal) => Ok(Geo::Arc(
            xy(&a.center),
            a.radius,
            a.start_angle,
            a.end_angle,
        )),
        EntityType::Circle(_) | EntityType::Arc(_) => Err(not_flat()),
        other => Err(format!(
            "Constraints hold lines, polylines, arcs and circles, not a {}",
            variant_name(other)
        )),
    }
}

fn polar(c: [f64; 2], r: f64, a: f64) -> [f64; 2] {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn unit(a: [f64; 2]) -> Option<[f64; 2]> {
    let n = a[0].hypot(a[1]);
    (n > 0.0 && n.is_finite()).then(|| [a[0] / n, a[1] / n])
}

/// The distance from `p` to the segment `a`-`b`.
fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    let t = if l2 > 0.0 {
        (dot(sub(p, a), d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(p, [a[0] + d[0] * t, a[1] + d[1] * t])
}

impl Geo {
    /// The vertices `Part::Point` counts.
    fn points(&self) -> Vec<[f64; 2]> {
        match self {
            Geo::Line(a, b) => vec![*a, *b],
            Geo::Poly { verts, .. } => verts.clone(),
            Geo::Circle(..) => Vec::new(),
            Geo::Arc(c, r, a0, a1) => vec![polar(*c, *r, *a0), polar(*c, *r, *a1)],
        }
    }

    fn centre(&self) -> Option<[f64; 2]> {
        match self {
            Geo::Circle(c, _) | Geo::Arc(c, ..) => Some(*c),
            _ => None,
        }
    }

    fn edges(&self) -> usize {
        match self {
            Geo::Poly { verts, closed, .. } => {
                if *closed && verts.len() > 2 {
                    verts.len()
                } else {
                    verts.len().saturating_sub(1)
                }
            }
            _ => 1,
        }
    }

    /// The vertices at the ends of edge `i` (none for a circle).
    fn edge_ends(&self, i: usize) -> Option<[usize; 2]> {
        match self {
            Geo::Line(..) | Geo::Arc(..) if i == 0 => Some([0, 1]),
            Geo::Poly { verts, .. } if i < self.edges() => Some([i, (i + 1) % verts.len()]),
            _ => None,
        }
    }

    /// Edge `i` when it is straight: its ends.
    fn straight(&self, i: usize) -> Option<([f64; 2], [f64; 2])> {
        match self {
            Geo::Line(a, b) if i == 0 => Some((*a, *b)),
            Geo::Poly { verts, bulges, .. } if i < self.edges() && bulges[i] == 0.0 => {
                Some((verts[i], verts[(i + 1) % verts.len()]))
            }
            _ => None,
        }
    }

    fn round(&self) -> bool {
        matches!(self, Geo::Circle(..) | Geo::Arc(..))
    }

    /// Where `part` is: a vertex, a centre, an edge's middle (a circle's top).
    fn position(&self, part: Part) -> Option<[f64; 2]> {
        match part {
            Part::Point { index } => self.points().get(index).copied(),
            Part::Centre => self.centre(),
            Part::Edge { index } => match self {
                Geo::Circle(c, r) if index == 0 => Some([c[0], c[1] + r]),
                Geo::Arc(c, r, a0, a1) if index == 0 => {
                    let mut a1 = *a1;
                    if a1 < *a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    Some(polar(*c, *r, (a0 + a1) / 2.0))
                }
                _ => {
                    let [i, j] = self.edge_ends(index)?;
                    let p = self.points();
                    Some([(p[i][0] + p[j][0]) / 2.0, (p[i][1] + p[j][1]) / 2.0])
                }
            },
        }
    }

    /// True when `part` names something of this shape.
    fn has(&self, part: Part) -> bool {
        match part {
            Part::Point { index } => index < self.points().len(),
            Part::Centre => self.centre().is_some(),
            Part::Edge { index } => {
                index < self.edges() && (self.round() || self.straight(index).is_some())
            }
        }
    }

    /// Its kind and how many vertices and edges it has: a change of these drops its constraints.
    fn shape_key(&self) -> (u8, usize, usize) {
        let kind = match self {
            Geo::Line(..) => 0,
            Geo::Poly { .. } => 1,
            Geo::Circle(..) => 2,
            Geo::Arc(..) => 3,
        };
        (kind, self.points().len(), self.edges())
    }
}

/// What a pick of a constraint tool takes from the entity.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
    /// A vertex or a centre.
    Point,
    /// A line or a straight polyline segment.
    Line,
    /// A circle or an arc.
    Round,
    /// Any of those edges.
    Curve,
}

/// The part of `geo` a click at `at` picks for `role`.
fn pick_part(geo: &Geo, at: [f64; 2], role: Role) -> Result<Part, CoreError> {
    match role {
        Role::Point => {
            let mut best: Option<(f64, Part)> = None;
            let mut consider = |p: [f64; 2], part: Part| {
                let d = dist(p, at);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, part));
                }
            };
            for (index, p) in geo.points().into_iter().enumerate() {
                consider(p, Part::Point { index });
            }
            if let Some(c) = geo.centre() {
                consider(c, Part::Centre);
            }
            best.map(|(_, p)| p)
                .ok_or_else(|| invalid("Pick a point: an end, a vertex or a centre"))
        }
        Role::Round => {
            if geo.round() {
                Ok(Part::Edge { index: 0 })
            } else {
                Err(invalid("Pick a circle or an arc"))
            }
        }
        Role::Line | Role::Curve => {
            if geo.round() {
                return if role == Role::Curve {
                    Ok(Part::Edge { index: 0 })
                } else {
                    Err(invalid("Pick a line or a straight polyline segment"))
                };
            }
            let pts = geo.points();
            let index = (0..geo.edges())
                .min_by(|&i, &j| {
                    let [a, b] = geo.edge_ends(i).expect("an edge");
                    let [c, d] = geo.edge_ends(j).expect("an edge");
                    to_segment(at, pts[a], pts[b]).total_cmp(&to_segment(at, pts[c], pts[d]))
                })
                .ok_or_else(|| invalid("This polyline has no segments"))?;
            if geo.straight(index).is_none() {
                return Err(invalid(
                    "Pick a straight segment: curved polyline segments can't be constrained yet",
                ));
            }
            Ok(Part::Edge { index })
        }
    }
}

/// The picks each geometric kind takes.
fn roles(kind: ConstraintKind, picks: usize) -> Result<Vec<Role>, CoreError> {
    use ConstraintKind::*;
    Ok(match (kind, picks) {
        (Coincident, _) => vec![Role::Point, Role::Point],
        (Collinear | Parallel | Perpendicular, _) => vec![Role::Line, Role::Line],
        (Concentric, _) => vec![Role::Round, Role::Round],
        (Fixed, _) => vec![Role::Point],
        (Horizontal | Vertical, 2) => vec![Role::Point, Role::Point],
        (Horizontal | Vertical, _) => vec![Role::Line],
        (Tangent | Smooth | Equal, _) => vec![Role::Curve, Role::Curve],
        (Symmetric, _) => vec![Role::Point, Role::Point, Role::Line],
        (Linear | Aligned, 2) => vec![Role::Point, Role::Point],
        (Linear | Aligned, _) => vec![Role::Line],
        (Angular, _) => vec![Role::Line, Role::Line],
        (Radius | Diameter, _) => vec![Role::Round],
    })
}

/// The sketch of some entities, with where each one's parts went.
#[derive(Default)]
struct Built {
    sketch: Sketch,
    ents: HashMap<u64, Ids>,
}

#[derive(Default)]
struct Ids {
    points: Vec<PointId>,
    centre: Option<PointId>,
    edges: Vec<Option<usize>>,
    radius: Option<usize>,
}

impl Built {
    fn add(&mut self, handle: u64, geo: &Geo) {
        let s = &mut self.sketch;
        let mut ids = Ids::default();
        match geo {
            Geo::Line(a, b) => {
                let (a, b) = (s.point(*a), s.point(*b));
                ids.points = vec![a, b];
                ids.edges = vec![Some(s.curve(Curve::Line { a, b }))];
            }
            Geo::Poly { verts, .. } => {
                ids.points = verts.iter().map(|v| s.point(*v)).collect();
                ids.edges = (0..geo.edges())
                    .map(|i| {
                        geo.straight(i).map(|_| {
                            let [a, b] = geo.edge_ends(i).expect("an edge");
                            s.curve(Curve::Line {
                                a: ids.points[a],
                                b: ids.points[b],
                            })
                        })
                    })
                    .collect();
            }
            Geo::Circle(c, r) => {
                let curve = s.circle(*c, *r);
                let Curve::Circle { centre, radius } = s.curves[curve] else {
                    unreachable!()
                };
                ids.centre = Some(centre);
                ids.radius = Some(radius);
                ids.edges = vec![Some(curve)];
            }
            Geo::Arc(..) => {
                let p = geo.points();
                let centre = s.point(geo.centre().expect("an arc's"));
                let (start, end) = (s.point(p[0]), s.point(p[1]));
                ids.points = vec![start, end];
                ids.centre = Some(centre);
                ids.edges = vec![Some(s.curve(Curve::Arc { centre, start, end }))];
            }
        }
        self.ents.insert(handle, ids);
    }

    fn point(&self, r: &Ref) -> Option<PointId> {
        let ids = self.ents.get(&r.handle)?;
        match r.part {
            Part::Point { index } => ids.points.get(index).copied(),
            Part::Centre => ids.centre,
            Part::Edge { .. } => None,
        }
    }

    fn edge(&self, r: &Ref) -> Option<usize> {
        let ids = self.ents.get(&r.handle)?;
        match r.part {
            Part::Edge { index } => ids.edges.get(index).copied().flatten(),
            _ => None,
        }
    }

    /// Every point of the entity `h`.
    fn all_points(&self, h: u64) -> Vec<PointId> {
        self.ents
            .get(&h)
            .map(|ids| ids.points.iter().copied().chain(ids.centre).collect())
            .unwrap_or_default()
    }
}

/// `v` written for an expression: up to 6 decimals, no trailing zeros.
fn number(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// A dimensional constraint's text: its name and value, in brackets (the lock mark), `fx:` when
/// its expression uses other parameters.
fn dim_text(d: &DimData) -> String {
    let formula = Expr::parse(&d.expression).is_ok_and(|e| !e.names().is_empty());
    if formula {
        format!("[fx: {}=<>]", d.name)
    } else {
        format!("[{}=<>]", d.name)
    }
}

/// Where the rays of an angle at `vertex` along `u1` and `u2` run to take in `toward`: the sign
/// along each line.
fn sides_toward(u1: [f64; 2], u2: [f64; 2], vertex: [f64; 2], toward: [f64; 2]) -> [f64; 2] {
    let w = sub(toward, vertex);
    let det = cross(u1, u2);
    let alpha = cross(w, u2) / det;
    let beta = cross(u1, w) / det;
    [
        if alpha < 0.0 { -1.0 } else { 1.0 },
        if beta < 0.0 { -1.0 } else { 1.0 },
    ]
}

/// Where two infinite lines cross.
fn crossing(a: [f64; 2], u: [f64; 2], b: [f64; 2], v: [f64; 2]) -> Option<[f64; 2]> {
    let det = cross(u, v);
    if det.abs() < 1e-12 {
        return None;
    }
    let t = cross(sub(b, a), v) / det;
    Some([a[0] + u[0] * t, a[1] + u[1] * t])
}

/// What a command's change of an entity's shape means for its constraints (`follow_param`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reshape {
    /// Moved, turned, scaled, stretched, aligned: its vertices are the same points.
    Placed,
    /// Trimmed, broken, reversed, extended...: its vertices may be other points.
    Edited,
    /// Mirrored: an arc's start and end change places.
    Mirrored,
}

impl Reshape {
    pub(crate) fn of(cmd: &crate::Command) -> Reshape {
        match cmd {
            crate::Command::Edit(_) => Reshape::Edited,
            crate::Command::Modify(crate::ModifySpec::Mirror { .. }) => Reshape::Mirrored,
            _ => Reshape::Placed,
        }
    }
}

/// A snapshot of the constrained entities before a command, to see what it did to them.
pub(crate) struct ParamSnapshot {
    shapes: HashMap<u64, (Handle, Geo)>,
    stored: Vec<Stored>,
}

impl Session {
    /// True when the drawing has no `PARAM_APP` application yet (so no constraints).
    pub(crate) fn needs_param_app(&self) -> bool {
        !self.drawing.doc.app_ids.contains(PARAM_APP)
    }

    fn register_param_app(&mut self) {
        if self.needs_param_app() {
            let doc = &mut self.drawing.doc;
            let mut app = AppId::new(PARAM_APP);
            app.handle = doc.allocate_handle();
            doc.app_ids.add(app).expect("checked it is missing");
        }
    }

    /// Every constraint of the drawing, each on its own holder.
    fn stored(&self) -> Vec<Stored> {
        if self.needs_param_app() {
            return Vec::new();
        }
        self.drawing
            .doc
            .entities()
            .filter(|e| e.common().extended_data.get_record(PARAM_APP).is_some())
            .flat_map(|e| {
                let h = e.common().handle.value();
                read_record(e.common())
                    .into_iter()
                    .filter(move |s| s.holder == h)
            })
            .collect()
    }

    /// The entity's shape, when constraints can hold it.
    fn geo(&self, h: u64) -> Option<Geo> {
        geo_of(self.drawing.doc.get_entity(Handle::new(h))?).ok()
    }

    /// The constraints of `holder`, rewritten by `f`.
    fn edit_holder(&mut self, holder: u64, f: impl FnOnce(&mut Vec<Stored>)) {
        let Some(e) = self.drawing.doc.get_entity_mut(Handle::new(holder)) else {
            return;
        };
        let mut list: Vec<Stored> = read_record(e.common())
            .into_iter()
            .filter(|s| s.holder == holder)
            .collect();
        f(&mut list);
        write_record(e.common_mut(), &list);
    }

    /// Stores `s` on its holder: a dimensional one's Dimension, else the entity it holds that
    /// holds the fewest. Its id is set.
    fn keep(&mut self, mut s: Stored, all: &[Stored]) -> u32 {
        self.register_param_app();
        s.id = all.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        if s.dim.is_none() {
            let mut load: HashMap<u64, usize> = HashMap::new();
            for c in all {
                *load.entry(c.holder).or_default() += 1;
            }
            s.holder = s
                .refs
                .iter()
                .map(|r| r.handle)
                .min_by_key(|h| load.get(h).copied().unwrap_or(0))
                .expect("a constraint holds something");
        }
        let id = s.id;
        let holder = s.holder;
        self.edit_holder(holder, |list| list.push(s));
        id
    }

    /// Every parameter's value (degrees for angles) or why it has none, each on its own: one
    /// broken expression (a name gone, a garbled string from another program) spoils only the
    /// parameters using it.
    fn evaluated(all: &[Stored]) -> BTreeMap<String, Result<f64, ExprError>> {
        let exprs: BTreeMap<String, String> = all
            .iter()
            .filter_map(|s| s.dim.as_ref())
            .map(|d| (d.name.clone(), d.expression.clone()))
            .collect();
        evaluate_each(&exprs)
    }

    /// The parameters that have a value, by name. A dimensional constraint without one is left
    /// out of every solve (`to_solver`) until it is set again or deleted.
    fn values(all: &[Stored]) -> BTreeMap<String, f64> {
        Self::evaluated(all)
            .into_iter()
            .filter_map(|(name, v)| v.ok().map(|v| (name, v)))
            .collect()
    }

    /// The constraints joined to the entities `seeds` through the entities they hold.
    fn component(all: &[Stored], seeds: &[u64]) -> (Vec<usize>, BTreeSet<u64>) {
        let mut handles: BTreeSet<u64> = seeds.iter().copied().collect();
        let mut taken = vec![false; all.len()];
        loop {
            let mut grew = false;
            for (i, s) in all.iter().enumerate() {
                if taken[i] || !s.refs.iter().any(|r| handles.contains(&r.handle)) {
                    continue;
                }
                taken[i] = true;
                grew = true;
                for r in &s.refs {
                    handles.insert(r.handle);
                }
            }
            if !grew {
                break;
            }
        }
        let members: Vec<usize> = (0..all.len()).filter(|&i| taken[i]).collect();
        let held = members
            .iter()
            .flat_map(|&i| all[i].refs.iter().map(|r| r.handle))
            .collect();
        (members, held)
    }

    /// The solver's constraints for `s` in `built`, with parameter `values`.
    fn to_solver(
        built: &Built,
        s: &Stored,
        values: &BTreeMap<String, f64>,
    ) -> Result<Vec<Constraint>, CoreError> {
        use ConstraintKind::*;
        let lost = || invalid(format!("Constraint {} holds a part that is gone", s.id));
        let p = |i: usize| built.point(&s.refs[i]).ok_or_else(lost);
        let e = |i: usize| built.edge(&s.refs[i]).ok_or_else(lost);
        let need = |n: usize| {
            if s.refs.len() == n {
                Ok(())
            } else {
                Err(lost())
            }
        };
        // A parameter whose expression can't be worked out holds nothing for now.
        if s.dim
            .as_ref()
            .is_some_and(|d| !values.contains_key(&d.name))
        {
            return Ok(Vec::new());
        }
        let value = || {
            let d = s.dim.as_ref().ok_or_else(lost)?;
            values
                .get(&d.name)
                .copied()
                .ok_or_else(|| invalid(format!("{} has no value", d.name)))
        };
        Ok(match s.kind {
            Coincident => {
                need(2)?;
                vec![Constraint::Coincident(p(0)?, p(1)?)]
            }
            Fixed => {
                need(1)?;
                vec![Constraint::Fixed(p(0)?, s.at.ok_or_else(lost)?)]
            }
            Horizontal | Vertical if s.refs.len() == 2 => {
                let (a, b) = (p(0)?, p(1)?);
                vec![if s.kind == Horizontal {
                    Constraint::HorizontalPoints(a, b)
                } else {
                    Constraint::VerticalPoints(a, b)
                }]
            }
            Horizontal => {
                need(1)?;
                vec![Constraint::Horizontal(e(0)?)]
            }
            Vertical => {
                need(1)?;
                vec![Constraint::Vertical(e(0)?)]
            }
            Collinear => {
                need(2)?;
                vec![Constraint::Collinear(e(0)?, e(1)?)]
            }
            Parallel => {
                need(2)?;
                vec![Constraint::Parallel(e(0)?, e(1)?)]
            }
            Perpendicular => {
                need(2)?;
                vec![Constraint::Perpendicular(e(0)?, e(1)?)]
            }
            Concentric => {
                need(2)?;
                vec![Constraint::Concentric(e(0)?, e(1)?)]
            }
            Tangent => {
                need(2)?;
                vec![Constraint::Tangent(e(0)?, e(1)?)]
            }
            Equal => {
                need(2)?;
                vec![Constraint::Equal(e(0)?, e(1)?)]
            }
            Symmetric => {
                need(3)?;
                vec![Constraint::Symmetric(p(0)?, p(1)?, e(2)?)]
            }
            // Edges 0 and 1 meet at points 2 and 3, smoothly: tangent, or a straight run.
            Smooth => {
                need(4)?;
                let (a, b) = (e(0)?, e(1)?);
                let lines =
                    built.sketch.line_ends(a).is_some() && built.sketch.line_ends(b).is_some();
                vec![
                    Constraint::Coincident(p(2)?, p(3)?),
                    if lines {
                        Constraint::Parallel(a, b)
                    } else {
                        Constraint::Tangent(a, b)
                    },
                ]
            }
            Linear | Aligned => {
                need(2)?;
                let axis = (s.kind == Linear).then(|| {
                    let r = s.dim.as_ref().map_or(0.0, |d| d.rotation);
                    [r.cos(), r.sin()]
                });
                vec![Constraint::Distance {
                    a: p(0)?,
                    b: p(1)?,
                    axis,
                    value: value()?,
                }]
            }
            Angular => {
                need(2)?;
                let sides = s.dim.as_ref().map_or([1.0, 1.0], |d| d.sides);
                let v = value()?.to_radians();
                // The rays run along the lines or against them: against one, the lines' own
                // directions make the supplement.
                let value = if sides[0] * sides[1] > 0.0 {
                    v
                } else {
                    std::f64::consts::PI - v
                };
                vec![Constraint::Angle {
                    first: e(0)?,
                    second: e(1)?,
                    value,
                }]
            }
            Radius => {
                need(1)?;
                vec![Constraint::Radius {
                    curve: e(0)?,
                    value: value()?,
                }]
            }
            Diameter => {
                need(1)?;
                vec![Constraint::Radius {
                    curve: e(0)?,
                    value: value()? / 2.0,
                }]
            }
        })
    }

    /// Solves the constraints joined to `seeds` again, the entities `pin` and the parts
    /// `pin_parts` held where they are, writes the moved entities back and draws the
    /// dimensional constraints' Dimensions again. The entities that moved.
    fn solve_around(
        &mut self,
        seeds: &[u64],
        pin: &[u64],
        pin_parts: &[Ref],
    ) -> Result<Vec<Handle>, CoreError> {
        let all = self.stored();
        let (members, handles) = Self::component(&all, seeds);
        if members.is_empty() {
            return Ok(Vec::new());
        }
        let values = Self::values(&all);
        let mut built = Built::default();
        let mut before: HashMap<u64, Geo> = HashMap::new();
        for &h in &handles {
            let entity = self
                .drawing
                .doc
                .get_entity(Handle::new(h))
                .ok_or(CoreError::NoSuchEntity(h))?;
            let geo = geo_of(entity).map_err(invalid)?;
            built.add(h, &geo);
            before.insert(h, geo);
        }
        for &i in &members {
            for c in Self::to_solver(&built, &all[i], &values)? {
                built.sketch.constrain(c);
            }
        }
        let mut pinned: Vec<PointId> = pin.iter().flat_map(|&h| built.all_points(h)).collect();
        pinned.extend(pin_parts.iter().filter_map(|r| built.point(r)));
        qiyas_param::solve(&mut built.sketch, &pinned).map_err(|e| match e {
            SolveError::Conflict { .. } => invalid(e.to_string()),
            SolveError::Invalid(why) => invalid(why),
        })?;
        let mut changes = Vec::new();
        for (&h, geo) in &before {
            if let Some(e) = self.solved_entity(h, geo, &built)? {
                changes.push((Handle::new(h), e));
            }
        }
        if let Some((h, _)) = changes.iter().find(|(h, _)| self.is_locked(*h)) {
            return Err(invalid(format!(
                "Entity {:X} is locked and would have to move to keep its constraints",
                h.value()
            )));
        }
        let mut moved = Vec::with_capacity(changes.len());
        for (h, e) in changes {
            *self.drawing.doc.get_entity_mut(h).expect("found above") = e;
            moved.push(h);
        }
        let dims: Vec<Stored> = members
            .iter()
            .map(|&i| all[i].clone())
            .filter(|s| s.dim.is_some())
            .collect();
        for s in &dims {
            self.refresh_dimension(s)?;
        }
        moved.sort();
        Ok(moved)
    }

    /// `solve_around` with `pin_parts` held, or, when that conflicts, without them.
    fn solve_preferring(
        &mut self,
        seeds: &[u64],
        pin_parts: &[Ref],
    ) -> Result<Vec<Handle>, CoreError> {
        if !pin_parts.is_empty() {
            if let Some(moved) = self.solve_trial(seeds, pin_parts) {
                return Ok(moved);
            }
        }
        self.solve_around(seeds, &[], &[])
    }

    /// `solve_around` holding `pin_parts`, or None when it can't (nothing is written then:
    /// `solve_around` writes only once the solve worked and no locked entity moves).
    fn solve_trial(&mut self, seeds: &[u64], pin_parts: &[Ref]) -> Option<Vec<Handle>> {
        self.solve_around(seeds, &[], pin_parts).ok()
    }

    /// The entity `h` with its solved shape, when it moved.
    fn solved_entity(
        &self,
        h: u64,
        geo: &Geo,
        built: &Built,
    ) -> Result<Option<EntityType>, CoreError> {
        let ids = &built.ents[&h];
        let s = &built.sketch;
        let p = |i: PointId| s.points[i];
        let near = same_point;
        let entity = self
            .drawing
            .doc
            .get_entity(Handle::new(h))
            .ok_or(CoreError::NoSuchEntity(h))?;
        let mut e = entity.clone();
        let moved = match (&mut e, geo) {
            (EntityType::Line(l), Geo::Line(a, b)) => {
                let (na, nb) = (p(ids.points[0]), p(ids.points[1]));
                if near(na, *a) && near(nb, *b) {
                    false
                } else {
                    l.start = Vector3::new(na[0], na[1], l.start.z);
                    l.end = Vector3::new(nb[0], nb[1], l.end.z);
                    true
                }
            }
            (EntityType::LwPolyline(pl), Geo::Poly { verts, .. }) => {
                let mut any = false;
                for (i, v) in pl.vertices.iter_mut().enumerate() {
                    let n = p(ids.points[i]);
                    if !near(n, verts[i]) {
                        v.location.x = n[0];
                        v.location.y = n[1];
                        any = true;
                    }
                }
                any
            }
            (EntityType::Circle(c), Geo::Circle(centre, r)) => {
                let (nc, nr) = (
                    p(ids.centre.expect("a centre")),
                    s.radii[ids.radius.expect("a radius")],
                );
                if near(nc, *centre) && (nr - r).abs() <= SAME * (1.0 + r.abs()) {
                    false
                } else {
                    c.center = Vector3::new(nc[0], nc[1], c.center.z);
                    c.radius = nr;
                    true
                }
            }
            (EntityType::Arc(a), Geo::Arc(..)) => {
                let old = geo.points();
                let (nc, ns, ne) = (
                    p(ids.centre.expect("a centre")),
                    p(ids.points[0]),
                    p(ids.points[1]),
                );
                if near(nc, geo.centre().expect("a centre")) && near(ns, old[0]) && near(ne, old[1])
                {
                    false
                } else {
                    let tau = std::f64::consts::TAU;
                    a.center = Vector3::new(nc[0], nc[1], a.center.z);
                    a.radius = dist(ns, nc);
                    a.start_angle = (ns[1] - nc[1]).atan2(ns[0] - nc[0]).rem_euclid(tau);
                    a.end_angle = (ne[1] - nc[1]).atan2(ne[0] - nc[0]).rem_euclid(tau);
                    true
                }
            }
            _ => return Err(invalid(format!("Entity {h:X} changed kind"))),
        };
        if !moved {
            return Ok(None);
        }
        e.common_mut().graphic_data = None;
        Ok(Some(e))
    }

    /// Where the part `r` is now.
    fn part_position(&self, r: &Ref) -> Result<[f64; 2], CoreError> {
        self.geo(r.handle)
            .and_then(|g| g.position(r.part))
            .ok_or_else(|| invalid(format!("Entity {:X} no longer has that part", r.handle)))
    }

    /// The ends of the straight edge `r`, start first.
    fn line_of(&self, r: &Ref) -> Result<([f64; 2], [f64; 2]), CoreError> {
        let Part::Edge { index } = r.part else {
            return Err(invalid("Pick a line"));
        };
        self.geo(r.handle)
            .and_then(|g| g.straight(index))
            .ok_or_else(|| invalid("Pick a line or a straight polyline segment"))
    }

    /// The centre and radius of the round edge `r`.
    fn round_of(&self, r: &Ref) -> Result<([f64; 2], f64), CoreError> {
        match self.geo(r.handle) {
            Some(Geo::Circle(c, rad)) | Some(Geo::Arc(c, rad, ..)) => Ok((c, rad)),
            _ => Err(invalid("Pick a circle or an arc")),
        }
    }

    /// The Dimension of the dimensional constraint `s`, its points following its parts now; its
    /// block drawn again when they moved.
    fn refresh_dimension(&mut self, s: &Stored) -> Result<(), CoreError> {
        let h = Handle::new(s.holder);
        let Some(EntityType::Dimension(old)) = self.drawing.doc.get_entity(h) else {
            return Ok(());
        };
        let mut d = old.clone();
        let v3 = |p: [f64; 2]| Vector3::new(p[0], p[1], 0.0);
        let p2 = |v: &Vector3| [v.x, v.y];
        match (&mut d, s.kind) {
            (Dimension::Linear(l), ConstraintKind::Linear) => {
                let (a, b) = (
                    self.part_position(&s.refs[0])?,
                    self.part_position(&s.refs[1])?,
                );
                let n = [-l.rotation.sin(), l.rotation.cos()];
                let off = dot(sub(p2(&l.definition_point), p2(&l.second_point)), n);
                l.first_point = v3(a);
                l.second_point = v3(b);
                let at = [b[0] + n[0] * off, b[1] + n[1] * off];
                d.set_definition_point(v3(at));
            }
            (Dimension::Aligned(l), ConstraintKind::Aligned) => {
                let (a, b) = (
                    self.part_position(&s.refs[0])?,
                    self.part_position(&s.refs[1])?,
                );
                let old_n = unit(sub(p2(&l.second_point), p2(&l.first_point)))
                    .map_or([0.0, 1.0], |u| [-u[1], u[0]]);
                let off = dot(sub(p2(&l.definition_point), p2(&l.second_point)), old_n);
                let n = unit(sub(b, a)).map_or(old_n, |u| [-u[1], u[0]]);
                l.first_point = v3(a);
                l.second_point = v3(b);
                let at = [b[0] + n[0] * off, b[1] + n[1] * off];
                d.set_definition_point(v3(at));
            }
            (Dimension::Angular3Pt(g), ConstraintKind::Angular) => {
                let sides = s.dim.as_ref().map_or([1.0, 1.0], |d| d.sides);
                let ((a1, b1), (a2, b2)) = (self.line_of(&s.refs[0])?, self.line_of(&s.refs[1])?);
                let (Some(u1), Some(u2)) = (unit(sub(b1, a1)), unit(sub(b2, a2))) else {
                    return Ok(());
                };
                let Some(vertex) = crossing(a1, u1, a2, u2) else {
                    return Ok(());
                };
                let r1 = [u1[0] * sides[0], u1[1] * sides[0]];
                let r2 = [u2[0] * sides[1], u2[1] * sides[1]];
                let (l1, l2) = (dist(a1, b1), dist(a2, b2));
                let reach = dist(p2(&g.definition_point), p2(&g.angle_vertex));
                let bisector = unit([r1[0] + r2[0], r1[1] + r2[1]]).unwrap_or(r1);
                g.angle_vertex = v3(vertex);
                g.first_point = v3([vertex[0] + r1[0] * l1, vertex[1] + r1[1] * l1]);
                g.second_point = v3([vertex[0] + r2[0] * l2, vertex[1] + r2[1] * l2]);
                let at = [
                    vertex[0] + bisector[0] * reach,
                    vertex[1] + bisector[1] * reach,
                ];
                d.set_definition_point(v3(at));
            }
            (Dimension::Radius(g), ConstraintKind::Radius) => {
                let (c, r) = self.round_of(&s.refs[0])?;
                let u =
                    unit(sub(p2(&g.definition_point), p2(&g.angle_vertex))).unwrap_or([1.0, 0.0]);
                g.angle_vertex = v3(c);
                let at = [c[0] + u[0] * r, c[1] + u[1] * r];
                d.set_definition_point(v3(at));
            }
            (Dimension::Diameter(g), ConstraintKind::Diameter) => {
                let (c, r) = self.round_of(&s.refs[0])?;
                let u =
                    unit(sub(p2(&g.definition_point), p2(&g.angle_vertex))).unwrap_or([1.0, 0.0]);
                g.angle_vertex = v3([c[0] - u[0] * r, c[1] - u[1] * r]);
                let at = [c[0] + u[0] * r, c[1] + u[1] * r];
                d.set_definition_point(v3(at));
            }
            _ => return Ok(()),
        }
        if let Some(data) = &s.dim {
            d.base_mut().set_text_override(Some(dim_text(data)));
        }
        if EntityType::Dimension(d.clone()) == *self.drawing.doc.get_entity(h).expect("found above")
        {
            return Ok(());
        }
        *self.drawing.doc.get_entity_mut(h).expect("found above") = EntityType::Dimension(d);
        self.rebuild_dimension(h)
    }

    /// The parts `targets` pick at `points` for `kind`, each target at the top of the sheet.
    fn picked_refs(
        &self,
        kind: ConstraintKind,
        targets: &[Target],
        points: &[[f64; 2]],
    ) -> Result<Vec<Ref>, CoreError> {
        if targets.is_empty() {
            return Err(invalid("Pick the entities first"));
        }
        if points.len() != targets.len() {
            return Err(invalid("Give one point for each entity picked"));
        }
        let roles = roles(kind, targets.len())?;
        if roles.len() != targets.len() {
            return Err(invalid(format!(
                "This constraint takes {} pick{}",
                roles.len(),
                if roles.len() == 1 { "" } else { "s" }
            )));
        }
        let mut refs = Vec::with_capacity(targets.len());
        for ((t, at), role) in targets.iter().zip(points).zip(roles) {
            if !at.iter().all(|v| v.is_finite()) {
                return Err(invalid("The points must be numbers"));
            }
            let h = self.sheet_targets(std::slice::from_ref(t))?[0];
            let geo = geo_of(self.entity(h)).map_err(invalid)?;
            refs.push(Ref {
                handle: h.value(),
                part: pick_part(&geo, *at, role)?,
            });
        }
        if refs.len() >= 2 && refs[0] == refs[1] {
            return Err(invalid("Pick two different parts"));
        }
        Ok(refs)
    }

    /// `Command::AddConstraint`: the geometric constraint `kind` on the parts `targets` pick at
    /// `points`; the first entity picked stays where it is when it can. Its id.
    pub(crate) fn add_constraint(
        &mut self,
        kind: ConstraintKind,
        targets: &[Target],
        points: &[[f64; 2]],
    ) -> Result<u32, CoreError> {
        if kind.is_dimensional() {
            return Err(invalid(
                "Dimensional constraints are added with AddDimConstraint",
            ));
        }
        let mut refs = self.picked_refs(kind, targets, points)?;
        let mut at = None;
        match kind {
            ConstraintKind::Fixed => at = Some(self.part_position(&refs[0])?),
            ConstraintKind::Tangent => {
                if self.round_of(&refs[0]).is_err() && self.round_of(&refs[1]).is_err() {
                    return Err(invalid("Tangent takes a circle or an arc"));
                }
            }
            ConstraintKind::Smooth => {
                // The ends of the two edges nearest each other meet.
                let ends = |r: &Ref| -> Result<[Ref; 2], CoreError> {
                    let Part::Edge { index } = r.part else {
                        unreachable!("picked as an edge")
                    };
                    let [a, b] = self
                        .geo(r.handle)
                        .and_then(|g| g.edge_ends(index))
                        .ok_or_else(|| invalid("Smooth takes lines, segments and arcs"))?;
                    Ok([a, b].map(|index| Ref {
                        handle: r.handle,
                        part: Part::Point { index },
                    }))
                };
                let (ea, eb) = (ends(&refs[0])?, ends(&refs[1])?);
                let mut best = (f64::MAX, ea[0], eb[0]);
                for a in ea {
                    for b in eb {
                        let d = dist(self.part_position(&a)?, self.part_position(&b)?);
                        if d < best.0 {
                            best = (d, a, b);
                        }
                    }
                }
                refs.extend([best.1, best.2]);
            }
            _ => {}
        }
        let all = self.stored();
        if all.iter().any(|s| s.kind == kind && s.refs == refs) {
            return Err(invalid("That constraint is there already"));
        }
        let id = self.keep(
            Stored {
                id: 0,
                holder: 0,
                kind,
                refs: refs.clone(),
                at,
                dim: None,
            },
            &all,
        );
        // The first entity picked stays; the others follow it.
        let first = refs[0].handle;
        let seeds: Vec<u64> = refs.iter().map(|r| r.handle).collect();
        let moved = if refs.iter().any(|r| r.handle != first) {
            match self.solve_trial_whole(&seeds, first) {
                Some(moved) => moved,
                None => self.solve_around(&seeds, &[], &[])?,
            }
        } else {
            self.solve_around(&seeds, &[], &[])?
        };
        self.follow_moved(&moved)?;
        Ok(id)
    }

    /// `solve_around` with the whole entity `pin` held, or None when that can't be.
    fn solve_trial_whole(&mut self, seeds: &[u64], pin: u64) -> Option<Vec<Handle>> {
        let geo = self.geo(pin)?;
        let parts: Vec<Ref> = (0..geo.points().len())
            .map(|index| Ref {
                handle: pin,
                part: Part::Point { index },
            })
            .chain(geo.centre().map(|_| Ref {
                handle: pin,
                part: Part::Centre,
            }))
            .collect();
        self.solve_trial(seeds, &parts)
    }

    /// The take-off rows and fields that read entities the solver moved follow them.
    fn follow_moved(&mut self, moved: &[Handle]) -> Result<(), CoreError> {
        if !moved.is_empty() {
            self.follow_takeoff(moved, &[]);
            self.follow_fields()?;
        }
        Ok(())
    }

    /// `Command::AddDimConstraint`: a dimensional constraint `kind` on the parts `targets` pick at
    /// `points` (a linear or aligned one: one line or segment, or two points; an angular one: two
    /// lines; a radius or diameter: a circle or an arc), its Dimension through `line_point`, its
    /// value `expression` (the measured value when none). Its id.
    pub(crate) fn add_dim_constraint(
        &mut self,
        kind: ConstraintKind,
        targets: &[Target],
        points: &[[f64; 2]],
        line_point: [f64; 2],
        expression: Option<&str>,
    ) -> Result<u32, CoreError> {
        if !kind.is_dimensional() {
            return Err(invalid(
                "Geometric constraints are added with AddConstraint",
            ));
        }
        if !line_point.iter().all(|v| v.is_finite()) {
            return Err(invalid("The points must be numbers"));
        }
        let picked = self.picked_refs(kind, targets, points)?;
        let mut rotation = 0.0;
        let mut sides = [1.0, 1.0];
        let (refs, spec, measured) = match kind {
            ConstraintKind::Linear | ConstraintKind::Aligned => {
                let refs = if picked.len() == 1 {
                    let Part::Edge { index } = picked[0].part else {
                        unreachable!("picked as a line")
                    };
                    let geo = self.geo(picked[0].handle).expect("picked");
                    let [a, b] = geo.edge_ends(index).expect("a straight edge");
                    [a, b]
                        .map(|index| Ref {
                            handle: picked[0].handle,
                            part: Part::Point { index },
                        })
                        .to_vec()
                } else {
                    picked
                };
                let (p1, p2) = (self.part_position(&refs[0])?, self.part_position(&refs[1])?);
                if dist(p1, p2) <= SAME {
                    return Err(invalid("The two points are one"));
                }
                let spec = if kind == ConstraintKind::Linear {
                    DimSpec::Linear {
                        p1,
                        p2,
                        line_point,
                        rotation: None,
                    }
                } else {
                    let u = unit(sub(p2, p1)).expect("apart");
                    DimSpec::Aligned {
                        p1,
                        p2,
                        offset: cross(u, sub(line_point, p1)),
                    }
                };
                (refs, spec, 0.0)
            }
            ConstraintKind::Angular => {
                let ((a1, b1), (a2, b2)) = (self.line_of(&picked[0])?, self.line_of(&picked[1])?);
                let (Some(u1), Some(u2)) = (unit(sub(b1, a1)), unit(sub(b2, a2))) else {
                    return Err(invalid("The lines have no length"));
                };
                let vertex = crossing(a1, u1, a2, u2)
                    .ok_or_else(|| invalid("The lines are parallel: there is no angle"))?;
                sides = sides_toward(u1, u2, vertex, line_point);
                let (r1, r2) = (
                    [u1[0] * sides[0], u1[1] * sides[0]],
                    [u2[0] * sides[1], u2[1] * sides[1]],
                );
                let (l1, l2) = (dist(a1, b1), dist(a2, b2));
                let angle = cross(r1, r2).abs().atan2(dot(r1, r2)).to_degrees();
                let spec = DimSpec::Angular {
                    vertex,
                    p1: [vertex[0] + r1[0] * l1, vertex[1] + r1[1] * l1],
                    p2: [vertex[0] + r2[0] * l2, vertex[1] + r2[1] * l2],
                    arc_point: line_point,
                };
                (picked, spec, angle)
            }
            ConstraintKind::Radius | ConstraintKind::Diameter => {
                let target = Target {
                    handle: picked[0].handle,
                    chain: Vec::new(),
                };
                let spec = if kind == ConstraintKind::Radius {
                    DimSpec::Radius {
                        target,
                        point: line_point,
                    }
                } else {
                    DimSpec::Diameter {
                        target,
                        point: line_point,
                    }
                };
                (picked, spec, 0.0)
            }
            _ => unreachable!("dimensional"),
        };
        let all = self.stored();
        let mut taken: HashSet<String> = all
            .iter()
            .filter_map(|s| s.dim.as_ref().map(|d| d.name.to_ascii_lowercase()))
            .collect();
        let name = (1..)
            .map(|n| format!("{}{n}", kind.prefix()))
            .find(|n| !taken.contains(n))
            .expect("a free name");
        taken.insert(name.clone());
        let dim_handle = self.add_dimensions(spec)?[0];
        let measured = match self.drawing.doc.get_entity(Handle::new(dim_handle)) {
            Some(EntityType::Dimension(d)) => {
                if let Dimension::Linear(l) = d {
                    rotation = l.rotation;
                }
                if kind == ConstraintKind::Angular {
                    measured
                } else {
                    d.measurement()
                }
            }
            _ => measured,
        };
        let given = expression.map(normalize).filter(|e| !e.is_empty());
        let data = DimData {
            name: name.clone(),
            expression: given.unwrap_or_else(|| number(measured)),
            rotation,
            sides,
        };
        let mut stored = Stored {
            id: 0,
            holder: dim_handle,
            kind,
            refs: refs.clone(),
            at: None,
            dim: Some(data),
        };
        let mut with = all.clone();
        with.push(stored.clone());
        match &Self::evaluated(&with)[&name] {
            Ok(v) => Self::check_value(kind, *v)?,
            Err(e) => return Err(invalid(format!("{name}: {e}"))),
        }
        stored.holder = dim_handle;
        let id = self.keep(stored, &all);
        let seeds: Vec<u64> = refs.iter().map(|r| r.handle).collect();
        let anchor = self.anchor(kind, &refs);
        let moved = self.solve_preferring(&seeds, &anchor)?;
        // Measured again, its block drawn with its text.
        let now = self
            .stored()
            .into_iter()
            .find(|s| s.id == id)
            .expect("kept");
        self.refresh_dimension(&now)?;
        self.rebuild_dimension(Handle::new(dim_handle))?;
        self.follow_moved(&moved)?;
        Ok(id)
    }

    /// The parts that stay when a dimensional constraint's value changes: a distance's first
    /// point, an angle's first line, a circle's centre.
    fn anchor(&self, kind: ConstraintKind, refs: &[Ref]) -> Vec<Ref> {
        match kind {
            ConstraintKind::Linear | ConstraintKind::Aligned => vec![refs[0]],
            ConstraintKind::Angular => {
                let Part::Edge { index } = refs[0].part else {
                    return Vec::new();
                };
                let handle = refs[0].handle;
                let ends = self.geo(handle).and_then(|g| g.edge_ends(index));
                ends.into_iter()
                    .flatten()
                    .map(|index| Ref {
                        handle,
                        part: Part::Point { index },
                    })
                    .collect()
            }
            _ => vec![Ref {
                handle: refs[0].handle,
                part: Part::Centre,
            }],
        }
    }

    /// A length must be above 0, an angle between 0 and 180 degrees.
    fn check_value(kind: ConstraintKind, v: f64) -> Result<(), CoreError> {
        let ok = match kind {
            ConstraintKind::Angular => v > 0.0 && v < 180.0,
            _ => v > 0.0,
        };
        if ok {
            Ok(())
        } else if kind == ConstraintKind::Angular {
            Err(invalid(format!(
                "An angle must be between 0 and 180 degrees, not {}",
                number(v)
            )))
        } else {
            Err(invalid(format!(
                "A length must be more than 0, not {}",
                number(v)
            )))
        }
    }

    /// `Command::SetParameter`: the parameter `name` takes `expression`; every constraint whose
    /// value changes with it is solved again.
    pub(crate) fn set_parameter(&mut self, name: &str, expression: &str) -> Result<(), CoreError> {
        // Stored in ASCII: an older DWG's code page has no Arabic digits.
        let expression = normalize(expression);
        let expression = expression.as_str();
        if expression.is_empty() {
            return Err(invalid("Give the parameter a value or an expression"));
        }
        let all = self.stored();
        let Some(target) = all
            .iter()
            .find(|s| {
                s.dim
                    .as_ref()
                    .is_some_and(|d| d.name.eq_ignore_ascii_case(name.trim()))
            })
            .cloned()
        else {
            return Err(invalid(format!("There is no parameter named {name}")));
        };
        // A broken parameter (this one or another) never stops the user mending it.
        let before = Self::evaluated(&all);
        let mut with = all.clone();
        for s in &mut with {
            if s.id == target.id {
                s.dim.as_mut().expect("a parameter").expression = expression.to_string();
            }
        }
        let after = Self::evaluated(&with);
        let target_name = &target.dim.as_ref().expect("a parameter").name;
        if let Err(e) = &after[target_name] {
            return Err(invalid(format!("{target_name}: {e}")));
        }
        for s in &with {
            let Some(d) = &s.dim else { continue };
            match (&before[&d.name], &after[&d.name]) {
                (Ok(_), Err(e)) => return Err(invalid(format!("{}: {e}", d.name))),
                (_, Ok(v)) => Self::check_value(s.kind, *v)
                    .map_err(|e| invalid(format!("{}: {e}", d.name)))?,
                (Err(_), Err(_)) => {}
            }
        }
        let value = |m: &BTreeMap<String, Result<f64, ExprError>>, n: &str| {
            m.get(n).and_then(|v| v.as_ref().ok().copied())
        };
        let new_expression = expression.to_string();
        self.edit_holder(target.holder, |list| {
            for s in list.iter_mut() {
                if s.id == target.id {
                    s.dim.as_mut().expect("a parameter").expression = new_expression.clone();
                }
            }
        });
        // Those whose value changed are solved again, each keeping its anchor.
        let changed: Vec<&Stored> = with
            .iter()
            .filter(|s| {
                s.dim.as_ref().is_some_and(|d| {
                    match (value(&before, &d.name), value(&after, &d.name)) {
                        (Some(a), Some(b)) => (a - b).abs() > 1e-12,
                        (None, Some(_)) => true,
                        _ => false,
                    }
                }) || s.id == target.id
            })
            .collect();
        let seeds: Vec<u64> = changed
            .iter()
            .flat_map(|s| s.refs.iter().map(|r| r.handle))
            .collect();
        let anchors: Vec<Ref> = changed
            .iter()
            .flat_map(|s| self.anchor(s.kind, &s.refs))
            .collect();
        let moved = self.solve_preferring(&seeds, &anchors)?;
        // Its text says whether it is a formula now.
        let now = self
            .stored()
            .into_iter()
            .find(|s| s.id == target.id)
            .expect("kept");
        self.refresh_dimension(&now)?;
        self.follow_moved(&moved)
    }

    /// Removes the constraints `ids`: a dimensional one's Dimension goes too, and a parameter
    /// that used its name keeps its value as a number.
    fn remove_constraints(&mut self, ids: &HashSet<u32>) -> Result<(), CoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let all = self.stored();
        let values = Self::values(&all);
        let gone: Vec<&Stored> = all.iter().filter(|s| ids.contains(&s.id)).collect();
        let names: HashSet<String> = gone
            .iter()
            .filter_map(|s| s.dim.as_ref().map(|d| d.name.clone()))
            .collect();
        for s in &gone {
            if s.dim.is_some() {
                self.drawing.doc.remove_entity_arc(Handle::new(s.holder));
            } else {
                self.edit_holder(s.holder, |list| list.retain(|c| !ids.contains(&c.id)));
            }
        }
        self.freeze_users(&names, &values)
    }

    /// The parameters that use one of `names` (gone) keep their `values` as numbers.
    fn freeze_users(
        &mut self,
        names: &HashSet<String>,
        values: &BTreeMap<String, f64>,
    ) -> Result<(), CoreError> {
        if names.is_empty() {
            return Ok(());
        }
        let all = self.stored();
        for s in &all {
            let Some(d) = &s.dim else { continue };
            let uses = Expr::parse(&d.expression)
                .map(|e| e.names().iter().any(|n| names.contains(n)))
                .unwrap_or(false);
            if !uses {
                continue;
            }
            let Some(v) = values.get(&d.name).copied() else {
                continue;
            };
            let frozen = number(v);
            self.edit_holder(s.holder, |list| {
                for c in list.iter_mut() {
                    if c.id == s.id {
                        c.dim.as_mut().expect("a parameter").expression = frozen.clone();
                    }
                }
            });
            let now = self
                .stored()
                .into_iter()
                .find(|c| c.id == s.id)
                .expect("kept");
            self.refresh_dimension(&now)?;
        }
        Ok(())
    }

    /// `Command::DeleteConstraint`.
    pub(crate) fn delete_constraint(&mut self, id: u32) -> Result<(), CoreError> {
        if !self.stored().iter().any(|s| s.id == id) {
            return Err(invalid(format!("There is no constraint {id}")));
        }
        self.remove_constraints(&HashSet::from([id]))
    }

    /// `Command::DeleteConstraintsOn`: every constraint holding one of `targets` (a dimensional
    /// constraint's Dimension picked counts). Their ids.
    pub(crate) fn delete_constraints_on(
        &mut self,
        targets: &[Target],
    ) -> Result<Vec<u32>, CoreError> {
        let handles: HashSet<u64> = self
            .sheet_targets_any(targets)?
            .into_iter()
            .map(|h| h.value())
            .collect();
        let ids: HashSet<u32> = self
            .stored()
            .iter()
            .filter(|s| {
                (s.dim.is_some() && handles.contains(&s.holder))
                    || s.refs.iter().any(|r| handles.contains(&r.handle))
            })
            .map(|s| s.id)
            .collect();
        self.remove_constraints(&ids)?;
        let mut ids: Vec<u32> = ids.into_iter().collect();
        ids.sort_unstable();
        Ok(ids)
    }

    /// `Command::AutoConstrain`: the constraints `settings` find already true of `targets` (lines,
    /// polylines' straight segments, arcs and circles), less those already there, solved exact.
    /// Their ids.
    pub(crate) fn auto_constrain(
        &mut self,
        targets: &[Target],
        settings: &AutoSettings,
    ) -> Result<Vec<u32>, CoreError> {
        let distance = settings
            .distance_tolerance
            .unwrap_or_else(|| self.batch_tol());
        if !(distance >= 0.0 && distance.is_finite())
            || !(settings.angle_tolerance >= 0.0 && settings.angle_tolerance.is_finite())
        {
            return Err(invalid("The tolerances must be 0 or more"));
        }
        let handles = self.sheet_targets(targets)?;
        // Each straight edge, arc and circle as a kernel primitive, with its entity and edge.
        let mut prims: Vec<ParametricPrimitive> = Vec::new();
        let mut owners: Vec<(u64, usize)> = Vec::new();
        for h in &handles {
            let Ok(geo) = geo_of(self.entity(*h)) else {
                continue;
            };
            match geo {
                Geo::Circle(centre, radius) => {
                    prims.push(ParametricPrimitive::Circle(KCircle { centre, radius }));
                    owners.push((h.value(), 0));
                }
                Geo::Arc(centre, radius, start_angle, end_angle) => {
                    prims.push(ParametricPrimitive::Arc(KArc {
                        centre,
                        radius,
                        start_angle,
                        end_angle,
                    }));
                    owners.push((h.value(), 0));
                }
                _ => {
                    for i in 0..geo.edges() {
                        if let Some((start, end)) = geo.straight(i) {
                            prims.push(ParametricPrimitive::Line(KLine { start, end }));
                            owners.push((h.value(), i));
                        }
                    }
                }
            }
        }
        if prims.is_empty() {
            return Err(invalid("Pick lines, polylines, arcs or circles"));
        }
        let priority: Vec<InferenceKind> = settings
            .kinds
            .iter()
            .filter_map(|k| k.inference())
            .collect();
        let found = infer(
            &prims,
            &InferenceSettings {
                priority,
                distance_tolerance: distance,
                angle_tolerance_radians: settings.angle_tolerance.to_radians(),
                ..InferenceSettings::default()
            },
        )?;
        let edge = |i: usize| Ref {
            handle: owners[i].0,
            part: Part::Edge { index: owners[i].1 },
        };
        let geos: HashMap<u64, Geo> = owners
            .iter()
            .filter_map(|&(h, _)| Some((h, self.geo(h)?)))
            .collect();
        let end = |i: usize, which: Endpoint| -> Ref {
            let (handle, e) = owners[i];
            let [a, b] = geos[&handle].edge_ends(e).expect("an edge with ends");
            Ref {
                handle,
                part: Part::Point {
                    index: if which == Endpoint::Start { a } else { b },
                },
            }
        };
        let mut wanted: Vec<(ConstraintKind, Vec<Ref>)> = Vec::new();
        for c in found {
            use ConstraintKind as K;
            let (kind, refs) = match c {
                InferredConstraint::Coincident {
                    first,
                    first_endpoint,
                    second,
                    second_endpoint,
                } => (
                    K::Coincident,
                    vec![end(first, first_endpoint), end(second, second_endpoint)],
                ),
                InferredConstraint::Collinear { first, second } => {
                    (K::Collinear, vec![edge(first), edge(second)])
                }
                InferredConstraint::Concentric { first, second } => {
                    (K::Concentric, vec![edge(first), edge(second)])
                }
                InferredConstraint::Parallel { first, second } => {
                    (K::Parallel, vec![edge(first), edge(second)])
                }
                InferredConstraint::Perpendicular { first, second } => {
                    (K::Perpendicular, vec![edge(first), edge(second)])
                }
                InferredConstraint::Horizontal { entity } => (K::Horizontal, vec![edge(entity)]),
                InferredConstraint::Vertical { entity } => (K::Vertical, vec![edge(entity)]),
                InferredConstraint::Tangent { first, second } => {
                    (K::Tangent, vec![edge(first), edge(second)])
                }
                InferredConstraint::Equal { first, second } => {
                    (K::Equal, vec![edge(first), edge(second)])
                }
            };
            wanted.push((kind, refs));
        }
        // The drawing is read once; the new constraints are written holder by holder.
        let all = self.stored();
        let mut known: HashSet<(ConstraintKind, Vec<Ref>)> =
            all.iter().map(|s| (s.kind, s.refs.clone())).collect();
        let fresh: Vec<(ConstraintKind, Vec<Ref>)> = wanted
            .into_iter()
            .filter(|(kind, refs)| known.insert((*kind, refs.clone())))
            .collect();
        if fresh.len() > MAX_AUTO {
            return Err(invalid(format!(
                "AUTOCONSTRAIN found {} constraints, more than 2,000 at once: pick fewer \
                 entities, or fewer kinds in its settings",
                fresh.len()
            )));
        }
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        self.register_param_app();
        let mut next = all.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        let mut load: HashMap<u64, usize> = HashMap::new();
        for c in &all {
            *load.entry(c.holder).or_default() += 1;
        }
        let mut by_holder: BTreeMap<u64, Vec<Stored>> = BTreeMap::new();
        let mut ids = Vec::with_capacity(fresh.len());
        let mut seeds = Vec::new();
        for (kind, refs) in fresh {
            seeds.extend(refs.iter().map(|r| r.handle));
            let holder = refs
                .iter()
                .map(|r| r.handle)
                .min_by_key(|h| load.get(h).copied().unwrap_or(0))
                .expect("a constraint holds something");
            *load.entry(holder).or_default() += 1;
            ids.push(next);
            by_holder.entry(holder).or_default().push(Stored {
                id: next,
                holder,
                kind,
                refs,
                at: None,
                dim: None,
            });
            next += 1;
        }
        for (holder, list) in by_holder {
            self.edit_holder(holder, |kept| kept.extend(list));
        }
        seeds.sort_unstable();
        seeds.dedup();
        let moved = self.solve_around(&seeds, &[], &[])?;
        self.follow_moved(&moved)?;
        Ok(ids)
    }

    /// `Command::ListConstraints`: the constraints of the sheet on screen, by id.
    pub fn list_constraints(&self) -> Vec<ConstraintInfo> {
        let sheet = self.sheet_owner();
        let all = self.stored();
        let values = Self::values(&all);
        let mut out: Vec<ConstraintInfo> = all
            .iter()
            .filter(|s| {
                self.drawing
                    .doc
                    .get_entity(Handle::new(s.holder))
                    .is_some_and(|e| Some(e.common().owner_handle) == sheet)
            })
            .map(|s| {
                let mut handles: Vec<u64> = Vec::new();
                for r in &s.refs {
                    if !handles.contains(&r.handle) {
                        handles.push(r.handle);
                    }
                }
                let at = self
                    .part_position(&s.refs[0])
                    .unwrap_or([f64::NAN, f64::NAN]);
                ConstraintInfo {
                    id: s.id,
                    kind: s.kind,
                    handles,
                    at,
                    name: s.dim.as_ref().map(|d| d.name.clone()),
                    expression: s.dim.as_ref().map(|d| d.expression.clone()),
                    value: s.dim.as_ref().and_then(|d| values.get(&d.name).copied()),
                    dimension: s.dim.as_ref().map(|_| s.holder),
                }
            })
            .collect();
        out.sort_by_key(|c| c.id);
        out
    }

    /// `Command::ListParameters`: every dimensional constraint's parameter, by name.
    pub fn list_parameters(&self) -> Vec<ParameterInfo> {
        let all = self.stored();
        let evaluated = Self::evaluated(&all);
        let mut out: Vec<ParameterInfo> = all
            .iter()
            .filter_map(|s| {
                let d = s.dim.as_ref()?;
                Some(ParameterInfo {
                    id: s.id,
                    name: d.name.clone(),
                    kind: s.kind,
                    expression: d.expression.clone(),
                    value: evaluated
                        .get(&d.name)
                        .and_then(|v| v.as_ref().ok().copied()),
                    dimension: s.holder,
                    error: evaluated
                        .get(&d.name)
                        .and_then(|v| v.as_ref().err().map(|e| e.to_string())),
                })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Besides what `cmd` says it changes, what keeping the constraints may change: dimensional
    /// constraints' blocks (the tables) and the take-off rows on entities the solver moves.
    pub(crate) fn param_extras(&self, cmd: &crate::Command) -> (bool, bool) {
        use crate::Command as C;
        let own = matches!(
            cmd,
            C::AddConstraint { .. }
                | C::DeleteConstraint { .. }
                | C::DeleteConstraintsOn { .. }
                | C::AutoConstrain { .. }
                | C::AddDimConstraint { .. }
                | C::SetParameter { .. }
        );
        if own {
            return (true, !self.takeoff.items.is_empty());
        }
        let all = self.stored();
        (
            all.iter().any(|s| s.dim.is_some()),
            !all.is_empty() && !self.takeoff.items.is_empty(),
        )
    }

    /// True for the commands that keep the constraints themselves (no snapshot and follow).
    pub(crate) fn keeps_constraints(cmd: &crate::Command) -> bool {
        use crate::Command as C;
        matches!(
            cmd,
            C::AddConstraint { .. }
                | C::DeleteConstraint { .. }
                | C::DeleteConstraintsOn { .. }
                | C::AutoConstrain { .. }
                | C::AddDimConstraint { .. }
                | C::SetParameter { .. }
        )
    }

    /// Before a drawing command: the constrained entities as they are.
    pub(crate) fn param_snapshot(&self) -> Option<ParamSnapshot> {
        let all = self.stored();
        if all.is_empty() {
            return None;
        }
        let mut shapes = HashMap::new();
        for s in &all {
            for r in &s.refs {
                if shapes.contains_key(&r.handle) {
                    continue;
                }
                if let Some(e) = self.drawing.doc.get_entity(Handle::new(r.handle)) {
                    if let Ok(geo) = geo_of(e) {
                        shapes.insert(r.handle, (e.common().owner_handle, geo));
                    }
                }
            }
        }
        Some(ParamSnapshot {
            shapes,
            stored: all,
        })
    }

    /// After a drawing command: constraints on entities it erased or reshaped go; those on
    /// entities it moved are solved again with them held where they went.
    ///
    /// `reshape` says what a change of shape means. A placement (move, rotate, scale, stretch,
    /// align) keeps every vertex what it was, so the entity is held and the rest solved again.
    /// An edit (trim, break, reverse, extend, fillet...) may keep the handle and the vertex count
    /// while its vertices become other points (a trimmed end, a reversed line), so every entity
    /// it changed loses its constraints; a mirror turns an arc's start and end round, so a
    /// mirrored arc loses its own. How many constraints went.
    pub(crate) fn follow_param(
        &mut self,
        snap: Option<ParamSnapshot>,
        reshape: Reshape,
    ) -> Result<usize, CoreError> {
        let Some(snap) = snap else {
            return Ok(0);
        };
        // Dimensions the command erased took their constraints along: whoever used their
        // parameters keeps the values.
        let now: HashSet<u32> = self.stored().iter().map(|s| s.id).collect();
        let vanished: HashSet<String> = snap
            .stored
            .iter()
            .filter(|s| !now.contains(&s.id))
            .filter_map(|s| s.dim.as_ref().map(|d| d.name.clone()))
            .collect();
        let old_values = Self::values(&snap.stored);
        self.freeze_users(&vanished, &old_values)?;
        let mut broken: HashSet<u64> = HashSet::new();
        let mut moved: Vec<u64> = Vec::new();
        for (&h, (owner, before)) in &snap.shapes {
            let now = self.drawing.doc.get_entity(Handle::new(h));
            match now.map(|e| (e.common().owner_handle, geo_of(e))) {
                Some((o, Ok(geo))) if o == *owner && geo.shape_key() == before.shape_key() => {
                    if geo != *before {
                        let reshaped = match reshape {
                            Reshape::Placed => false,
                            Reshape::Edited => true,
                            Reshape::Mirrored => matches!(geo, Geo::Arc(..)),
                        };
                        if reshaped {
                            broken.insert(h);
                        } else {
                            moved.push(h);
                        }
                    }
                }
                _ => {
                    broken.insert(h);
                }
            }
        }
        let all = self.stored();
        let dropped: HashSet<u32> = all
            .iter()
            .filter(|s| {
                s.refs.iter().any(|r| {
                    broken.contains(&r.handle) || self.geo(r.handle).is_some_and(|g| !g.has(r.part))
                })
            })
            .map(|s| s.id)
            .collect();
        self.remove_constraints(&dropped)?;
        let left: HashSet<u32> = self.stored().iter().map(|s| s.id).collect();
        let removed = snap.stored.iter().filter(|s| !left.contains(&s.id)).count();
        moved.retain(|h| !broken.contains(h));
        if moved.is_empty() {
            return Ok(removed);
        }
        moved.sort_unstable();
        let solved = self.solve_around(&moved, &moved, &[])?;
        self.follow_moved(&solved)?;
        Ok(removed)
    }
}

/// The kernel's `infer_constraints_with_settings`, kept from its quadratic filter. The kernel
/// checks every pair relation (parallel, collinear, perpendicular, equal...) against every other
/// candidate to drop those an axis relation already implies, which on a plan of 500 lines takes
/// seconds. Pair relations among lines can only hold between lines that are neither horizontal
/// nor vertical (two horizontal lines' parallel is dropped, a line parallel or perpendicular to an
/// axis line is itself an axis line), so they are looked for among those lines alone; the rest
/// in a pass without them. Plans that would give more than `MAX_AUTO` pair relations are refused
/// before the kernel runs (counted from directions and lengths).
fn infer(
    prims: &[ParametricPrimitive],
    settings: &InferenceSettings,
) -> Result<Vec<InferredConstraint>, CoreError> {
    let wants = |k: InferenceKind| settings.priority.contains(&k);
    let sin_angle = settings.angle_tolerance_radians.sin().abs();
    let direction = |p: &ParametricPrimitive| match p {
        ParametricPrimitive::Line(l) => unit(sub(l.end, l.start)),
        _ => None,
    };
    let axis = |u: [f64; 2]| {
        (wants(InferenceKind::Horizontal) && u[1].abs() <= sin_angle)
            || (wants(InferenceKind::Vertical) && u[0].abs() <= sin_angle)
    };
    let too_many = |n: usize| {
        invalid(format!(
            "AUTOCONSTRAIN would find more than 2,000 constraints ({n} or more) at once: pick \
             fewer entities, or fewer kinds in its settings"
        ))
    };
    // Parallel pairs among the lines off the axes, by direction (mod 180 degrees).
    if wants(InferenceKind::Parallel) || wants(InferenceKind::Collinear) {
        let mut angles: Vec<f64> = prims
            .iter()
            .filter_map(direction)
            .filter(|u| !axis(*u))
            .map(|u| u[1].atan2(u[0]).rem_euclid(std::f64::consts::PI))
            .collect();
        let n = pairs_within(&mut angles, settings.angle_tolerance_radians);
        if n > MAX_AUTO {
            return Err(too_many(n));
        }
    }
    // Equal lengths, and equal radii.
    if wants(InferenceKind::Equal) {
        let (mut lengths, mut radii): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
        for p in prims {
            match p {
                ParametricPrimitive::Line(l) => lengths.push(dist(l.start, l.end)),
                ParametricPrimitive::Circle(c) => radii.push(c.radius),
                ParametricPrimitive::Arc(a) => radii.push(a.radius),
            }
        }
        let tol = |v: &[f64]| 1e-9 * v.iter().fold(1.0f64, |m, x| m.max(x.abs()));
        let (tl, tr) = (tol(&lengths), tol(&radii));
        let n = pairs_within(&mut lengths, tl) + pairs_within(&mut radii, tr);
        if n > MAX_AUTO {
            return Err(too_many(n));
        }
    }
    let pair_kinds = [
        InferenceKind::Collinear,
        InferenceKind::Parallel,
        InferenceKind::Perpendicular,
    ];
    if !(wants(InferenceKind::Horizontal) && wants(InferenceKind::Vertical)) {
        return Ok(infer_constraints_with_settings(prims, settings));
    }
    let rest = InferenceSettings {
        priority: settings
            .priority
            .iter()
            .copied()
            .filter(|k| !pair_kinds.contains(k))
            .collect(),
        ..settings.clone()
    };
    let mut found = infer_constraints_with_settings(prims, &rest);
    let off_axis: Vec<usize> = (0..prims.len())
        .filter(|&i| direction(&prims[i]).is_some_and(|u| !axis(u)))
        .collect();
    let lines = InferenceSettings {
        priority: settings
            .priority
            .iter()
            .copied()
            .filter(|k| pair_kinds.contains(k))
            .collect(),
        ..settings.clone()
    };
    if !lines.priority.is_empty() && off_axis.len() > 1 {
        let subset: Vec<ParametricPrimitive> = off_axis.iter().map(|&i| prims[i]).collect();
        for c in infer_constraints_with_settings(&subset, &lines) {
            found.push(match c {
                InferredConstraint::Collinear { first, second } => InferredConstraint::Collinear {
                    first: off_axis[first],
                    second: off_axis[second],
                },
                InferredConstraint::Parallel { first, second } => InferredConstraint::Parallel {
                    first: off_axis[first],
                    second: off_axis[second],
                },
                InferredConstraint::Perpendicular { first, second } => {
                    InferredConstraint::Perpendicular {
                        first: off_axis[first],
                        second: off_axis[second],
                    }
                }
                other => other,
            });
        }
    }
    // In the settings' order of kinds, as the kernel gives them.
    let rank = |c: &InferredConstraint| {
        settings
            .priority
            .iter()
            .position(|k| *k == c.kind())
            .unwrap_or(usize::MAX)
    };
    found.sort_by_key(rank);
    Ok(found)
}

/// How many pairs of `values` lie within `tol` of each other (sorted in place); counting stops
/// just past `MAX_AUTO`.
fn pairs_within(values: &mut [f64], tol: f64) -> usize {
    values.sort_by(f64::total_cmp);
    let (mut n, mut from) = (0usize, 0usize);
    for i in 0..values.len() {
        while values[i] - values[from] > tol {
            from += 1;
        }
        n += i - from;
        if n > MAX_AUTO {
            break;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_round_trip_through_xdata_strings() {
        let mut common = EntityCommon::new();
        let long_name = "w".repeat(400);
        let list = vec![
            Stored {
                id: 1,
                holder: 0x2A,
                kind: ConstraintKind::Horizontal,
                refs: vec![Ref {
                    handle: 0x2A,
                    part: Part::Edge { index: 2 },
                }],
                at: None,
                dim: None,
            },
            Stored {
                id: 2,
                holder: 0x2A,
                kind: ConstraintKind::Linear,
                refs: vec![
                    Ref {
                        handle: 0x2A,
                        part: Part::Point { index: 0 },
                    },
                    Ref {
                        handle: 0x30,
                        part: Part::Centre,
                    },
                ],
                at: None,
                dim: Some(DimData {
                    name: long_name,
                    expression: "2*d2 + 1.5".into(),
                    rotation: 0.5,
                    sides: [1.0, -1.0],
                }),
            },
        ];
        write_record(&mut common, &list);
        let record = common.extended_data.get_record(PARAM_APP).unwrap();
        assert!(record.values.iter().all(|v| match v {
            XDataValue::String(s) => s.len() <= CHUNK,
            _ => true,
        }));
        assert_eq!(read_record(&common), list);
        write_record(&mut common, &[]);
        assert!(common.extended_data.get_record(PARAM_APP).is_none());
    }

    #[test]
    fn inference_finds_pair_relations_among_the_lines_off_the_axes_only() {
        let line = |a: [f64; 2], b: [f64; 2]| ParametricPrimitive::Line(KLine { start: a, end: b });
        let prims = [
            line([0.0, 0.0], [1.0, 1.0]),
            line([5.0, 0.0], [6.0, 1.0]),
            line([0.0, 10.0], [4.0, 10.0]),
            line([0.0, 20.0], [4.0, 20.0]),
            line([20.0, 0.0], [21.0, -1.0]),
        ];
        let found = infer(&prims, &InferenceSettings::default()).unwrap();
        assert!(
            found.contains(&InferredConstraint::Parallel {
                first: 0,
                second: 1
            }),
            "{found:?}"
        );
        assert!(found.contains(&InferredConstraint::Horizontal { entity: 2 }));
        // Two horizontal lines: their parallel is implied, not kept (as the kernel does).
        assert!(!found.contains(&InferredConstraint::Parallel {
            first: 2,
            second: 3
        }));
        // In the settings' order: coincident, collinear, parallel... horizontal last but one.
        let rank = |c: &InferredConstraint| {
            InferenceSettings::default()
                .priority
                .iter()
                .position(|k| *k == c.kind())
                .unwrap()
        };
        assert!(found.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])));
        assert_eq!(pairs_within(&mut [3.0, 1.0, 2.0, 1.0], 0.5), 1);
    }

    #[test]
    fn a_pick_takes_the_nearest_part_of_its_kind() {
        let rect = Geo::Poly {
            verts: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
            bulges: vec![0.0; 4],
            closed: true,
        };
        assert_eq!(
            pick_part(&rect, [3.9, 0.2], Role::Point).unwrap(),
            Part::Point { index: 1 }
        );
        assert_eq!(
            pick_part(&rect, [0.1, 1.5], Role::Line).unwrap(),
            Part::Edge { index: 3 }
        );
        assert!(pick_part(&rect, [0.1, 1.5], Role::Round).is_err());
        let arc = Geo::Arc([0.0, 0.0], 2.0, 0.0, std::f64::consts::PI);
        assert_eq!(
            pick_part(&arc, [0.1, 0.1], Role::Point).unwrap(),
            Part::Centre
        );
        assert_eq!(
            pick_part(&arc, [0.0, 2.0], Role::Curve).unwrap(),
            Part::Edge { index: 0 }
        );
        assert_eq!(number(4.0), "4");
        assert_eq!(number(2.5000001), "2.5");
    }
}
