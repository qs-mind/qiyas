//! The solver: each geometric constraint holds after a solve, points pinned stay put, conflicting
//! constraints are refused with the sketch as it was, and Review Focus 4: a rectangle of four
//! lines (4 coincident, 2 horizontal, 2 vertical) with a width constraint stays a rectangle when
//! the width changes, solved in under 50 ms.

use std::time::Instant;

use qiyas_param::{solve, Constraint, Curve, Sketch, SolveError};

const TOL: f64 = 1e-7;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    close(a[0], b[0]) && close(a[1], b[1])
}

/// Four separate lines round (0, 0) to (4, 3), a little off square, constrained into a
/// rectangle; the bottom line's width is `width`. The lines, bottom first, counter-clockwise.
fn rectangle(width: f64) -> (Sketch, [usize; 4]) {
    let mut s = Sketch::default();
    let bottom = s.line([0.0, 0.0], [4.0, 0.01]);
    let right = s.line([4.0, 0.0], [4.02, 3.0]);
    let top = s.line([4.0, 3.0], [0.0, 2.98]);
    let left = s.line([0.01, 3.0], [0.0, 0.0]);
    let lines = [bottom, right, top, left];
    for i in 0..4 {
        let (_, end) = s.line_ends(lines[i]).unwrap();
        let (start, _) = s.line_ends(lines[(i + 1) % 4]).unwrap();
        s.constrain(Constraint::Coincident(end, start));
    }
    s.constrain(Constraint::Horizontal(bottom));
    s.constrain(Constraint::Horizontal(top));
    s.constrain(Constraint::Vertical(right));
    s.constrain(Constraint::Vertical(left));
    let (a, b) = s.line_ends(bottom).unwrap();
    s.constrain(Constraint::Distance {
        a,
        b,
        axis: Some([1.0, 0.0]),
        value: width,
    });
    (s, lines)
}

/// The corners of the rectangle's lines, bottom left first, counter-clockwise; panics unless the
/// four lines still make a rectangle with sides along the axes.
fn corners(s: &Sketch, lines: [usize; 4]) -> [[f64; 2]; 4] {
    let ends: Vec<([f64; 2], [f64; 2])> = lines
        .iter()
        .map(|&l| {
            let (a, b) = s.line_ends(l).unwrap();
            (s.points[a], s.points[b])
        })
        .collect();
    for i in 0..4 {
        assert!(
            same(ends[i].1, ends[(i + 1) % 4].0),
            "corner {i} is open: {ends:?}"
        );
    }
    for (i, (a, b)) in ends.iter().enumerate() {
        let along_x = i % 2 == 0;
        if along_x {
            assert!(close(a[1], b[1]), "line {i} is not horizontal: {a:?} {b:?}");
        } else {
            assert!(close(a[0], b[0]), "line {i} is not vertical: {a:?} {b:?}");
        }
    }
    [ends[0].0, ends[1].0, ends[2].0, ends[3].0]
}

#[test]
fn review_focus_4_a_rectangle_stays_a_rectangle_when_its_width_changes() {
    let (mut s, lines) = rectangle(4.0);
    solve(&mut s, &[]).unwrap();
    let c = corners(&s, lines);
    assert!(close(c[1][0] - c[0][0], 4.0), "{c:?}");

    // The width constraint goes from 4 to 6; the first point of the width stays where it is.
    let Constraint::Distance { a, .. } = s.constraints[8] else {
        panic!("the width is the last constraint")
    };
    let before = c;
    if let Constraint::Distance { value, .. } = &mut s.constraints[8] {
        *value = 6.0;
    }
    let reference = performance::reference();
    let t = Instant::now();
    solve(&mut s, &[a]).unwrap();
    let took = t.elapsed();
    let c = corners(&s, lines);
    assert!(close(c[1][0] - c[0][0], 6.0), "width {c:?}");
    assert!(
        close(c[2][1] - c[1][1], before[2][1] - before[1][1]),
        "height {c:?}"
    );
    assert!(same(c[0], before[0]), "the pinned corner moved: {c:?}");
    performance::assert_relative(took, reference, 0.125, "rectangle solve");
}

#[test]
fn each_geometric_constraint_holds_after_a_solve() {
    let mut s = Sketch::default();
    let l1 = s.line([0.0, 0.0], [5.0, 0.3]);
    let l2 = s.line([0.0, 2.0], [5.0, 2.6]);
    s.constrain(Constraint::Parallel(l1, l2));
    let l3 = s.line([10.0, 0.0], [15.0, 0.4]);
    let l4 = s.line([10.0, 0.0], [10.5, 5.0]);
    s.constrain(Constraint::Perpendicular(l3, l4));
    let l5 = s.line([20.0, 0.0], [25.0, 0.0]);
    let l6 = s.line([27.0, 0.2], [30.0, -0.1]);
    s.constrain(Constraint::Collinear(l5, l6));
    let c1 = s.circle([40.0, 0.0], 2.0);
    let c2 = s.circle([40.3, 0.2], 1.0);
    s.constrain(Constraint::Concentric(c1, c2));
    let c3 = s.circle([50.0, 0.0], 2.0);
    let l7 = s.line([45.0, 2.2], [55.0, 2.1]);
    s.constrain(Constraint::Tangent(l7, c3));
    let c4 = s.circle([60.0, 0.0], 2.0);
    let c5 = s.circle([63.2, 0.0], 1.0);
    s.constrain(Constraint::Tangent(c4, c5));
    let l8 = s.line([70.0, 0.0], [74.0, 0.0]);
    let l9 = s.line([70.0, 3.0], [72.0, 4.0]);
    s.constrain(Constraint::Equal(l8, l9));
    let mirror = s.line([80.0, -5.0], [80.0, 5.0]);
    let p = s.point([78.0, 1.0]);
    let q = s.point([82.5, 1.3]);
    s.constrain(Constraint::Symmetric(p, q, mirror));
    let centre = s.point([90.0, 0.0]);
    let start = s.point([93.0, 0.0]);
    let end = s.point([90.0, 2.8]);
    let arc = s.curve(Curve::Arc { centre, start, end });
    s.constrain(Constraint::Radius {
        curve: arc,
        value: 2.5,
    });
    let l10 = s.line([100.0, 0.0], [105.0, 0.0]);
    let l11 = s.line([100.0, 0.0], [104.0, 3.5]);
    s.constrain(Constraint::Angle {
        first: l10,
        second: l11,
        value: 30f64.to_radians(),
    });

    solve(&mut s, &[]).unwrap();

    let dir = |l: usize| {
        let (a, b) = s.line_ends(l).unwrap();
        let (a, b) = (s.points[a], s.points[b]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let n = dx.hypot(dy);
        ([dx / n, dy / n], a, n)
    };
    let cross = |u: [f64; 2], v: [f64; 2]| u[0] * v[1] - u[1] * v[0];
    let dot = |u: [f64; 2], v: [f64; 2]| u[0] * v[0] + u[1] * v[1];
    assert!(close(cross(dir(l1).0, dir(l2).0), 0.0), "parallel");
    assert!(close(dot(dir(l3).0, dir(l4).0), 0.0), "perpendicular");
    let (u5, a5, _) = dir(l5);
    let (_, a6, _) = dir(l6);
    let (u6, ..) = dir(l6);
    assert!(close(cross(u5, u6), 0.0), "collinear: parallel");
    assert!(
        close(cross(u5, [a6[0] - a5[0], a6[1] - a5[1]]), 0.0),
        "collinear: on one line"
    );
    let centre_of = |c: usize| match s.curves[c] {
        Curve::Circle { centre, .. } | Curve::Arc { centre, .. } => s.points[centre],
        Curve::Line { .. } => unreachable!(),
    };
    assert!(same(centre_of(c1), centre_of(c2)), "concentric");
    let (u7, a7, _) = dir(l7);
    let c = centre_of(c3);
    let gap = cross(u7, [c[0] - a7[0], c[1] - a7[1]]).abs();
    assert!(close(gap, s.radius_of(c3).unwrap()), "line tangent: {gap}");
    let (k4, k5) = (centre_of(c4), centre_of(c5));
    let apart = (k5[0] - k4[0]).hypot(k5[1] - k4[1]);
    let sum = s.radius_of(c4).unwrap() + s.radius_of(c5).unwrap();
    assert!(close(apart, sum), "circles tangent outside: {apart} {sum}");
    assert!(close(dir(l8).2, dir(l9).2), "equal");
    let (m, a, _) = dir(mirror);
    let (p, q) = (s.points[p], s.points[q]);
    let mid = [(p[0] + q[0]) / 2.0 - a[0], (p[1] + q[1]) / 2.0 - a[1]];
    assert!(close(cross(m, mid), 0.0), "symmetric: middle on the line");
    assert!(
        close(dot(m, [q[0] - p[0], q[1] - p[1]]), 0.0),
        "symmetric: across"
    );
    let Curve::Arc { centre, start, end } = s.curves[arc] else {
        unreachable!()
    };
    let (c, a, b) = (s.points[centre], s.points[start], s.points[end]);
    assert!(close((a[0] - c[0]).hypot(a[1] - c[1]), 2.5), "arc radius");
    assert!(
        close((b[0] - c[0]).hypot(b[1] - c[1]), 2.5),
        "arc ends on one circle"
    );
    let angle = dot(dir(l10).0, dir(l11).0).acos();
    assert!(
        close(angle, 30f64.to_radians()),
        "angle {}",
        angle.to_degrees()
    );
}

#[test]
fn pinned_points_and_fixed_points_stay_where_they_are() {
    let mut s = Sketch::default();
    let l = s.line([0.0, 0.0], [3.0, 1.0]);
    let (a, b) = s.line_ends(l).unwrap();
    s.constrain(Constraint::Horizontal(l));
    solve(&mut s, &[b]).unwrap();
    assert!(same(s.points[b], [3.0, 1.0]));
    assert!(close(s.points[a][1], 1.0));

    let mut s = Sketch::default();
    let l = s.line([0.0, 0.0], [3.0, 1.0]);
    let (a, _) = s.line_ends(l).unwrap();
    s.constrain(Constraint::Fixed(a, [0.0, 0.0]));
    s.constrain(Constraint::Vertical(l));
    solve(&mut s, &[]).unwrap();
    assert!(same(s.points[a], [0.0, 0.0]));
}

#[test]
fn conflicting_constraints_are_refused_and_the_sketch_is_left_as_it_was() {
    let mut s = Sketch::default();
    let l = s.line([0.0, 0.0], [3.0, 0.0]);
    let (a, b) = s.line_ends(l).unwrap();
    s.constrain(Constraint::Fixed(a, [0.0, 0.0]));
    s.constrain(Constraint::Fixed(b, [3.0, 0.0]));
    s.constrain(Constraint::Distance {
        a,
        b,
        axis: None,
        value: 5.0,
    });
    let before = s.clone();
    let err = solve(&mut s, &[]).unwrap_err();
    assert!(matches!(err, SolveError::Conflict { .. }), "{err:?}");
    assert_eq!(s, before);
}

#[test]
fn a_constraint_on_the_wrong_curve_is_refused() {
    let mut s = Sketch::default();
    let c = s.circle([0.0, 0.0], 1.0);
    s.constrain(Constraint::Horizontal(c));
    assert!(matches!(solve(&mut s, &[]), Err(SolveError::Invalid(_))));
}

#[test]
fn a_one_metre_rectangle_far_from_the_origin_solves_as_one_at_the_origin() {
    // A site grid in millimetres puts details billions of units from the origin.
    let (x0, y0) = (3.3e9, 1.2e9);
    let mut s = Sketch::default();
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let lines: Vec<usize> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            s.line([x0 + a[0], y0 + a[1]], [x0 + b[0] + 0.001, y0 + b[1]])
        })
        .collect();
    for i in 0..4 {
        let (_, end) = s.line_ends(lines[i]).unwrap();
        let (start, _) = s.line_ends(lines[(i + 1) % 4]).unwrap();
        s.constrain(Constraint::Coincident(end, start));
    }
    s.constrain(Constraint::Horizontal(lines[0]));
    s.constrain(Constraint::Horizontal(lines[2]));
    s.constrain(Constraint::Perpendicular(lines[0], lines[1]));
    s.constrain(Constraint::Parallel(lines[1], lines[3]));
    let (a, b) = s.line_ends(lines[0]).unwrap();
    s.constrain(Constraint::Distance {
        a,
        b,
        axis: Some([1.0, 0.0]),
        value: 0.25,
    });
    s.constrain(Constraint::Fixed(a, [x0, y0]));
    solve(&mut s, &[]).unwrap();
    let c = corners_of(&s, &lines);
    // Within a micrometre (f64 holds about 1e-6 at 3e9).
    assert!((c[1][0] - c[0][0] - 0.25).abs() < 1e-5, "{c:?}");
    assert!(
        (c[3][0] - c[0][0]).abs() < 1e-5,
        "the left side stays upright: {c:?}"
    );
    assert!(
        (c[0][0] - x0).abs() < 1e-5 && (c[0][1] - y0).abs() < 1e-5,
        "{c:?}"
    );
}

/// Each line's start.
fn corners_of(s: &Sketch, lines: &[usize]) -> Vec<[f64; 2]> {
    lines
        .iter()
        .map(|&l| s.points[s.line_ends(l).unwrap().0])
        .collect()
}

#[path = "../../../tests/support/performance.rs"]
mod performance;
