//! Modify commands: move, copy, rotate, scale, mirror, stretch, align, arrays and explode on the
//! entities at the top of the sheet, each one undo step; and the window and crossing selection.

use std::f64::consts::FRAC_PI_2;

use opencadcodec::entities::{Insert, TextHorizontalAlignment, TextVerticalAlignment};
use opencadcodec::{
    BlockRecord, CadDocument, Circle, Color, EntityType, Handle, Layer, Line, LineWeight,
    LwPolyline, Text, Vector2, Vector3,
};
use qiyas_core::fixtures::{write_drawing, write_sample, write_xref_pair};
use qiyas_core::{Command, CoreError, ModifySpec, Outcome, Picked, Session, Target};

const MODEL: &str = "*Model_Space";

/// What `modify_doc` holds, by handle.
struct Ids {
    /// A line from (1, 0) to (2, 0) on A-WALL.
    line: u64,
    /// A closed rectangle (10, 0) (14, 0) (14, 3) (10, 3) on A-WALL.
    rect: u64,
    /// A reference to block B at (20, 0), scale 2, on A-DOOR, colour 5.
    insert: u64,
    /// "AB" at (30, 0), 1 high, left on its baseline.
    text: u64,
    /// A circle at (40, 0), radius 1.
    circle: u64,
    /// A reference to block LEAF at (50, 0) with its normal along -Z.
    flipped: u64,
    /// The line inside block B (on layer 0, ByBlock).
    inside: u64,
}

/// Adds a block named `name` holding `entities`.
fn add_block(doc: &mut CadDocument, name: &str, entities: Vec<EntityType>) -> Vec<u64> {
    let mut record = BlockRecord::new(name);
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    entities
        .into_iter()
        .map(|mut e| {
            e.common_mut().owner_handle = owner;
            doc.add_entity(e).unwrap().value()
        })
        .collect()
}

fn on(layer: &str, mut e: EntityType) -> EntityType {
    e.common_mut().layer = layer.to_string();
    e
}

/// A drawing in metres with layers A-WALL, A-DOOR and OWN; blocks B (a ByBlock line on layer 0
/// from (0, 0) to (1, 0), a red circle on OWN at (0, 1) and a LEAF reference at (2, 0)) and LEAF
/// (a line from (0, 0) to (0, 1)); and the entities `Ids` lists.
fn modify_doc() -> (CadDocument, Ids) {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    for (name, index) in [("A-WALL", 1), ("A-DOOR", 3), ("OWN", 4)] {
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.color = Color::Index(index);
        doc.layers.add(layer).unwrap();
    }
    add_block(
        &mut doc,
        "LEAF",
        vec![EntityType::Line(Line::from_coords(
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ))],
    );
    let mut by_block = Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    by_block.common.color = Color::ByBlock;
    by_block.common.line_weight = LineWeight::ByBlock;
    by_block.common.linetype = "ByBlock".to_string();
    let mut red = Circle::from_center_radius(Vector3::new(0.0, 1.0, 0.0), 0.5);
    red.common.layer = "OWN".to_string();
    red.common.color = Color::Index(1);
    let nested = Insert::new("LEAF", Vector3::new(2.0, 0.0, 0.0));
    let inside = add_block(
        &mut doc,
        "B",
        vec![
            EntityType::Line(by_block),
            EntityType::Circle(red),
            EntityType::Insert(nested),
        ],
    )[0];

    let mut add = |e: EntityType| doc.add_entity(e).unwrap().value();
    let line = add(on(
        "A-WALL",
        EntityType::Line(Line::from_coords(1.0, 0.0, 0.0, 2.0, 0.0, 0.0)),
    ));
    let mut rectangle = LwPolyline::new();
    for (x, y) in [(10.0, 0.0), (14.0, 0.0), (14.0, 3.0), (10.0, 3.0)] {
        rectangle.add_point(Vector2::new(x, y));
    }
    rectangle.is_closed = true;
    let rect = add(on("A-WALL", EntityType::LwPolyline(rectangle)));
    let mut b = Insert::new("B", Vector3::new(20.0, 0.0, 0.0)).with_scale(2.0, 2.0, 2.0);
    b.common.color = Color::Index(5);
    let insert = add(on("A-DOOR", EntityType::Insert(b)));
    let text = add(EntityType::Text(
        Text::with_value("AB", Vector3::new(30.0, 0.0, 0.0)).with_height(1.0),
    ));
    let circle = add(EntityType::Circle(Circle::from_center_radius(
        Vector3::new(40.0, 0.0, 0.0),
        1.0,
    )));
    // Normal -Z: the insertion point is in object coordinates, x mirrored: (-50, 0) is (50, 0).
    let flipped = add(EntityType::Insert(
        Insert::new("LEAF", Vector3::new(-50.0, 0.0, 0.0))
            .with_normal(Vector3::new(0.0, 0.0, -1.0)),
    ));
    let ids = Ids {
        line,
        rect,
        insert,
        text,
        circle,
        flipped,
        inside,
    };
    (doc, ids)
}

fn modify_session() -> (tempfile::TempDir, Session, Ids) {
    let dir = tempfile::tempdir().unwrap();
    let (doc, ids) = modify_doc();
    let path = write_drawing(&doc, &dir.path().join("modify.dxf"));
    (dir, Session::open(&path).unwrap(), ids)
}

fn targets(handles: &[u64]) -> Vec<Target> {
    handles
        .iter()
        .map(|&handle| Target {
            handle,
            chain: Vec::new(),
        })
        .collect()
}

fn run(s: &mut Session, spec: ModifySpec) -> Outcome {
    s.run(Command::Modify(spec)).unwrap()
}

/// Runs a modify that adds entities and returns their handles.
fn added(s: &mut Session, spec: ModifySpec) -> Vec<u64> {
    match run(s, spec) {
        Outcome::Added { handles } | Outcome::Exploded { handles, .. } => handles,
        other => panic!("expected new entities, got {other:?}"),
    }
}

/// Runs a modify that must be refused, checks nothing changed, and returns why.
fn refused(s: &mut Session, spec: ModifySpec) -> String {
    let before: Vec<EntityType> = s.drawing.doc.entities_in_block(MODEL).cloned().collect();
    let undo = s.can_undo();
    let err = s.run(Command::Modify(spec)).unwrap_err();
    let after: Vec<EntityType> = s.drawing.doc.entities_in_block(MODEL).cloned().collect();
    assert_eq!(before, after, "a refused modify changes nothing");
    assert_eq!(s.can_undo(), undo, "a refused modify is no step");
    match err {
        CoreError::InvalidCommand(m) => m,
        other => panic!("{other:?}"),
    }
}

fn entity(s: &Session, h: u64) -> EntityType {
    s.drawing.doc.get_entity(Handle::new(h)).cloned().unwrap()
}

fn ends(s: &Session, h: u64) -> [[f64; 2]; 2] {
    match entity(s, h) {
        EntityType::Line(l) => [[l.start.x, l.start.y], [l.end.x, l.end.y]],
        other => panic!("a line, got {other:?}"),
    }
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

fn assert_ends(s: &Session, h: u64, want: [[f64; 2]; 2]) {
    let got = ends(s, h);
    assert!(
        near(got[0], want[0]) && near(got[1], want[1]),
        "{got:?} != {want:?}"
    );
}

/// The drawn segments' end points of everything under the top-level entity `top`, absolute.
fn drawn(s: &Session, top: u64) -> Vec<[f64; 2]> {
    let batch = s.batch();
    let [ox, oy] = batch.origin;
    let mut points = Vec::new();
    for r in batch.ranges.iter().filter(|r| r.top == top) {
        for i in r.first..r.first + r.count {
            let seg = &batch.segments[4 * i as usize..4 * i as usize + 4];
            points.push([f64::from(seg[0]) + ox, f64::from(seg[1]) + oy]);
            points.push([f64::from(seg[2]) + ox, f64::from(seg[3]) + oy]);
        }
    }
    points
}

/// Every point of `a` is within 1e-4 of one of `b` and the other way round.
fn same_points(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    let close = |p: &[f64; 2], q: &[f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-4;
    !a.is_empty()
        && a.iter().all(|p| b.iter().any(|q| close(p, q)))
        && b.iter().all(|q| a.iter().any(|p| close(p, q)))
}

fn sheet_handles(s: &Session) -> Vec<u64> {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .map(|e| e.common().handle.value())
        .collect()
}

#[test]
fn move_keeps_the_handles_and_is_one_step() {
    let (_dir, mut s, ids) = modify_session();
    let v0 = s.batch_version();
    let out = run(
        &mut s,
        ModifySpec::Move {
            targets: targets(&[ids.line, ids.rect]),
            delta: [2.0, 3.0],
        },
    );
    assert!(matches!(out, Outcome::Changed), "{out:?}");
    assert_ends(&s, ids.line, [[3.0, 3.0], [4.0, 3.0]]);
    let EntityType::LwPolyline(rect) = entity(&s, ids.rect) else {
        panic!()
    };
    assert_eq!(
        [rect.vertices[0].location.x, rect.vertices[0].location.y],
        [12.0, 3.0]
    );
    assert_ne!(s.batch_version(), v0, "the sheet is drawn again");
    assert!(s.is_dirty());

    // One step: undo puts both back under the same handles; redo moves them again.
    assert!(s.undo());
    assert_ends(&s, ids.line, [[1.0, 0.0], [2.0, 0.0]]);
    assert!(!s.can_undo());
    assert!(s.redo());
    assert_ends(&s, ids.line, [[3.0, 3.0], [4.0, 3.0]]);
    assert!(sheet_handles(&s).contains(&ids.line));
}

#[test]
fn copy_three_times_gives_three_new_handles_a_step_apart() {
    let (_dir, mut s, ids) = modify_session();
    let new = added(
        &mut s,
        ModifySpec::Copy {
            targets: targets(&[ids.line]),
            delta: [0.0, 5.0],
            count: 3,
        },
    );
    assert_eq!(new.len(), 3);
    let mut distinct = new.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 3);
    assert!(!new.contains(&ids.line));
    assert_ends(&s, ids.line, [[1.0, 0.0], [2.0, 0.0]]);
    for (i, &h) in new.iter().enumerate() {
        let y = 5.0 * (i + 1) as f64;
        assert_ends(&s, h, [[1.0, y], [2.0, y]]);
        let EntityType::Line(copy) = entity(&s, h) else {
            panic!()
        };
        assert_eq!(copy.common.layer, "A-WALL");
        assert_eq!(
            copy.common.owner_handle,
            entity(&s, ids.line).common().owner_handle
        );
    }
    // Undo takes all three away; redo brings them back under the same handles.
    assert!(s.undo());
    assert!(new.iter().all(|h| !sheet_handles(&s).contains(h)));
    assert!(s.redo());
    assert!(new.iter().all(|h| sheet_handles(&s).contains(h)));
}

#[test]
fn a_copied_block_reference_gets_new_handles_for_its_attributes_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_schedule(dir.path(), "dxf");
    let mut s = Session::open(&path).unwrap();
    let tag = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(i) if !i.attributes.is_empty()))
        .map(|e| e.common().handle.value())
        .unwrap();
    let new = added(
        &mut s,
        ModifySpec::Copy {
            targets: targets(&[tag]),
            delta: [1.0, 0.0],
            count: 1,
        },
    );
    let (EntityType::Insert(original), EntityType::Insert(copy)) =
        (entity(&s, tag), entity(&s, new[0]))
    else {
        panic!()
    };
    let attribute = &copy.attributes[0];
    assert_eq!(attribute.value, "D9");
    assert_ne!(
        attribute.common.handle,
        original.attributes[0].common.handle
    );
    assert!(!attribute.common.handle.is_null());
    assert_eq!(attribute.common.owner_handle, copy.common.handle);
    assert!(
        (attribute.insertion_point.x - original.attributes[0].insertion_point.x - 1.0).abs() < 1e-9
    );
    // The copy saves and opens again.
    s.save_as(&dir.path().join("copied.dwg")).unwrap();
    let again = Session::open(&dir.path().join("copied.dwg")).unwrap();
    let tags = again
        .drawing
        .doc
        .entities_in_block(MODEL)
        .filter(|e| matches!(e, EntityType::Insert(i) if i.block_name == "TAG"))
        .count();
    assert_eq!(tags, 2);
}

#[test]
fn rotate_90_degrees_maps_1_0_to_0_1() {
    let (_dir, mut s, ids) = modify_session();
    let out = run(
        &mut s,
        ModifySpec::Rotate {
            targets: targets(&[ids.line]),
            base: [0.0, 0.0],
            angle: 90.0,
            copy: false,
        },
    );
    assert!(matches!(out, Outcome::Changed));
    assert_ends(&s, ids.line, [[0.0, 1.0], [0.0, 2.0]]);

    // With copy, the original stays and a turned copy is added.
    let new = added(
        &mut s,
        ModifySpec::Rotate {
            targets: targets(&[ids.line]),
            base: [0.0, 0.0],
            angle: 90.0,
            copy: true,
        },
    );
    assert_ends(&s, ids.line, [[0.0, 1.0], [0.0, 2.0]]);
    assert_ends(&s, new[0], [[-1.0, 0.0], [-2.0, 0.0]]);
}

#[test]
fn a_block_reference_with_a_flipped_normal_turns_where_it_is_drawn() {
    let (_dir, mut s, ids) = modify_session();
    let before = drawn(&s, ids.flipped);
    // Drawn at (50, 0) up to (50, 1).
    assert!(
        same_points(&before, &[[50.0, 0.0], [50.0, 1.0]]),
        "{before:?}"
    );
    run(
        &mut s,
        ModifySpec::Rotate {
            targets: targets(&[ids.flipped]),
            base: [0.0, 0.0],
            angle: 90.0,
            copy: false,
        },
    );
    let turned: Vec<[f64; 2]> = before.iter().map(|p| [-p[1], p[0]]).collect();
    assert!(
        same_points(&drawn(&s, ids.flipped), &turned),
        "{:?}",
        drawn(&s, ids.flipped)
    );
    run(
        &mut s,
        ModifySpec::Mirror {
            targets: targets(&[ids.flipped]),
            p1: [0.0, 0.0],
            p2: [1.0, 0.0],
            keep: false,
        },
    );
    let mirrored: Vec<[f64; 2]> = turned.iter().map(|p| [p[0], -p[1]]).collect();
    assert!(
        same_points(&drawn(&s, ids.flipped), &mirrored),
        "{:?}",
        drawn(&s, ids.flipped)
    );
}

#[test]
fn scale_about_a_base_point_and_as_a_copy() {
    let (_dir, mut s, ids) = modify_session();
    run(
        &mut s,
        ModifySpec::Scale {
            targets: targets(&[ids.line, ids.circle]),
            base: [0.0, 0.0],
            factor: 2.0,
            copy: false,
        },
    );
    assert_ends(&s, ids.line, [[2.0, 0.0], [4.0, 0.0]]);
    let EntityType::Circle(c) = entity(&s, ids.circle) else {
        panic!()
    };
    assert!(near([c.center.x, c.center.y], [80.0, 0.0]) && (c.radius - 2.0).abs() < 1e-9);
    let new = added(
        &mut s,
        ModifySpec::Scale {
            targets: targets(&[ids.line]),
            base: [2.0, 0.0],
            factor: 0.5,
            copy: true,
        },
    );
    assert_ends(&s, new[0], [[2.0, 0.0], [3.0, 0.0]]);
    assert_ends(&s, ids.line, [[2.0, 0.0], [4.0, 0.0]]);
}

#[test]
fn mirror_keeps_the_original_when_asked() {
    let (_dir, mut s, ids) = modify_session();
    let new = added(
        &mut s,
        ModifySpec::Mirror {
            targets: targets(&[ids.line]),
            p1: [0.0, 0.0],
            p2: [0.0, 1.0],
            keep: true,
        },
    );
    assert_eq!(new.len(), 1);
    assert_ends(&s, ids.line, [[1.0, 0.0], [2.0, 0.0]]);
    assert_ends(&s, new[0], [[-1.0, 0.0], [-2.0, 0.0]]);

    // Without keep, the entity itself is mirrored under its own handle.
    let out = run(
        &mut s,
        ModifySpec::Mirror {
            targets: targets(&[ids.line]),
            p1: [0.0, 5.0],
            p2: [1.0, 5.0],
            keep: false,
        },
    );
    assert!(matches!(out, Outcome::Changed));
    assert_ends(&s, ids.line, [[1.0, 10.0], [2.0, 10.0]]);
}

#[test]
fn a_mirrored_text_still_reads_forwards() {
    let (_dir, mut s, ids) = modify_session();
    // Across a vertical line: the text goes to the other side and keeps reading left to right,
    // now ending where the mirror puts its start.
    run(
        &mut s,
        ModifySpec::Mirror {
            targets: targets(&[ids.text]),
            p1: [0.0, 0.0],
            p2: [0.0, 1.0],
            keep: false,
        },
    );
    let EntityType::Text(t) = entity(&s, ids.text) else {
        panic!()
    };
    assert!(
        t.rotation.sin().abs() < 1e-9 && t.rotation.cos() > 0.0,
        "{}",
        t.rotation
    );
    assert_eq!(t.horizontal_alignment, TextHorizontalAlignment::Right);
    let a = t.alignment_point.unwrap();
    assert!(near([a.x, a.y], [-30.0, 0.0]), "{a:?}");
    assert!(t.oblique_angle.abs() < 1e-12);

    // Across a horizontal line: still upright, hanging below the mirrored point.
    let (_dir2, mut s, ids) = modify_session();
    run(
        &mut s,
        ModifySpec::Mirror {
            targets: targets(&[ids.text]),
            p1: [0.0, -1.0],
            p2: [1.0, -1.0],
            keep: false,
        },
    );
    let EntityType::Text(t) = entity(&s, ids.text) else {
        panic!()
    };
    assert!(
        t.rotation.sin().abs() < 1e-9 && t.rotation.cos() > 0.0,
        "{}",
        t.rotation
    );
    assert_eq!(t.horizontal_alignment, TextHorizontalAlignment::Left);
    assert_eq!(t.vertical_alignment, TextVerticalAlignment::Top);
    let a = t.alignment_point.unwrap();
    assert!(near([a.x, a.y], [30.0, -2.0]), "{a:?}");
}

#[test]
fn stretch_moves_only_the_vertices_inside_the_window() {
    let (_dir, mut s, ids) = modify_session();
    // The rectangle's right side and the circle's centre are inside; the line is wholly outside.
    let out = run(
        &mut s,
        ModifySpec::Stretch {
            targets: targets(&[ids.rect, ids.line, ids.circle]),
            window: [[13.0, -1.0], [41.0, 4.0]],
            delta: [2.0, 0.0],
        },
    );
    assert!(matches!(out, Outcome::Changed));
    let EntityType::LwPolyline(rect) = entity(&s, ids.rect) else {
        panic!()
    };
    let at: Vec<[f64; 2]> = rect
        .vertices
        .iter()
        .map(|v| [v.location.x, v.location.y])
        .collect();
    assert_eq!(at, vec![[10.0, 0.0], [16.0, 0.0], [16.0, 3.0], [10.0, 3.0]]);
    assert_ends(&s, ids.line, [[1.0, 0.0], [2.0, 0.0]]);
    let EntityType::Circle(c) = entity(&s, ids.circle) else {
        panic!()
    };
    assert!(near([c.center.x, c.center.y], [42.0, 0.0]));

    // One end of the line inside: only that end moves.
    run(
        &mut s,
        ModifySpec::Stretch {
            targets: targets(&[ids.line]),
            window: [[1.5, -1.0], [3.0, 1.0]],
            delta: [0.0, 1.0],
        },
    );
    assert_ends(&s, ids.line, [[1.0, 0.0], [2.0, 1.0]]);
}

#[test]
fn align_by_one_or_two_pairs() {
    let (_dir, mut s, ids) = modify_session();
    run(
        &mut s,
        ModifySpec::Align {
            targets: targets(&[ids.line]),
            pairs: vec![[[1.0, 0.0], [5.0, 5.0]], [[2.0, 0.0], [5.0, 9.0]]],
        },
    );
    // Moved to (5, 5) and turned to point up; not scaled.
    assert_ends(&s, ids.line, [[5.0, 5.0], [5.0, 6.0]]);
    run(
        &mut s,
        ModifySpec::Align {
            targets: targets(&[ids.line]),
            pairs: vec![[[5.0, 5.0], [0.0, 0.0]]],
        },
    );
    assert_ends(&s, ids.line, [[0.0, 0.0], [0.0, 1.0]]);
}

#[test]
fn a_rectangular_array_copies_rows_by_columns() {
    let (_dir, mut s, ids) = modify_session();
    let new = added(
        &mut s,
        ModifySpec::ArrayRect {
            targets: targets(&[ids.line]),
            rows: 2,
            cols: 3,
            dy: 4.0,
            dx: 10.0,
        },
    );
    assert_eq!(new.len(), 5);
    let mut starts: Vec<[f64; 2]> = new.iter().map(|&h| ends(&s, h)[0]).collect();
    starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        starts,
        vec![
            [1.0, 4.0],
            [11.0, 0.0],
            [11.0, 4.0],
            [21.0, 0.0],
            [21.0, 4.0]
        ]
    );
    assert!(s.undo());
    assert!(new.iter().all(|h| !sheet_handles(&s).contains(h)));
}

#[test]
fn a_polar_array_of_4_at_90_degrees_places_4_references() {
    let (_dir, mut s, ids) = modify_session();
    let new = added(
        &mut s,
        ModifySpec::ArrayPolar {
            targets: targets(&[ids.insert]),
            centre: [0.0, 0.0],
            count: 4,
            angle: 90.0,
        },
    );
    assert_eq!(new.len(), 3);
    let mut placed: Vec<([f64; 2], f64)> = std::iter::once(ids.insert)
        .chain(new)
        .map(|h| match entity(&s, h) {
            EntityType::Insert(i) => {
                assert_eq!(i.block_name, "B");
                let turns = (i.rotation / FRAC_PI_2).round().rem_euclid(4.0);
                ([i.insert_point.x.round(), i.insert_point.y.round()], turns)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    placed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert_eq!(
        placed,
        vec![
            ([20.0, 0.0], 0.0),
            ([0.0, 20.0], 1.0),
            ([-20.0, 0.0], 2.0),
            ([0.0, -20.0], 3.0)
        ]
    );
    let references = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .filter(|e| matches!(e, EntityType::Insert(i) if i.block_name == "B"))
        .count();
    assert_eq!(references, 4);
}

#[test]
fn exploding_a_block_reference_puts_its_entities_on_the_sheet_by_block_resolved() {
    let (_dir, mut s, ids) = modify_session();
    let new = added(
        &mut s,
        ModifySpec::Explode {
            targets: targets(&[ids.insert]),
        },
    );
    assert!(!sheet_handles(&s).contains(&ids.insert));
    assert_eq!(
        new.len(),
        3,
        "the line, the circle and the nested reference"
    );
    let owner = entity(&s, ids.line).common().owner_handle;
    let parts: Vec<EntityType> = new.iter().map(|&h| entity(&s, h)).collect();
    for p in &parts {
        assert_eq!(p.common().owner_handle, owner);
        assert!(!new.contains(&ids.inside) && p.common().handle.value() != ids.inside);
    }
    // The ByBlock line on layer 0 takes the reference's layer, colour, linetype and weight, and
    // sits where the reference drew it (scale 2 at (20, 0)).
    let line = parts
        .iter()
        .find_map(|p| match p {
            EntityType::Line(l) => Some(l),
            _ => None,
        })
        .unwrap();
    assert_eq!(line.common.layer, "A-DOOR");
    assert_eq!(line.common.color, Color::Index(5));
    assert_eq!(line.common.line_weight, LineWeight::ByLayer);
    assert_eq!(line.common.linetype, "");
    assert!(near([line.start.x, line.start.y], [20.0, 0.0]));
    assert!(near([line.end.x, line.end.y], [22.0, 0.0]));
    // The red circle on OWN stays a circle, on OWN, red, scaled.
    let circle = parts
        .iter()
        .find_map(|p| match p {
            EntityType::Circle(c) => Some(c),
            _ => None,
        })
        .expect("a circle stays a circle under a uniform scale");
    assert_eq!(circle.common.layer, "OWN");
    assert_eq!(circle.common.color, Color::Index(1));
    assert!(
        near([circle.center.x, circle.center.y], [20.0, 2.0]) && (circle.radius - 1.0).abs() < 1e-9
    );
    // One level only: the nested reference stays a reference, placed by the outer one.
    let nested = parts
        .iter()
        .find_map(|p| match p {
            EntityType::Insert(i) => Some(i),
            _ => None,
        })
        .unwrap();
    assert_eq!(nested.block_name, "LEAF");
    assert!(near(
        [nested.insert_point.x, nested.insert_point.y],
        [24.0, 0.0]
    ));
    assert!((nested.x_scale() - 2.0).abs() < 1e-9);

    // Undo brings the reference back under its handle and takes the parts away.
    assert!(s.undo());
    assert!(sheet_handles(&s).contains(&ids.insert));
    assert!(new.iter().all(|h| !sheet_handles(&s).contains(h)));
}

#[test]
fn exploding_a_polyline_gives_its_lines_and_what_cannot_explode_is_left_alone() {
    let (_dir, mut s, ids) = modify_session();
    let new = added(
        &mut s,
        ModifySpec::Explode {
            targets: targets(&[ids.rect, ids.line]),
        },
    );
    assert_eq!(new.len(), 4);
    assert!(!sheet_handles(&s).contains(&ids.rect));
    assert!(
        sheet_handles(&s).contains(&ids.line),
        "a line cannot explode"
    );
    for &h in &new {
        assert_eq!(entity(&s, h).common().layer, "A-WALL");
    }
    let message = refused(
        &mut s,
        ModifySpec::Explode {
            targets: targets(&[ids.line]),
        },
    );
    assert!(message.contains("explode"), "{message}");
}

#[test]
fn entities_inside_blocks_or_xrefs_are_refused_and_nothing_changes() {
    let (_dir, mut s, ids) = modify_session();
    let message = refused(
        &mut s,
        ModifySpec::Move {
            targets: vec![
                Target {
                    handle: ids.line,
                    chain: Vec::new(),
                },
                Target {
                    handle: ids.inside,
                    chain: vec![ids.insert],
                },
            ],
            delta: [1.0, 0.0],
        },
    );
    assert!(message.contains("inside a block"), "{message}");
    assert!(refused(
        &mut s,
        ModifySpec::Move {
            targets: vec![],
            delta: [1.0, 0.0]
        }
    )
    .contains("Pick"));

    // An entity of an xref: read-only.
    let dir = tempfile::tempdir().unwrap();
    let mut host = Session::open(&write_xref_pair(dir.path())).unwrap();
    let reference = host
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .map(|e| e.common().handle.value())
        .unwrap();
    let inner = host.pick([100.0, 1.0], 0.5).unwrap();
    assert_eq!(inner.chain, vec![reference]);
    let message = refused(
        &mut host,
        ModifySpec::Rotate {
            targets: vec![Target {
                handle: inner.entity,
                chain: inner.chain,
            }],
            base: [0.0, 0.0],
            angle: 10.0,
            copy: false,
        },
    );
    assert!(message.contains("read-only"), "{message}");
    // The xref's reference itself can't be exploded.
    let message = refused(
        &mut host,
        ModifySpec::Explode {
            targets: targets(&[reference]),
        },
    );
    assert!(message.contains("xref"), "{message}");
}

#[test]
fn bad_numbers_and_unsupported_entities_are_refused() {
    let (_dir, mut s, ids) = modify_session();
    let line = targets(&[ids.line]);
    for spec in [
        ModifySpec::Scale {
            targets: line.clone(),
            base: [0.0, 0.0],
            factor: 0.0,
            copy: false,
        },
        ModifySpec::Mirror {
            targets: line.clone(),
            p1: [1.0, 1.0],
            p2: [1.0, 1.0],
            keep: true,
        },
        ModifySpec::Copy {
            targets: line.clone(),
            delta: [1.0, 0.0],
            count: 0,
        },
        ModifySpec::ArrayRect {
            targets: line.clone(),
            rows: 1,
            cols: 1,
            dy: 1.0,
            dx: 1.0,
        },
        ModifySpec::ArrayPolar {
            targets: line.clone(),
            centre: [0.0, 0.0],
            count: 1,
            angle: 90.0,
        },
        ModifySpec::Move {
            targets: line.clone(),
            delta: [f64::NAN, 0.0],
        },
        ModifySpec::Align {
            targets: line.clone(),
            pairs: vec![],
        },
        ModifySpec::Stretch {
            targets: line.clone(),
            window: [[2.0, 2.0], [1.0, 1.0]],
            delta: [1.0, 0.0],
        },
    ] {
        refused(&mut s, spec);
    }
    // Far too many copies.
    let message = refused(
        &mut s,
        ModifySpec::ArrayRect {
            targets: line,
            rows: 1000,
            cols: 1000,
            dy: 1.0,
            dx: 1.0,
        },
    );
    assert!(message.contains("copies"), "{message}");

    // A dimension draws from a block of its own that isn't made again yet: it only moves or
    // copies; turning it is refused.
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&qiyas_core::fixtures::write_schedule(dir.path(), "dxf")).unwrap();
    let dimension = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Dimension(_)))
        .map(|e| e.common().handle.value())
        .unwrap();
    let message = refused(
        &mut s,
        ModifySpec::Rotate {
            targets: targets(&[dimension]),
            base: [0.0, 0.0],
            angle: 10.0,
            copy: false,
        },
    );
    assert!(message.contains("Dimension"), "{message}");
}

#[test]
fn modify_commands_read_from_json() {
    let cmd: Command = serde_json::from_str(
        r#"{"type":"modify","kind":"move","targets":[{"handle":42}],"delta":[1,2]}"#,
    )
    .unwrap();
    let Command::Modify(ModifySpec::Move { targets, delta }) = cmd else {
        panic!("{cmd:?}")
    };
    assert_eq!(targets[0].handle, 42);
    assert_eq!(delta, [1.0, 2.0]);
    let cmd: Command = serde_json::from_str(
        r#"{"type":"modify","kind":"array_polar","targets":[],"centre":[0,0],"count":4,"angle":90}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        Command::Modify(ModifySpec::ArrayPolar { count: 4, .. })
    ));
}

#[test]
fn moved_and_copied_entities_save_and_open_again() {
    let (dir, mut s, ids) = modify_session();
    run(
        &mut s,
        ModifySpec::Rotate {
            targets: targets(&[ids.insert, ids.text]),
            base: [0.0, 0.0],
            angle: 30.0,
            copy: false,
        },
    );
    let copies = added(
        &mut s,
        ModifySpec::Copy {
            targets: targets(&[ids.rect, ids.insert]),
            delta: [0.0, 50.0],
            count: 2,
        },
    );
    for ext in ["dxf", "dwg"] {
        let path = dir.path().join(format!("saved.{ext}"));
        s.save_as(&path).unwrap();
        let again = Session::open(&path).unwrap();
        for &h in copies.iter().chain([&ids.insert, &ids.text]) {
            assert!(
                same_points(&drawn(&again, h), &drawn(&s, h)),
                "{ext}: {h:X} is drawn where it was"
            );
        }
    }
}

/// The handles a window or crossing picks, sorted.
fn window(s: &Session, lo: [f64; 2], hi: [f64; 2], crossing: bool) -> Vec<u64> {
    let picked = s.pick_window([lo, hi], crossing);
    for p in &picked {
        assert_eq!(p.top, p.entity);
        assert!(p.chain.is_empty() && p.xref.is_none());
    }
    picked.into_iter().map(|p: Picked| p.top).collect()
}

#[test]
fn a_window_takes_what_is_wholly_inside_a_crossing_what_it_touches() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let find = |s: &Session, want: fn(&EntityType) -> bool| -> Vec<u64> {
        s.drawing
            .doc
            .entities_in_block(MODEL)
            .filter(|e| want(e))
            .map(|e| e.common().handle.value())
            .collect()
    };
    let line = find(&s, |e| matches!(e, EntityType::Line(_)))[0];
    let rect = find(&s, |e| matches!(e, EntityType::LwPolyline(_)))[0];
    let doors = find(&s, |e| matches!(e, EntityType::Insert(_)));

    // The rectangle and the first door (its leaf from (1, 1) to (1.9, 1)) lie wholly inside; the
    // 10 m line only crosses.
    let mut wholly = vec![rect, doors[0]];
    wholly.sort_unstable();
    assert_eq!(window(&s, [-1.0, -1.0], [4.5, 3.5], false), wholly);
    let mut touched = vec![rect, doors[0], line];
    touched.sort_unstable();
    assert_eq!(window(&s, [-1.0, -1.0], [4.5, 3.5], true), touched);

    // A box over the middle of the line: crossing only.
    assert_eq!(window(&s, [5.5, -0.5], [6.0, 0.5], true), vec![line]);
    assert!(window(&s, [5.5, -0.5], [6.0, 0.5], false).is_empty());

    // A box inside a diagonal line's bounding box but off the line itself takes nothing.
    let diagonal = match s
        .run(Command::Draw(qiyas_core::DrawSpec::Line {
            a: [0.0, 10.0],
            b: [10.0, 20.0],
        }))
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    assert!(window(&s, [8.0, 10.0], [10.0, 12.0], true).is_empty());
    assert_eq!(window(&s, [4.0, 14.0], [6.0, 16.0], true), vec![diagonal]);

    // Hidden layers are not picked.
    s.set_layer_hidden("A-WALL", true);
    assert_eq!(window(&s, [-1.0, -1.0], [4.5, 3.5], true), vec![doors[0]]);
}

#[test]
fn a_window_over_an_xref_takes_its_reference_never_what_is_inside() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&write_xref_pair(dir.path())).unwrap();
    let reference = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .map(|e| e.common().handle.value())
        .unwrap();
    // The xref's content is drawn around (100, 0), turned a quarter turn.
    assert_eq!(window(&s, [99.0, 0.5], [101.0, 1.5], true), vec![reference]);
    assert_eq!(
        window(&s, [-1000.0, -1000.0], [1000.0, 1000.0], false).len(),
        2
    );
}

#[test]
fn mirrored_attributes_and_multiline_texts_still_read_forwards() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&qiyas_core::fixtures::write_schedule(dir.path(), "dxf")).unwrap();
    let find = |s: &Session, want: fn(&EntityType) -> bool| {
        s.drawing
            .doc
            .entities_in_block(MODEL)
            .find(|e| want(e))
            .map(|e| e.common().handle.value())
            .unwrap()
    };
    let tag = find(
        &s,
        |e| matches!(e, EntityType::Insert(i) if !i.attributes.is_empty()),
    );
    let note = find(&s, |e| matches!(e, EntityType::MText(_)));
    run(
        &mut s,
        ModifySpec::Mirror {
            targets: targets(&[tag, note]),
            p1: [0.0, 0.0],
            p2: [0.0, 1.0],
            keep: false,
        },
    );
    let EntityType::Insert(insert) = entity(&s, tag) else {
        panic!()
    };
    let a = &insert.attributes[0];
    assert!(
        a.rotation.sin().abs() < 1e-9 && a.rotation.cos() > 0.0,
        "{}",
        a.rotation
    );
    assert!(a.insertion_point.x < 0.0);
    let EntityType::MText(m) = entity(&s, note) else {
        panic!()
    };
    assert!(
        m.rotation.sin().abs() < 1e-9 && m.rotation.cos() > 0.0,
        "{}",
        m.rotation
    );
    assert_eq!(m.attachment_point as u8, 3, "top left becomes top right");
}

// ---- Fix round 1 ----

use opencadcodec::entities::{
    Dimension, DimensionLinear, PolyfaceFace, PolyfaceMesh, Polyline3D, Table, TableColumn,
    TableRow, Vertex3DPolyline,
};
use opencadcodec::Arc as CadArc;
use qiyas_core::{Kind, Measurement};

/// Adds an anonymous block `name` holding `entities`; returns the record's handle.
fn add_anonymous(doc: &mut CadDocument, name: &str, entities: Vec<EntityType>) -> Handle {
    let mut record = BlockRecord::new(name);
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    record.flags.anonymous = true;
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    for mut e in entities {
        e.common_mut().owner_handle = owner;
        doc.add_entity(e).unwrap();
    }
    owner
}

fn open_doc(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    let s = Session::open(&path).unwrap();
    (dir, s)
}

/// Every drawn point of the entities `tops`, together.
fn drawn_all(s: &Session, tops: &[u64]) -> Vec<[f64; 2]> {
    tops.iter().flat_map(|&h| drawn(s, h)).collect()
}

#[test]
fn explode_places_the_parts_where_the_reference_drew_them_base_point_and_array_included() {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    let mut record = BlockRecord::new("BP");
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    record.base_point = Vector3::new(5.0, 5.0, 0.0);
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    for mut e in [
        EntityType::Line(Line::from_coords(5.0, 5.0, 0.0, 6.0, 5.0, 0.0)),
        EntityType::Arc(CadArc::from_center_radius_angles(
            Vector3::new(5.0, 6.0, 0.0),
            0.5,
            0.0,
            PI_HALF,
        )),
    ] {
        e.common_mut().owner_handle = owner;
        doc.add_entity(e).unwrap();
    }
    let single = doc
        .add_entity(EntityType::Insert(
            Insert::new("BP", Vector3::new(20.0, 20.0, 0.0))
                .with_rotation(30f64.to_radians())
                .with_scale(2.0, 2.0, 2.0),
        ))
        .unwrap()
        .value();
    let mut array = Insert::new("BP", Vector3::new(-20.0, 0.0, 0.0)).with_rotation(FRAC_PI_2);
    array.column_count = 2;
    array.column_spacing = 3.0;
    let array = doc.add_entity(EntityType::Insert(array)).unwrap().value();
    let (_dir, mut s) = open_doc(&doc, "base.dxf");
    for reference in [single, array] {
        let before = drawn(&s, reference);
        let parts = added(
            &mut s,
            ModifySpec::Explode {
                targets: targets(&[reference]),
            },
        );
        let after = drawn_all(&s, &parts);
        assert!(
            same_points(&before, &after),
            "{reference:X}: {before:?} != {after:?}"
        );
    }
}

const PI_HALF: f64 = FRAC_PI_2;

#[test]
fn copies_of_3d_polylines_and_polyface_meshes_save_without_shared_handles() {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    let mut pl = Polyline3D::new();
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)] {
        pl.vertices
            .push(Vertex3DPolyline::new(Vector3::new(x, y, 0.0)));
    }
    doc.add_entity(EntityType::Polyline3D(pl)).unwrap();
    let mut mesh = PolyfaceMesh::new();
    for (x, y) in [(5.0, 0.0), (6.0, 0.0), (6.0, 1.0)] {
        mesh.add_vertex_xyz(x, y, 0.0);
    }
    mesh.add_face(PolyfaceFace::triangle(1, 2, 3));
    doc.add_entity(EntityType::PolyfaceMesh(mesh)).unwrap();
    // Read back from DWG, so the vertices carry handles of their own.
    let (dir, mut s) = open_doc(&doc, "meshes.dwg");
    let originals = sheet_handles(&s);
    let copies = added(
        &mut s,
        ModifySpec::Copy {
            targets: targets(&originals),
            delta: [0.0, 10.0],
            count: 1,
        },
    );
    assert_eq!(copies.len(), 2);
    let path = dir.path().join("copied.dwg");
    s.save_as(&path).unwrap();
    let again = Session::open(&path).unwrap();
    let mut all = Vec::new();
    for e in again.drawing.doc.entities_in_block(MODEL) {
        all.push(e.common().handle.value());
        match e {
            EntityType::Polyline3D(p) => all.extend(p.vertices.iter().map(|v| v.handle.value())),
            EntityType::PolyfaceMesh(m) => {
                all.extend(m.vertices.iter().map(|v| v.common.handle.value()));
                all.extend(m.faces.iter().map(|f| f.common.handle.value()));
                all.extend(m.seqend_handle.map(|h| h.value()));
            }
            _ => {}
        }
    }
    let real: Vec<u64> = all.iter().copied().filter(|&h| h != 0).collect();
    let mut unique = real.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), real.len(), "no handle is used twice: {all:?}");
    let kinds: Vec<String> = again
        .drawing
        .doc
        .entities_in_block(MODEL)
        .map(|e| match e {
            EntityType::Polyline3D(p) => format!("pl{}", p.vertices.len()),
            EntityType::PolyfaceMesh(m) => format!("mesh{}/{}", m.vertices.len(), m.faces.len()),
            other => format!("{other:?}").chars().take(12).collect(),
        })
        .collect();
    assert_eq!(kinds.iter().filter(|k| *k == "pl3").count(), 2, "{kinds:?}");
    assert_eq!(
        kinds.iter().filter(|k| *k == "mesh3/1").count(),
        2,
        "{kinds:?}"
    );
}

/// A drawing with a linear dimension drawn by its own `*D1` block (a line at y 5), and a table
/// drawn by its `*T1` block (a line from its insertion point).
fn annotated_doc() -> (CadDocument, u64, u64) {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    add_anonymous(
        &mut doc,
        "*D1",
        vec![EntityType::Line(Line::from_coords(
            0.0, 5.0, 0.0, 4.0, 5.0, 0.0,
        ))],
    );
    let mut linear = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(4.0, 0.0, 0.0));
    linear.definition_point = Vector3::new(0.0, 5.0, 0.0);
    linear.base.definition_point = linear.definition_point;
    linear.base.block_name = "*D1".to_string();
    let dim = doc
        .add_entity(EntityType::Dimension(Dimension::Linear(linear)))
        .unwrap()
        .value();
    let t_block = add_anonymous(
        &mut doc,
        "*T1",
        vec![EntityType::Line(Line::from_coords(
            0.0, 0.0, 0.0, 2.0, 0.0, 0.0,
        ))],
    );
    let mut table = Table::new(Vector3::new(20.0, 0.0, 0.0), 0, 0);
    table.rows = vec![TableRow::from_texts(&["A"])];
    table.columns = vec![TableColumn::with_width(2.0)];
    table.block_name = "*T1".to_string();
    table.block_record_handle = Some(t_block);
    let table = doc.add_entity(EntityType::Table(table)).unwrap().value();
    (doc, dim, table)
}

/// The line inside the block named `name`.
fn block_line(s: &Session, name: &str) -> [[f64; 2]; 2] {
    let h = s
        .drawing
        .doc
        .entities_in_block(name)
        .find(|e| matches!(e, EntityType::Line(_)))
        .map(|e| e.common().handle.value())
        .unwrap();
    ends(s, h)
}

#[test]
fn dimensions_and_tables_move_and_copy_with_their_blocks() {
    let (doc, dim, table) = annotated_doc();
    let (_dir, mut s) = open_doc(&doc, "annotated.dxf");
    run(
        &mut s,
        ModifySpec::Move {
            targets: targets(&[dim, table]),
            delta: [1.0, 2.0],
        },
    );
    let EntityType::Dimension(Dimension::Linear(d)) = entity(&s, dim) else {
        panic!()
    };
    assert!(near([d.first_point.x, d.first_point.y], [1.0, 2.0]));
    assert!(near(
        [d.definition_point.x, d.definition_point.y],
        [1.0, 7.0]
    ));
    assert!(
        near(
            [d.base.insertion_point.x, d.base.insertion_point.y],
            [0.0, 0.0]
        ),
        "the block's own placement stays"
    );
    // The dimension's block is drawn in world coordinates: its line moves too.
    let line = block_line(&s, "*D1");
    assert!(
        near(line[0], [1.0, 7.0]) && near(line[1], [5.0, 7.0]),
        "{line:?}"
    );
    // A table is a reference to its block: the reference moves, the block stays.
    let EntityType::Table(t) = entity(&s, table) else {
        panic!()
    };
    assert!(near(
        [t.insertion_point.x, t.insertion_point.y],
        [21.0, 2.0]
    ));
    let line = block_line(&s, "*T1");
    assert!(near(line[0], [0.0, 0.0]), "{line:?}");

    // A copy gets a block of its own under a new anonymous name; undo takes both away.
    let blocks_before = s.drawing.doc.block_records.iter().count();
    let copies = added(
        &mut s,
        ModifySpec::Copy {
            targets: targets(&[dim, table]),
            delta: [0.0, 10.0],
            count: 1,
        },
    );
    let (EntityType::Dimension(copy), EntityType::Table(tcopy)) =
        (entity(&s, copies[0]), entity(&s, copies[1]))
    else {
        panic!()
    };
    let name = copy.base().block_name.clone();
    assert!(name.starts_with("*D") && name != "*D1", "{name}");
    let line = block_line(&s, &name);
    assert!(near(line[0], [1.0, 17.0]), "{line:?}");
    assert_eq!(block_line(&s, "*D1")[0], [1.0, 7.0]);
    assert!(tcopy.block_name.starts_with("*T") && tcopy.block_name != "*T1");
    assert_eq!(
        tcopy.block_record_handle,
        s.drawing
            .doc
            .block_records
            .get(&tcopy.block_name)
            .map(|b| b.handle)
    );
    assert_eq!(
        s.drawing.doc.block_records.iter().count(),
        blocks_before + 2
    );
    // Saved and opened again, both copies keep their blocks.
    s.save_as(&_dir.path().join("annotated-copy.dwg")).unwrap();
    let again = Session::open(&_dir.path().join("annotated-copy.dwg")).unwrap();
    assert!(again.drawing.doc.block_records.get(&name).is_some());
    assert!(s.undo());
    assert_eq!(s.drawing.doc.block_records.iter().count(), blocks_before);
    assert!(s.drawing.doc.block_records.get(&name).is_none());

    // Turning a dimension is still refused.
    let message = refused(
        &mut s,
        ModifySpec::Rotate {
            targets: targets(&[dim]),
            base: [0.0, 0.0],
            angle: 10.0,
            copy: false,
        },
    );
    assert!(message.contains("Dimension"), "{message}");
}

fn measure_line(s: &mut Session, h: u64) -> Measurement {
    match s
        .run(Command::MeasureEntity {
            handle: h,
            chain: vec![],
        })
        .unwrap()
    {
        Outcome::Measured { measurement } => *measurement,
        other => panic!("{other:?}"),
    }
}

fn row(s: &Session, id: &str) -> Measurement {
    s.takeoff
        .items
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .unwrap()
}

#[test]
fn the_take_off_follows_moves_scales_and_erases_in_the_same_step() {
    let (_dir, mut s, ids) = modify_session();
    let m = measure_line(&mut s, ids.line);
    assert_eq!(m.kind, Kind::Length);
    assert!((m.value - 1.0).abs() < 1e-9);
    run(
        &mut s,
        ModifySpec::Move {
            targets: targets(&[ids.line]),
            delta: [5.0, 5.0],
        },
    );
    let moved = row(&s, &m.id);
    assert!((moved.value - 1.0).abs() < 1e-9);
    assert_eq!(moved.handles, vec![ids.line]);
    run(
        &mut s,
        ModifySpec::Scale {
            targets: targets(&[ids.line]),
            base: [0.0, 0.0],
            factor: 3.0,
            copy: false,
        },
    );
    let scaled = row(&s, &m.id);
    assert!((scaled.value - 3.0).abs() < 1e-9, "{}", scaled.value);
    assert!(
        (scaled.shown - 3.0).abs() < 1e-9,
        "metres: {}",
        scaled.shown
    );
    // One step: undo puts the line and the row back together.
    assert!(s.undo());
    assert!((row(&s, &m.id).value - 1.0).abs() < 1e-9);
    assert!(s.redo());
    // Erase keeps the row and its value, without the link.
    s.run(Command::DeleteEntities {
        targets: targets(&[ids.line]),
    })
    .unwrap();
    let erased = row(&s, &m.id);
    assert!(erased.handles.is_empty());
    assert!((erased.value - 3.0).abs() < 1e-9);
    assert!(s.undo());
    assert_eq!(row(&s, &m.id).handles, vec![ids.line]);

    // Explode of a counted block reference drops it from the count's links.
    let count = match s
        .run(Command::CountBlock {
            block_name: "B".into(),
            layer: None,
        })
        .unwrap()
    {
        Outcome::Measured { measurement } => *measurement,
        other => panic!("{other:?}"),
    };
    assert_eq!(count.handles, vec![ids.insert]);
    added(
        &mut s,
        ModifySpec::Explode {
            targets: targets(&[ids.insert]),
        },
    );
    let exploded = row(&s, &count.id);
    assert!(exploded.handles.is_empty());
    assert!((exploded.value - 1.0).abs() < 1e-9);
}

#[test]
fn a_partly_erased_row_is_measured_again_and_a_count_counts_one_less() {
    let (_dir, mut s, ids) = modify_session();
    // A row on a line cut in two by a break: both pieces.
    let m = measure_line(&mut s, ids.line);
    let Outcome::Edited { added: pieces, .. } = s
        .run(Command::Edit(qiyas_core::EditSpec::Break {
            target: Target {
                handle: ids.line,
                chain: Vec::new(),
            },
            p1: [1.25, 0.0],
            p2: [1.25, 0.0],
        }))
        .unwrap()
    else {
        panic!("a break")
    };
    assert_eq!(row(&s, &m.id).handles, vec![ids.line, pieces[0]]);
    // Erasing one piece: the row measures the other one, still linked, and the outcome says so.
    let out = s
        .run(Command::DeleteEntities {
            targets: targets(&[pieces[0]]),
        })
        .unwrap();
    assert!(
        matches!(
            out,
            Outcome::TakeoffFollowed {
                remeasured: 1,
                unlinked: 0
            }
        ),
        "{out:?}"
    );
    let r = row(&s, &m.id);
    assert_eq!(r.handles, vec![ids.line]);
    // The break keeps the line's handle on the longer piece, 1.25 to 2.
    assert!((r.value - 0.75).abs() < 1e-9, "{}", r.value);
    assert!((r.shown - 0.75).abs() < 1e-9, "{}", r.shown);
    // Erasing the last one: the value stays, unlinked.
    let out = s
        .run(Command::DeleteEntities {
            targets: targets(&[ids.line]),
        })
        .unwrap();
    assert!(
        matches!(
            out,
            Outcome::TakeoffFollowed {
                remeasured: 0,
                unlinked: 1
            }
        ),
        "{out:?}"
    );
    assert!(row(&s, &m.id).handles.is_empty());

    // A count of two references, one erased: 1 piece, linked to the other.
    let Outcome::Added { handles: copy } = run(
        &mut s,
        ModifySpec::Copy {
            targets: targets(&[ids.insert]),
            delta: [0.0, 10.0],
            count: 1,
        },
    ) else {
        panic!("a copy")
    };
    let count = match s
        .run(Command::CountBlock {
            block_name: "B".into(),
            layer: None,
        })
        .unwrap()
    {
        Outcome::Measured { measurement } => *measurement,
        other => panic!("{other:?}"),
    };
    assert_eq!(count.value, 2.0);
    s.run(Command::DeleteEntities {
        targets: targets(&[copy[0]]),
    })
    .unwrap();
    let r = row(&s, &count.id);
    assert_eq!(r.handles, vec![ids.insert]);
    assert_eq!((r.value, r.shown), (1.0, 1.0));
    // One step: undo counts both again.
    assert!(s.undo());
    assert_eq!(row(&s, &count.id).value, 2.0);
}

#[test]
fn exploding_a_huge_array_is_refused_before_anything_is_built() {
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, _) = modify_doc();
    // Block B (two lines inside) arrayed 1,000 x 1,000: 2 M parts.
    let mut array = Insert::new("B", Vector3::new(0.0, 100.0, 0.0));
    array.column_count = 1000;
    array.row_count = 1000;
    array.column_spacing = 1.0;
    array.row_spacing = 1.0;
    let h = doc.add_entity(EntityType::Insert(array)).unwrap().value();
    let path = write_drawing(&doc, &dir.path().join("array.dxf"));
    let mut s = Session::open(&path).unwrap();
    let reference = performance::reference();
    let t = std::time::Instant::now();
    let refused = s.run(Command::Modify(ModifySpec::Explode {
        targets: targets(&[h]),
    }));
    assert!(
        matches!(&refused, Err(CoreError::InvalidCommand(why)) if why.contains("100000")),
        "{refused:?}"
    );
    performance::assert_relative(
        t.elapsed(),
        reference,
        1.25,
        "refuse huge array before building",
    );
}

#[test]
fn erasing_an_image_cleans_its_definition_link() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let png = qiyas_core::fixtures::write_png(dir.path(), "pic.png");
    let image = match s
        .run(Command::AttachImage {
            path: png.display().to_string(),
            insert_at: [0.0, 0.0],
            width_units: 2.0,
        })
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    let reactor = match entity(&s, image) {
        EntityType::RasterImage(i) => i.definition_reactor_handle.unwrap(),
        other => panic!("{other:?}"),
    };
    assert!(s.drawing.doc.objects.contains_key(&reactor));
    s.run(Command::DeleteEntities {
        targets: targets(&[image]),
    })
    .unwrap();
    assert!(!s.drawing.doc.objects.contains_key(&reactor));
    assert!(s.undo());
    assert!(s.drawing.doc.objects.contains_key(&reactor));
}

#[test]
fn explode_keeps_attribute_values_as_texts_and_says_what_it_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&qiyas_core::fixtures::write_schedule(dir.path(), "dxf")).unwrap();
    let tag = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(i) if !i.attributes.is_empty()))
        .map(|e| e.common().handle.value())
        .unwrap();
    let a_text = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Text(_)))
        .map(|e| e.common().handle.value())
        .unwrap();
    let out = run(
        &mut s,
        ModifySpec::Explode {
            targets: targets(&[tag, a_text]),
        },
    );
    let Outcome::Exploded {
        handles,
        left_alone,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(left_alone, 1, "the text has no parts");
    let texts: Vec<Text> = handles
        .iter()
        .filter_map(|&h| match entity(&s, h) {
            EntityType::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    let d9 = texts
        .iter()
        .find(|t| t.value == "D9")
        .expect("the attribute value stays");
    assert!(near(
        [d9.insertion_point.x, d9.insertion_point.y],
        [30.5, 10.0]
    ));
    assert!((d9.height - 0.2).abs() < 1e-9);
    assert!(
        texts.iter().any(|t| t.value == "T1"),
        "the block's own text"
    );
}

#[test]
fn picks_say_what_kind_of_entity_they_are() {
    let (_dir, s, ids) = modify_session();
    let picked = s.pick_window([[0.5, -0.5], [2.5, 0.5]], false);
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].top, ids.line);
    assert_eq!(picked[0].kind.as_deref(), Some("Line"));
    assert_eq!(
        s.pick([1.5, 0.0], 0.1).unwrap().kind.as_deref(),
        Some("Line")
    );
}

use crate::performance;
