//! What the properties palette's Geometry section reads from `Session::entity_info`: ends,
//! centres, angles, vertex counts, insertion points and a hatch's pattern, all in world
//! coordinates (an entity inside a block is placed by its block references).

use opencadcodec::{CadDocument, EntityType, Handle};
use qiyas_core::fixtures::{sample_doc, write_drawing};
use qiyas_core::{Command, DrawSpec, EntityInfo, HatchPattern, Outcome, Session};

const MODEL: &str = "*Model_Space";

fn open(doc: &CadDocument) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join("plan.dxf"));
    let s = Session::open(&path).unwrap();
    (dir, s)
}

/// The handles in model space whose variant name starts with `kind`.
fn all(s: &Session, kind: &str) -> Vec<u64> {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .filter(|e| format!("{e:?}").starts_with(kind))
        .map(|e| e.common().handle.value())
        .collect()
}

fn info(s: &Session, kind: &str) -> EntityInfo {
    s.entity_info(all(s, kind)[0], &[]).unwrap()
}

fn draw(s: &mut Session, spec: DrawSpec) -> u64 {
    match s.run(Command::Draw(spec)).unwrap() {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}

fn near(a: Option<[f64; 2]>, b: [f64; 2]) {
    let a = a.expect("a point");
    assert!(
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9,
        "{a:?} is not {b:?}"
    );
}

#[test]
fn a_line_gives_its_ends_and_nothing_of_a_curve() {
    let (_d, s) = open(&sample_doc());
    let i = info(&s, "Line");
    near(i.start, [0.0, 0.0]);
    near(i.end, [10.0, 0.0]);
    assert_eq!(i.length, Some(10.0));
    assert!(i.centre.is_none() && i.start_angle.is_none() && i.vertex_count.is_none());
    assert!(i.position.is_none() && i.pattern.is_none());
}

#[test]
fn a_closed_polyline_counts_its_vertices() {
    let (_d, s) = open(&sample_doc());
    let i = info(&s, "LwPolyline");
    assert_eq!(i.vertex_count, Some(4));
    assert_eq!(i.closed, Some(true));
    assert_eq!(i.length, Some(14.0));
    assert!((i.area.unwrap() - 12.0).abs() < 1e-9);
    assert!(i.start.is_none() && i.end.is_none() && i.centre.is_none());
}

#[test]
fn an_arc_gives_centre_ends_and_angles() {
    let (_d, s) = open(&sample_doc());
    let i = info(&s, "Arc");
    near(i.centre, [20.0, 0.0]);
    near(i.start, [22.0, 0.0]);
    near(i.end, [18.0, 0.0]);
    assert!((i.start_angle.unwrap() - 0.0).abs() < 1e-9);
    assert!((i.end_angle.unwrap() - 180.0).abs() < 1e-9);
    assert_eq!(i.radius, Some(2.0));
    // Half a circle of radius 2.
    assert!((i.length.unwrap() - 2.0 * std::f64::consts::PI).abs() < 1e-9);
}

#[test]
fn a_circle_and_an_ellipse_give_their_centre() {
    let (_d, mut s) = open(&sample_doc());
    let c = draw(
        &mut s,
        DrawSpec::Circle {
            centre: [50.0, 60.0],
            radius: 2.0,
        },
    );
    let i = s.entity_info(c, &[]).unwrap();
    near(i.centre, [50.0, 60.0]);
    assert_eq!(i.radius, Some(2.0));
    assert!(i.start.is_none() && i.end.is_none() && i.start_angle.is_none());
    let e = draw(
        &mut s,
        DrawSpec::Ellipse {
            centre: [70.0, 80.0],
            major: [4.0, 0.0],
            ratio: 0.5,
        },
    );
    let i = s.entity_info(e, &[]).unwrap();
    near(i.centre, [70.0, 80.0]);
    assert!(i.radius.is_none());
    assert!(i.length.is_some() && i.area.is_some());
}

#[test]
fn a_texts_and_a_block_references_insertion_point() {
    let (_d, s) = open(&sample_doc());
    near(info(&s, "Text").position, [0.0, -2.0]);
    let inserts = all(&s, "Insert");
    near(s.entity_info(inserts[0], &[]).unwrap().position, [1.0, 1.0]);
    near(s.entity_info(inserts[1], &[]).unwrap().position, [5.0, 1.0]);
}

#[test]
fn a_line_inside_a_block_is_placed_by_its_references() {
    let (_d, s) = open(&sample_doc());
    let door = s
        .drawing
        .doc
        .entities_in_block("DOOR")
        .next()
        .map(|e| e.common().handle.value())
        .unwrap();
    let inserts = all(&s, "Insert");
    // The door (0,0)-(0.9,0) at (1,1); turned a quarter about (5,1); doubled at (9,1).
    let at = |n: usize| s.entity_info(door, &[inserts[n]]).unwrap();
    near(at(0).start, [1.0, 1.0]);
    near(at(0).end, [1.9, 1.0]);
    near(at(1).start, [5.0, 1.0]);
    near(at(1).end, [5.0, 1.9]);
    near(at(2).start, [9.0, 1.0]);
    near(at(2).end, [10.8, 1.0]);
    assert!((at(2).length.unwrap() - 1.8).abs() < 1e-9);
}

#[test]
fn a_hatch_names_its_pattern() {
    let (_d, mut s) = open(&sample_doc());
    let h = draw(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]].into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
    );
    let i = s.entity_info(h, &[]).unwrap();
    assert_eq!(i.pattern.as_deref(), Some("SOLID"));
    assert!((i.area.unwrap() - 100.0).abs() < 1e-9);
    let EntityType::Hatch(_) = s.drawing.doc.get_entity(Handle::new(h)).unwrap() else {
        panic!("a hatch")
    };
}
