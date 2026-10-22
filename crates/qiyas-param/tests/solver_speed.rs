//! Release only: 200 constraints in one connected sketch, relative to a fixed CPU reference. Run by
//! `scripts/check.sh` (`cargo test -p qiyas-param --release --test solver_speed`).
#![cfg(not(debug_assertions))]

use std::time::Instant;

use qiyas_param::{solve, Constraint, Sketch};

/// A staircase of 50 lines, each a little off: 49 coincident corners, 50 horizontal or vertical,
/// 50 lengths, 48 parallels, one fixed start and two equals: 200 constraints, one component.
fn staircase() -> Sketch {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        ((seed >> 40) as f64 / (1u64 << 24) as f64 - 0.5) * 0.05
    };
    let mut s = Sketch::default();
    let mut at = [0.0, 0.0];
    let mut lines = Vec::new();
    for i in 0..50 {
        let step = if i % 2 == 0 { [2.0, 0.0] } else { [0.0, 1.0] };
        let end = [at[0] + step[0], at[1] + step[1]];
        lines.push(s.line(
            [at[0] + noise(), at[1] + noise()],
            [end[0] + noise(), end[1] + noise()],
        ));
        at = end;
    }
    for i in 0..49 {
        let (_, end) = s.line_ends(lines[i]).unwrap();
        let (start, _) = s.line_ends(lines[i + 1]).unwrap();
        s.constrain(Constraint::Coincident(end, start));
    }
    for (i, &l) in lines.iter().enumerate() {
        s.constrain(if i % 2 == 0 {
            Constraint::Horizontal(l)
        } else {
            Constraint::Vertical(l)
        });
        let (a, b) = s.line_ends(l).unwrap();
        let value = if i % 2 == 0 { 2.0 } else { 1.0 };
        s.constrain(Constraint::Distance {
            a,
            b,
            axis: None,
            value,
        });
    }
    for i in 0..48 {
        s.constrain(Constraint::Parallel(lines[i], lines[i + 2]));
    }
    let (first, _) = s.line_ends(lines[0]).unwrap();
    s.constrain(Constraint::Fixed(first, [0.0, 0.0]));
    s.constrain(Constraint::Equal(lines[0], lines[2]));
    s.constrain(Constraint::Equal(lines[1], lines[3]));
    assert_eq!(s.constraints.len(), 200);
    s
}

#[test]
fn two_hundred_constraints_solve_within_reference_ratio() {
    let reference = performance::reference();
    let mut s = staircase();
    let t = Instant::now();
    let solved = solve(&mut s, &[]).unwrap();
    let first = t.elapsed();
    performance::assert_relative(first, reference, 0.125, "200 constraints");
    assert!(solved.iterations > 0);
    // Line 10's length changes; the staircase follows.
    if let Constraint::Distance { value, .. } = &mut s.constraints[70] {
        *value = 3.0;
    }
    let t = Instant::now();
    solve(&mut s, &[]).unwrap();
    let again = t.elapsed();
    performance::assert_relative(again, reference, 0.125, "200 constraints re-solve");
    println!("first solve {first:?}, re-solve {again:?}");
}

#[path = "../../../tests/support/performance.rs"]
mod performance;
