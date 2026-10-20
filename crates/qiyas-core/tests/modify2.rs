//! Edit commands (`Command::Edit`): trim, extend, break, join, lengthen, reverse, offset, fillet,
//! chamfer, blend, pedit, overkill and flatten on the entities at the top of the sheet, each one
//! undo step.

use std::f64::consts::{FRAC_PI_2, PI};

use opencadcodec::{
    Arc, CadDocument, Circle, Ellipse, EntityType, Handle, Line, LwPolyline, Vector2, Vector3,
};
use qiyas_core::fixtures::write_drawing;
use qiyas_core::{
    Command, CoreError, EditSpec, Kind, LengthenBy, Measurement, Outcome, PEditOp, Session, Target,
};

const MODEL: &str = "*Model_Space";

fn line(a: [f64; 2], b: [f64; 2]) -> EntityType {
    EntityType::Line(Line::from_coords(a[0], a[1], 0.0, b[0], b[1], 0.0))
}

fn circle(c: [f64; 2], r: f64) -> EntityType {
    EntityType::Circle(Circle::from_center_radius(Vector3::new(c[0], c[1], 0.0), r))
}

fn arc(c: [f64; 2], r: f64, start: f64, end: f64) -> EntityType {
    let mut a = Arc::new();
    a.center = Vector3::new(c[0], c[1], 0.0);
    a.radius = r;
    a.start_angle = start;
    a.end_angle = end;
    EntityType::Arc(a)
}

fn polyline(points: &[([f64; 2], f64)], closed: bool) -> EntityType {
    let mut pl = LwPolyline::new();
    for &(p, bulge) in points {
        pl.add_point(Vector2::new(p[0], p[1]));
        pl.vertices.last_mut().unwrap().bulge = bulge;
    }
    pl.is_closed = closed;
    EntityType::LwPolyline(pl)
}

/// A session on a drawing in metres holding `entities` in model space, and their handles.
fn session(entities: Vec<EntityType>) -> (tempfile::TempDir, Session, Vec<u64>) {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    let handles = entities
        .into_iter()
        .map(|e| doc.add_entity(e).unwrap().value())
        .collect();
    let path = write_drawing(&doc, &dir.path().join("edit.dxf"));
    (dir, Session::open(&path).unwrap(), handles)
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn ts(handles: &[u64]) -> Vec<Target> {
    handles.iter().map(|&h| t(h)).collect()
}

fn run(s: &mut Session, spec: EditSpec) -> Outcome {
    s.run(Command::Edit(spec)).unwrap()
}

/// Runs an edit; its new and removed handles.
fn edited(s: &mut Session, spec: EditSpec) -> (Vec<u64>, Vec<u64>, usize) {
    match run(s, spec) {
        Outcome::Edited {
            added,
            removed,
            left_alone,
            ..
        } => (added, removed, left_alone),
        other => panic!("expected an edit, got {other:?}"),
    }
}

/// Runs an edit that must be refused, checks nothing changed, and returns why.
fn refused(s: &mut Session, spec: EditSpec) -> String {
    let before: Vec<EntityType> = s.drawing.doc.entities_in_block(MODEL).cloned().collect();
    let undo = s.can_undo();
    let err = s.run(Command::Edit(spec)).unwrap_err();
    let after: Vec<EntityType> = s.drawing.doc.entities_in_block(MODEL).cloned().collect();
    assert_eq!(before, after, "a refused edit changes nothing");
    assert_eq!(s.can_undo(), undo, "a refused edit is no step");
    match err {
        CoreError::InvalidCommand(m) => m,
        other => panic!("{other:?}"),
    }
}

fn entity(s: &Session, h: u64) -> EntityType {
    s.drawing.doc.get_entity(Handle::new(h)).cloned().unwrap()
}

fn gone(s: &Session, h: u64) -> bool {
    s.drawing.doc.get_entity(Handle::new(h)).is_none()
}

fn sheet(s: &Session) -> Vec<u64> {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .map(|e| e.common().handle.value())
        .collect()
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

fn ends(s: &Session, h: u64) -> [[f64; 2]; 2] {
    match entity(s, h) {
        EntityType::Line(l) => [[l.start.x, l.start.y], [l.end.x, l.end.y]],
        other => panic!("a line, got {other:?}"),
    }
}

fn assert_line(s: &Session, h: u64, want: [[f64; 2]; 2]) {
    let got = ends(s, h);
    assert!(
        near(got[0], want[0]) && near(got[1], want[1]),
        "{got:?} != {want:?}"
    );
}

/// A line's ends in either order.
fn assert_segment(s: &Session, h: u64, want: [[f64; 2]; 2]) {
    let got = ends(s, h);
    let same = near(got[0], want[0]) && near(got[1], want[1]);
    let reversed = near(got[0], want[1]) && near(got[1], want[0]);
    assert!(same || reversed, "{got:?} != {want:?}");
}

fn vertices(s: &Session, h: u64) -> (Vec<([f64; 2], f64)>, bool) {
    match entity(s, h) {
        EntityType::LwPolyline(pl) => (
            pl.vertices
                .iter()
                .map(|v| ([v.location.x, v.location.y], v.bulge))
                .collect(),
            pl.is_closed,
        ),
        other => panic!("a polyline, got {other:?}"),
    }
}

fn measure(s: &mut Session, h: u64) -> Measurement {
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

// Review Focus 1.
#[test]
fn trim_against_a_circle_crossed_twice_removes_only_the_clicked_piece() {
    let (_dir, mut s, h) = session(vec![line([0.0, 0.0], [12.0, 0.0]), circle([5.0, 0.0], 2.0)]);
    let (wall, round) = (h[0], h[1]);
    let (added, removed, _) = edited(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[round]),
            target: t(wall),
            at_point: [5.0, 0.1],
        },
    );
    assert!(removed.is_empty());
    assert_eq!(added.len(), 1);
    // The longer piece keeps the line's handle; the shorter one is new.
    assert_line(&s, wall, [[7.0, 0.0], [12.0, 0.0]]);
    assert_line(&s, added[0], [[0.0, 0.0], [3.0, 0.0]]);
    assert_eq!(sheet(&s).len(), 3, "the two pieces and the circle");
    // One step: undo gives back the single line under its own handle.
    assert!(s.undo());
    assert_line(&s, wall, [[0.0, 0.0], [12.0, 0.0]]);
    assert!(gone(&s, added[0]));
    assert_eq!(sheet(&s), vec![wall, round]);
    assert!(s.redo());
    assert_line(&s, wall, [[7.0, 0.0], [12.0, 0.0]]);
    assert_line(&s, added[0], [[0.0, 0.0], [3.0, 0.0]]);
}

#[test]
fn trim_an_end_piece_and_with_every_entity_as_cutter() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [10.0, 0.0]),
        line([2.0, -1.0], [2.0, 1.0]),
        line([6.0, -1.0], [6.0, 1.0]),
    ]);
    // No cutters named: everything drawn cuts. The clicked end piece goes, nothing is added.
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Trim {
            cutters: vec![],
            target: t(h[0]),
            at_point: [9.0, 0.0],
        },
    );
    assert!(added.is_empty());
    assert_line(&s, h[0], [[0.0, 0.0], [6.0, 0.0]]);
    // A line nothing crosses can't be trimmed.
    let message = refused(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[2]]),
            target: t(h[1]),
            at_point: [2.0, 0.9],
        },
    );
    assert!(message.contains("cut"), "{message}");
}

#[test]
fn trimming_a_circle_leaves_an_arc_under_its_handle() {
    let (_dir, mut s, h) = session(vec![circle([0.0, 0.0], 2.0), line([-3.0, 1.0], [3.0, 1.0])]);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[1]]),
            target: t(h[0]),
            at_point: [0.0, 2.0],
        },
    );
    assert!(added.is_empty());
    let EntityType::Arc(a) = entity(&s, h[0]) else {
        panic!("an arc")
    };
    // The top cap above y = 1 is gone: the arc runs from 150° round the bottom to 30°.
    let (x, y) = (3f64.sqrt(), 1.0);
    let at = |angle: f64| [a.radius * angle.cos(), a.radius * angle.sin()];
    assert!(near(at(a.start_angle), [-x, y]), "{a:?}");
    assert!(near(at(a.end_angle), [x, y]), "{a:?}");
    assert!(s.undo());
    assert!(matches!(entity(&s, h[0]), EntityType::Circle(_)));
}

#[test]
fn trim_polylines_through_their_arcs_and_across_their_seam() {
    let (_dir, mut s, h) = session(vec![
        rounded_rectangle(),
        line([2.0, -1.0], [2.0, 3.0]),
        // A half circle below (0, 0) to (4, 0), through (2, -2).
        polyline(&[([0.0, 0.0], 1.0), ([4.0, 0.0], 0.0)], false),
        line([2.0, -3.0], [2.0, -1.5]),
    ]);
    // The closed outline loses its right half and opens: what is left runs round the left.
    edited(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[1]]),
            target: t(h[0]),
            at_point: [4.0, 1.0],
        },
    );
    let (v, closed) = vertices(&s, h[0]);
    assert!(!closed);
    let (first, last) = (v[0].0, v[v.len() - 1].0);
    assert!(
        (near(first, [2.0, 2.0]) && near(last, [2.0, 0.0]))
            || (near(first, [2.0, 0.0]) && near(last, [2.0, 2.0])),
        "{v:?}"
    );
    let m = measure(&mut s, h[0]);
    assert!((m.value - (8.0 + PI) / 2.0).abs() < 1e-9, "{}", m.value);
    // The open polyline is cut in the middle of its arc: a quarter circle is left.
    edited(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[3]]),
            target: t(h[2]),
            at_point: [3.5, -1.4],
        },
    );
    let (v, _) = vertices(&s, h[2]);
    assert_eq!(v.len(), 2);
    assert!(
        near(v[0].0, [0.0, 0.0]) && near(v[1].0, [2.0, -2.0]),
        "{v:?}"
    );
    assert!((v[0].1 - (PI / 8.0).tan()).abs() < 1e-9, "{v:?}");
}

#[test]
fn extend_a_line_to_a_circle() {
    let (_dir, mut s, h) = session(vec![line([0.0, 0.0], [2.0, 0.0]), circle([10.0, 0.0], 3.0)]);
    let (added, removed, _) = edited(
        &mut s,
        EditSpec::Extend {
            boundaries: ts(&[h[1]]),
            target: t(h[0]),
            at_end: [1.8, 0.0],
        },
    );
    assert!(added.is_empty() && removed.is_empty());
    assert_line(&s, h[0], [[0.0, 0.0], [7.0, 0.0]]);
    // Its start, away from the circle, reaches nothing.
    let message = refused(
        &mut s,
        EditSpec::Extend {
            boundaries: ts(&[h[1]]),
            target: t(h[0]),
            at_end: [0.1, 0.0],
        },
    );
    assert!(message.contains("boundary"), "{message}");
}

#[test]
fn extend_an_arc_and_a_polyline_end() {
    let (_dir, mut s, h) = session(vec![
        arc([0.0, 0.0], 2.0, 0.0, FRAC_PI_2),
        line([-5.0, 0.0], [5.0, 0.0]),
        polyline(
            &[([0.0, 5.0], 0.0), ([3.0, 5.0], 0.0), ([3.0, 7.0], 0.0)],
            false,
        ),
        line([0.0, 10.0], [5.0, 10.0]),
    ]);
    // The arc's end at 90° runs on round to the line y = 0 at 180°.
    edited(
        &mut s,
        EditSpec::Extend {
            boundaries: ts(&[h[1]]),
            target: t(h[0]),
            at_end: [0.0, 2.0],
        },
    );
    let EntityType::Arc(a) = entity(&s, h[0]) else {
        panic!()
    };
    assert!(
        a.start_angle.abs() < 1e-9 && (a.end_angle - PI).abs() < 1e-9,
        "{a:?}"
    );
    // The polyline's last side runs up to y = 10.
    edited(
        &mut s,
        EditSpec::Extend {
            boundaries: ts(&[h[3]]),
            target: t(h[2]),
            at_end: [3.0, 6.9],
        },
    );
    let (v, _) = vertices(&s, h[2]);
    assert!(near(v[2].0, [3.0, 10.0]), "{v:?}");
}

#[test]
fn break_a_line_between_two_points_and_at_one() {
    let (_dir, mut s, h) = session(vec![line([0.0, 0.0], [10.0, 0.0])]);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Break {
            target: t(h[0]),
            p1: [2.0, 0.0],
            p2: [5.0, 0.0],
        },
    );
    assert_line(&s, h[0], [[5.0, 0.0], [10.0, 0.0]]);
    assert_line(&s, added[0], [[0.0, 0.0], [2.0, 0.0]]);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Break {
            target: t(h[0]),
            p1: [8.0, 0.0],
            p2: [8.0, 0.0],
        },
    );
    assert_line(&s, h[0], [[5.0, 0.0], [8.0, 0.0]]);
    assert_line(&s, added[0], [[8.0, 0.0], [10.0, 0.0]]);
}

#[test]
fn join_three_lines_into_a_closed_polyline() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        // Drawn the other way round.
        line([4.0, 3.0], [4.0, 0.0]),
        line([4.0, 3.0], [0.0, 0.0]),
    ]);
    let (added, removed, left_alone) = edited(&mut s, EditSpec::Join { targets: ts(&h) });
    assert!(added.is_empty());
    assert_eq!(removed, vec![h[1], h[2]]);
    assert_eq!(left_alone, 0);
    let (v, closed) = vertices(&s, h[0]);
    assert!(closed);
    let points: Vec<_> = v.iter().map(|(p, _)| *p).collect();
    assert_eq!(points.len(), 3);
    assert!(
        near(points[0], [0.0, 0.0]) && near(points[1], [4.0, 0.0]) && near(points[2], [4.0, 3.0])
    );
    assert!(v.iter().all(|(_, b)| *b == 0.0));
    assert_eq!(sheet(&s), vec![h[0]]);
    let m = measure(&mut s, h[0]);
    assert!((m.value - 6.0).abs() < 1e-9);
    // The measurement, then the join.
    assert!(s.undo() && s.undo());
    assert_eq!(sheet(&s), h);
    assert!(matches!(entity(&s, h[0]), EntityType::Line(_)));
}

#[test]
fn join_a_line_and_an_arc_keeps_the_arc_as_a_bulge() {
    let (_dir, mut s, h) = session(vec![
        line([-2.0, 0.0], [2.0, 0.0]),
        // A half circle above, from (2, 0) round to (-2, 0)... drawn from 0 to 180°.
        arc([0.0, 0.0], 2.0, 0.0, PI),
        line([10.0, 0.0], [11.0, 0.0]),
    ]);
    let (_, removed, left_alone) = edited(&mut s, EditSpec::Join { targets: ts(&h) });
    assert_eq!(removed, vec![h[1]]);
    assert_eq!(left_alone, 1, "the far line touches neither");
    let (v, closed) = vertices(&s, h[0]);
    assert!(closed);
    assert_eq!(v.len(), 2);
    assert!(
        (v[1].1 - 1.0).abs() < 1e-9,
        "a half circle is bulge 1: {v:?}"
    );
    let m = measure(&mut s, h[0]);
    assert!((m.value - 2.0 * PI).abs() < 1e-6, "{}", m.value);
}

#[test]
fn lengthen_by_delta_total_and_percent() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        arc([0.0, 0.0], 2.0, 0.0, FRAC_PI_2),
    ]);
    let lengthen = |by| EditSpec::Lengthen {
        target: t(h[0]),
        at_end: [3.9, 0.0],
        by,
    };
    edited(&mut s, lengthen(LengthenBy::Delta(1.0)));
    assert_line(&s, h[0], [[0.0, 0.0], [5.0, 0.0]]);
    edited(&mut s, lengthen(LengthenBy::Total(3.0)));
    assert_line(&s, h[0], [[0.0, 0.0], [3.0, 0.0]]);
    edited(&mut s, lengthen(LengthenBy::Percent(200.0)));
    assert_line(&s, h[0], [[0.0, 0.0], [6.0, 0.0]]);
    // At its start end.
    edited(
        &mut s,
        EditSpec::Lengthen {
            target: t(h[0]),
            at_end: [0.1, 0.0],
            by: LengthenBy::Delta(-1.0),
        },
    );
    assert_line(&s, h[0], [[1.0, 0.0], [6.0, 0.0]]);
    // An arc to a total of a half circle's length, at its end.
    edited(
        &mut s,
        EditSpec::Lengthen {
            target: t(h[1]),
            at_end: [0.0, 2.0],
            by: LengthenBy::Total(2.0 * PI),
        },
    );
    let EntityType::Arc(a) = entity(&s, h[1]) else {
        panic!()
    };
    assert!((a.end_angle - PI).abs() < 1e-9, "{a:?}");
    let message = refused(
        &mut s,
        EditSpec::Lengthen {
            target: t(h[0]),
            at_end: [5.9, 0.0],
            by: LengthenBy::Delta(-5.0),
        },
    );
    assert!(message.contains("short"), "{message}");
}

#[test]
fn reverse_a_line_and_a_polyline() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        polyline(
            &[([0.0, 0.0], 0.5), ([2.0, 0.0], 0.0), ([2.0, 2.0], 0.0)],
            false,
        ),
    ]);
    edited(&mut s, EditSpec::Reverse { targets: ts(&h) });
    assert_line(&s, h[0], [[4.0, 0.0], [0.0, 0.0]]);
    let (v, _) = vertices(&s, h[1]);
    assert!(near(v[0].0, [2.0, 2.0]) && near(v[1].0, [2.0, 0.0]) && near(v[2].0, [0.0, 0.0]));
    // The arc from (0, 0) to (2, 0) now runs from (2, 0) back: the bulge turns the other way.
    assert_eq!([v[0].1, v[1].1, v[2].1], [0.0, -0.5, 0.0]);
}

/// A 4 x 2 rectangle with corners rounded to 0.5, counter-clockwise: lines and quarter arcs.
fn rounded_rectangle() -> EntityType {
    let b = (PI / 8.0).tan();
    polyline(
        &[
            ([0.5, 0.0], 0.0),
            ([3.5, 0.0], b),
            ([4.0, 0.5], 0.0),
            ([4.0, 1.5], b),
            ([3.5, 2.0], 0.0),
            ([0.5, 2.0], b),
            ([0.0, 1.5], 0.0),
            ([0.0, 0.5], b),
        ],
        true,
    )
}

// Review Focus 2.
#[test]
fn offset_of_a_polyline_with_arcs_keeps_its_arcs_and_measures_exactly() {
    let (_dir, mut s, h) = session(vec![rounded_rectangle()]);
    let original = measure(&mut s, h[0]);
    assert_eq!(original.kind, Kind::Area);
    let r: f64 = 0.5;
    let area = 8.0 - (4.0 - PI) * r * r;
    let perimeter = 2.0 * (3.0 + 1.0) + 2.0 * PI * r;
    assert!((original.value - area).abs() < 1e-9);
    let added = match run(
        &mut s,
        EditSpec::Offset {
            target: t(h[0]),
            distance: 0.2,
            side_point: [2.0, -1.0],
        },
    ) {
        Outcome::Added { handles } => handles,
        other => panic!("{other:?}"),
    };
    assert_eq!(added.len(), 1, "a single closed polyline");
    let (v, closed) = vertices(&s, added[0]);
    assert!(closed);
    assert_eq!(
        v.iter().filter(|(_, b)| *b != 0.0).count(),
        4,
        "its four arcs: {v:?}"
    );
    let m = measure(&mut s, added[0]);
    let want = area + perimeter * 0.2 + PI * 0.2 * 0.2;
    assert!((m.value - want).abs() < 1e-6, "{} != {want}", m.value);
    // The original stays; undoing the measurement, then the offset, removes it.
    assert!(!gone(&s, h[0]));
    assert!(s.undo() && s.undo());
    assert!(gone(&s, added[0]));
}

#[test]
fn offset_lines_circles_and_arcs_toward_the_side_point() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        circle([10.0, 0.0], 2.0),
        arc([20.0, 0.0], 2.0, 0.0, PI),
    ]);
    let offset = |s: &mut Session, h: u64, side: [f64; 2]| match run(
        s,
        EditSpec::Offset {
            target: t(h),
            distance: 0.5,
            side_point: side,
        },
    ) {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    let below = offset(&mut s, h[0], [1.0, -3.0]);
    assert_line(&s, below, [[0.0, -0.5], [4.0, -0.5]]);
    let inner = offset(&mut s, h[1], [10.5, 0.0]);
    let EntityType::Circle(c) = entity(&s, inner) else {
        panic!()
    };
    assert!((c.radius - 1.5).abs() < 1e-12 && near([c.center.x, c.center.y], [10.0, 0.0]));
    let outer = offset(&mut s, h[2], [20.0, 5.0]);
    let EntityType::Arc(a) = entity(&s, outer) else {
        panic!()
    };
    assert!((a.radius - 2.5).abs() < 1e-12);
    let message = refused(
        &mut s,
        EditSpec::Offset {
            target: t(h[1]),
            distance: 3.0,
            side_point: [10.1, 0.0],
        },
    );
    assert!(message.contains("distance"), "{message}");
}

#[test]
fn chamfer_half_a_metre_each_way_on_a_rectangle_corner() {
    let (_dir, mut s, h) = session(vec![
        polyline(
            &[
                ([0.0, 0.0], 0.0),
                ([4.0, 0.0], 0.0),
                ([4.0, 3.0], 0.0),
                ([0.0, 3.0], 0.0),
            ],
            true,
        ),
        line([10.0, 0.0], [14.0, 0.0]),
        line([14.0, 0.0], [14.0, 3.0]),
    ]);
    // The rectangle's corner at (4, 0): its two sides are picked.
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Chamfer {
            a: t(h[0]),
            b: t(h[0]),
            at_a: [2.0, 0.0],
            at_b: [4.0, 1.5],
            d1: 0.5,
            d2: 0.5,
        },
    );
    assert!(added.is_empty(), "the polyline gets the chamfer itself");
    let (v, closed) = vertices(&s, h[0]);
    assert!(closed);
    let points: Vec<_> = v.iter().map(|(p, _)| *p).collect();
    assert_eq!(points.len(), 5);
    assert!(
        near(points[1], [3.5, 0.0]) && near(points[2], [4.0, 0.5]),
        "{points:?}"
    );
    let m = measure(&mut s, h[0]);
    assert!((m.value - (12.0 - 0.125)).abs() < 1e-9, "{}", m.value);
    // Two lines: both trimmed, and a line across.
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Chamfer {
            a: t(h[1]),
            b: t(h[2]),
            at_a: [11.0, 0.0],
            at_b: [14.0, 2.0],
            d1: 0.5,
            d2: 0.5,
        },
    );
    assert_line(&s, h[1], [[10.0, 0.0], [13.5, 0.0]]);
    assert_line(&s, h[2], [[14.0, 0.5], [14.0, 3.0]]);
    assert_segment(&s, added[0], [[13.5, 0.0], [14.0, 0.5]]);
    // One step: undo puts both lines back and removes the new one.
    assert!(s.undo());
    assert_line(&s, h[1], [[10.0, 0.0], [14.0, 0.0]]);
    assert!(gone(&s, added[0]));
}

#[test]
fn fillet_two_lines_and_a_polyline_corner() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        // Short of the corner: the fillet extends it too.
        line([4.0, 0.5], [4.0, 3.0]),
        polyline(
            &[([10.0, 0.0], 0.0), ([14.0, 0.0], 0.0), ([14.0, 3.0], 0.0)],
            false,
        ),
    ]);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Fillet {
            a: t(h[0]),
            b: t(h[1]),
            at_a: [1.0, 0.0],
            at_b: [4.0, 2.0],
            radius: 1.0,
        },
    );
    assert_line(&s, h[0], [[0.0, 0.0], [3.0, 0.0]]);
    assert_line(&s, h[1], [[4.0, 1.0], [4.0, 3.0]]);
    let EntityType::Arc(a) = entity(&s, added[0]) else {
        panic!()
    };
    assert!(near([a.center.x, a.center.y], [3.0, 1.0]) && (a.radius - 1.0).abs() < 1e-12);
    assert!(
        (a.start_angle - 1.5 * PI).abs() < 1e-9 && a.end_angle.abs() < 1e-9,
        "{a:?}"
    );
    // Radius 0 on two lines meets them at their corner.
    // The polyline's corner becomes a quarter arc.
    edited(
        &mut s,
        EditSpec::Fillet {
            a: t(h[2]),
            b: t(h[2]),
            at_a: [12.0, 0.0],
            at_b: [14.0, 2.0],
            radius: 1.0,
        },
    );
    let (v, _) = vertices(&s, h[2]);
    assert_eq!(v.len(), 4);
    assert!(
        near(v[1].0, [13.0, 0.0]) && near(v[2].0, [14.0, 1.0]),
        "{v:?}"
    );
    assert!(
        (v[1].1 - (PI / 8.0).tan()).abs() < 1e-9,
        "a left turn bulges positive"
    );
}

#[test]
fn fillet_radius_zero_meets_two_lines_at_their_corner() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [3.0, 0.0]),
        line([4.0, 1.0], [4.0, 3.0]),
    ]);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Fillet {
            a: t(h[0]),
            b: t(h[1]),
            at_a: [1.0, 0.0],
            at_b: [4.0, 2.0],
            radius: 0.0,
        },
    );
    assert!(added.is_empty());
    assert_line(&s, h[0], [[0.0, 0.0], [4.0, 0.0]]);
    assert_line(&s, h[1], [[4.0, 0.0], [4.0, 3.0]]);
}

#[test]
fn blend_two_lines_with_a_spline_tangent_to_both() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [2.0, 0.0]),
        line([4.0, 2.0], [4.0, 4.0]),
    ]);
    let added = match run(
        &mut s,
        EditSpec::Blend {
            a: t(h[0]),
            b: t(h[1]),
            at_a: [1.9, 0.0],
            at_b: [4.0, 2.1],
        },
    ) {
        Outcome::Added { handles } => handles,
        other => panic!("{other:?}"),
    };
    let EntityType::Spline(sp) = entity(&s, added[0]) else {
        panic!()
    };
    let cp: Vec<[f64; 2]> = sp.control_points.iter().map(|p| [p.x, p.y]).collect();
    assert_eq!(sp.degree, 3);
    assert_eq!(cp.len(), 4);
    assert!(near(cp[0], [2.0, 0.0]) && near(cp[3], [4.0, 2.0]), "{cp:?}");
    // Leaving along +x, arriving along +y.
    assert!(cp[1][1].abs() < 1e-9 && cp[1][0] > 2.0, "{cp:?}");
    assert!((cp[2][0] - 4.0).abs() < 1e-9 && cp[2][1] < 2.0, "{cp:?}");
}

#[test]
fn pedit_close_open_width_vertices_fit_decurve_and_convert() {
    let (_dir, mut s, h) = session(vec![
        polyline(
            &[([0.0, 0.0], 0.0), ([4.0, 0.0], 0.0), ([4.0, 3.0], 0.0)],
            false,
        ),
        line([10.0, 0.0], [12.0, 0.0]),
    ]);
    let pedit = |s: &mut Session, target: u64, op: PEditOp| {
        edited(
            s,
            EditSpec::PEdit {
                target: t(target),
                op,
            },
        )
    };
    pedit(&mut s, h[0], PEditOp::Close);
    assert!(vertices(&s, h[0]).1);
    pedit(&mut s, h[0], PEditOp::Open);
    assert!(!vertices(&s, h[0]).1);
    pedit(&mut s, h[0], PEditOp::Width(0.2));
    let EntityType::LwPolyline(pl) = entity(&s, h[0]) else {
        panic!()
    };
    assert_eq!(pl.constant_width, 0.2);
    pedit(&mut s, h[0], PEditOp::AddVertex([2.0, 0.1]));
    let (v, _) = vertices(&s, h[0]);
    assert_eq!(v.len(), 4);
    assert!(
        near(v[1].0, [2.0, 0.0]),
        "on the side, nearest the point: {v:?}"
    );
    pedit(&mut s, h[0], PEditOp::DeleteVertex([2.1, 0.0]));
    assert_eq!(vertices(&s, h[0]).0.len(), 3);
    pedit(&mut s, h[0], PEditOp::FitCurve);
    let (v, _) = vertices(&s, h[0]);
    assert!(
        v.iter().any(|(_, b)| *b != 0.0),
        "arcs through the vertices: {v:?}"
    );
    let through = [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]];
    assert!(through.iter().all(|p| v.iter().any(|(q, _)| near(*p, *q))));
    pedit(&mut s, h[0], PEditOp::Decurve);
    assert!(vertices(&s, h[0]).0.iter().all(|(_, b)| *b == 0.0));
    // A line becomes a polyline under its own handle.
    pedit(&mut s, h[1], PEditOp::ConvertToPolyline);
    let (v, closed) = vertices(&s, h[1]);
    assert!(!closed && near(v[0].0, [10.0, 0.0]) && near(v[1].0, [12.0, 0.0]));
    let message = refused(
        &mut s,
        EditSpec::PEdit {
            target: t(h[1]),
            op: PEditOp::DeleteVertex([10.0, 0.0]),
        },
    );
    assert!(message.contains("vert"), "{message}");
}

#[test]
fn overkill_removes_an_exact_duplicate_and_merges_overlaps() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        line([0.0, 0.0], [4.0, 0.0]),
        // Overlapping on the same line, drawn backwards.
        line([6.0, 5.0], [2.0, 5.0]),
        line([0.0, 5.0], [3.0, 5.0]),
        circle([20.0, 0.0], 1.0),
        circle([20.0, 0.0], 1.0),
        // Apart on the same line: both stay.
        line([0.0, 9.0], [1.0, 9.0]),
        line([2.0, 9.0], [3.0, 9.0]),
    ]);
    let (added, removed, _) = edited(
        &mut s,
        EditSpec::Overkill {
            targets: ts(&h),
            tol: Some(1e-6),
        },
    );
    assert!(added.is_empty());
    assert_eq!(removed, vec![h[1], h[3], h[5]]);
    assert_segment(&s, h[2], [[0.0, 5.0], [6.0, 5.0]]);
    assert!(!gone(&s, h[6]) && !gone(&s, h[7]));
    assert!(s.undo());
    assert_eq!(sheet(&s), h);
}

#[test]
fn flatten_puts_everything_on_the_plan_facing_up() {
    let mut flipped = Arc::new();
    flipped.center = Vector3::new(-5.0, 0.0, 2.0);
    flipped.radius = 1.0;
    flipped.start_angle = 0.0;
    flipped.end_angle = FRAC_PI_2;
    flipped.normal = Vector3::new(0.0, 0.0, -1.0);
    let mut high = LwPolyline::new();
    high.add_point(Vector2::new(0.0, 0.0));
    high.add_point(Vector2::new(1.0, 0.0));
    high.elevation = 3.0;
    let (_dir, mut s, h) = session(vec![
        EntityType::Line(Line::from_coords(0.0, 0.0, 1.0, 1.0, 1.0, 2.0)),
        EntityType::Arc(flipped),
        EntityType::LwPolyline(high),
        EntityType::Ellipse(Ellipse::from_center_axes(
            Vector3::new(0.0, 0.0, 4.0),
            Vector3::new(2.0, 0.0, 0.0),
            0.5,
        )),
    ]);
    edited(&mut s, EditSpec::Flatten { targets: ts(&h) });
    let EntityType::Line(l) = entity(&s, h[0]) else {
        panic!()
    };
    assert_eq!((l.start.z, l.end.z), (0.0, 0.0));
    // The arc drawn with its normal down sits at (5, 0) in the world, from 90° round to 180°.
    let EntityType::Arc(a) = entity(&s, h[1]) else {
        panic!()
    };
    assert_eq!(a.normal, Vector3::new(0.0, 0.0, 1.0));
    assert!(
        near([a.center.x, a.center.y], [5.0, 0.0]) && a.center.z == 0.0,
        "{a:?}"
    );
    assert!((a.start_angle - FRAC_PI_2).abs() < 1e-9 && (a.end_angle - PI).abs() < 1e-9);
    let EntityType::LwPolyline(pl) = entity(&s, h[2]) else {
        panic!()
    };
    assert_eq!(pl.elevation, 0.0);
    let EntityType::Ellipse(e) = entity(&s, h[3]) else {
        panic!()
    };
    assert_eq!(e.center.z, 0.0);
}

#[test]
fn edits_refuse_what_they_cant_change_and_change_nothing() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [10.0, 0.0]),
        EntityType::Ellipse(Ellipse::from_center_axes(
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            0.5,
        )),
    ]);
    let message = refused(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[0]]),
            target: t(h[1]),
            at_point: [7.0, 0.0],
        },
    );
    assert!(message.contains("Ellipse"), "{message}");
    // Inside a block: refused as every edit of the sheet is.
    let message = refused(
        &mut s,
        EditSpec::Reverse {
            targets: vec![Target {
                handle: h[0],
                chain: vec![h[1]],
            }],
        },
    );
    assert!(message.contains("inside a block"), "{message}");
}

#[test]
fn the_take_off_follows_a_trim_in_the_same_step() {
    let (_dir, mut s, h) = session(vec![line([0.0, 0.0], [12.0, 0.0]), circle([5.0, 0.0], 2.0)]);
    let m = measure(&mut s, h[0]);
    assert!((m.value - 12.0).abs() < 1e-9);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[1]]),
            target: t(h[0]),
            at_point: [5.0, 0.0],
        },
    );
    // Both pieces left are measured: 12 less the 4 cut away.
    assert_eq!(row(&s, &m.id).handles, vec![h[0], added[0]]);
    assert!(
        (row(&s, &m.id).value - 8.0).abs() < 1e-9,
        "{}",
        row(&s, &m.id).value
    );
    assert!(s.undo());
    assert!((row(&s, &m.id).value - 12.0).abs() < 1e-9);
    assert_eq!(row(&s, &m.id).handles, vec![h[0]]);
}

#[test]
fn the_wire_form_names_the_kind() {
    let cmd: Command = serde_json::from_str(
        r#"{"type":"edit","kind":"lengthen","target":{"handle":1},"at_end":[0,0],"by":{"delta":1.5}}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        Command::Edit(EditSpec::Lengthen {
            by: LengthenBy::Delta(d),
            ..
        }) if d == 1.5
    ));
    let cmd: Command = serde_json::from_str(
        r#"{"type":"edit","kind":"pedit","target":{"handle":1},"op":{"width":0.2}}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        Command::Edit(EditSpec::PEdit {
            op: PEditOp::Width(_),
            ..
        })
    ));
    let cmd: Command = serde_json::from_str(
        r#"{"type":"edit","kind":"pedit","target":{"handle":1},"op":"close"}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        Command::Edit(EditSpec::PEdit {
            op: PEditOp::Close,
            ..
        })
    ));
}

fn row(s: &Session, id: &str) -> Measurement {
    s.takeoff
        .items
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .unwrap()
}

/// Runs an edit; how many take-off rows it unlinked and, for a join, how many measure the result.
fn rows_after(s: &mut Session, spec: EditSpec) -> (usize, usize) {
    match run(s, spec) {
        Outcome::Edited {
            unlinked,
            joined_rows,
            ..
        } => (unlinked, joined_rows),
        other => panic!("expected an edit, got {other:?}"),
    }
}

#[test]
fn an_area_row_on_a_circle_trimmed_open_is_unlinked_keeping_its_value() {
    let (_dir, mut s, h) = session(vec![circle([0.0, 0.0], 2.0), line([-3.0, 1.0], [3.0, 1.0])]);
    let m = measure(&mut s, h[0]);
    assert_eq!(m.kind, Kind::Area);
    let (unlinked, _) = rows_after(
        &mut s,
        EditSpec::Trim {
            cutters: ts(&[h[1]]),
            target: t(h[0]),
            at_point: [0.0, 2.0],
        },
    );
    assert_eq!(unlinked, 1);
    let r = row(&s, &m.id);
    assert!(r.handles.is_empty(), "no longer linked: {:?}", r.handles);
    assert!((r.value - 4.0 * PI).abs() < 1e-9, "the value stays");
    // Undo links it again.
    assert!(s.undo());
    assert_eq!(row(&s, &m.id).handles, vec![h[0]]);
}

#[test]
fn breaking_a_measured_line_at_a_point_keeps_its_length_on_both_pieces() {
    let (_dir, mut s, h) = session(vec![line([0.0, 0.0], [10.0, 0.0])]);
    let m = measure(&mut s, h[0]);
    let (added, _, _) = edited(
        &mut s,
        EditSpec::Break {
            target: t(h[0]),
            p1: [4.0, 0.0],
            p2: [4.0, 0.0],
        },
    );
    let r = row(&s, &m.id);
    assert_eq!(r.handles, vec![h[0], added[0]]);
    assert!((r.value - 10.0).abs() < 1e-9, "{}", r.value);
}

#[test]
fn joining_measured_lines_points_their_rows_at_the_joined_polyline() {
    let (_dir, mut s, h) = session(vec![
        line([0.0, 0.0], [4.0, 0.0]),
        line([4.0, 0.0], [4.0, 3.0]),
    ]);
    let first = measure(&mut s, h[0]);
    let second = measure(&mut s, h[1]);
    let (unlinked, joined_rows) = rows_after(&mut s, EditSpec::Join { targets: ts(&h) });
    assert_eq!(unlinked, 0);
    assert_eq!(joined_rows, 2, "two rows now measure it: the user decides");
    for id in [&first.id, &second.id] {
        let r = row(&s, id);
        assert_eq!(r.handles, vec![h[0]]);
        assert!((r.value - 7.0).abs() < 1e-9, "{}", r.value);
    }
    assert!(s.undo());
    assert_eq!(row(&s, &second.id).handles, vec![h[1]]);
    assert!((row(&s, &second.id).value - 3.0).abs() < 1e-9);
}

#[test]
fn chamfer_distances_follow_the_sides_as_picked() {
    let (_dir, mut s, h) = session(vec![polyline(
        &[
            ([0.0, 0.0], 0.0),
            ([4.0, 0.0], 0.0),
            ([4.0, 3.0], 0.0),
            ([0.0, 3.0], 0.0),
        ],
        true,
    )]);
    // The right side first: d1 runs up it, d2 along the bottom.
    edited(
        &mut s,
        EditSpec::Chamfer {
            a: t(h[0]),
            b: t(h[0]),
            at_a: [4.0, 1.5],
            at_b: [2.0, 0.0],
            d1: 0.5,
            d2: 0.25,
        },
    );
    let (v, _) = vertices(&s, h[0]);
    let points: Vec<_> = v.iter().map(|(p, _)| *p).collect();
    assert!(
        near(points[1], [3.75, 0.0]) && near(points[2], [4.0, 0.5]),
        "{points:?}"
    );
}
