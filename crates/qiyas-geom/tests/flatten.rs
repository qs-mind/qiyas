use std::f64::consts::{FRAC_PI_2, PI, TAU};

use std::time::Instant;

use opencadcodec::entities::{
    BoundaryEdge, BoundaryPath, CircularArcEdge, EntityType, Hatch, Insert, LineEdge, LwPolyline,
};
use opencadcodec::tables::{BlockRecord, TableEntry};
use opencadcodec::{Arc, CadDocument, Circle, Color, Ellipse, Layer, Line, Vector2, Vector3};
use qiyas_geom::{exact_area, exact_length, flatten, Path, Xform};

fn flat(doc: &CadDocument, entity: EntityType, tol: f64) -> Vec<Path> {
    flatten(doc, &entity, &Xform::identity(), tol).into_paths()
}

fn closed_lwpolyline(points: &[(f64, f64, f64)]) -> EntityType {
    let mut pl = LwPolyline::new();
    for &(x, y, bulge) in points {
        pl.add_point_with_bulge(Vector2::new(x, y), bulge);
    }
    pl.is_closed = true;
    EntityType::LwPolyline(pl)
}

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(x1, y1, 0.0),
        Vector3::new(x2, y2, 0.0),
    ))
}

fn assert_points(actual: &[[f64; 2]], expected: &[[f64; 2]]) {
    assert_eq!(actual.len(), expected.len(), "{actual:?}");
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a[0] - e[0]).abs() < 1e-9 && (a[1] - e[1]).abs() < 1e-9,
            "{actual:?} != {expected:?}"
        );
    }
}

/// Adds a block definition named `name` holding `entities`.
fn add_block(doc: &mut CadDocument, name: &str, entities: Vec<EntityType>) {
    add_block_at(doc, name, Vector3::new(0.0, 0.0, 0.0), entities);
}

/// Adds a block definition with the given base point.
fn add_block_at(doc: &mut CadDocument, name: &str, base: Vector3, entities: Vec<EntityType>) {
    let mut record = BlockRecord::new(name);
    record.set_handle(doc.allocate_handle());
    record.base_point = base;
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    for mut entity in entities {
        entity.common_mut().owner_handle = owner;
        doc.add_entity(entity).unwrap();
    }
}

#[test]
fn rectangle_area_and_perimeter() {
    let doc = CadDocument::new();
    let rect = closed_lwpolyline(&[
        (0.0, 0.0, 0.0),
        (4.0, 0.0, 0.0),
        (4.0, 3.0, 0.0),
        (0.0, 3.0, 0.0),
    ]);
    let paths = flat(&doc, rect, 1e-3);
    assert_eq!(paths.len(), 1);
    assert!(paths[0].closed);
    assert!((paths[0].area() - 12.0).abs() < 1e-9);
    assert!((paths[0].length() - 14.0).abs() < 1e-9);
}

#[test]
fn circle_area_converges() {
    let doc = CadDocument::new();
    let circle = Circle::from_center_radius(Vector3::new(0.0, 0.0, 0.0), 1.0);
    let paths = flat(&doc, EntityType::Circle(circle), 1e-4);
    assert_eq!(paths.len(), 1);
    assert!(paths[0].closed);
    assert!((paths[0].area() - PI).abs() < 1e-3);
    assert!((paths[0].length() - 2.0 * PI).abs() < 1e-3);
}

#[test]
fn bulge_semicircle() {
    let doc = CadDocument::new();
    let half = closed_lwpolyline(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)]);
    let paths = flat(&doc, half, 1e-4);
    assert_eq!(paths.len(), 1);
    assert!((paths[0].area() - FRAC_PI_2).abs() < 1e-3);
}

#[test]
fn nested_scaled_rotated_insert() {
    let mut doc = CadDocument::new();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    add_block(&mut doc, "A", vec![EntityType::Line(line)]);
    let a = Insert::new("A", Vector3::new(0.0, 0.0, 0.0)).with_scale(2.0, 2.0, 2.0);
    add_block(&mut doc, "B", vec![EntityType::Insert(a)]);

    let mut b = Insert::new("B", Vector3::new(5.0, 5.0, 0.0)).with_scale(3.0, 3.0, 3.0);
    b.rotation = FRAC_PI_2;
    let paths = flat(&doc, EntityType::Insert(b), 1e-3);

    assert_eq!(paths.len(), 1);
    let p = &paths[0];
    assert!((p.length() - 6.0).abs() < 1e-9);
    let (start, end) = (p.points[0], p.points[p.points.len() - 1]);
    assert!((start[0] - 5.0).abs() < 1e-9 && (start[1] - 5.0).abs() < 1e-9);
    assert!((end[0] - 5.0).abs() < 1e-9 && (end[1] - 11.0).abs() < 1e-9);
}

#[test]
fn self_intersecting_bowtie() {
    let bowtie = Path {
        points: vec![[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]],
        closed: true,
    };
    assert!(bowtie.self_intersects());
    assert!((bowtie.area() - 2.0).abs() < 1e-9);
}

#[test]
fn open_path_has_zero_area() {
    let doc = CadDocument::new();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(3.0, 4.0, 0.0));
    let paths = flat(&doc, EntityType::Line(line), 1e-3);
    assert_eq!(paths.len(), 1);
    assert!(!paths[0].closed);
    assert_eq!(paths[0].area(), 0.0);
}

#[test]
fn negative_bulge_turns_clockwise() {
    let doc = CadDocument::new();
    let half = closed_lwpolyline(&[(0.0, 0.0, -1.0), (2.0, 0.0, 0.0)]);
    let paths = flat(&doc, half, 1e-4);
    assert!((paths[0].area() - FRAC_PI_2).abs() < 1e-3);
    // Going left to right clockwise, the arc rises above the chord.
    let [lo, hi] = paths[0].bounds().unwrap();
    assert!(lo[1].abs() < 1e-9 && (hi[1] - 1.0).abs() < 1e-3);
}

#[test]
fn near_zero_bulge_is_straight() {
    let doc = CadDocument::new();
    let mut pl = LwPolyline::new();
    pl.add_point_with_bulge(Vector2::new(0.0, 0.0), 1e-15);
    pl.add_point_with_bulge(Vector2::new(2.0, 0.0), 0.0);
    let paths = flat(&doc, EntityType::LwPolyline(pl), 1e-4);
    assert_points(&paths[0].points, &[[0.0, 0.0], [2.0, 0.0]]);
}

#[test]
fn block_base_point_is_subtracted() {
    let mut doc = CadDocument::new();
    add_block_at(
        &mut doc,
        "D",
        Vector3::new(10.0, 0.0, 0.0),
        vec![line(10.0, 0.0, 11.0, 0.0)],
    );
    let insert = Insert::new("D", Vector3::new(100.0, 0.0, 0.0)).with_scale(2.0, 2.0, 2.0);
    let paths = flat(&doc, EntityType::Insert(insert), 1e-3);
    assert_eq!(paths.len(), 1);
    assert_points(&paths[0].points, &[[100.0, 0.0], [102.0, 0.0]]);
}

#[test]
fn rotated_minsert_offsets_rotate_with_insert() {
    let mut doc = CadDocument::new();
    add_block(&mut doc, "D", vec![line(0.0, 0.0, 1.0, 0.0)]);
    let mut insert = Insert::new("D", Vector3::new(100.0, 0.0, 0.0));
    insert.rotation = FRAC_PI_2;
    insert.column_count = 2;
    insert.row_count = 2;
    insert.column_spacing = 5.0;
    insert.row_spacing = 7.0;
    let paths = flat(&doc, EntityType::Insert(insert), 1e-3);
    // Columns step along the rotated x axis (+y), rows along the rotated y axis (-x).
    let expected = [
        [[100.0, 0.0], [100.0, 1.0]],
        [[93.0, 0.0], [93.0, 1.0]],
        [[100.0, 5.0], [100.0, 6.0]],
        [[93.0, 5.0], [93.0, 6.0]],
    ];
    assert_eq!(paths.len(), 4);
    for (path, want) in paths.iter().zip(expected) {
        assert_points(&path.points, &want);
    }
}

#[test]
fn clockwise_hatch_arc_edge() {
    let doc = CadDocument::new();
    let mut boundary = BoundaryPath::new();
    boundary.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
        center: Vector2::new(0.0, 0.0),
        radius: 1.0,
        start_angle: 0.0,
        end_angle: PI,
        counter_clockwise: false,
    }));
    boundary.add_edge(BoundaryEdge::Line(LineEdge {
        start: Vector2::new(-1.0, 0.0),
        end: Vector2::new(1.0, 0.0),
    }));
    let mut hatch = Hatch::new();
    hatch.paths.push(boundary);
    let paths = flat(&doc, EntityType::Hatch(hatch), 1e-4);
    assert_eq!(paths.len(), 1);
    assert!(paths[0].closed);
    assert!((paths[0].area() - FRAC_PI_2).abs() < 1e-3);
    // Clockwise from angle 0 to PI runs through the lower half.
    let [lo, hi] = paths[0].bounds().unwrap();
    assert!((lo[1] + 1.0).abs() < 1e-3 && hi[1].abs() < 1e-9);
}

#[test]
fn duplicate_vertex_is_not_a_self_intersection() {
    let rect = Path {
        points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
        closed: true,
    };
    assert!(!rect.self_intersects());
    assert!((rect.area() - 12.0).abs() < 1e-9);
}

#[test]
fn self_referencing_block_stops_at_depth_limit() {
    let mut doc = CadDocument::new();
    let inner = Insert::new("X", Vector3::new(1.0, 0.0, 0.0));
    add_block(
        &mut doc,
        "X",
        vec![line(0.0, 0.0, 1.0, 0.0), EntityType::Insert(inner)],
    );
    let reference = performance::reference();
    let start = Instant::now();
    let outcome = flatten(
        &doc,
        &EntityType::Insert(Insert::new("X", Vector3::new(0.0, 0.0, 0.0))),
        &Xform::identity(),
        1e-3,
    );
    // Inserts at depths 0 to 31 expand, each adding one line; the rest is reported cut.
    assert_eq!(outcome.paths.len(), 32);
    assert!(outcome.truncated);
    performance::assert_relative(start.elapsed(), reference, 2.5, "recursive flatten limit");
}

#[test]
fn self_referencing_minsert_stops_at_work_budget() {
    let mut doc = CadDocument::new();
    let mut inner = Insert::new("X", Vector3::new(1.0, 0.0, 0.0));
    inner.column_count = 2;
    inner.row_count = 2;
    add_block(
        &mut doc,
        "X",
        vec![line(0.0, 0.0, 1.0, 0.0), EntityType::Insert(inner)],
    );
    // Without a budget this would expand 4^32 instances.
    let outcome = flatten(
        &doc,
        &EntityType::Insert(Insert::new("X", Vector3::new(0.0, 0.0, 0.0))),
        &Xform::identity(),
        1e-3,
    );
    assert!(!outcome.paths.is_empty() && outcome.paths.len() <= 1_000_000);
    assert!(outcome.truncated);
}

/// A line with the given layer and colour.
fn styled_line(layer: &str, color: Color) -> EntityType {
    let mut e = line(0.0, 0.0, 1.0, 0.0);
    e.common_mut().layer = layer.to_string();
    e.common_mut().color = color;
    e
}

#[test]
fn nested_insert_yields_chain_inner_layer_and_colours() {
    let mut doc = CadDocument::new();
    for (name, index) in [("A-DOOR", 3), ("A-FRAME", 5)] {
        let mut layer = Layer::new(name);
        layer.color = Color::Index(index);
        doc.layers.add(layer).unwrap();
    }
    // LEAF: a line on its own layer (ByLayer), one on layer 0 (ByLayer) and one ByBlock.
    add_block(
        &mut doc,
        "LEAF",
        vec![
            styled_line("A-FRAME", Color::ByLayer),
            styled_line("0", Color::ByLayer),
            styled_line("0", Color::ByBlock),
        ],
    );
    let mut leaf = EntityType::Insert(Insert::new("LEAF", Vector3::new(0.0, 0.0, 0.0)));
    leaf.common_mut().layer = "0".to_string();
    leaf.common_mut().color = Color::ByBlock;
    add_block(&mut doc, "DOOR", vec![leaf]);
    let leaf_handle = doc
        .entities_in_block("DOOR")
        .next()
        .unwrap()
        .common()
        .handle
        .value();
    let inner: Vec<u64> = doc
        .entities_in_block("LEAF")
        .map(|e| e.common().handle.value())
        .collect();

    let mut door = EntityType::Insert(Insert::new("DOOR", Vector3::new(0.0, 0.0, 0.0)));
    door.common_mut().layer = "A-DOOR".to_string();
    door.common_mut().color = Color::Index(1);
    let door_handle = doc.add_entity(door).unwrap().value();
    let door = doc
        .get_entity(opencadcodec::Handle::new(door_handle))
        .unwrap();

    let outcome = flatten(&doc, door, &Xform::identity(), 1e-3);
    assert!(!outcome.truncated);
    let paths = outcome.paths;
    assert_eq!(paths.len(), 3);
    for (p, &entity) in paths.iter().zip(&inner) {
        assert_eq!(p.chain, vec![door_handle, leaf_handle]);
        assert_eq!(p.entity, entity);
        assert_eq!(p.xref, None);
    }
    // Its own layer and that layer's colour.
    assert_eq!(
        (paths[0].layer.as_str(), paths[0].color),
        ("A-FRAME", Color::Index(5))
    );
    // Layer 0 takes the insert's layer (through LEAF, also on 0) and its colour.
    assert_eq!(
        (paths[1].layer.as_str(), paths[1].color),
        ("A-DOOR", Color::Index(3))
    );
    // ByBlock takes the colour of the insert (LEAF is ByBlock too, so DOOR's red).
    assert_eq!(
        (paths[2].layer.as_str(), paths[2].color),
        ("A-DOOR", Color::Index(1))
    );
}

#[test]
fn top_level_entity_has_no_chain() {
    let doc = CadDocument::new();
    let e = line(0.0, 0.0, 1.0, 0.0);
    let outcome = flatten(&doc, &e, &Xform::identity(), 1e-3);
    assert_eq!(outcome.paths.len(), 1);
    assert!(outcome.paths[0].chain.is_empty());
    assert_eq!(outcome.paths[0].layer, "0");
}

#[test]
fn mirrored_normal_flips_the_block_and_its_arcs() {
    let mut doc = CadDocument::new();
    let mut arc = Arc::from_center_radius_angles(Vector3::new(1.0, 0.0, 0.0), 1.0, 0.0, FRAC_PI_2);
    arc.normal = Vector3::new(0.0, 0.0, 1.0);
    add_block(
        &mut doc,
        "D",
        vec![line(0.0, 0.0, 1.0, 0.0), EntityType::Arc(arc)],
    );
    // Mirrored in X: the object x axis points along world -x.
    let insert =
        Insert::new("D", Vector3::new(5.0, 0.0, 0.0)).with_normal(Vector3::new(0.0, 0.0, -1.0));
    let paths = flat(&doc, EntityType::Insert(insert), 1e-4);
    assert_eq!(paths.len(), 2);
    assert_points(&paths[0].points, &[[-5.0, 0.0], [-6.0, 0.0]]);
    let arc = &paths[1];
    assert_points(
        &[arc.points[0], *arc.points.last().unwrap()],
        &[[-7.0, 0.0], [-6.0, 1.0]],
    );
    // Still a quarter circle of radius 1.
    assert!((arc.length() - FRAC_PI_2).abs() < 1e-3);

    // An arc mirrored on its own lands the same way.
    let mut own = Arc::from_center_radius_angles(Vector3::new(1.0, 0.0, 0.0), 1.0, 0.0, FRAC_PI_2);
    own.normal = Vector3::new(0.0, 0.0, -1.0);
    let paths = flat(&doc, EntityType::Arc(own), 1e-4);
    let p = &paths[0].points;
    assert_points(&[p[0], *p.last().unwrap()], &[[-2.0, 0.0], [-1.0, 1.0]]);
}

#[test]
fn exact_circle_length_and_area() {
    let circle = EntityType::Circle(Circle::from_center_radius(Vector3::new(3.0, 4.0, 0.0), 2.5));
    let id = Xform::identity();
    assert!((exact_area(&circle, &id).unwrap() - PI * 2.5 * 2.5).abs() < 1e-9);
    assert!((exact_length(&circle, &id).unwrap() - 2.0 * PI * 2.5).abs() < 1e-9);
    // A uniform scale (here with a mirror and a turn) scales both; a stretch has no exact value.
    let xf = Xform::scale(-2.0, 2.0).then(&Xform::rotate(0.3));
    assert!((exact_area(&circle, &xf).unwrap() - 4.0 * PI * 6.25).abs() < 1e-9);
    assert!((exact_length(&circle, &xf).unwrap() - 2.0 * TAU * 2.5).abs() < 1e-9);
    assert_eq!(exact_area(&circle, &Xform::scale(1.0, 2.0)), None);
}

#[test]
fn exact_bulged_polyline_and_arc() {
    let id = Xform::identity();
    // A 2x2 square whose top edge bulges out into a half circle of radius 1.
    let shape = closed_lwpolyline(&[
        (0.0, 0.0, 0.0),
        (2.0, 0.0, 0.0),
        (2.0, 2.0, 1.0),
        (0.0, 2.0, 0.0),
    ]);
    assert!((exact_area(&shape, &id).unwrap() - (4.0 + FRAC_PI_2)).abs() < 1e-9);
    assert!((exact_length(&shape, &id).unwrap() - (6.0 + PI)).abs() < 1e-9);
    // The same edge bulging in takes the half circle away.
    let notch = closed_lwpolyline(&[
        (0.0, 0.0, 0.0),
        (2.0, 0.0, 0.0),
        (2.0, 2.0, -1.0),
        (0.0, 2.0, 0.0),
    ]);
    assert!((exact_area(&notch, &id).unwrap() - (4.0 - FRAC_PI_2)).abs() < 1e-9);

    let arc = EntityType::Arc(Arc::from_center_radius_angles(
        Vector3::new(0.0, 0.0, 0.0),
        2.0,
        0.0,
        PI,
    ));
    assert!((exact_length(&arc, &id).unwrap() - 2.0 * PI).abs() < 1e-9);
    assert_eq!(exact_area(&arc, &id), None);
}

#[test]
fn exact_two_vertex_bulged_circle() {
    let id = Xform::identity();
    // Two half-circle bulges: the usual way a polyline draws a circle of radius 1.
    let circle = closed_lwpolyline(&[(0.0, 0.0, 1.0), (2.0, 0.0, 1.0)]);
    assert!((exact_length(&circle, &id).unwrap() - TAU).abs() < 1e-9);
    assert!((exact_area(&circle, &id).unwrap() - PI).abs() < 1e-9);
    // A half circle closed by its chord.
    let half = closed_lwpolyline(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)]);
    assert!((exact_length(&half, &id).unwrap() - (PI + 2.0)).abs() < 1e-9);
    // With no bulge, the flattened path is one segment: no exact value.
    let straight = closed_lwpolyline(&[(0.0, 0.0, 0.0), (2.0, 0.0, 0.0)]);
    assert_eq!(exact_length(&straight, &id), None);
    // The flattened path agrees with the exact values.
    let doc = CadDocument::new();
    let paths = flat(&doc, circle, 1e-5);
    assert!((paths[0].length() - TAU).abs() < 1e-3);
}

#[test]
fn exact_full_ellipse() {
    let ellipse = Ellipse::from_center_axes(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(2.0, 0.0, 0.0),
        0.5,
    );
    let e = EntityType::Ellipse(ellipse);
    let id = Xform::identity();
    assert!((exact_area(&e, &id).unwrap() - PI * 2.0).abs() < 1e-9);
    // Ramanujan for a = 2, b = 1: 9.688448220...
    assert!((exact_length(&e, &id).unwrap() - 9.688_448_220_5).abs() < 1e-6);
}

#[path = "../../../tests/support/performance.rs"]
mod performance;
