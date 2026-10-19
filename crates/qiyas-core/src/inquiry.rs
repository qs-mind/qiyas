//! Inquiry: what one entity's geometry is (radius, centre, angles), where a point is, points
//! placed along a path (divide and measure), and entity data pulled into a table.
//!
//! Values are shown in metres, square metres and degrees, with the drafting precision. Row labels
//! are short keys ("radius", "centre") the window translates.

use std::f64::consts::TAU;

use opencadcodec::{Color, EntityType, Point, Vector3};
use opencadkernel::geom2d::{distance_to, Curve};
use qiyas_geom::{object_to_world, snap_shapes, XrefDocs};
use serde::{Deserialize, Serialize};

use crate::commands::insert_point;
use crate::drawing::short_layer_name;
use crate::query::{bounds, plain_text, texts_of, Candidate};
use crate::session::{effective_block_name, locate, variant_name, Located};
use crate::{CoreError, EntityFilter, Kind, Measurement, Session, SnapKind};

/// The layer divided and measured points go on, and its colour index (yellow).
pub const POINT_LAYER: &str = "QIYAS-POINT";
const POINT_COLOR: u8 = 2;
/// Most points one divide or measure places.
const MAX_POINTS: usize = 1000;
/// Most rows one extraction returns.
const MAX_EXTRACT: usize = 5000;

/// One column of an extraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Handle,
    Kind,
    Layer,
    BlockName,
    Text,
    /// In metres; empty for a block reference (counted, not measured).
    Length,
    /// In square metres; empty for a block reference.
    Area,
    /// Where the entity sits (insertion point, centre, start, or the middle of its bounds), in
    /// metres.
    X,
    Y,
    /// The value of a block reference's attribute with this tag (any case).
    Attribute(String),
    /// The sheet's name.
    Sheet,
    /// The xref the entity is in.
    Xref,
}

impl Field {
    /// The column's heading.
    pub fn heading(&self) -> String {
        match self {
            Field::Handle => "Handle".into(),
            Field::Kind => "Kind".into(),
            Field::Layer => "Layer".into(),
            Field::BlockName => "Block".into(),
            Field::Text => "Text".into(),
            Field::Length => "Length (m)".into(),
            Field::Area => "Area (m²)".into(),
            Field::X => "X (m)".into(),
            Field::Y => "Y (m)".into(),
            Field::Attribute(tag) => tag.clone(),
            Field::Sheet => "Sheet".into(),
            Field::Xref => "Xref".into(),
        }
    }
}

/// How points go along a path: this many equal parts, or one every this many drawing units.
pub(crate) enum Spacing {
    Count(usize),
    Step(f64),
}

/// Numbers as the user reads them: drawing units turned into metres, `precision` decimals.
struct Shown {
    metres: f64,
    precision: usize,
}

impl Shown {
    fn fixed(&self, v: f64) -> String {
        let s = format!("{:.*}", self.precision, v);
        // "-0.000" reads as a mistake.
        match s.strip_prefix('-') {
            Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
            _ => s,
        }
    }
    fn number(&self, v: f64) -> String {
        self.fixed(v * self.metres)
    }
    fn length(&self, v: f64) -> String {
        format!("{} m", self.number(v))
    }
    fn area(&self, v: f64) -> String {
        format!("{} m²", self.fixed(v * self.metres * self.metres))
    }
    fn point(&self, p: [f64; 2]) -> String {
        format!("{}, {} m", self.number(p[0]), self.number(p[1]))
    }
    /// An angle in radians as degrees from 0 up to 360.
    fn angle(&self, radians: f64) -> String {
        let d = radians.rem_euclid(TAU).to_degrees();
        format!("{}°", self.fixed(if d >= 360.0 { 0.0 } else { d }))
    }
    /// A sweep in radians as degrees (360 stays 360).
    fn sweep(&self, radians: f64) -> String {
        format!("{}°", self.fixed(radians.to_degrees()))
    }
}

type Rows = Vec<(String, String)>;

fn row(rows: &mut Rows, label: &str, value: String) {
    rows.push((label.to_string(), value));
}

impl Session {
    fn shown(&self) -> Shown {
        Shown {
            metres: self.drawing.units.to_metres(),
            precision: usize::from(self.drafting.precision),
        }
    }

    /// The geometry of the entity `handle` inside `chain`, as placed in the world: a title (its
    /// type) and rows. A polyline gives the segment nearest `at` (its first when `None`).
    pub(crate) fn geometry_info(
        &self,
        handle: u64,
        chain: &[u64],
        at: Option<[f64; 2]>,
    ) -> Result<(String, Rows), CoreError> {
        let xrefs = self.xref_docs();
        let located = locate(&self.drawing.doc, &xrefs, handle, chain)?;
        let s = self.shown();
        let e = located.entity;
        let mut rows = Rows::new();
        row(
            &mut rows,
            "layer",
            short_layer_name(&located.layer).to_string(),
        );
        match e {
            EntityType::Insert(insert) => {
                row(
                    &mut rows,
                    "block_name",
                    effective_block_name(located.doc, insert),
                );
                if let Some(p) = insert_point(&located) {
                    row(&mut rows, "insertion", s.point(p));
                }
                let scale = format!(
                    "{}, {}",
                    s.fixed(insert.x_scale()),
                    s.fixed(insert.y_scale())
                );
                row(&mut rows, "scale", scale);
                row(&mut rows, "rotation", s.angle(insert.rotation));
                for a in &insert.attributes {
                    row(&mut rows, "attribute", format!("{} = {}", a.tag, a.value));
                }
            }
            EntityType::Text(t) => {
                row(&mut rows, "text", t.value.clone());
                row(&mut rows, "height", s.length(t.height));
                row(&mut rows, "style", t.style.clone());
            }
            EntityType::MText(t) => {
                row(&mut rows, "text", plain_text(&t.value));
                row(&mut rows, "height", s.length(t.height));
                row(&mut rows, "style", t.style.clone());
            }
            EntityType::Point(p) => {
                let at = located.xf.apply([p.location.x, p.location.y]);
                row(&mut rows, "position", s.point(at));
            }
            _ => self.curve_rows(&located, &s, at, &mut rows),
        }
        Ok((variant_name(e), rows))
    }

    /// Rows for a line, arc, circle, ellipse or polyline; the length and area of anything else.
    fn curve_rows(&self, located: &Located, s: &Shown, at: Option<[f64; 2]>, rows: &mut Rows) {
        let e = located.entity;
        let extent = self.extent(located.doc, e, &located.xf);
        let shapes = snap_shapes(located.doc, e, &located.xf, self.measure_tol());
        let polyline = matches!(
            e,
            EntityType::LwPolyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Polyline(_)
                | EntityType::Polyline3D(_)
        );
        let exact = shapes.len() == 1 && shapes[0].exact && !shapes[0].pieces.is_empty();
        if exact && polyline {
            let pieces = &shapes[0].pieces;
            let i = at.map_or(0, |p| nearest_piece(pieces, p));
            row(rows, "segment", format!("{} / {}", i + 1, pieces.len()));
            piece_rows(&pieces[i], s, rows);
            if let Some(length) = extent.length {
                row(rows, "total_length", s.length(length));
            }
            if let Some(area) = extent.area {
                row(rows, "area", s.area(area));
            }
            return;
        }
        if exact {
            match &shapes[0].pieces[0] {
                Curve::Ellipse(arc) => {
                    let el = &arc.ellipse;
                    row(rows, "centre", s.point(el.centre));
                    row(rows, "major_radius", s.length(el.major_radius));
                    row(rows, "minor_radius", s.length(el.minor_radius));
                    if (arc.sweep() - TAU).abs() > 1e-12 {
                        row(rows, "start_angle", s.angle(arc.start_parameter));
                        row(rows, "end_angle", s.angle(arc.end_parameter));
                        row(rows, "included_angle", s.sweep(arc.sweep()));
                    }
                }
                piece => {
                    piece_rows(piece, s, rows);
                    // A circle's area; an arc's or a line's length is in its rows already.
                    if let (Curve::Circle(_), Some(area)) = (piece, extent.area) {
                        row(rows, "area", s.area(area));
                    }
                    return;
                }
            }
        }
        if let Some(length) = extent.length {
            row(rows, "length", s.length(length));
        }
        if let Some(area) = extent.area {
            row(rows, "area", s.area(area));
        }
    }

    /// Where `p` is, in metres, and the snap that found it.
    pub(crate) fn point_info(&self, p: [f64; 2], kind: Option<SnapKind>) -> Rows {
        let s = self.shown();
        let mut rows = Rows::new();
        row(&mut rows, "x", s.length(p[0]));
        row(&mut rows, "y", s.length(p[1]));
        if let Some(kind) = kind {
            row(&mut rows, "snap", snap_name(kind).to_string());
        }
        rows
    }

    /// Places point entities on `QIYAS-POINT` along the entity `handle` inside `chain` (as the
    /// world sees it, by true arc length from its start), and returns the count of them, made by
    /// the user on the current sheet. The caller records the undo step.
    pub(crate) fn place_along(
        &mut self,
        handle: u64,
        chain: Vec<u64>,
        spacing: Spacing,
    ) -> Result<Measurement, CoreError> {
        let (positions, layer, description) = {
            let xrefs: XrefDocs = self.xref_docs();
            let located = locate(&self.drawing.doc, &xrefs, handle, &chain)?;
            let path = oriented_path(&located, self.measure_tol());
            let total: f64 = path.iter().map(|(c, _)| c.length()).sum();
            if path.is_empty() || !(total > 0.0 && total.is_finite()) {
                return Err(invalid("This entity has no path to place points along"));
            }
            let closed = {
                let start = point_at(&path, 0.0);
                let end = end_of(path.last().expect("path is not empty"));
                dist(start, end) <= 1e-9 * total.max(1.0)
            };
            let (distances, description): (Vec<f64>, String) = match spacing {
                Spacing::Count(n) => {
                    if !(2..=MAX_POINTS).contains(&n) {
                        return Err(invalid("Divide into 2 to 1000 parts"));
                    }
                    let first = if closed { 0 } else { 1 };
                    let d = (first..n).map(|k| total * k as f64 / n as f64).collect();
                    (
                        d,
                        format!("Divide {n} on {}", short_layer_name(&located.layer)),
                    )
                }
                Spacing::Step(step) => {
                    if !(step > 0.0 && step.is_finite()) {
                        return Err(invalid("The step must be more than 0"));
                    }
                    // Points before the end only: a step that fits exactly stops one short.
                    let n = (total / step * (1.0 - 1e-12)).floor();
                    if n < 1.0 {
                        return Err(invalid("The step is longer than the path"));
                    }
                    if n > MAX_POINTS as f64 {
                        return Err(invalid("That step places more than 1000 points"));
                    }
                    let d = (1..=n as usize).map(|k| step * k as f64).collect();
                    (
                        d,
                        format!(
                            "Measure every {step} on {}",
                            short_layer_name(&located.layer)
                        ),
                    )
                }
            };
            let positions: Vec<[f64; 2]> = distances.iter().map(|&d| point_at(&path, d)).collect();
            (positions, located.layer.clone(), description)
        };

        self.ensure_layer(POINT_LAYER, Color::Index(POINT_COLOR));
        let owner = self.sheet_owner();
        let mut handles = Vec::with_capacity(positions.len());
        for p in &positions {
            let mut point = Point::new();
            point.location = Vector3::new(p[0], p[1], 0.0);
            point.common.layer = POINT_LAYER.to_string();
            if let Some(owner) = owner {
                point.common.owner_handle = owner;
            }
            let h = self
                .drawing
                .doc
                .add_entity(EntityType::Point(point))
                .map_err(|e| CoreError::InvalidCommand(e.to_string()))?;
            handles.push(h.value());
        }
        let mut m = Measurement::new(
            Kind::Count,
            handles.len() as f64,
            self.drawing.units,
            description,
            layer,
            self.sheet.clone(),
        );
        m.handles = handles;
        // Where the points are too, so the count is drawn and survives a save without the project.
        m.points = positions;
        m.auto_description = true;
        Ok(m)
    }

    /// The entities `filter` finds (up to 5,000 a page; `limit` and `offset` page through them),
    /// one row of `fields` each.
    pub(crate) fn extract(
        &self,
        filter: &EntityFilter,
        fields: &[Field],
    ) -> Result<(Vec<String>, Vec<Vec<String>>), CoreError> {
        if fields.is_empty() {
            return Err(invalid("Choose at least one field"));
        }
        let s = self.shown();
        let xrefs = self.xref_docs();
        let sheet_block = self.sheet_block(filter.sheet.as_deref());
        let sheet_name = self
            .drawing
            .sheets()
            .into_iter()
            .find(|sh| sh.block.eq_ignore_ascii_case(&sheet_block))
            .map_or(sheet_block, |sh| sh.name);
        let rows = self
            .matching(filter, &xrefs)
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit.min(MAX_EXTRACT))
            .map(|c| {
                // A block reference is counted, not measured: no length or area.
                let needs_extent = !matches!(c.entity, EntityType::Insert(_))
                    && fields
                        .iter()
                        .any(|f| matches!(f, Field::Length | Field::Area));
                let extent = needs_extent.then(|| self.extent(c.doc, c.entity, &c.xf));
                let needs_at = fields.iter().any(|f| matches!(f, Field::X | Field::Y));
                let at = needs_at.then(|| self.position(&xrefs, &c));
                fields
                    .iter()
                    .map(|f| match f {
                        Field::Handle => c.entity.common().handle.value().to_string(),
                        Field::Kind => variant_name(c.entity),
                        Field::Layer => c.layer.to_string(),
                        Field::BlockName => match c.entity {
                            EntityType::Insert(i) => effective_block_name(c.doc, i),
                            _ => String::new(),
                        },
                        Field::Text => texts_of(c.entity).join(" | "),
                        Field::Length => extent
                            .as_ref()
                            .and_then(|x| x.length)
                            .map_or_else(String::new, |v| s.number(v)),
                        Field::Area => extent
                            .as_ref()
                            .and_then(|x| x.area)
                            .map_or_else(String::new, |v| s.fixed(v * s.metres * s.metres)),
                        Field::X => at.map_or_else(String::new, |p| s.number(p[0])),
                        Field::Y => at.map_or_else(String::new, |p| s.number(p[1])),
                        Field::Attribute(tag) => match c.entity {
                            EntityType::Insert(i) => i
                                .attributes
                                .iter()
                                .find(|a| a.tag.eq_ignore_ascii_case(tag))
                                .map_or_else(String::new, |a| a.value.clone()),
                            _ => String::new(),
                        },
                        Field::Sheet => sheet_name.clone(),
                        Field::Xref => c.xref.unwrap_or_default().to_string(),
                    })
                    .collect()
            })
            .collect();
        Ok((fields.iter().map(Field::heading).collect(), rows))
    }

    /// Where an entity sits, in world coordinates: a block reference's insertion point, a text's,
    /// a point's, a circle's or arc's centre, a line's start; else the middle of its bounds.
    fn position(&self, xrefs: &XrefDocs, c: &Candidate) -> [f64; 2] {
        let own = match c.entity {
            EntityType::Insert(i) => {
                let p = i.insert_point;
                Some(object_to_world(&i.normal, p.z).apply([p.x, p.y]))
            }
            EntityType::Text(t) => Some([t.insertion_point.x, t.insertion_point.y]),
            EntityType::MText(t) => Some([t.insertion_point.x, t.insertion_point.y]),
            EntityType::Point(p) => Some([p.location.x, p.location.y]),
            EntityType::Circle(e) => Some([e.center.x, e.center.y]),
            EntityType::Arc(e) => Some([e.center.x, e.center.y]),
            EntityType::Line(e) => Some([e.start.x, e.start.y]),
            _ => None,
        };
        match own {
            Some(p) => c.xf.apply(p),
            None => {
                let [lo, hi] = bounds(&self.points_of(xrefs, c));
                [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]
            }
        }
    }
}

/// Rows for one line, arc or circle.
fn piece_rows(piece: &Curve, s: &Shown, rows: &mut Rows) {
    match piece {
        Curve::Line(l) => {
            let (dx, dy) = (l.end[0] - l.start[0], l.end[1] - l.start[1]);
            row(rows, "length", s.length(dx.hypot(dy)));
            row(rows, "angle", s.angle(dy.atan2(dx)));
            row(rows, "delta_x", s.length(dx));
            row(rows, "delta_y", s.length(dy));
            row(rows, "start", s.point(l.start));
            row(rows, "end", s.point(l.end));
        }
        Curve::Arc(a) => {
            row(rows, "radius", s.length(a.radius));
            row(rows, "diameter", s.length(2.0 * a.radius));
            row(rows, "centre", s.point(a.centre));
            row(rows, "start_angle", s.angle(a.start_angle));
            row(rows, "end_angle", s.angle(a.end_angle));
            row(rows, "included_angle", s.sweep(a.sweep()));
            row(rows, "arc_length", s.length(a.radius * a.sweep()));
        }
        Curve::Circle(c) => {
            row(rows, "radius", s.length(c.radius));
            row(rows, "diameter", s.length(2.0 * c.radius));
            row(rows, "centre", s.point(c.centre));
            row(rows, "perimeter", s.length(TAU * c.radius));
        }
        other => row(rows, "length", s.length(other.length())),
    }
}

/// The index of the piece nearest `p`.
fn nearest_piece(pieces: &[Curve], p: [f64; 2]) -> usize {
    pieces
        .iter()
        .map(|c| distance_to(c, p))
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(i, _)| i)
}

/// The entity's pieces in the order a walk from its start meets them, each with whether it runs
/// backwards (a clockwise bulge is stored counter-clockwise, so from its far end).
fn oriented_path(located: &Located, tol: f64) -> Vec<(Curve, bool)> {
    let shapes = snap_shapes(located.doc, located.entity, &located.xf, tol);
    let mut out = Vec::new();
    for shape in shapes {
        let mut cursor = shape.vertices.first().copied();
        for piece in shape.pieces {
            let (a, b) = (piece.point_at(0.0), piece.point_at(1.0));
            let reversed = cursor.is_some_and(|c| dist(b, c) < dist(a, c));
            cursor = Some(if reversed { a } else { b });
            out.push((piece, reversed));
        }
    }
    out
}

fn end_of((c, reversed): &(Curve, bool)) -> [f64; 2] {
    c.point_at(if *reversed { 0.0 } else { 1.0 })
}

/// The point `d` along `path` from its start (its end when past it).
fn point_at(path: &[(Curve, bool)], d: f64) -> [f64; 2] {
    let mut walked = 0.0;
    for (i, (c, reversed)) in path.iter().enumerate() {
        let len = c.length();
        if d <= walked + len || i + 1 == path.len() {
            let local = (d - walked).clamp(0.0, len);
            return if *reversed {
                c.point_at_distance(len - local)
            } else {
                c.point_at_distance(local)
            };
        }
        walked += len;
    }
    [0.0, 0.0]
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn snap_name(kind: SnapKind) -> &'static str {
    match kind {
        SnapKind::End => "end",
        SnapKind::Intersection => "intersection",
        SnapKind::Mid => "mid",
        SnapKind::Centre => "centre",
        SnapKind::Node => "node",
        SnapKind::Perpendicular => "perpendicular",
        SnapKind::Nearest => "nearest",
    }
}

fn invalid(why: &str) -> CoreError {
    CoreError::InvalidCommand(why.to_string())
}
