//! Drawing new entities (`Command::Draw`) on the sheet on screen, styled by the current drawing
//! state (`Session::current`: layer, colour, linetype, lineweight), and the hatch boundary around
//! a picked point.

use std::f64::consts::{PI, TAU};

use opencadcodec::entities::{
    BoundaryEdge, BoundaryPath, CircularArcEdge, Hatch, HatchPattern as CodecPattern,
    HatchPatternLine, HatchPatternType, MLine, MLineJustification, PolylineEdge, Ray, Solid,
    Wipeout, XLine,
};
use opencadcodec::objects::ObjectType;
use opencadcodec::{
    Arc, Circle, Color, Ellipse, EntityType, Handle, LineWeight, LwPolyline, Point, Spline,
    Vector2, Vector3,
};
use opencadkernel::geom2d::{bounded_faces, Curve, Line as KernelLine, NurbsCurve, Tolerance};
use qiyas_geom::{flatten, flatten_within, snap_shapes, Paint, Xform};
use qiyas_render::layer_shown_with;
use serde::{Deserialize, Serialize};

use crate::session::{locate, Near};
use crate::{CoreError, Session};

/// The drawing state new entities take: their layer, colour, linetype and lineweight. Saved in
/// the project file; changing it is not an undo step (it is how the window draws, not the
/// drawing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Current {
    /// A layer of the drawing itself (not an xref's), as the drawing spells it.
    pub layer: String,
    /// "ByLayer", "ByBlock", "ACI n" (1 to 255) or "#rrggbb", as `EntityInfo::color` shows them.
    pub color: String,
    /// "ByLayer", "ByBlock", or a linetype of the drawing.
    pub linetype: String,
    /// Hundredths of a millimetre (0 to 211); -1 ByLayer, -2 ByBlock, -3 Default.
    pub lineweight: i16,
}

impl Default for Current {
    /// Layer "0", everything else ByLayer.
    fn default() -> Self {
        Current {
            layer: "0".to_string(),
            color: "ByLayer".to_string(),
            linetype: "ByLayer".to_string(),
            lineweight: -1,
        }
    }
}

/// A colour as `Current::color` writes it; `None` when it is none of those forms.
pub(crate) fn parse_color(text: &str) -> Option<Color> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("ByLayer") {
        return Some(Color::ByLayer);
    }
    if t.eq_ignore_ascii_case("ByBlock") {
        return Some(Color::ByBlock);
    }
    if let Some(n) = t
        .get(..4)
        .filter(|p| p.eq_ignore_ascii_case("ACI "))
        .and_then(|_| t[4..].trim().parse::<u8>().ok())
    {
        return (n >= 1).then_some(Color::Index(n));
    }
    let hex = t.strip_prefix('#').filter(|h| h.len() == 6)?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    let [_, r, g, b] = v.to_be_bytes();
    Some(Color::Rgb { r, g, b })
}

/// A hatch's fill. Angles are degrees, counter-clockwise; scale multiplies the pattern's own
/// sizes (millimetres, as in `acadiso.pat`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HatchPattern {
    Solid,
    /// 45 degree lines 3.175 apart.
    Ansi31,
    /// ANSI31 crossed at 135 degrees.
    Ansi37,
    /// A pattern of the built-in table by name (ANSI31, ANSI37, LINE, NET, DASH, BRICK), turned by
    /// `angle` degrees and scaled by `scale`.
    Custom {
        name: String,
        angle: f64,
        scale: f64,
    },
}

/// What `Command::Draw` adds, in drawing coordinates. Angles are degrees, counter-clockwise from
/// +X. Each adds one entity (a trace one per segment); `Outcome::Added` lists their handles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawSpec {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    /// A lightweight polyline; `bulges` (one per point, or none for straight segments) bends the
    /// segment starting at each point (tan of a quarter of its included angle, positive
    /// counter-clockwise).
    Polyline {
        points: Vec<[f64; 2]>,
        #[serde(default)]
        bulges: Vec<f64>,
        #[serde(default)]
        closed: bool,
    },
    /// A closed polyline with corners `a` and `b`.
    Rectangle {
        a: [f64; 2],
        b: [f64; 2],
    },
    /// A regular polygon (3 to 1024 sides) with a flat bottom edge: its corners on the circle of
    /// `radius` when `inscribed`, its sides touching it otherwise.
    Polygon {
        centre: [f64; 2],
        sides: u32,
        radius: f64,
        inscribed: bool,
    },
    Circle {
        centre: [f64; 2],
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end` (degrees).
    Arc {
        centre: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
    },
    /// The arc from `a` through `b` to `c`.
    ThreePointArc {
        a: [f64; 2],
        b: [f64; 2],
        c: [f64; 2],
    },
    /// A full ellipse: `major` is the major semi-axis (from the centre to its end), `ratio` the
    /// minor axis over the major (above 0, at most 1).
    Ellipse {
        centre: [f64; 2],
        major: [f64; 2],
        ratio: f64,
    },
    /// The cubic through `fit_points` (2 or more).
    Spline {
        fit_points: Vec<[f64; 2]>,
    },
    Point {
        p: [f64; 2],
    },
    /// A filled ring: `inner` and `outer` are diameters (`inner` 0 for a dot), as AutoCAD asks.
    Donut {
        centre: [f64; 2],
        inner: f64,
        outer: f64,
    },
    /// A half-infinite line from `origin` along `dir`.
    Ray {
        origin: [f64; 2],
        dir: [f64; 2],
    },
    /// An infinite construction line through `origin` along `dir`.
    #[serde(rename = "xline")]
    XLine {
        origin: [f64; 2],
        dir: [f64; 2],
    },
    /// A revision cloud around the polygon `points`: arcs about `arc_len` long bulging outward.
    #[serde(rename = "revcloud")]
    RevCloud {
        points: Vec<[f64; 2]>,
        arc_len: f64,
    },
    /// Covers the polygon `points` with the background colour.
    Wipeout {
        points: Vec<[f64; 2]>,
    },
    /// A hatch inside the closed entities `boundary_handles` (on the sheet; islands inside the
    /// outer one stay empty) or inside the ring `points`: one of the two.
    Hatch {
        #[serde(default)]
        boundary_handles: Vec<u64>,
        #[serde(default)]
        points: Vec<[f64; 2]>,
        /// With `points`: islands inside the ring, left empty (as `Boundary::Ring` gives them).
        #[serde(default)]
        holes: Vec<Vec<[f64; 2]>>,
        /// With `points`: per ring (the ring first, then each island) the bulge of the edge from
        /// each point to the next, as `Boundary::Ring` gives them; missing ones are straight.
        #[serde(default)]
        bulges: Vec<Vec<f64>>,
        pattern: HatchPattern,
    },
    /// A filled triangle or quadrilateral (AutoCAD's SOLID), its corners in AutoCAD's order: the
    /// outline runs first, second, fourth, third.
    #[serde(rename = "solid2d")]
    Solid2D {
        points: Vec<[f64; 2]>,
    },
    /// A wide line: one TRACE quad `width` wide per segment.
    Trace {
        points: Vec<[f64; 2]>,
        width: f64,
    },
    /// A multiline in the drawing's multiline style `style` ("Standard" when left out), its
    /// offsets times `scale` (1 when left out).
    #[serde(rename = "mline")]
    MLine {
        points: Vec<[f64; 2]>,
        #[serde(default)]
        style: Option<String>,
        #[serde(default)]
        scale: Option<f64>,
    },
    /// Freehand: an open polyline through `points`.
    Sketch {
        points: Vec<[f64; 2]>,
    },
}

/// One line family of a hatch pattern as `acadiso.pat` writes it: angle (degrees), origin, the
/// offset to the next line in the line's own axes (along, across) and the dashes (positive
/// drawn, negative gaps; none for a solid line).
struct PatternLine {
    angle: f64,
    origin: [f64; 2],
    offset: [f64; 2],
    dashes: &'static [f64],
}

const fn line(
    angle: f64,
    origin: [f64; 2],
    offset: [f64; 2],
    dashes: &'static [f64],
) -> PatternLine {
    PatternLine {
        angle,
        origin,
        offset,
        dashes,
    }
}

/// The built-in patterns, from `acadiso.pat` (millimetres).
const PATTERNS: &[(&str, &[PatternLine])] = &[
    ("ANSI31", &[line(45.0, [0.0, 0.0], [0.0, 3.175], &[])]),
    (
        "ANSI37",
        &[
            line(45.0, [0.0, 0.0], [0.0, 3.175], &[]),
            line(135.0, [0.0, 0.0], [0.0, 3.175], &[]),
        ],
    ),
    ("LINE", &[line(0.0, [0.0, 0.0], [0.0, 3.175], &[])]),
    (
        "NET",
        &[
            line(0.0, [0.0, 0.0], [0.0, 3.175], &[]),
            line(90.0, [0.0, 0.0], [0.0, 3.175], &[]),
        ],
    ),
    (
        "DASH",
        &[line(0.0, [0.0, 0.0], [3.175, 3.175], &[3.175, -3.175])],
    ),
    (
        "BRICK",
        &[
            line(0.0, [0.0, 0.0], [0.0, 6.35], &[]),
            line(90.0, [0.0, 0.0], [6.35, 6.35], &[6.35, -6.35]),
            line(90.0, [3.175, 0.0], [6.35, 6.35], &[-6.35, 6.35]),
        ],
    ),
];

/// The codec's pattern for a built-in one, turned by `angle` (radians) and scaled by `scale`,
/// as DXF keeps it: each line's angle with the turn in it, its origin and offset in drawing
/// axes, its dashes scaled.
fn pattern_lines(name: &str, angle: f64, scale: f64) -> Option<(String, CodecPattern)> {
    let (name, lines) = PATTERNS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))?;
    let turn = |[x, y]: [f64; 2], a: f64| {
        let (s, c) = a.sin_cos();
        Vector2::new(x * c - y * s, x * s + y * c)
    };
    let mut pattern = CodecPattern::new(*name);
    for l in *lines {
        let own = l.angle.to_radians();
        pattern.add_line(HatchPatternLine {
            angle: own + angle,
            base_point: turn([l.origin[0] * scale, l.origin[1] * scale], angle),
            offset: turn([l.offset[0] * scale, l.offset[1] * scale], own + angle),
            dash_lengths: l.dashes.iter().map(|d| d * scale).collect(),
        });
    }
    Some((name.to_string(), pattern))
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn finite(points: &[[f64; 2]]) -> Result<(), CoreError> {
    if points.iter().flatten().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("Every point must be two numbers"))
    }
}

fn positive(value: f64, what: &str) -> Result<f64, CoreError> {
    if value > 0.0 && value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(format!("The {what} must be more than 0")))
    }
}

fn v3([x, y]: [f64; 2]) -> Vector3 {
    Vector3::new(x, y, 0.0)
}

fn polyline(points: &[[f64; 2]], bulges: &[f64], closed: bool) -> LwPolyline {
    let mut p = LwPolyline::new();
    for (i, q) in points.iter().enumerate() {
        p.add_point_with_bulge(
            Vector2::new(q[0], q[1]),
            bulges.get(i).copied().unwrap_or(0.0),
        );
    }
    p.is_closed = closed;
    p
}

/// Twice the signed area of a ring: positive counter-clockwise.
fn twice_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// A ring of 3 points or more that encloses some area.
fn check_ring(points: &[[f64; 2]], what: &str) -> Result<(), CoreError> {
    finite(points)?;
    if points.len() < 3 || twice_area(points).abs() < 1e-12 {
        return Err(invalid(format!(
            "A {what} needs 3 points or more around an area"
        )));
    }
    Ok(())
}

/// A direction made unit length.
fn unit(dir: [f64; 2]) -> Result<Vector3, CoreError> {
    finite(&[dir])?;
    let len = dir[0].hypot(dir[1]);
    if len < 1e-12 {
        return Err(invalid("The direction must not be zero"));
    }
    Ok(Vector3::new(dir[0] / len, dir[1] / len, 0.0))
}

/// The circle through three points; `None` when they are in a line.
fn circle_through(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    let scale = [a, b, c]
        .iter()
        .flat_map(|p| [p[0].abs(), p[1].abs()])
        .fold(1.0, f64::max);
    if d.abs() < 1e-12 * scale * scale {
        return None;
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let x = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let y = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    Some(([x, y], (a[0] - x).hypot(a[1] - y)))
}

/// Counter-clockwise turn from `from` to `to`, in `[0, TAU)`.
fn ccw(from: f64, to: f64) -> f64 {
    (to - from).rem_euclid(TAU)
}

/// A closed boundary path made of one closed polyline edge.
fn ring_path(points: &[[f64; 2]], bulges: &[f64]) -> BoundaryPath {
    let mut edge = PolylineEdge::new(Vec::new(), true);
    for (i, p) in points.iter().enumerate() {
        edge.add_vertex(
            Vector2::new(p[0], p[1]),
            bulges.get(i).copied().unwrap_or(0.0),
        );
    }
    let mut path = BoundaryPath::external();
    path.add_edge(BoundaryEdge::Polyline(edge));
    path
}

/// AutoCAD's lineweights, in hundredths of a millimetre.
pub(crate) const LINEWEIGHTS: [i16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

impl Session {
    /// `Command::SetCurrent`: checks `current` against the drawing and keeps it, the layer as the
    /// drawing spells it.
    pub(crate) fn set_current(&mut self, mut current: Current) -> Result<(), CoreError> {
        let doc = &self.drawing.doc;
        let layer = doc
            .layers
            .get(current.layer.trim())
            .ok_or_else(|| invalid(format!("The drawing has no layer {}", current.layer)))?;
        current.layer = layer.name.clone();
        if parse_color(&current.color).is_none() {
            return Err(invalid(format!("{} is not a colour", current.color)));
        }
        let lt = current.linetype.trim();
        current.linetype = if lt.eq_ignore_ascii_case("ByLayer") {
            "ByLayer".to_string()
        } else if lt.eq_ignore_ascii_case("ByBlock") {
            "ByBlock".to_string()
        } else {
            doc.line_types
                .get(lt)
                .map(|t| t.name.clone())
                .ok_or_else(|| invalid(format!("The drawing has no linetype {lt}")))?
        };
        if !(LINEWEIGHTS.contains(&current.lineweight) || matches!(current.lineweight, -3..=-1)) {
            return Err(invalid(format!(
                "{} is not a lineweight",
                current.lineweight
            )));
        }
        self.current = current;
        Ok(())
    }

    /// `Command::Draw`: adds what `spec` describes to the sheet on screen, styled by the
    /// current drawing state, and returns the new handles. Checks everything first, so a refused
    /// draw adds nothing.
    pub(crate) fn draw(&mut self, spec: DrawSpec) -> Result<Vec<u64>, CoreError> {
        let entities = self.draw_entities(spec)?;
        if entities.iter().any(|e| matches!(e, EntityType::Wipeout(_))) {
            // WIPEOUT needs its class for the DWG and DXF writers.
            self.drawing.doc.classes.update_defaults();
        }
        self.add_styled(entities)
    }

    /// Adds `entities` to the sheet on screen styled by the current drawing state, and returns
    /// their handles.
    pub(crate) fn add_styled(&mut self, entities: Vec<EntityType>) -> Result<Vec<u64>, CoreError> {
        let doc = &self.drawing.doc;
        let c = &self.current;
        let layer = if doc.layers.contains(&c.layer) {
            c.layer.clone()
        } else {
            "0".to_string()
        };
        let color = parse_color(&c.color).unwrap_or(Color::ByLayer);
        let linetype = if c.linetype.eq_ignore_ascii_case("ByLayer") {
            String::new()
        } else {
            c.linetype.clone()
        };
        let weight = LineWeight::from_value(c.lineweight);
        let owner = self.sheet_owner();
        let mut handles = Vec::with_capacity(entities.len());
        for mut entity in entities {
            let common = entity.common_mut();
            common.layer = layer.clone();
            common.color = color;
            common.linetype = linetype.clone();
            common.line_weight = weight;
            if let Some(owner) = owner {
                common.owner_handle = owner;
            }
            let h = self
                .drawing
                .doc
                .add_entity(entity)
                .map_err(|e| invalid(e.to_string()))?;
            handles.push(h.value());
        }
        Ok(handles)
    }

    /// The entities `spec` describes, unstyled and not yet in the drawing.
    pub(crate) fn draw_entities(&self, spec: DrawSpec) -> Result<Vec<EntityType>, CoreError> {
        let one = |e: EntityType| Ok(vec![e]);
        match spec {
            DrawSpec::Line { a, b } => {
                finite(&[a, b])?;
                if a == b {
                    return Err(invalid("A line needs two different points"));
                }
                one(EntityType::Line(opencadcodec::Line::from_points(
                    v3(a),
                    v3(b),
                )))
            }
            DrawSpec::Polyline {
                points,
                bulges,
                closed,
            } => {
                finite(&points)?;
                if points.len() < 2 {
                    return Err(invalid("A polyline needs 2 points or more"));
                }
                if !bulges.is_empty() && bulges.len() != points.len() {
                    return Err(invalid("Give one bulge per point, or none"));
                }
                if !bulges.iter().all(|b| b.is_finite()) {
                    return Err(invalid("Every bulge must be a number"));
                }
                one(EntityType::LwPolyline(polyline(&points, &bulges, closed)))
            }
            DrawSpec::Rectangle { a, b } => {
                finite(&[a, b])?;
                if a[0] == b[0] || a[1] == b[1] {
                    return Err(invalid("A rectangle needs a width and a height"));
                }
                let corners = [a, [b[0], a[1]], b, [a[0], b[1]]];
                one(EntityType::LwPolyline(polyline(&corners, &[], true)))
            }
            DrawSpec::Polygon {
                centre,
                sides,
                radius,
                inscribed,
            } => {
                finite(&[centre])?;
                positive(radius, "radius")?;
                if !(3..=1024).contains(&sides) {
                    return Err(invalid("A polygon has 3 to 1024 sides"));
                }
                let n = f64::from(sides);
                let corner = if inscribed {
                    radius
                } else {
                    radius / (PI / n).cos()
                };
                // The first corner at the bottom left of a flat bottom edge.
                let first = -PI / 2.0 - PI / n;
                let points: Vec<[f64; 2]> = (0..sides)
                    .map(|k| {
                        let a = first + TAU * f64::from(k) / n;
                        [centre[0] + corner * a.cos(), centre[1] + corner * a.sin()]
                    })
                    .collect();
                one(EntityType::LwPolyline(polyline(&points, &[], true)))
            }
            DrawSpec::Circle { centre, radius } => {
                finite(&[centre])?;
                positive(radius, "radius")?;
                one(EntityType::Circle(Circle::from_center_radius(
                    v3(centre),
                    radius,
                )))
            }
            DrawSpec::Arc {
                centre,
                radius,
                start,
                end,
            } => {
                finite(&[centre, [start, end]])?;
                positive(radius, "radius")?;
                if ccw(start.to_radians(), end.to_radians()) < 1e-12 {
                    return Err(invalid("An arc's start and end angles must differ"));
                }
                one(EntityType::Arc(Arc::from_center_radius_angles(
                    v3(centre),
                    radius,
                    start.to_radians().rem_euclid(TAU),
                    end.to_radians().rem_euclid(TAU),
                )))
            }
            DrawSpec::ThreePointArc { a, b, c } => {
                finite(&[a, b, c])?;
                let (o, r) = circle_through(a, b, c)
                    .ok_or_else(|| invalid("The three points of an arc must not be in a line"))?;
                let angle = |p: [f64; 2]| (p[1] - o[1]).atan2(p[0] - o[0]).rem_euclid(TAU);
                let (aa, ab, ac) = (angle(a), angle(b), angle(c));
                // Counter-clockwise from a reaches b before c: a to c; otherwise c to a.
                let (start, end) = if ccw(aa, ab) < ccw(aa, ac) {
                    (aa, ac)
                } else {
                    (ac, aa)
                };
                one(EntityType::Arc(Arc::from_center_radius_angles(
                    v3(o),
                    r,
                    start,
                    end,
                )))
            }
            DrawSpec::Ellipse {
                centre,
                major,
                ratio,
            } => {
                finite(&[centre, major])?;
                if major[0].hypot(major[1]) < 1e-12 {
                    return Err(invalid("An ellipse's major axis must not be zero"));
                }
                if !(ratio > 0.0 && ratio <= 1.0) {
                    return Err(invalid("An ellipse's ratio is above 0 and at most 1"));
                }
                one(EntityType::Ellipse(Ellipse::from_center_axes(
                    v3(centre),
                    v3(major),
                    ratio,
                )))
            }
            DrawSpec::Spline { fit_points } => {
                finite(&fit_points)?;
                if fit_points.len() < 2 {
                    return Err(invalid("A spline needs 2 points or more"));
                }
                one(EntityType::Spline(spline_through(&fit_points)?))
            }
            DrawSpec::Point { p } => {
                finite(&[p])?;
                one(EntityType::Point(Point::at(v3(p))))
            }
            DrawSpec::Donut {
                centre,
                inner,
                outer,
            } => {
                finite(&[centre, [inner, outer]])?;
                if !(inner >= 0.0 && outer > inner) {
                    return Err(invalid(
                        "A donut's outer diameter must be more than its inner one (0 or more)",
                    ));
                }
                // Two half circles on the middle of the ring, as wide as the ring.
                let r = (inner + outer) / 4.0;
                let points = [[centre[0] - r, centre[1]], [centre[0] + r, centre[1]]];
                let mut p = polyline(&points, &[1.0, 1.0], true);
                p.constant_width = (outer - inner) / 2.0;
                one(EntityType::LwPolyline(p))
            }
            DrawSpec::Ray { origin, dir } => {
                finite(&[origin])?;
                one(EntityType::Ray(Ray::new(v3(origin), unit(dir)?)))
            }
            DrawSpec::XLine { origin, dir } => {
                finite(&[origin])?;
                one(EntityType::XLine(XLine::new(v3(origin), unit(dir)?)))
            }
            DrawSpec::RevCloud { points, arc_len } => {
                check_ring(&points, "revision cloud")?;
                positive(arc_len, "arc length")?;
                one(EntityType::LwPolyline(rev_cloud(points, arc_len)?))
            }
            DrawSpec::Wipeout { mut points } => {
                check_ring(&points, "wipeout")?;
                // A polygonal boundary is kept closed: the first point again at the end.
                points.push(points[0]);
                let vertices: Vec<Vector2> =
                    points.iter().map(|p| Vector2::new(p[0], p[1])).collect();
                one(EntityType::Wipeout(Wipeout::polygonal(&vertices, 0.0)))
            }
            DrawSpec::Hatch {
                boundary_handles,
                points,
                holes,
                bulges,
                pattern,
            } => one(EntityType::Hatch(self.hatch(
                &boundary_handles,
                &points,
                &holes,
                &bulges,
                pattern,
            )?)),
            DrawSpec::Solid2D { points } => {
                finite(&points)?;
                let solid = match points[..] {
                    [a, b, c] => Solid::triangle(v3(a), v3(b), v3(c)),
                    [a, b, c, d] => Solid::new(v3(a), v3(b), v3(c), v3(d)),
                    _ => return Err(invalid("A 2D solid has 3 or 4 corners")),
                };
                let outline: Vec<[f64; 2]> = solid
                    .boundary_corners()
                    .iter()
                    .map(|v| [v.x, v.y])
                    .collect();
                check_ring(&outline, "2D solid")?;
                one(EntityType::Solid(solid))
            }
            DrawSpec::Trace { points, width } => {
                finite(&points)?;
                positive(width, "width")?;
                if points.len() < 2 {
                    return Err(invalid("A trace needs 2 points or more"));
                }
                let mut traces = Vec::new();
                for pair in points.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                    if len < 1e-12 {
                        continue;
                    }
                    // Left of the segment first, then right, at each end.
                    let h = width / 2.0;
                    let n = [-(b[1] - a[1]) / len * h, (b[0] - a[0]) / len * h];
                    let corner = |p: [f64; 2], s: f64| v3([p[0] + s * n[0], p[1] + s * n[1]]);
                    let mut t = Solid::new(
                        corner(a, 1.0),
                        corner(a, -1.0),
                        corner(b, 1.0),
                        corner(b, -1.0),
                    );
                    t.is_trace = true;
                    traces.push(EntityType::Solid(t));
                }
                if traces.is_empty() {
                    return Err(invalid("A trace needs two different points"));
                }
                Ok(traces)
            }
            DrawSpec::MLine {
                points,
                style,
                scale,
            } => {
                finite(&points)?;
                if points.len() < 2 {
                    return Err(invalid("A multiline needs 2 points or more"));
                }
                // AutoCAD's CMLSCALE 20 in a millimetre drawing: 20 mm whatever the units.
                let scale = scale.unwrap_or(0.02 / self.drawing.units.to_metres());
                let scale = positive(scale, "scale")?;
                one(EntityType::MLine(self.mline(
                    &points,
                    style.as_deref().unwrap_or("Standard"),
                    scale,
                )?))
            }
            DrawSpec::Sketch { points } => {
                finite(&points)?;
                if points.len() < 2 {
                    return Err(invalid("A sketch needs 2 points or more"));
                }
                one(EntityType::LwPolyline(polyline(&points, &[], false)))
            }
        }
    }

    /// A hatch inside the closed entities `handles` (at the top of the sheet on screen) or the
    /// ring `points` less the islands `holes`.
    fn hatch(
        &self,
        handles: &[u64],
        points: &[[f64; 2]],
        holes: &[Vec<[f64; 2]>],
        bulges: &[Vec<f64>],
        pattern: HatchPattern,
    ) -> Result<Hatch, CoreError> {
        if !holes.is_empty() && points.is_empty() {
            return Err(invalid("Islands go with a ring of points"));
        }
        if bulges.iter().flatten().any(|b| !b.is_finite()) {
            return Err(invalid("Bulges must be finite numbers"));
        }
        let bulges_of = |i: usize| bulges.get(i).map_or(&[][..], Vec::as_slice);
        let pattern = match pattern {
            HatchPattern::Solid => None,
            HatchPattern::Ansi31 => Some(("ANSI31".to_string(), 0.0, 1.0)),
            HatchPattern::Ansi37 => Some(("ANSI37".to_string(), 0.0, 1.0)),
            HatchPattern::Custom { name, angle, scale } => Some((name, angle, scale)),
        };
        let mut hatch = match pattern {
            None => Hatch::solid(),
            Some((name, angle, scale)) => {
                finite(&[[angle, scale]])?;
                positive(scale, "pattern scale")?;
                let (name, lines) = pattern_lines(&name, angle.to_radians(), scale)
                    .ok_or_else(|| invalid(format!("There is no hatch pattern {name}")))?;
                let mut hatch = Hatch::with_pattern(lines);
                hatch.pattern.name = name;
                hatch.pattern_type = HatchPatternType::Predefined;
                hatch.pattern_angle = angle.to_radians();
                hatch.pattern_scale = scale;
                hatch
            }
        };
        match (handles.is_empty(), points.is_empty()) {
            (true, false) => {
                check_ring(points, "hatch boundary")?;
                hatch.add_path(ring_path(points, bulges_of(0)));
                for (i, hole) in holes.iter().enumerate() {
                    check_ring(hole, "hatch island")?;
                    let mut path = ring_path(hole, bulges_of(i + 1));
                    // Not external, still a polyline path: the DWG writer writes a polyline
                    // edge whole only under the polyline flag.
                    path.flags.set_external(false);
                    hatch.add_path(path);
                }
            }
            (false, true) => {
                let sheet = self.sheet_owner();
                for &h in handles {
                    let entity = self
                        .drawing
                        .doc
                        .get_entity(Handle::new(h))
                        .ok_or(CoreError::NoSuchEntity(h))?;
                    if Some(entity.common().owner_handle) != sheet {
                        return Err(invalid(format!("Entity {h:X} is not on this sheet")));
                    }
                    hatch.add_path(self.boundary_path(entity).ok_or_else(|| {
                        invalid(format!(
                            "Entity {h:X} is not closed: it can't bound a hatch"
                        ))
                    })?);
                }
            }
            _ => return Err(invalid(
                "A hatch needs its boundary: closed entities or a ring of points, one of the two",
            )),
        }
        Ok(hatch)
    }

    /// The hatch boundary of a closed entity: a closed polyline keeps its bulges, a circle is
    /// one full arc, anything else closed is its flattened outline. `None` when it is open.
    fn boundary_path(&self, entity: &EntityType) -> Option<BoundaryPath> {
        match entity {
            EntityType::LwPolyline(p) if p.is_closed && p.vertices.len() >= 3 => {
                let points: Vec<[f64; 2]> = p
                    .vertices
                    .iter()
                    .map(|v| [v.location.x, v.location.y])
                    .collect();
                let bulges: Vec<f64> = p.vertices.iter().map(|v| v.bulge).collect();
                Some(ring_path(&points, &bulges))
            }
            EntityType::Circle(c) => {
                let mut path = BoundaryPath::external();
                path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                    center: Vector2::new(c.center.x, c.center.y),
                    radius: c.radius,
                    start_angle: 0.0,
                    end_angle: TAU,
                    counter_clockwise: true,
                }));
                Some(path)
            }
            _ => {
                let paths = flatten(
                    &self.drawing.doc,
                    entity,
                    &Xform::identity(),
                    self.measure_tol(),
                )
                .into_paths();
                let ring = paths
                    .into_iter()
                    .find(|p| p.closed && p.points.len() >= 3)?;
                Some(ring_path(&ring.points, &[]))
            }
        }
    }

    /// A multiline through `points` in the drawing's multiline style `name`.
    fn mline(&self, points: &[[f64; 2]], name: &str, scale: f64) -> Result<MLine, CoreError> {
        let style = self
            .drawing
            .doc
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(name.trim()) => Some(s),
                _ => None,
            })
            .ok_or_else(|| invalid(format!("The drawing has no multiline style {name}")))?;
        let mut m = MLine::new();
        m.style_name = style.name.clone();
        m.style_handle = Some(style.handle);
        m.style_element_count = style.elements.len();
        m.scale_factor = scale;
        m.justification = MLineJustification::Zero;
        for p in points {
            m.add_vertex(v3(*p));
        }
        if m.vertices.len() < 2 {
            return Err(invalid("A multiline needs two different points"));
        }
        // Each element starts its offset along the miter from the vertex.
        let offsets: Vec<f64> = style.elements.iter().map(|e| e.offset * scale).collect();
        for v in &mut m.vertices {
            let across = Vector3::new(-v.direction.y, v.direction.x, 0.0);
            let cos = v.miter.dot(&across);
            let cos = if cos.abs() < 1e-9 { 1.0 } else { cos };
            for (segment, offset) in v.segments.iter_mut().zip(&offsets) {
                segment.parameters = vec![offset / cos, 0.0];
            }
        }
        Ok(m)
    }

    /// The closed area around `p` (drawing coordinates) that the drawn line work on the sheet
    /// makes, for a hatch: the smallest bounded region holding `p`, counter-clockwise, and the
    /// regions inside it as islands (AutoCAD's Normal style, outermost only). Text boxes and
    /// hatch pattern lines are left out. Looks `start` around `p` first, then 4 times as far
    /// each time until the region found lies inside the box looked at, so nothing outside it can
    /// cut the region. A box with more than `BOUNDARY_SEGMENTS` segments is refused
    /// (`TooDense`), never searched in part.
    pub fn boundary_at(&self, p: [f64; 2], start: f64) -> Boundary {
        self.boundary_at_most(p, start, BOUNDARY_SEGMENTS)
    }

    /// `boundary_at` with at most `max` segments in the box looked at.
    pub fn boundary_at_most(&self, p: [f64; 2], start: f64, max: usize) -> Boundary {
        if !(p.iter().all(|v| v.is_finite()) && start > 0.0 && start.is_finite()) {
            return Boundary::None;
        }
        let mut radius = start;
        loop {
            let Some(near) = self.segments_near(p, radius, max) else {
                return Boundary::TooDense;
            };
            let lo = [p[0] - radius, p[1] - radius];
            let hi = [p[0] + radius, p[1] + radius];
            // The entities drawn there, flattened again as finely as they are measured: curves
            // take more segments than drawn, so the cap is the drawn one's, with room for them.
            let (segments, rounds) = self.exact_near(&near, lo, hi);
            if segments.len() > max.saturating_mul(4) {
                return Boundary::TooDense;
            }
            if let Some(found) = region_around(&segments, p, radius, lo, hi) {
                return with_bulges(found, &rounds, self.measure_tol());
            }
            // The box held every segment there is: nothing further can close a region.
            if near.whole_sheet {
                return Boundary::None;
            }
            radius *= 4.0;
        }
    }
}

impl Session {
    /// The line work of `near`'s entities flattened again at the measuring tolerance (`measure_tol`) in drawing
    /// coordinates, as the sheet draws them (multiline elements, rays across the box, no texts,
    /// pattern lines or hidden layers): the segments reaching into the box `lo hi`, and the
    /// circles and arcs among the entities' true shapes. An entity inside an arrayed block
    /// reference keeps its drawn segments.
    fn exact_near(&self, near: &Near, lo: [f64; 2], hi: [f64; 2]) -> (Vec<[f64; 4]>, Vec<Round>) {
        let xrefs = self.xref_docs();
        let doc = &self.drawing.doc;
        let mut segments = Vec::new();
        let mut rounds = Vec::new();
        let reaches = |[x0, y0, x1, y1]: [f64; 4]| {
            x0.min(x1) <= hi[0] && x0.max(x1) >= lo[0] && y0.min(y1) <= hi[1] && y0.max(y1) >= lo[1]
        };
        let across = (hi[0] - lo[0]).hypot(hi[1] - lo[1]);
        for (i, (handle, chain)) in near.sources.iter().enumerate() {
            let arrayed = chain.iter().any(|&h| {
                matches!(doc.get_entity(Handle::new(h)),
                    Some(EntityType::Insert(r)) if r.row_count > 1 || r.column_count > 1)
            });
            let located = locate(doc, &xrefs, *handle, chain)
                .ok()
                .filter(|_| !arrayed);
            let Some(located) = located else {
                let drawn = near.drawn.iter().filter(|(k, _)| *k == i);
                segments.extend(drawn.map(|(_, s)| *s));
                continue;
            };
            let mut budget = qiyas_geom::MAX_WORK;
            let flat = flatten_within(
                located.doc,
                &xrefs,
                located.entity,
                &located.xf,
                self.measure_tol(),
                &mut budget,
            );
            for f in flat.paths {
                // Layer "0" inside a block is drawn on the reference's layer.
                let own = if f.layer == "0" {
                    &located.layer
                } else {
                    &f.layer
                };
                let layer = match &located.xref {
                    Some(x) if !own.contains('|') => format!("{x}|{own}"),
                    _ => own.clone(),
                };
                if f.text.is_some() || !layer_shown_with(doc, &xrefs, &self.visibility, &layer) {
                    continue;
                }
                let pts = &f.path.points;
                match f.paint {
                    Paint::Outline => {
                        let n = pts.len();
                        let count = if f.path.closed && n > 2 {
                            n
                        } else {
                            n.saturating_sub(1)
                        };
                        for k in 0..count {
                            let (a, b) = (pts[k], pts[(k + 1) % n]);
                            let seg = [a[0], a[1], b[0], b[1]];
                            if reaches(seg) {
                                segments.push(seg);
                            }
                        }
                    }
                    // Across the box: as far as the box is wide past the base point both ways.
                    Paint::Infinite { both_ways } => {
                        let [a, b] = [pts[0], pts[1]];
                        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                        if len < 1e-12 {
                            continue;
                        }
                        let far = across
                            + (a[0] - (lo[0] + hi[0]) / 2.0).hypot(a[1] - (lo[1] + hi[1]) / 2.0);
                        let d = [(b[0] - a[0]) / len * far, (b[1] - a[1]) / len * far];
                        let from = if both_ways {
                            [a[0] - d[0], a[1] - d[1]]
                        } else {
                            a
                        };
                        let seg = [from[0], from[1], a[0] + d[0], a[1] + d[1]];
                        if reaches(seg) {
                            segments.push(seg);
                        }
                    }
                    _ => {}
                }
            }
            let shapes = snap_shapes(located.doc, located.entity, &located.xf, self.measure_tol());
            for piece in shapes.iter().filter(|s| s.exact).flat_map(|s| &s.pieces) {
                match piece {
                    Curve::Circle(c) => rounds.push(Round {
                        centre: c.centre,
                        radius: c.radius,
                        start: 0.0,
                        sweep: TAU,
                    }),
                    Curve::Arc(a) => rounds.push(Round {
                        centre: a.centre,
                        radius: a.radius,
                        start: a.start_angle,
                        sweep: a.sweep(),
                    }),
                    _ => {}
                }
            }
        }
        (segments, rounds)
    }
}

/// A circle, or an arc of one (counter-clockwise from `start`), drawn near a hatch's point.
struct Round {
    centre: [f64; 2],
    radius: f64,
    start: f64,
    sweep: f64,
}

/// `found` with each edge that is a chord of a drawn circle or arc turned back into that arc:
/// its bulge (zero for a straight edge), per ring, the ring first.
fn with_bulges(found: Boundary, rounds: &[Round], tol: f64) -> Boundary {
    let Boundary::Ring { ring, holes, .. } = found else {
        return found;
    };
    // Chords are within `tol` (the measuring tolerance) of their arc; points where a line crosses
    // one sit on the chord, as far off the circle at most.
    let near = 1.5 * tol;
    let bulge = |a: [f64; 2], b: [f64; 2]| -> f64 {
        for r in rounds {
            let on =
                |q: [f64; 2]| ((q[0] - r.centre[0]).hypot(q[1] - r.centre[1]) - r.radius).abs();
            if on(a) > near || on(b) > near {
                continue;
            }
            let (u, v) = (
                [a[0] - r.centre[0], a[1] - r.centre[1]],
                [b[0] - r.centre[0], b[1] - r.centre[1]],
            );
            let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]);
            // Only a chord as fine as the flattening: never a straight line across the circle.
            if turn.abs() < 1e-12 || r.radius * (1.0 - (turn / 2.0).cos()) > near {
                continue;
            }
            let mid = u[1].atan2(u[0]) + turn / 2.0;
            if (mid - r.start).rem_euclid(TAU) > r.sweep + 1e-9 {
                continue;
            }
            return (turn / 4.0).tan();
        }
        0.0
    };
    let bulges_of = |pts: &[[f64; 2]]| -> Vec<f64> {
        let n = pts.len();
        (0..n).map(|i| bulge(pts[i], pts[(i + 1) % n])).collect()
    };
    let bulges = std::iter::once(&ring)
        .chain(&holes)
        .map(|r| bulges_of(r))
        .collect();
    Boundary::Ring {
        ring,
        holes,
        bulges,
    }
}

/// What `Session::boundary_at` finds around a point.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "found", rename_all = "snake_case")]
pub enum Boundary {
    /// The region's ring (counter-clockwise) and its islands, and per ring (the ring first,
    /// then each island) the bulge of the edge from each point to the next: an arc where the
    /// edge runs along a drawn circle or arc, else 0.
    Ring {
        ring: Vec<[f64; 2]>,
        holes: Vec<Vec<[f64; 2]>>,
        bulges: Vec<Vec<f64>>,
    },
    /// The point is inside no closed region.
    None,
    /// Too much line work around the point to look through: zoom in.
    TooDense,
}

/// Most segments `boundary_at` looks through at once.
pub const BOUNDARY_SEGMENTS: usize = 50_000;

/// The smallest region of `segments` holding `p` with its islands, when it lies inside the box
/// `lo hi`; `None` when there is none there yet.
fn region_around(
    segments: &[[f64; 4]],
    p: [f64; 2],
    radius: f64,
    lo: [f64; 2],
    hi: [f64; 2],
) -> Option<Boundary> {
    let lines: Vec<KernelLine> = segments
        .iter()
        .map(|&[x0, y0, x1, y1]| KernelLine {
            start: [x0, y0],
            end: [x1, y1],
        })
        .collect();
    // Weld points a millionth of the box apart.
    let tol = (radius * 1e-6).max(1e-6);
    let faces = bounded_faces(&lines, Tolerance::new(tol));
    let (at, ring) = faces
        .iter()
        .enumerate()
        .filter(|(_, ring)| inside(ring, p))
        .min_by(|(_, a), (_, b)| twice_area(a).abs().total_cmp(&twice_area(b).abs()))?;
    let fits = ring
        .iter()
        .all(|q| q[0] >= lo[0] && q[0] <= hi[0] && q[1] >= lo[1] && q[1] <= hi[1]);
    if !fits {
        return None;
    }
    // Regions wholly inside the ring are islands; one inside another island is not.
    let within = |outer: &[[f64; 2]], inner: &[[f64; 2]]| inner.iter().all(|q| inside(outer, *q));
    let inner: Vec<usize> = (0..faces.len())
        .filter(|&i| i != at && within(ring, &faces[i]))
        .collect();
    let holes = inner
        .iter()
        .filter(|&&i| {
            !inner
                .iter()
                .any(|&o| o != i && within(&faces[o], &faces[i]))
        })
        .map(|&i| faces[i].clone())
        .collect();
    Some(Boundary::Ring {
        ring: ring.clone(),
        holes,
        bulges: Vec::new(),
    })
}

/// Even-odd: `p` is inside `ring`.
fn inside(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = ring.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x > p[0] {
                odd = !odd;
            }
        }
    }
    odd
}

/// The cubic spline through `fit`: its fit points, and the control points and knots of the curve
/// through them (chord-length parameters, natural ends).
fn spline_through(fit: &[[f64; 2]]) -> Result<Spline, CoreError> {
    use opencadkernel::geom2d::Parameterization;
    let curve = NurbsCurve::interpolate(fit, None, None, Parameterization::Chord)
        .ok_or_else(|| invalid("A spline needs 2 different points or more"))?;
    let mut spline = Spline::from_fit_points(fit.iter().map(|p| v3(*p)).collect());
    spline.degree = curve.degree() as i32;
    spline.control_points = curve.control_points().iter().map(|p| v3(*p)).collect();
    spline.knots = curve.knots().to_vec();
    spline.flags.planar = true;
    Ok(spline)
}

/// A closed polyline of outward arcs about `arc_len` long around the polygon `points`.
fn rev_cloud(mut points: Vec<[f64; 2]>, arc_len: f64) -> Result<LwPolyline, CoreError> {
    if twice_area(&points) < 0.0 {
        points.reverse();
    }
    // Counter-clockwise, so a positive bulge bends outward: an arc of about 106 degrees.
    const BULGE: f64 = 0.5;
    let n = points.len();
    let mut corners = Vec::new();
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let pieces = (len / arc_len).round().max(1.0);
        if pieces > 10_000.0 {
            return Err(invalid("That arc length makes too many arcs"));
        }
        for k in 0..pieces as usize {
            let t = k as f64 / pieces;
            corners.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    let bulges = vec![BULGE; corners.len()];
    Ok(polyline(&corners, &bulges, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_as_the_properties_palette_writes_them() {
        assert_eq!(parse_color("ByLayer"), Some(Color::ByLayer));
        assert_eq!(parse_color("byblock"), Some(Color::ByBlock));
        assert_eq!(parse_color("ACI 7"), Some(Color::Index(7)));
        assert_eq!(parse_color("ACI 0"), None);
        assert_eq!(parse_color("ACI 256"), None);
        assert_eq!(
            parse_color("#0a0B0c"),
            Some(Color::Rgb {
                r: 10,
                g: 11,
                b: 12
            })
        );
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn every_built_in_pattern_has_a_spacing() {
        for (name, _) in PATTERNS {
            let (_, p) = pattern_lines(name, 0.3, 2.0).unwrap();
            for l in &p.lines {
                let across = -l.angle.sin() * l.offset.x + l.angle.cos() * l.offset.y;
                assert!(across.abs() > 1.0, "{name}: {across}");
            }
        }
        assert!(pattern_lines("ansi31", 0.0, 1.0).is_some());
        assert!(pattern_lines("NOPE", 0.0, 1.0).is_none());
    }
}
