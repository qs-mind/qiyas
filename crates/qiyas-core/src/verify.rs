//! Read-only checks of stated intent. Picking and geometric residuals are shared with the
//! constraint tools; nothing is solved, annotated or stored in the drawing.

use opencadcodec::EntityType;
use qiyas_geom::{flatten, snap_shapes, Path, Xform};
use qiyas_param::{Constraint, Curve, Sketch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::{effective_block_name, variant_name};
use crate::{ConstraintKind, CoreError, Session, Target};

/// A measurable property or relationship, independent of the drawing's subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerifyKind {
    /// Total entity length, or the straight segment picked by one point.
    Length,
    /// Distance between two literal points, or the two picked vertices or centres.
    Distance,
    /// Angle between two directed straight segments, from 0 to 180 degrees.
    Angle,
    /// Radius of a circle or arc.
    Radius,
    /// Enclosed area, in squared drawing units or square metres.
    Area,
    /// Closed paths; an open path passes only when its end gap is within tolerance.
    Closed,
    /// Two straight segments run in the same or opposite direction.
    Parallel,
    /// Two straight segments meet at a right angle.
    Perpendicular,
    /// Two finite curves touch, including the actual sweep of an arc.
    Tangent,
    /// Two circles or arcs have the same centre.
    Concentric,
    /// Two straight segments lie on the same infinite line.
    Collinear,
    /// A straight segment or two picked points run horizontally.
    Horizontal,
    /// A straight segment or two picked points run vertically.
    Vertical,
    /// Two picked vertices or centres coincide.
    Coincident,
    /// Two straight segments have equal lengths, or two round curves equal radii.
    Equal,
    /// Two points mirror about a picked line, or an entire straight closed polyline
    /// mirrors about the second target's line.
    Symmetric,
    /// All the targets fit inside the supplied bounds.
    InsideBounds,
    /// No path crosses or doubles back over itself.
    NoSelfIntersection,
    /// Number of top-level entities on the current sheet, filtered by layer, type or block.
    Count,
}

/// One stated requirement. For example, `{kind:"length", targets:[{handle:42}], value:3,
/// in_metres:true}` or `{kind:"parallel", targets:[{handle:42},{handle:43}],
/// points:[[0,0],[0,1]]}`. Target points pick the same parts as AddConstraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Expectation {
    pub kind: VerifyKind,
    #[serde(default)]
    pub targets: Vec<Target>,
    /// One point per target for part picking. Distance without targets takes two literal
    /// points. Whole-entity length, area, bounds and closure do not need points. Repeated
    /// points are retained as separate picks; they never imply a closed boundary.
    #[serde(default)]
    pub points: crate::PointRun,
    /// Required for length, distance, angle, radius, area and count. Relationships expect zero
    /// deviation. Angles are degrees; lengths and areas follow in_metres.
    #[serde(default)]
    pub value: Option<f64>,
    /// Same units as the measured value. Default: 0.5 mm for distances, 0.1 degree for angles,
    /// (0.5 mm)^2 for area, zero for counts and intersection counts.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// Report and compare distances in metres and areas in square metres. Pick points and
    /// bounds always remain drawing coordinates.
    #[serde(default)]
    pub in_metres: bool,
    /// Lower-left and upper-right corners for inside_bounds, in drawing coordinates.
    #[serde(default)]
    pub bounds: Option<[[f64; 2]; 2]>,
    /// Count filter; names are case-insensitive. Filters combine when more than one is given.
    #[serde(default)]
    pub layer: Option<String>,
    /// Count filter: entity type such as Line, Circle, LwPolyline or Insert.
    #[serde(default)]
    pub entity_type: Option<String>,
    /// Count filter: the effective block name, including dynamic block references.
    #[serde(default)]
    pub block: Option<String>,
}

/// One answer per expectation. A target that cannot be measured fails with measured:null and
/// an explanation; it never prevents the remaining expectations from being checked.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerificationResult {
    pub kind: VerifyKind,
    pub passed: bool,
    pub measured: Option<f64>,
    pub expected: f64,
    pub tolerance: f64,
    pub unit: &'static str,
    pub message: String,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

impl VerifyKind {
    fn angular(self) -> bool {
        matches!(
            self,
            Self::Angle | Self::Parallel | Self::Perpendicular | Self::Horizontal | Self::Vertical
        )
    }

    fn needs_value(self) -> bool {
        matches!(
            self,
            Self::Length | Self::Distance | Self::Angle | Self::Radius | Self::Area | Self::Count
        )
    }

    fn discrete(self) -> bool {
        matches!(self, Self::Count | Self::NoSelfIntersection)
    }
}

impl Session {
    /// Checks every expectation on the current sheet without changing drawing or history.
    pub fn verify(
        &self,
        expectations: &[Expectation],
    ) -> Result<Vec<VerificationResult>, CoreError> {
        Ok(expectations.iter().map(|e| self.verify_one(e)).collect())
    }

    /// The omitted tolerance follows the expected quantity and its reported units.
    pub(crate) fn verification_default_tolerance(&self, e: &Expectation) -> f64 {
        let factor = if e.in_metres {
            self.drawing.units.to_metres()
        } else {
            1.0
        };
        let distance_tolerance = 0.0005 / self.drawing.units.to_metres() * factor;
        if e.kind.angular() {
            0.1
        } else if e.kind.discrete() {
            0.0
        } else if e.kind == VerifyKind::Area {
            distance_tolerance.powi(2)
        } else {
            distance_tolerance
        }
    }

    fn verify_one(&self, e: &Expectation) -> VerificationResult {
        let factor = if e.in_metres {
            self.drawing.units.to_metres()
        } else {
            1.0
        };
        let default_tolerance = self.verification_default_tolerance(e);
        let tolerance = e.tolerance.unwrap_or(default_tolerance);
        let expected = if e.kind.needs_value() {
            e.value.unwrap_or(0.0)
        } else {
            0.0
        };
        let unit = if e.kind.angular() {
            "degrees"
        } else if e.kind.discrete() {
            "count"
        } else if e.kind == VerifyKind::Area {
            if e.in_metres {
                "square_metres"
            } else {
                "square_drawing_units"
            }
        } else if e.in_metres {
            "metres"
        } else {
            "drawing_units"
        };
        let mut result = VerificationResult {
            kind: e.kind,
            passed: false,
            measured: None,
            expected: if expected.is_finite() { expected } else { 0.0 },
            tolerance: if tolerance.is_finite() {
                tolerance
            } else {
                default_tolerance
            },
            unit,
            message: String::new(),
        };
        let measure = || {
            if !tolerance.is_finite() || tolerance < 0.0 {
                return Err(invalid("Tolerance must be a finite nonnegative number"));
            }
            if e.kind.needs_value() && e.value.is_none_or(|v| !v.is_finite() || v < 0.0) {
                return Err(invalid("This expectation needs a finite nonnegative value"));
            }
            if e.kind == VerifyKind::Angle && expected > 180.0 {
                return Err(invalid("An angle must be between 0 and 180 degrees"));
            }
            if e.kind == VerifyKind::Count && expected.fract() != 0.0 {
                return Err(invalid("An entity count must be a whole number"));
            }
            if e.points.iter().flatten().any(|v| !v.is_finite()) {
                return Err(invalid("Pick points must be finite numbers"));
            }
            let raw = self.measure_expectation(e, tolerance / factor)?;
            let measured = if e.kind.angular() || e.kind.discrete() {
                raw
            } else if e.kind == VerifyKind::Area {
                raw * factor.powi(2)
            } else {
                raw * factor
            };
            if !measured.is_finite() {
                return Err(invalid("The measured value is not finite"));
            }
            Ok(measured)
        };
        match measure() {
            Ok(measured) => {
                result.measured = Some(measured);
                // Cover arithmetic noise only; it must not become a second geometric tolerance.
                let noise = f64::EPSILON * 16.0 * measured.abs().max(expected.abs());
                result.passed = (measured - expected).abs() <= tolerance + noise;
                result.message = format!(
                    "{}: measured {measured} {unit}, expected {expected}, tolerance {tolerance}",
                    if result.passed { "Pass" } else { "Fail" }
                );
            }
            Err(error) => result.message = error.to_string(),
        }
        result
    }

    fn verify_entities<'a>(&'a self, e: &Expectation) -> Result<Vec<&'a EntityType>, CoreError> {
        let handles = self.sheet_targets_any(&e.targets)?;
        Ok(handles
            .into_iter()
            .map(|h| self.drawing.doc.get_entity(h).expect("validated target"))
            .collect())
    }

    fn verify_paths(
        &self,
        entities: &[&EntityType],
        tolerance: f64,
    ) -> Result<Vec<Path>, CoreError> {
        let mut paths = Vec::new();
        for entity in entities {
            let flattened = flatten(&self.drawing.doc, entity, &Xform::identity(), tolerance);
            if flattened.truncated {
                return Err(invalid("The target is too complex to verify completely"));
            }
            let entity_paths: Vec<_> = flattened
                .paths
                .into_iter()
                .filter(|p| p.text.is_none() && p.paint == qiyas_geom::Paint::Outline)
                .map(|p| p.path)
                .collect();
            if entity_paths.is_empty() {
                return Err(invalid("Every target must have measurable paths"));
            }
            paths.extend(entity_paths);
        }
        if paths.is_empty() || paths.iter().any(|p| p.points.len() < 2) {
            return Err(invalid("These targets have no measurable paths"));
        }
        if paths
            .iter()
            .flat_map(|p| p.points.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err(invalid("The target has non-finite coordinates"));
        }
        Ok(paths)
    }

    fn measure_expectation(&self, e: &Expectation, tolerance: f64) -> Result<f64, CoreError> {
        use VerifyKind::*;
        if e.kind == Count {
            if !e.targets.is_empty() || !e.points.is_empty() {
                return Err(invalid(
                    "Count uses layer, entity_type and block filters, not picks",
                ));
            }
            return Ok(self
                .drawing
                .doc
                .entities()
                .filter(|entity| Some(entity.common().owner_handle) == self.sheet_owner())
                .filter(|entity| e.layer.as_ref().is_none_or(|l| l.eq_ignore_ascii_case(&entity.common().layer)))
                .filter(|entity| e.entity_type.as_ref().is_none_or(|t| type_key(t) == type_key(&variant_name(entity))))
                .filter(|entity| e.block.as_ref().is_none_or(|name| {
                    matches!(entity, EntityType::Insert(insert) if name.eq_ignore_ascii_case(&effective_block_name(&self.drawing.doc, insert)))
                }))
                .count() as f64);
        }
        if e.kind == Distance && e.targets.is_empty() {
            if e.points.len() != 2 {
                return Err(invalid(
                    "Distance needs two points, or two targets with pick points",
                ));
            }
            let mut sketch = Sketch::default();
            let a = sketch.point(e.points[0]);
            let b = sketch.point(e.points[1]);
            sketch.constrain(Constraint::Distance {
                a,
                b,
                axis: None,
                value: 0.0,
            });
            return residual_measure(&sketch, Distance);
        }
        let entities = self.verify_entities(e)?;
        if e.kind == Symmetric && e.targets.len() == 2 {
            return self.outline_symmetry(e, entities[0]);
        }
        if matches!(e.kind, Length | Area) && e.points.is_empty() {
            let mut total = 0.0;
            for entity in &entities {
                let extent = self.extent(&self.drawing.doc, entity, &Xform::identity());
                total += if e.kind == Length {
                    extent.length
                } else {
                    extent.area
                }
                .ok_or_else(|| invalid("The target has no requested length or enclosed area"))?;
            }
            return Ok(total);
        }
        if e.kind == InsideBounds {
            let bounds = e
                .bounds
                .ok_or_else(|| invalid("inside_bounds needs bounds"))?;
            if bounds.iter().flatten().any(|v| !v.is_finite())
                || bounds[0][0] > bounds[1][0]
                || bounds[0][1] > bounds[1][1]
            {
                return Err(invalid(
                    "Bounds need finite ordered minimum and maximum corners",
                ));
            }
            let mut outside: f64 = 0.0;
            for entity in &entities {
                let shapes = snap_shapes(
                    &self.drawing.doc,
                    entity,
                    &Xform::identity(),
                    self.measure_tol(),
                );
                if shapes.is_empty() || shapes.iter().any(|s| !s.exact) {
                    return Err(invalid(
                        "Exact bounds need lines, circles, arcs, ellipses or polylines",
                    ));
                }
                let curves: Vec<_> = shapes.into_iter().flat_map(|s| s.pieces).collect();
                let (low, high) = opencadkernel::geom2d::analytic_curve_bounds(&curves)
                    .ok_or_else(|| invalid("This target has no finite analytic bounds"))?;
                for axis in 0..2 {
                    outside = outside
                        .max(bounds[0][axis] - low[axis])
                        .max(high[axis] - bounds[1][axis]);
                }
            }
            return Ok(outside);
        }
        if matches!(e.kind, Closed | NoSelfIntersection) {
            // Flatten at a fraction of the default physical tolerance even when the caller
            // allows a larger deviation.
            let flatten_tolerance = 0.00005 / self.drawing.units.to_metres();
            let paths = self.verify_paths(
                &entities,
                if e.kind.discrete() {
                    flatten_tolerance
                } else {
                    flatten_tolerance.min(tolerance.max(flatten_tolerance * 0.001))
                },
            )?;
            return match e.kind {
                Closed => Ok(paths
                    .iter()
                    .map(|p| {
                        if p.closed {
                            0.0
                        } else {
                            distance(p.points[0], *p.points.last().expect("nonempty"))
                        }
                    })
                    .fold(0.0, f64::max)),
                NoSelfIntersection => {
                    if paths.iter().any(|p| p.points.len() > 4096) {
                        return Err(invalid("Check simpler paths: self-intersection checks allow 4096 vertices per path"));
                    }
                    Ok(paths
                        .iter()
                        .filter(|p| p.self_intersects() || doubles_back(p))
                        .count() as f64)
                }
                _ => unreachable!(),
            };
        }
        let kind = match e.kind {
            Length | Distance => ConstraintKind::Aligned,
            Angle => ConstraintKind::Angular,
            Radius => ConstraintKind::Radius,
            Parallel => ConstraintKind::Parallel,
            Perpendicular => ConstraintKind::Perpendicular,
            Tangent => ConstraintKind::Tangent,
            Concentric => ConstraintKind::Concentric,
            Collinear => ConstraintKind::Collinear,
            Horizontal => ConstraintKind::Horizontal,
            Vertical => ConstraintKind::Vertical,
            Coincident => ConstraintKind::Coincident,
            Equal => ConstraintKind::Equal,
            Symmetric => ConstraintKind::Symmetric,
            _ => return Err(invalid("This expectation does not take part picks")),
        };
        let points = if e.points.is_empty() {
            if entities
                .iter()
                .any(|e| matches!(e, EntityType::LwPolyline(_)))
                || matches!(e.kind, Distance | Coincident | Symmetric)
            {
                return Err(invalid("Give one pick point for each target"));
            }
            vec![[0.0, 0.0]; e.targets.len()]
        } else {
            e.points.to_vec()
        };
        if e.kind == Distance && e.targets.len() != 2 {
            return Err(invalid(
                "Distance between parts needs two targets and two pick points",
            ));
        }
        let sketch = self.verification_sketch(kind, &e.targets, &points)?;
        residual_measure(&sketch, e.kind)
    }

    fn outline_symmetry(&self, e: &Expectation, entity: &EntityType) -> Result<f64, CoreError> {
        let EntityType::LwPolyline(poly) = entity else {
            return Err(invalid(
                "Whole-outline symmetry needs a closed straight polyline and an axis",
            ));
        };
        if !poly.is_closed
            || poly.vertices.len() < 3
            || poly.vertices.len() > 1024
            || poly.vertices.iter().any(|v| v.bulge != 0.0)
            || poly.normal.x.abs() > 1e-9
            || poly.normal.y.abs() > 1e-9
            || poly.normal.z <= 0.0
        {
            return Err(invalid("Whole-outline symmetry needs a flat closed straight polyline with 3 to 1024 vertices"));
        }
        let axis_point = match e.points.as_slice() {
            [] => [0.0, 0.0],
            [_, axis] => *axis,
            _ => {
                return Err(invalid(
                    "Outline symmetry takes no points or one point per target",
                ))
            }
        };
        let base =
            self.verification_sketch(ConstraintKind::Horizontal, &e.targets[1..], &[axis_point])?;
        let Constraint::Horizontal(axis) = base.constraints[0] else {
            unreachable!()
        };
        let mut corners: Vec<_> = poly
            .vertices
            .iter()
            .map(|v| [v.location.x, v.location.y])
            .collect();
        remove_straight_subdivisions(&mut corners)?;
        if corners.len() < 3 {
            return Err(invalid(
                "A symmetric outline needs at least three distinct corners",
            ));
        }
        let mut sketch = base;
        sketch.constraints.clear();
        let ids: Vec<_> = corners.into_iter().map(|p| sketch.point(p)).collect();
        let n = ids.len();
        let mut best = f64::INFINITY;
        // Reflection reverses winding; matching a reversed cyclic order also checks edges,
        // rather than merely finding the same unconnected set of vertices.
        for offset in 0..n {
            sketch.constraints = (0..n)
                .map(|i| Constraint::Symmetric(ids[i], ids[(offset + n - i) % n], axis))
                .collect();
            let residuals = qiyas_param::solver::residuals(&sketch, 1.0)
                .map_err(|error| invalid(error.to_string()))?;
            let worst = residuals
                .as_chunks::<2>()
                .0
                .iter()
                .map(|r| (2.0 * r[0]).hypot(r[1]))
                .fold(0.0, f64::max);
            best = best.min(worst);
        }
        Ok(best)
    }
}

/// Extra vertices on a straight run do not change an outline's symmetry. Remove only
/// subdivisions at floating-point precision; the expectation's tolerance must not reshape it.
fn remove_straight_subdivisions(points: &mut Vec<[f64; 2]>) -> Result<(), CoreError> {
    loop {
        let n = points.len();
        if n < 3 {
            return Ok(());
        }
        let mut remove = None;
        for i in 0..n {
            let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            let (ab, bc, ac) = (distance(a, b), distance(b, c), distance(a, c));
            if ab == 0.0 || bc == 0.0 {
                remove = Some(i);
                break;
            }
            let noise = f64::EPSILON * 32.0 * (ab + bc);
            if (ab + bc - ac).abs() > noise {
                continue;
            }
            let mut sketch = Sketch::default();
            let first = sketch.line(a, b);
            let second = sketch.line(b, c);
            sketch.constrain(Constraint::Collinear(first, second));
            let residuals = qiyas_param::solver::residuals(&sketch, 1.0)
                .map_err(|error| invalid(error.to_string()))?;
            if residuals.iter().all(|r| r.abs() <= noise) {
                remove = Some(i);
                break;
            }
        }
        match remove {
            Some(i) => {
                points.remove(i);
            }
            None => return Ok(()),
        }
    }
}

fn type_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn residual_measure(sketch: &Sketch, kind: VerifyKind) -> Result<f64, CoreError> {
    let all =
        qiyas_param::solver::residuals(sketch, 1.0).map_err(|error| invalid(error.to_string()))?;
    let internal = sketch
        .curves
        .iter()
        .filter(|c| matches!(c, Curve::Arc { .. }))
        .count();
    let residuals = &all[internal..];
    let worst = residuals.iter().map(|v| v.abs()).fold(0.0, f64::max);
    Ok(match kind {
        VerifyKind::Angle => worst.to_degrees(),
        VerifyKind::Parallel | VerifyKind::Perpendicular => worst.min(1.0).asin().to_degrees(),
        VerifyKind::Horizontal | VerifyKind::Vertical => {
            let (a, b) = match sketch.constraints[0] {
                Constraint::Horizontal(c) | Constraint::Vertical(c) => {
                    sketch.line_ends(c).expect("a line")
                }
                Constraint::HorizontalPoints(a, b) | Constraint::VerticalPoints(a, b) => (a, b),
                _ => unreachable!(),
            };
            let length = distance(sketch.points[a], sketch.points[b]);
            if length == 0.0 {
                return Err(invalid("Coincident points have no direction"));
            }
            (worst / length).min(1.0).asin().to_degrees()
        }
        VerifyKind::Coincident | VerifyKind::Concentric => residuals[0].hypot(residuals[1]),
        VerifyKind::Symmetric => (2.0 * residuals[0]).hypot(residuals[1]),
        VerifyKind::Tangent => worst.max(tangent_contact_gap(sketch)?),
        _ => worst,
    })
}

/// Distance to a finite curve at the contact its supporting circle or line predicts. The
/// solver supplies tangency itself; this prevents a line extension or the missing half of an
/// arc from being reported as a touch.
fn tangent_contact_gap(sketch: &Sketch) -> Result<f64, CoreError> {
    let Constraint::Tangent(a, b) = sketch.constraints[0] else {
        unreachable!()
    };
    let centre = |curve: usize| match sketch.curves[curve] {
        Curve::Circle { centre, .. } | Curve::Arc { centre, .. } => Some(sketch.points[centre]),
        _ => None,
    };
    let on_curve = |curve: usize, p: [f64; 2]| -> f64 {
        match sketch.curves[curve] {
            Curve::Line { a, b } => {
                let (a, b) = (sketch.points[a], sketch.points[b]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]))
                    .clamp(0.0, 1.0);
                distance(p, [a[0] + t * d[0], a[1] + t * d[1]])
            }
            Curve::Circle { .. } => 0.0,
            Curve::Arc { centre, start, end } => {
                let (c, s, e) = (
                    sketch.points[centre],
                    sketch.points[start],
                    sketch.points[end],
                );
                if s == e {
                    // Equal start and end angles describe a full arc in the drawing.
                    return 0.0;
                }
                let angle = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
                let sweep = (angle(e) - angle(s)).rem_euclid(std::f64::consts::TAU);
                let to = (angle(p) - angle(s)).rem_euclid(std::f64::consts::TAU);
                if to <= sweep + 1e-12 || (std::f64::consts::TAU - to) <= 1e-12 {
                    0.0
                } else {
                    distance(p, s).min(distance(p, e))
                }
            }
        }
    };
    match (centre(a), centre(b)) {
        (None, Some(c)) | (Some(c), None) => {
            let line = if centre(a).is_none() { a } else { b };
            let (p, q) = sketch.line_ends(line).expect("line");
            let (p, q) = (sketch.points[p], sketch.points[q]);
            let d = [q[0] - p[0], q[1] - p[1]];
            let t = ((c[0] - p[0]) * d[0] + (c[1] - p[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]);
            let touch = [p[0] + t * d[0], p[1] + t * d[1]];
            Ok(on_curve(a, touch).max(on_curve(b, touch)))
        }
        (Some(c), Some(d)) => {
            let apart = distance(c, d);
            if apart == 0.0 {
                return Err(invalid(
                    "Concentric circles or arcs have no unique tangent contact",
                ));
            }
            let r1 = sketch.radius_of(a).expect("round");
            let r2 = sketch.radius_of(b).expect("round");
            let outside = (apart - r1 - r2).abs() <= (apart - (r1 - r2).abs()).abs();
            let u = [(d[0] - c[0]) / apart, (d[1] - c[1]) / apart];
            let direction = if outside || r1 >= r2 { 1.0 } else { -1.0 };
            let first = [c[0] + direction * u[0] * r1, c[1] + direction * u[1] * r1];
            let second_direction = if outside { -1.0 } else { direction };
            let second = [
                d[0] + second_direction * u[0] * r2,
                d[1] + second_direction * u[1] * r2,
            ];
            Ok(on_curve(a, first).max(on_curve(b, second)))
        }
        _ => Err(invalid("Two lines cannot be tangent")),
    }
}

/// qiyas-geom already checks crossings. Also catch collinear overlaps, including an adjacent
/// segment that reverses over its predecessor.
fn doubles_back(path: &Path) -> bool {
    let n = path.points.len();
    let count = if path.closed { n } else { n - 1 };
    for i in 0..count {
        let (a, b) = (path.points[i], path.points[(i + 1) % n]);
        let length = distance(a, b);
        if length == 0.0 {
            continue;
        }
        let u = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        for j in i + 1..count {
            let (c, d) = (path.points[j], path.points[(j + 1) % n]);
            let side = |p: [f64; 2]| u[0] * (p[1] - a[1]) - u[1] * (p[0] - a[0]);
            let epsilon = f64::EPSILON * 32.0 * length.max(distance(c, d));
            if side(c).abs() > epsilon || side(d).abs() > epsilon {
                continue;
            }
            let along = |p: [f64; 2]| u[0] * (p[0] - a[0]) + u[1] * (p[1] - a[1]);
            let (c, d) = (along(c), along(d));
            if c.max(d).min(length) - c.min(d).max(0.0) > epsilon {
                return true;
            }
        }
    }
    false
}
