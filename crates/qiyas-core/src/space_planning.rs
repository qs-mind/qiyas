//! Space geometry and comparisons with the job’s cited limits.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::EntityType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::rooms::{LabelLanguage, RoomsQuery};
use crate::walls::OpeningKind;
use crate::{CoreError, Session};

const MAX_FLOORS: usize = 20;
const MAX_ROOMS: usize = 300;

/// A plan to check: floors with their rooms, doors and stairs. Sizes are in metres and square metres.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanModel {
    #[serde(default)]
    pub floors: Vec<PlanFloor>,
}

/// One floor of the plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFloor {
    /// For example "Ground floor".
    pub name: String,
    #[serde(default)]
    pub rooms: Vec<PlanRoom>,
    #[serde(default)]
    pub doors: Vec<PlanDoor>,
    #[serde(default)]
    pub stairs: Vec<PlanStair>,
}

/// A room with an optional classification supplied by the job.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanRoom {
    pub name: String,
    /// Any classification supplied by the caller.
    #[serde(default)]
    pub kind: Option<String>,
    /// Explicit designation; a name does not establish design intent.
    #[serde(default)]
    pub master: Option<bool>,
    pub area_m2: f64,
    /// The narrowest clear side, metres.
    #[serde(default)]
    pub width_m: Option<f64>,
    /// The longest side, metres.
    #[serde(default)]
    pub length_m: Option<f64>,
    /// Centre `[x, y]` in metres, for distances between rooms.
    #[serde(default)]
    pub centre_m: Option<[f64; 2]>,
    /// Names of the rooms this room has a door to. A bathroom that opens only to one bedroom is
    /// its en-suite.
    #[serde(default)]
    pub opens_to: Vec<String>,
    /// Names of the rooms it shares a wall with.
    #[serde(default)]
    pub adjacent: Vec<String>,
    /// Fixture labels supplied by the drawing or caller.
    #[serde(default)]
    pub fixtures: Vec<String>,
}

/// A door that matters to the checks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanDoor {
    #[serde(default)]
    pub name: Option<String>,
    /// The room it opens into, by name.
    #[serde(default)]
    pub to: Option<String>,
    /// Clear leaf width, or both leaves together, metres.
    pub width_m: f64,
    /// Number of leaves, when known.
    #[serde(default)]
    pub leaves: Option<u8>,
    #[serde(default)]
    pub main_entrance: bool,
    /// Position `[x, y]` in metres.
    #[serde(default)]
    pub position_m: Option<[f64; 2]>,
    /// What its swing runs into, in words ("stair landing"), when it does.
    #[serde(default)]
    pub swing_conflict: Option<String>,
}

/// A stair on a floor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanStair {
    #[serde(default)]
    pub name: Option<String>,
    /// Clear width of the flight, metres.
    #[serde(default)]
    pub width_m: Option<f64>,
    /// The room it stands in, by name (a corridor or hall, or its own stair room).
    #[serde(default)]
    pub located_in: Option<String>,
    /// True when walls enclose it.
    #[serde(default)]
    pub enclosed: Option<bool>,
    /// Clear passage left beside the stair in its corridor or hall, metres.
    #[serde(default)]
    pub passage_beside_m: Option<f64>,
    #[serde(default)]
    pub riser_m: Option<f64>,
    #[serde(default)]
    pub going_m: Option<f64>,
    #[serde(default)]
    pub steps_per_flight: Option<u32>,
    #[serde(default)]
    pub headroom_m: Option<f64>,
    /// Distance from the nearest door to the first or last step, metres.
    #[serde(default)]
    pub door_distance_m: Option<f64>,
}

/// A floor to read from the drawing's rooms.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FloorQuery {
    #[serde(default, flatten)]
    pub boundary_layers: crate::object_regions::BoundaryLayers,
    pub name: String,
    /// `[[min_x, min_y], [max_x, max_y]]` in drawing units: the floor's part of the sheet.
    #[serde(default)]
    pub within: Option<[[f64; 2]; 2]>,
    /// The layer of the room-name texts.
    #[serde(default)]
    pub text_layer: Option<String>,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}
fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn check_model(plan: &PlanModel) -> Result<(), CoreError> {
    if plan.floors.is_empty() || plan.floors.len() > MAX_FLOORS {
        return Err(invalid(format!("A plan has 1 to {MAX_FLOORS} floors")));
    }
    for floor in &plan.floors {
        if floor.name.trim().is_empty() {
            return Err(invalid("Each floor needs a name"));
        }
        if floor.rooms.len() > MAX_ROOMS {
            return Err(invalid(format!(
                "Floor {} has more than {MAX_ROOMS} rooms",
                floor.name
            )));
        }
        for room in &floor.rooms {
            if room.name.trim().is_empty() || !room.area_m2.is_finite() || room.area_m2 < 0.0 {
                return Err(invalid(format!(
                    "Each room needs a name and a non-negative area (floor {})",
                    floor.name
                )));
            }
            for v in [room.width_m, room.length_m].into_iter().flatten() {
                if !v.is_finite() || v < 0.0 {
                    return Err(invalid(format!(
                        "Room {}: sizes are non-negative metres",
                        room.name
                    )));
                }
            }
            if room
                .centre_m
                .is_some_and(|p| p.into_iter().any(|v| !v.is_finite()))
            {
                return Err(invalid("Room centres must be finite metres"));
            }
        }
        for door in &floor.doors {
            if !door.width_m.is_finite() || door.width_m <= 0.0 {
                return Err(invalid("A door needs a positive width in metres"));
            }
            if door.leaves == Some(0)
                || door
                    .position_m
                    .is_some_and(|p| p.into_iter().any(|v| !v.is_finite()))
            {
                return Err(invalid(
                    "A door needs a positive leaf count and a finite position",
                ));
            }
        }
        for stair in &floor.stairs {
            if [
                stair.width_m,
                stair.riser_m,
                stair.going_m,
                stair.headroom_m,
            ]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v <= 0.0)
                || [stair.passage_beside_m, stair.door_distance_m]
                    .into_iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v < 0.0)
                || stair.steps_per_flight == Some(0)
            {
                return Err(invalid(
                    "Stair sizes must be finite positive metres; clearances may be zero",
                ));
            }
        }
    }
    Ok(())
}

fn facts(plan: &PlanModel) -> Vec<crate::job_checks::Fact> {
    let mut facts = Vec::new();
    for floor in &plan.floors {
        for (target, items) in [
            (
                "room",
                floor.rooms.iter().map(|v| json!(v)).collect::<Vec<_>>(),
            ),
            ("door", floor.doors.iter().map(|v| json!(v)).collect()),
            ("stair", floor.stairs.iter().map(|v| json!(v)).collect()),
            (
                "floor",
                vec![
                    json!({"name":floor.name,"area_m2":floor.rooms.iter().map(|r|r.area_m2).sum::<f64>(),"room_count":floor.rooms.len(),"door_count":floor.doors.len(),"stair_count":floor.stairs.len()}),
                ],
            ),
        ] {
            for item in items {
                for (metric, value) in item.as_object().expect("plan object") {
                    if let Some(measured) = value.as_f64() {
                        let unit = if metric.ends_with("_m2") {
                            "m2"
                        } else if metric.ends_with("_m") {
                            "m"
                        } else {
                            "pcs"
                        };
                        facts.push(crate::job_checks::Fact {
                            target: target.into(),
                            floor: floor.name.clone(),
                            name: item["name"].as_str().unwrap_or("").into(),
                            classification: item["kind"].as_str().map(str::to_owned),
                            metric: metric.clone(),
                            unit: unit.into(),
                            measured,
                            handle: None,
                        });
                    }
                }
            }
        }
    }
    facts
}

type Point = [f64; 2];

fn edges(ring: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len())
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn on_edge(p: Point, a: Point, b: Point, tol: f64) -> bool {
    let ab = sub(b, a);
    let len = ab[0].hypot(ab[1]);
    len > tol
        && cross(ab, sub(p, a)).abs() <= tol * len
        && dot(sub(p, a), ab) >= -tol * len
        && dot(sub(p, b), ab) <= tol * len
}
fn inside(ring: &[Point], p: Point, tol: f64) -> bool {
    if edges(ring).any(|(a, b)| on_edge(p, a, b, tol)) {
        return true;
    }
    let mut hit = false;
    for (a, b) in edges(ring) {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            hit = !hit;
        }
    }
    hit
}
fn shared_segments(a: &[Point], b: &[Point], tol: f64) -> Vec<[Point; 2]> {
    let mut shared = Vec::new();
    for (p, q) in edges(a) {
        let d = sub(q, p);
        let len = d[0].hypot(d[1]);
        if len <= tol {
            continue;
        }
        let u = [d[0] / len, d[1] / len];
        for (r, s) in edges(b) {
            if cross(u, sub(r, p)).abs() > tol || cross(u, sub(s, p)).abs() > tol {
                continue;
            }
            let x = dot(sub(r, p), u);
            let y = dot(sub(s, p), u);
            let lo = x.min(y).max(0.0);
            let hi = x.max(y).min(len);
            if hi - lo > tol {
                shared.push([
                    [p[0] + u[0] * lo, p[1] + u[1] * lo],
                    [p[0] + u[0] * hi, p[1] + u[1] * hi],
                ]);
            }
        }
    }
    shared
}

/// Interior chords sampled between projected vertices in every polygon-edge direction.
/// Unlike an enclosing box these stop at concave boundaries and island edges.
pub(crate) fn clear_widths(rings: &[Vec<Point>], tol: f64) -> Vec<Value> {
    let mut result = Vec::new();
    visit_clear_widths(rings, tol, |u, t, lo, hi| {
        let n = [-u[1], u[0]];
        result.push(json!({"width_m":hi-lo,"direction":u,
            "from_m":[u[0]*lo+n[0]*t,u[1]*lo+n[1]*t],
            "to_m":[u[0]*hi+n[0]*t,u[1]*hi+n[1]*t]}));
    });
    result
}

pub(crate) fn minimum_clear_width(rings: &[Vec<Point>], tol: f64) -> Option<f64> {
    let mut minimum: Option<f64> = None;
    visit_clear_widths(rings, tol, |_, _, lo, hi| {
        minimum = Some(minimum.map_or(hi - lo, |old| old.min(hi - lo)));
    });
    minimum
}

fn visit_clear_widths(rings: &[Vec<Point>], tol: f64, mut visit: impl FnMut(Point, f64, f64, f64)) {
    let Some(outer) = rings.first() else {
        return;
    };
    let mut directions: Vec<Point> = Vec::new();
    for (a, b) in edges(outer) {
        let d = sub(b, a);
        let len = d[0].hypot(d[1]);
        if len <= tol {
            continue;
        }
        let mut u = [d[0] / len, d[1] / len];
        if u[0] < -tol || (u[0].abs() <= tol && u[1] < 0.0) {
            u = [-u[0], -u[1]];
        }
        if !directions.iter().any(|v| cross(*v, u).abs() <= 1e-9) {
            directions.push(u);
        }
    }
    let segments: Vec<_> = rings.iter().flat_map(|ring| edges(ring)).collect();
    for u in directions {
        let n = [-u[1], u[0]];
        let mut offsets: Vec<_> = rings.iter().flatten().map(|p| dot(*p, n)).collect();
        offsets.sort_by(f64::total_cmp);
        offsets.dedup_by(|a, b| (*a - *b).abs() <= tol);
        let projected: Vec<_> = segments
            .iter()
            .map(|(a, b)| (dot(*a, n), dot(*b, n)))
            .collect();
        let mut events = Vec::with_capacity(2 * segments.len());
        for (i, &(ta, tb)) in projected.iter().enumerate() {
            if ta != tb {
                events.push((ta.min(tb), true, i));
                events.push((ta.max(tb), false, i));
            }
        }
        events.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut next = 0;
        let mut active = BTreeSet::new();
        let mut hits = Vec::new();
        for interval in offsets.windows(2) {
            let t = (interval[0] + interval[1]) / 2.0;
            // Only edges crossing this interval can contribute a chord.
            while next < events.len() && events[next].0 <= t {
                let (_, entering, i) = events[next];
                if entering {
                    active.insert(i);
                } else {
                    active.remove(&i);
                }
                next += 1;
            }
            hits.clear();
            for &i in &active {
                let (a, b) = segments[i];
                let (ta, tb) = projected[i];
                let f = (t - ta) / (tb - ta);
                hits.push(dot([a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])], u));
            }
            hits.sort_by(f64::total_cmp);
            for pair in hits.as_chunks::<2>().0 {
                if pair[1] - pair[0] > tol {
                    visit(u, t, pair[0], pair[1]);
                }
            }
        }
    }
}

fn point_along(points: &[[f64; 2]], closed: bool, at: f64) -> Option<[f64; 2]> {
    let mut left = at;
    for (a, b) in points.iter().zip(
        points
            .iter()
            .skip(1)
            .chain(points.first().filter(|_| closed)),
    ) {
        let w = [a, b];
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if len > 0.0 && left <= len {
            let t = left / len;
            return Some([
                w[0][0] + t * (w[1][0] - w[0][0]),
                w[0][1] + t * (w[1][1] - w[0][1]),
            ]);
        }
        left -= len;
    }
    points.last().copied()
}

impl Session {
    /// Read real room polygons, including holes. Curves use the drawing's geometric tolerance.
    fn plan_floor_from_drawing(
        &self,
        query: &FloorQuery,
    ) -> Result<(PlanFloor, Vec<Value>), CoreError> {
        let k = self.drawing.units.to_metres();
        let tol = self.measure_tol() * k;
        let report = self.rooms(&RoomsQuery {
            boundary_layers: query.boundary_layers.clone(),
            text_layer: query.text_layer.clone(),
            within: query.within,
            ..Default::default()
        })?;
        let mut rooms = Vec::new();
        let mut polygons = Vec::new();
        let mut facts = Vec::new();
        let owner = self.sheet_owner();
        let items: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter(|e| owner.is_none_or(|o| e.common().owner_handle == o))
            .filter_map(|e| match e {
                EntityType::Insert(i) => Some((
                    e.common().handle.value(),
                    i.block_name.clone(),
                    [i.insert_point.x * k, i.insert_point.y * k],
                )),
                EntityType::Point(p) => Some((
                    e.common().handle.value(),
                    String::new(),
                    [p.location.x * k, p.location.y * k],
                )),
                _ => None,
            })
            .collect();
        for (index, row) in report.rooms.iter().enumerate() {
            let name = row
                .name
                .clone()
                .unwrap_or_else(|| format!("Room / فراغ {}", index + 1));
            let mut rings = Vec::new();
            let mut perimeter = 0.0;
            if let crate::Boundary::Ring {
                ring,
                holes,
                bulges,
            } = self.room_boundary_in(&self.sheet, row.point, &query.boundary_layers)
            {
                for (i, ring) in std::iter::once(ring).chain(holes).enumerate() {
                    let entity = EntityType::LwPolyline(crate::object_regions::region_polyline(
                        &ring,
                        bulges.get(i).map_or(&[], Vec::as_slice),
                    ));
                    perimeter += qiyas_geom::exact_length(&entity, &qiyas_geom::Xform::identity())
                        .unwrap_or(0.0)
                        * k;
                    let paths = qiyas_geom::flatten(
                        &self.drawing.doc,
                        &entity,
                        &qiyas_geom::Xform::identity(),
                        self.measure_tol(),
                    )
                    .into_paths();
                    if let Some(path) = paths.first() {
                        rings.push(
                            path.points
                                .iter()
                                .map(|p| [p[0] * k, p[1] * k])
                                .collect::<Vec<_>>(),
                        );
                    }
                }
            }
            let widths = clear_widths(&rings, tol);
            let fixtures: Vec<_> = items
                .iter()
                .filter(|(_, _, p)| {
                    rings.first().is_some_and(|r| inside(r, *p, tol))
                        && !rings.iter().skip(1).any(|r| inside(r, *p, tol))
                })
                .map(|(h, n, p)| json!({"handle":h,"name":n,"position_m":p}))
                .collect();
            let kind = row
                .boundary_handle
                .and_then(|h| self.drawing.doc.get_entity(opencadcodec::Handle::new(h)))
                .and_then(crate::objects::instance)
                .and_then(|o| {
                    o.props
                        .get("room_kind")
                        .or_else(|| o.props.get("space_kind"))
                        .or_else(|| {
                            o.props
                                .get("kind")
                                .filter(|kind| kind.as_str() != Some("room"))
                        })
                        .and_then(Value::as_str)
                        .map(String::from)
                });
            rooms.push(PlanRoom {
                name: name.clone(),
                kind,
                area_m2: row.net_area_m2,
                width_m: widths
                    .iter()
                    .filter_map(|w| w["width_m"].as_f64())
                    .reduce(f64::min),
                centre_m: Some([row.point[0] * k, row.point[1] * k]),
                fixtures: fixtures
                    .iter()
                    .filter_map(|f| f["name"].as_str().map(String::from))
                    .collect(),
                ..Default::default()
            });
            facts.push(json!({"name":name,"area_m2":row.net_area_m2,"perimeter_m":if rings.is_empty(){None}else{Some(perimeter)},
                "clear_widths_m":widths,"width_method":"interior_chords_at_projected_vertex_interval_midpoints",
                "polygon_tolerance_m":tol,"boundary_m":rings,"fixtures":fixtures,"shared_boundaries":[],"openings":[]}));
            polygons.push(rings);
        }
        for i in 0..rooms.len() {
            let mut shared = Vec::new();
            for j in 0..rooms.len() {
                if i == j {
                    continue;
                }
                let segments: Vec<_> = polygons[i]
                    .iter()
                    .flat_map(|a| {
                        polygons[j]
                            .iter()
                            .flat_map(move |b| shared_segments(a, b, tol))
                    })
                    .collect();
                if !segments.is_empty() {
                    let other_name = rooms[j].name.clone();
                    rooms[i].adjacent.push(other_name);
                    let length: f64 = segments
                        .iter()
                        .map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1]))
                        .sum();
                    shared.push(
                        json!({"space":rooms[j].name,"length_m":length,"segments_m":segments}),
                    );
                }
            }
            facts[i]["shared_boundaries"] = json!(shared);
        }
        let mut doors = Vec::new();
        let mut stairs = Vec::new();
        let mut seen_openings = BTreeSet::new();
        for group in self.wall_groups(None)? {
            for run in &group.runs {
                for opening in &run.openings {
                    seen_openings.insert(opening.insert);
                    let o = &opening.opening;
                    let Some(p) = point_along(&run.points, run.closed, o.at) else {
                        continue;
                    };
                    if query.within.is_some_and(|[lo, hi]| {
                        p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1]
                    }) {
                        continue;
                    }
                    let p = [p[0] * k, p[1] * k];
                    // The physical opening spans the host's measured thickness, not an arbitrary box tolerance.
                    let distance = run.thickness * k / 2.0 + tol;
                    for (i, rings) in polygons.iter().enumerate() {
                        if rings
                            .iter()
                            .any(|r| edges(r).any(|(a, b)| on_edge(p, a, b, distance)))
                        {
                            facts[i]["openings"].as_array_mut().expect("array").push(json!({"handle":opening.insert,"kind":o.kind,"width_m":o.width*k,"position_m":p,"sill_m":o.sill,"head_m":o.head}));
                        }
                    }
                    if matches!(o.kind, OpeningKind::Door) {
                        doors.push(PlanDoor {
                            name: opening.mark.clone(),
                            width_m: o.width * k,
                            position_m: Some(p),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        // The drawing's object definitions identify openings and stairs.
        for object in self.objects(&crate::objects::ObjectFilter {
            sheet: Some(self.sheet.clone()),
            ..Default::default()
        })? {
            let id = object.instance.id;
            if !matches!(object.category.as_str(), "opening" | "door" | "stair")
                || seen_openings.contains(&id)
            {
                continue;
            }
            let Some(entity) = self.drawing.doc.get_entity(opencadcodec::Handle::new(id)) else {
                continue;
            };
            let p = match entity {
                EntityType::Point(p) => [p.location.x * k, p.location.y * k],
                EntityType::Insert(p) => [p.insert_point.x * k, p.insert_point.y * k],
                _ => continue,
            };
            if query.within.is_some_and(|[lo, hi]| {
                p[0] < lo[0] * k || p[0] > hi[0] * k || p[1] < lo[1] * k || p[1] > hi[1] * k
            }) {
                continue;
            }
            let props = &object.instance.props;
            if object.category == "stair" {
                let number = |key: &str| props.get(key).and_then(Value::as_f64);
                stairs.push(PlanStair {
                    name: props.get("name").and_then(Value::as_str).map(str::to_owned),
                    width_m: number("width_m").or_else(|| number("width")),
                    riser_m: number("riser_m").or_else(|| number("riser")),
                    going_m: number("going_m").or_else(|| number("going")),
                    headroom_m: number("headroom_m"),
                    passage_beside_m: number("passage_beside_m"),
                    door_distance_m: number("door_distance_m"),
                    steps_per_flight: props
                        .get("steps_per_flight")
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    ..Default::default()
                });
                continue;
            }
            seen_openings.insert(id);
            let kind = props
                .get("kind")
                .cloned()
                .unwrap_or_else(|| json!(object.category));
            let width = props.get("width").and_then(Value::as_f64);
            if kind == "door" {
                if let Some(width_m) = width {
                    doors.push(PlanDoor {
                        name: props.get("mark").and_then(Value::as_str).map(str::to_owned),
                        width_m,
                        position_m: Some(p),
                        ..Default::default()
                    });
                }
            }
            let distance = props
                .get("host_width")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                / 2.0
                + tol;
            for (i, rings) in polygons.iter().enumerate() {
                if rings
                    .iter()
                    .any(|r| edges(r).any(|(a, b)| on_edge(p, a, b, distance)))
                {
                    facts[i]["openings"]
                        .as_array_mut()
                        .expect("array")
                        .push(json!({
                            "handle":id,"kind":kind,"width_m":width,"position_m":p,
                            "sill_m":props.get("sill"),"head_m":props.get("head")
                        }));
                }
            }
        }
        for fact in &mut facts {
            fact["fixtures"].as_array_mut().expect("array").retain(|f| {
                f["handle"]
                    .as_u64()
                    .is_none_or(|h| !seen_openings.contains(&h))
            });
        }
        Ok((
            PlanFloor {
                name: query.name.clone(),
                rooms,
                doors,
                stairs,
            },
            facts,
        ))
    }

    /// Read measured floor facts and overlay explicit classifications and job limits.
    pub(crate) fn space_check(
        &self,
        plan: Option<PlanModel>,
        floors: &[FloorQuery],
        language: Option<LabelLanguage>,
    ) -> Result<Value, CoreError> {
        let mut model = PlanModel::default();
        let mut geometry = BTreeMap::new();
        for query in floors {
            let (floor, facts) = self.plan_floor_from_drawing(query)?;
            geometry.insert(query.name.clone(), facts);
            model.floors.push(floor);
        }
        if let Some(plan) = plan {
            for extra in plan.floors {
                match model.floors.iter_mut().find(|f| same(&f.name, &extra.name)) {
                    Some(floor) => {
                        for room in extra.rooms {
                            match floor.rooms.iter_mut().find(|r| same(&r.name, &room.name)) {
                                Some(old) => {
                                    if room.kind.is_some() {
                                        old.kind = room.kind;
                                    }
                                    if room.master.is_some() {
                                        old.master = room.master;
                                    }
                                    old.opens_to = room.opens_to;
                                }
                                None => floor.rooms.push(room),
                            }
                        }
                        floor.doors.extend(extra.doors);
                        floor.stairs.extend(extra.stairs);
                    }
                    None => model.floors.push(extra),
                }
            }
        }
        if model.floors.is_empty() {
            return Err(invalid(
                "Give a plan (floors with rooms) or the floors to read from the drawing",
            ));
        }
        check_model(&model)?;
        let classifications = self.space_classifications()?;
        for floor in &mut model.floors {
            for room in &mut floor.rooms {
                if room.kind.is_none() {
                    room.kind = classifications.get(&floor.name, &room.name);
                }
            }
        }
        let measurements = self.check_facts(&facts(&model))?;
        let findings: Vec<_> = measurements
            .iter()
            .filter(|r| r["passed"] == false)
            .cloned()
            .collect();
        let not_checked: Vec<_> = measurements
            .iter()
            .filter(|r| r["passed"].is_null())
            .cloned()
            .collect();
        let mut value = json!({"findings":findings,"measurements":measurements,"not_checked":not_checked,
            "status":"for_review","summary":"Measured facts; only supplied cited limits are compared. No limit means no pass or fail.",
            "summary_ar":"حقائق مقاسة؛ تقارن فقط بالحدود المقدمة ذات المصدر. دون حد لا توجد نتيجة نجاح أو إخفاق."});
        value["language"] = json!(
            if language.or(self.sheet_language()) == Some(LabelLanguage::Ar) {
                "ar"
            } else {
                "en"
            }
        );
        value["floors"] = json!(model.floors.iter().map(|f| {
            let mut spaces = geometry.get(&f.name).cloned().unwrap_or_else(|| f.rooms.iter().map(|r| json!({
                "name":r.name,"area_m2":r.area_m2,"perimeter_m":null,"clear_widths_m":[],
                "supplied_width_m":r.width_m,"supplied_length_m":r.length_m,
                "adjacent_supplied":r.adjacent,"fixtures_supplied":r.fixtures,
                "shared_boundaries":[],"openings":[],"source":"caller"
            })).collect());
            for room in &f.rooms {
                if !spaces.iter().any(|s|s["name"].as_str()==Some(room.name.as_str())) {
                    spaces.push(json!({"name":room.name,"area_m2":room.area_m2,"perimeter_m":null,
                        "clear_widths_m":[],"shared_boundaries":[],"openings":[],"source":"caller"}));
                }
            }
            for space in &mut spaces {
                if let Some(room) = f.rooms.iter().find(|r| r.name == space["name"].as_str().unwrap_or("")) {
                    space["kind"] = json!(room.kind);
                }
            }
            let total: f64 = f.rooms.iter().map(|r|r.area_m2).sum();
            json!({"name":f.name,"spaces":spaces,"doors":f.doors,"stairs":f.stairs,"area_m2":total})
        }).collect::<Vec<_>>());
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_sweep_matches_independent_scanlines_through_concavity_and_round_islands() {
        let outer = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [6.0, 4.0],
            [6.0, 10.0],
            [0.0, 10.0],
        ];
        let hole = (0..64)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / 64.0;
                [3.0 + angle.cos(), 3.0 + angle.sin()]
            })
            .collect();
        let rings = vec![outer, hole];
        let tol = 1e-6;
        let mut expected = Vec::new();
        for axis in 0..2 {
            let across = 1 - axis;
            let mut cuts: Vec<_> = rings.iter().flatten().map(|p| p[across]).collect();
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() <= tol);
            for interval in cuts.windows(2) {
                let cut = (interval[0] + interval[1]) / 2.0;
                let mut hits = Vec::new();
                for ring in &rings {
                    for (a, b) in edges(ring) {
                        if (a[across] > cut) != (b[across] > cut) {
                            hits.push(
                                a[axis]
                                    + (b[axis] - a[axis]) * (cut - a[across])
                                        / (b[across] - a[across]),
                            );
                        }
                    }
                }
                hits.sort_by(f64::total_cmp);
                expected.extend(
                    hits.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| pair[1] - pair[0])
                        .filter(|w| *w > tol),
                );
            }
        }
        let mut actual: Vec<_> = clear_widths(&rings, tol)
            .iter()
            .map(|row| row["width_m"].as_f64().unwrap())
            .collect();
        expected.sort_by(f64::total_cmp);
        actual.sort_by(f64::total_cmp);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(&expected) {
            assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
        }
        assert!((minimum_clear_width(&rings, tol).unwrap() - expected[0]).abs() < 1e-8);
        assert_eq!(minimum_clear_width(&[], tol), None);
    }

    #[test]
    fn polygon_facts_exclude_notches_and_corner_contacts() {
        let l = vec![
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 1.0],
            [1.0, 1.0],
            [1.0, 4.0],
            [0.0, 4.0],
        ];
        let notch = vec![[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]];
        let adjoining = vec![[-2.0, 0.0], [0.0, 0.0], [0.0, 4.0], [-2.0, 4.0]];
        let corner = vec![[4.0, 1.0], [5.0, 1.0], [5.0, 2.0], [4.0, 2.0]];
        assert!(!inside(&l, [2.5, 2.5], 1e-9));
        assert!(inside(&l, [0.5, 2.5], 1e-9));
        assert!(shared_segments(&l, &notch, 1e-9).is_empty());
        assert!(shared_segments(&l, &corner, 1e-9).is_empty());
        let shared = shared_segments(&l, &adjoining, 1e-9);
        assert_eq!(shared.len(), 1);
        assert!(
            ((shared[0][1][0] - shared[0][0][0]).hypot(shared[0][1][1] - shared[0][0][1]) - 4.0)
                .abs()
                < 1e-9
        );
        let widths = clear_widths(&[l], 1e-9);
        assert!(widths.iter().any(|w| w["width_m"] == 1.0));
    }

    #[test]
    fn clear_chords_stop_at_islands_and_follow_rotated_edges() {
        let outer = vec![[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [0.0, 4.0]];
        let hole = vec![[2.0, 1.0], [4.0, 1.0], [4.0, 3.0], [2.0, 3.0]];
        let widths = clear_widths(&[outer, hole], 1e-9);
        assert!(widths.iter().any(|w| w["width_m"] == 2.0));
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let rotated: Vec<_> = [[0.0, 0.0], [4.0, 0.0], [4.0, 1.0], [0.0, 1.0]]
            .into_iter()
            .map(|[x, y]| [(x - y) * k, (x + y) * k])
            .collect();
        let widths = clear_widths(&[rotated], 1e-9);
        assert!(widths
            .iter()
            .any(|w| (w["width_m"].as_f64().unwrap() - 1.0).abs() < 1e-8));
    }
}
