//! Polygon selection: a polygon (concave too) takes the entities drawn wholly inside it, or with
//! crossing those it touches; always whole entities at the top of the sheet.

use qiyas_core::{Command, DrawSpec, Outcome, Picked, Session, Target};

/// An L-shaped room: the notch (4..10, 4..10) is outside it, though inside its bounding box.
const ROOM: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [10.0, 0.0],
    [10.0, 4.0],
    [4.0, 4.0],
    [4.0, 10.0],
    [0.0, 10.0],
];

fn draw(s: &mut Session, spec: DrawSpec) -> u64 {
    match s.run(Command::Draw(spec)).unwrap() {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    draw(s, DrawSpec::Line { a, b })
}

fn picked(s: &Session, points: &[[f64; 2]], crossing: bool) -> Vec<u64> {
    let picked = s.pick_polygon(points, crossing);
    for p in &picked {
        assert_eq!(p.top, p.entity);
        assert!(p.chain.is_empty() && p.xref.is_none());
    }
    picked.into_iter().map(|p: Picked| p.top).collect()
}

fn sorted(mut v: Vec<u64>) -> Vec<u64> {
    v.sort_unstable();
    v
}

#[test]
fn a_polygon_takes_what_is_inside_it_and_crossing_what_it_touches() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 6);
    let inside = line(&mut s, [1.0, 1.0], [3.0, 1.0]);
    let circle = draw(
        &mut s,
        DrawSpec::Circle {
            centre: [2.0, 2.0],
            radius: 0.5,
        },
    );
    let in_notch = line(&mut s, [6.0, 6.0], [8.0, 6.0]);
    let into_notch = line(&mut s, [2.0, 8.0], [8.0, 8.0]);
    // Both ends inside the room, the middle across the notch.
    let across_notch = line(&mut s, [3.0, 6.0], [6.0, 3.0]);
    let far = line(&mut s, [20.0, 20.0], [21.0, 21.0]);
    let _ = far;

    assert_eq!(picked(&s, &ROOM, false), sorted(vec![inside, circle]));
    assert_eq!(
        picked(&s, &ROOM, true),
        sorted(vec![inside, circle, into_notch, across_notch])
    );
    assert!(!picked(&s, &ROOM, true).contains(&in_notch));

    // Either way round, the same.
    let mut reversed = ROOM;
    reversed.reverse();
    assert_eq!(picked(&s, &reversed, true), picked(&s, &ROOM, true));

    // Fewer than three corners is no polygon.
    assert!(picked(&s, &ROOM[..2], true).is_empty());
}

#[test]
fn a_block_reference_is_picked_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 6);
    let a = line(&mut s, [1.0, 1.0], [2.0, 1.0]);
    let b = line(&mut s, [1.0, 2.0], [2.0, 2.0]);
    let out = s
        .run(Command::CreateBlock {
            name: "PAIR".into(),
            base: [1.0, 1.0],
            targets: [a, b]
                .map(|handle| Target {
                    handle,
                    chain: vec![],
                })
                .to_vec(),
            convert_to_insert: true,
        })
        .unwrap();
    let _ = out;
    let picked = s.pick_polygon(&ROOM, false);
    assert_eq!(picked.len(), 1, "{picked:?}");
    assert_eq!(picked[0].kind.as_deref(), Some("Insert"));
}

#[test]
fn units_do_not_matter() {
    // The same room in millimetres.
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 4);
    let mm = ROOM.map(|[x, y]| [x * 1000.0, y * 1000.0]);
    let inside = line(&mut s, [1000.0, 1000.0], [3000.0, 1000.0]);
    let across = line(&mut s, [3000.0, 6000.0], [6000.0, 3000.0]);
    assert_eq!(picked(&s, &mm, false), vec![inside]);
    assert_eq!(picked(&s, &mm, true), sorted(vec![inside, across]));
}
