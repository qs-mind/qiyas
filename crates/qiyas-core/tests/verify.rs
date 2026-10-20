//! Intent verification measures ordinary geometry in every drawing unit and never edits it.

use opencadcodec::{EntityType, Handle};
use qiyas_core::{
    Command, DrawSpec, Expectation, Outcome, Session, Target, VerificationResult, VerifyKind,
};
use serde_json::json;

fn blank(units: i16) -> (Session, f64) {
    let path = std::env::temp_dir().join("qiyas-verify-test.dwg");
    let session = Session::new_blank(&path, units);
    let per_metre = 1.0 / session.drawing.units.to_metres();
    (session, per_metre)
}

fn draw(session: &mut Session, spec: DrawSpec) -> u64 {
    let Outcome::Added { handles } = session.run(Command::Draw(spec)).unwrap() else {
        panic!("expected drawn entities");
    };
    handles[0]
}

fn line(session: &mut Session, k: f64, a: [f64; 2], b: [f64; 2]) -> u64 {
    draw(
        session,
        DrawSpec::Line {
            a: a.map(|v| v * k),
            b: b.map(|v| v * k),
        },
    )
}

fn circle(session: &mut Session, k: f64, centre: [f64; 2], radius: f64) -> u64 {
    draw(
        session,
        DrawSpec::Circle {
            centre: centre.map(|v| v * k),
            radius: radius * k,
        },
    )
}

fn polyline(session: &mut Session, k: f64, points: &[[f64; 2]], closed: bool) -> u64 {
    draw(
        session,
        DrawSpec::Polyline {
            points: points.iter().map(|p| p.map(|v| v * k)).collect(),
            bulges: vec![],
            closed,
        },
    )
}

fn expectation(kind: VerifyKind, handles: &[u64], value: Option<f64>) -> Expectation {
    Expectation {
        kind,
        targets: handles
            .iter()
            .map(|&handle| Target {
                handle,
                chain: vec![],
            })
            .collect(),
        points: vec![].into(),
        value,
        tolerance: None,
        in_metres: false,
        bounds: None,
        layer: None,
        entity_type: None,
        block: None,
    }
}

fn picked(mut e: Expectation, k: f64, points: &[[f64; 2]]) -> Expectation {
    e.points = points.iter().map(|p| p.map(|v| v * k)).collect();
    e
}

fn verify(session: &mut Session, expectations: Vec<Expectation>) -> Vec<VerificationResult> {
    let Outcome::Verified { results } = session.run(Command::Verify { expectations }).unwrap()
    else {
        panic!("expected verification results");
    };
    results
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
        "{actual} differs from {expected}"
    );
}

#[test]
fn lengths_distances_angles_radii_and_areas_use_the_requested_units() {
    use VerifyKind::*;
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let diagonal = line(&mut s, k, [0.0, 0.0], [3.0, 4.0]);
        let horizontal = line(&mut s, k, [0.0, 0.0], [4.0, 0.0]);
        let vertical = line(&mut s, k, [0.0, 0.0], [0.0, 3.0]);
        let round = circle(&mut s, k, [10.0, 10.0], 2.0);
        let rect = draw(
            &mut s,
            DrawSpec::Rectangle {
                a: [0.0, 0.0],
                b: [4.0 * k, 3.0 * k],
            },
        );
        let arc = draw(
            &mut s,
            DrawSpec::Arc {
                centre: [20.0 * k, 0.0],
                radius: 2.0 * k,
                start: 0.0,
                end: 90.0,
            },
        );
        for in_metres in [false, true] {
            let scale = if in_metres { 1.0 } else { k };
            let mut expectations = vec![
                expectation(Length, &[diagonal], Some(5.0 * scale)),
                expectation(Length, &[rect], Some(14.0 * scale)),
                expectation(Length, &[arc], Some(std::f64::consts::PI * scale)),
                picked(
                    expectation(Distance, &[], Some(5.0 * scale)),
                    k,
                    &[[0.0, 0.0], [3.0, 4.0]],
                ),
                expectation(Radius, &[round], Some(2.0 * scale)),
                expectation(Radius, &[arc], Some(2.0 * scale)),
                expectation(Area, &[rect], Some(12.0 * scale * scale)),
                expectation(
                    Area,
                    &[round],
                    Some(4.0 * std::f64::consts::PI * scale * scale),
                ),
                expectation(Angle, &[horizontal, vertical], Some(90.0)),
            ];
            for e in &mut expectations {
                e.in_metres = in_metres;
            }
            for result in verify(&mut s, expectations) {
                assert!(result.passed, "{units}, metres={in_metres}: {result:?}");
                close(result.measured.unwrap(), result.expected);
                let tolerance = match result.kind {
                    Angle => 0.1,
                    Area => (0.0005 * scale).powi(2),
                    _ => 0.0005 * scale,
                };
                close(result.tolerance, tolerance);
            }
        }
    }
}

#[test]
fn geometric_relationships_pass_and_report_measured_failures_in_every_unit() {
    use VerifyKind::*;
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let h = line(&mut s, k, [0.0, 0.0], [4.0, 0.0]);
        let parallel = line(&mut s, k, [0.0, 2.0], [4.0, 2.0]);
        let v = line(&mut s, k, [0.0, 0.0], [0.0, 4.0]);
        let continued = line(&mut s, k, [6.0, 0.0], [10.0, 0.0]);
        let short = line(&mut s, k, [0.0, 6.0], [1.0, 6.0]);
        let c1 = circle(&mut s, k, [10.0, 10.0], 2.0);
        let c2 = circle(&mut s, k, [10.0, 10.0], 3.0);
        let c3 = circle(&mut s, k, [20.0, 10.0], 2.0);
        let left = circle(&mut s, k, [-2.0, 2.0], 0.25);
        let right = circle(&mut s, k, [2.0, 2.0], 0.25);
        let wrong_right = circle(&mut s, k, [3.0, 2.0], 0.25);
        let axis = line(&mut s, k, [0.0, -1.0], [0.0, 5.0]);
        let pass = vec![
            expectation(Parallel, &[h, parallel], None),
            expectation(Perpendicular, &[h, v], None),
            expectation(Collinear, &[h, continued], None),
            expectation(Horizontal, &[h], None),
            expectation(Vertical, &[v], None),
            expectation(Concentric, &[c1, c2], None),
            expectation(Equal, &[h, v], None),
            expectation(Equal, &[c1, c3], None),
            picked(
                expectation(Coincident, &[h, v], None),
                k,
                &[[0.0, 0.0], [0.0, 0.0]],
            ),
            picked(
                expectation(Symmetric, &[left, right, axis], None),
                k,
                &[[-2.0, 2.0], [2.0, 2.0], [0.0, 0.0]],
            ),
            picked(
                expectation(Distance, &[h, v], Some(32.0_f64.sqrt() * k)),
                k,
                &[[4.0, 0.0], [0.0, 4.0]],
            ),
            picked(
                expectation(Horizontal, &[left, right], None),
                k,
                &[[-2.0, 2.0], [2.0, 2.0]],
            ),
        ];
        for result in verify(&mut s, pass) {
            assert!(result.passed, "{units}: {result:?}");
        }
        let failures = vec![
            (expectation(Parallel, &[h, v], None), 90.0),
            (expectation(Perpendicular, &[h, parallel], None), 90.0),
            (expectation(Horizontal, &[v], None), 90.0),
            (expectation(Vertical, &[h], None), 90.0),
            (expectation(Collinear, &[h, parallel], None), 2.0 * k),
            (expectation(Concentric, &[c1, c3], None), 10.0 * k),
            (expectation(Equal, &[h, short], None), 3.0 * k),
            (
                picked(
                    expectation(Coincident, &[h, v], None),
                    k,
                    &[[4.0, 0.0], [0.0, 4.0]],
                ),
                32.0_f64.sqrt() * k,
            ),
            (
                picked(
                    expectation(Symmetric, &[left, wrong_right, axis], None),
                    k,
                    &[[-2.0, 2.0], [3.0, 2.0], [0.0, 0.0]],
                ),
                k,
            ),
        ];
        for (e, measured) in failures {
            let result = verify(&mut s, vec![e]).remove(0);
            assert!(!result.passed, "{units}: {result:?}");
            close(result.measured.unwrap(), measured);
            assert!(result.message.contains("measured"), "{result:?}");
            assert!(result.message.contains("tolerance"), "{result:?}");
        }
    }
}

#[test]
fn part_picks_measure_polyline_edges_and_vertices_without_adding_constraints() {
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let p = polyline(&mut s, k, &[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]], false);
        let expectations = vec![
            picked(
                expectation(VerifyKind::Length, &[p], Some(4.0 * k)),
                k,
                &[[2.0, 0.0]],
            ),
            picked(
                expectation(VerifyKind::Length, &[p], Some(3.0 * k)),
                k,
                &[[4.0, 2.0]],
            ),
            picked(
                expectation(VerifyKind::Distance, &[p, p], Some(5.0 * k)),
                k,
                &[[0.0, 0.0], [4.0, 3.0]],
            ),
            picked(
                expectation(VerifyKind::Perpendicular, &[p, p], None),
                k,
                &[[2.0, 0.0], [4.0, 2.0]],
            ),
        ];
        let before = s.drawing.doc.clone();
        for result in verify(&mut s, expectations) {
            assert!(result.passed, "{units}: {result:?}");
        }
        assert_eq!(s.drawing.doc, before);
    }
}

#[test]
fn tangency_requires_contact_on_both_the_arc_sweep_and_the_line_segment() {
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let arc = draw(
            &mut s,
            DrawSpec::Arc {
                centre: [0.0, 0.0],
                radius: 2.0 * k,
                start: 0.0,
                end: 90.0,
            },
        );
        let touches = line(&mut s, k, [2.0, -1.0], [2.0, 1.0]);
        let missing_sweep = line(&mut s, k, [-2.0, -1.0], [-2.0, 1.0]);
        let too_short = line(&mut s, k, [2.0, 3.0], [2.0, 4.0]);
        let outer = circle(&mut s, k, [4.0, 0.0], 2.0);
        let pass = verify(
            &mut s,
            vec![
                expectation(VerifyKind::Tangent, &[arc, touches], None),
                expectation(VerifyKind::Tangent, &[arc, outer], None),
            ],
        );
        assert!(pass.iter().all(|r| r.passed), "{units}: {pass:?}");
        let failures = verify(
            &mut s,
            vec![
                expectation(VerifyKind::Tangent, &[arc, missing_sweep], None),
                expectation(VerifyKind::Tangent, &[arc, too_short], None),
            ],
        );
        assert!(failures.iter().all(|r| !r.passed), "{units}: {failures:?}");
        close(failures[0].measured.unwrap(), 8.0_f64.sqrt() * k);
        close(failures[1].measured.unwrap(), 3.0 * k);
    }
}

#[test]
fn a_plane_outline_is_verified_as_general_geometry_in_every_unit() {
    use VerifyKind::*;
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let outline = polyline(
            &mut s,
            k,
            &[
                [0.0, 5.0],
                [0.5, 1.0],
                [4.0, -1.0],
                [4.0, -1.5],
                [0.5, -0.5],
                [0.5, -4.0],
                [1.5, -5.0],
                [0.0, -4.5],
                [-1.5, -5.0],
                [-0.5, -4.0],
                [-0.5, -0.5],
                [-4.0, -1.5],
                [-4.0, -1.0],
                [-0.5, 1.0],
            ],
            true,
        );
        let axis = line(&mut s, k, [0.0, -6.0], [0.0, 6.0]);
        let mut bounds = expectation(InsideBounds, &[outline], None);
        bounds.bounds = Some([[-4.0 * k, -5.0 * k], [4.0 * k, 5.0 * k]]);
        let results = verify(
            &mut s,
            vec![
                expectation(Symmetric, &[outline, axis], None),
                expectation(Closed, &[outline], None),
                expectation(NoSelfIntersection, &[outline], None),
                bounds,
            ],
        );
        assert!(results.iter().all(|r| r.passed), "{units}: {results:?}");

        let EntityType::LwPolyline(poly) =
            s.drawing.doc.get_entity_mut(Handle::new(outline)).unwrap()
        else {
            panic!("expected a polyline");
        };
        poly.vertices[2].location.x += 0.1 * k;
        let result = verify(&mut s, vec![expectation(Symmetric, &[outline, axis], None)]).remove(0);
        assert!(!result.passed, "{units}: {result:?}");
        close(result.measured.unwrap(), 0.1 * k);
    }
}

#[test]
fn closure_bounds_and_self_intersection_report_the_deviation() {
    use VerifyKind::*;
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let open = polyline(
            &mut s,
            k,
            &[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]],
            false,
        );
        let crossed = polyline(
            &mut s,
            k,
            &[[0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]],
            true,
        );
        let doubled = polyline(&mut s, k, &[[0.0, 0.0], [2.0, 0.0], [1.0, 0.0]], false);
        let round = circle(&mut s, k, [0.0, 0.0], 1.0);
        let mut bounds = expectation(InsideBounds, &[open], None);
        bounds.bounds = Some([[0.0, 0.0], [1.5 * k, 2.0 * k]]);
        let results = verify(
            &mut s,
            vec![
                expectation(Closed, &[open], None),
                bounds,
                expectation(NoSelfIntersection, &[crossed], None),
                expectation(NoSelfIntersection, &[doubled], None),
                expectation(NoSelfIntersection, &[round], None),
                expectation(Closed, &[round], None),
            ],
        );
        for result in &results[..4] {
            assert!(!result.passed, "{units}: {result:?}");
        }
        close(results[0].measured.unwrap(), 2.0 * k);
        close(results[1].measured.unwrap(), 0.5 * k);
        assert_eq!(results[2].measured, Some(1.0));
        assert_eq!(results[3].measured, Some(1.0));
        assert_eq!(results[2].tolerance, 0.0);
        assert!(results[4].passed, "{units}: {:?}", results[4]);
        assert!(results[5].passed, "{units}: {:?}", results[5]);
    }
}

#[test]
fn count_filters_cover_layers_types_block_references_and_zero_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        line(&mut s, k, [0.0, 0.0], [1.0, 0.0]);
        s.run(Command::NewLayer {
            name: "HOLES".into(),
        })
        .unwrap();
        let mut current = s.current.clone();
        current.layer = "HOLES".into();
        s.run(Command::SetCurrent { current }).unwrap();
        circle(&mut s, k, [2.0, 2.0], 0.5);
        let mut current = s.current.clone();
        current.layer = "0".into();
        s.run(Command::SetCurrent { current }).unwrap();
        for value in [
            json!({"type":"define_block", "name":"MARKER", "base":[0,0], "entities":[
                {"type":"Line", "data":{"start":{"x":0,"y":0,"z":0},"end":{"x":k,"y":0,"z":0}}}
            ]}),
            json!({"type":"insert_block", "name":"MARKER", "at":[0,0]}),
            json!({"type":"insert_block", "name":"MARKER", "at":[k,0]}),
        ] {
            s.run(serde_json::from_value(value).unwrap()).unwrap();
        }
        let mut layer = expectation(VerifyKind::Count, &[], Some(1.0));
        layer.layer = Some("holes".into());
        let mut kind = expectation(VerifyKind::Count, &[], Some(2.0));
        kind.entity_type = Some("insert".into());
        let mut block = expectation(VerifyKind::Count, &[], Some(2.0));
        block.block = Some("marker".into());
        block.layer = Some("0".into());
        let mut zero = expectation(VerifyKind::Count, &[], Some(0.0));
        zero.block = Some("ABSENT".into());
        let results = verify(
            &mut s,
            vec![
                expectation(VerifyKind::Count, &[], Some(4.0)),
                layer,
                kind,
                block,
                zero,
            ],
        );
        assert!(
            results.iter().all(|r| r.passed && r.tolerance == 0.0),
            "{units}: {results:?}"
        );
        let result = verify(&mut s, vec![expectation(VerifyKind::Count, &[], Some(3.0))]).remove(0);
        assert!(!result.passed);
        assert_eq!(result.measured, Some(4.0));
    }
}

#[test]
fn default_and_explicit_tolerances_are_applied_to_measured_values() {
    use VerifyKind::*;
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let h = line(&mut s, k, [0.0, 0.0], [1.0, 0.0]);
        let angle = 0.05_f64.to_radians();
        let almost = line(&mut s, k, [0.0, 1.0], [angle.cos(), 1.0 + angle.sin()]);
        let angle = 0.2_f64.to_radians();
        let off = line(&mut s, k, [0.0, 2.0], [angle.cos(), 2.0 + angle.sin()]);
        let mut explicit = expectation(Length, &[h], Some(1.001 * k));
        explicit.tolerance = Some(0.002 * k);
        let results = verify(
            &mut s,
            vec![
                expectation(Length, &[h], Some(1.0004 * k)),
                expectation(Length, &[h], Some(1.0006 * k)),
                expectation(Horizontal, &[almost], None),
                expectation(Horizontal, &[off], None),
                explicit,
            ],
        );
        assert_eq!(
            results.iter().map(|r| r.passed).collect::<Vec<_>>(),
            [true, false, true, false, true],
            "{units}: {results:?}"
        );
        close(results[0].tolerance, 0.0005 * k);
        close(results[1].measured.unwrap(), k);
        close(results[2].measured.unwrap(), 0.05);
        close(results[3].measured.unwrap(), 0.2);
        assert_eq!(results[2].tolerance, 0.1);
    }
}

#[test]
fn verification_reads_locked_layers_and_keeps_document_history_and_current_state() {
    let (mut s, _) = blank(6);
    let h = line(&mut s, 1.0, [0.0, 0.0], [3.0, 4.0]);
    s.drawing.doc.layers.get_mut("0").unwrap().flags.locked = true;
    let before_doc = s.drawing.doc.clone();
    let before_history = s.history_steps();
    let before_revision = s.history_revision();
    let before_current = s.current.clone();
    let before_dirty = s.is_dirty();
    let before_takeoff = serde_json::to_value(&s.takeoff).unwrap();
    let mut bad = expectation(VerifyKind::Length, &[h], Some(5.0));
    bad.tolerance = Some(-1.0);
    let results = verify(
        &mut s,
        vec![
            expectation(VerifyKind::Length, &[h], Some(5.0)),
            expectation(VerifyKind::Length, &[u64::MAX], Some(1.0)),
            bad,
            expectation(VerifyKind::Radius, &[h], Some(1.0)),
            expectation(VerifyKind::Length, &[h], None),
            expectation(VerifyKind::Length, &[h], Some(5.0)),
        ],
    );
    assert!(results[0].passed && results[5].passed);
    assert!(results[1..5]
        .iter()
        .all(|r| !r.passed && r.measured.is_none() && !r.message.is_empty()));
    assert_eq!(s.drawing.doc, before_doc);
    assert_eq!(s.history_steps(), before_history);
    assert_eq!(s.history_revision(), before_revision);
    assert_eq!(s.current, before_current);
    assert_eq!(s.is_dirty(), before_dirty);
    assert_eq!(serde_json::to_value(&s.takeoff).unwrap(), before_takeoff);
}

#[test]
fn bounds_use_exact_curve_extrema_even_at_zero_tolerance() {
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let round = circle(&mut s, k, [0.0, 0.0], 1.0);
        let mut e = expectation(VerifyKind::InsideBounds, &[round], None);
        e.tolerance = Some(0.0);
        e.bounds = Some([[-k, -k], [k, 0.99999 * k]]);
        let result = verify(&mut s, vec![e]).remove(0);
        assert!(!result.passed, "{units}: {result:?}");
        close(result.measured.unwrap(), 0.00001 * k);
    }
}

#[test]
fn extra_vertices_on_one_side_do_not_change_outline_symmetry() {
    for units in [4, 5, 6, 1] {
        let (mut s, k) = blank(units);
        let outline = polyline(
            &mut s,
            k,
            &[
                [-2.0, -1.0],
                [2.0, -1.0],
                [2.0, 0.0],
                [2.0, 1.0],
                [-2.0, 1.0],
            ],
            true,
        );
        let axis = line(&mut s, k, [0.0, -2.0], [0.0, 2.0]);
        let result = verify(
            &mut s,
            vec![expectation(VerifyKind::Symmetric, &[outline, axis], None)],
        )
        .remove(0);
        assert!(result.passed, "{units}: {result:?}");
    }
}

#[test]
fn path_checks_do_not_silently_ignore_an_unsupported_target() {
    let (mut s, _) = blank(6);
    let outline = polyline(
        &mut s,
        1.0,
        &[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]],
        true,
    );
    let Outcome::Added { handles } = s
        .run(Command::Text {
            at: [0.0, 3.0],
            height: 0.2,
            rotation: 0.0,
            value: "NOTE".into(),
            style: None,
        })
        .unwrap()
    else {
        panic!("expected a text");
    };
    let results = verify(
        &mut s,
        vec![
            expectation(VerifyKind::Closed, &[outline, handles[0]], None),
            expectation(VerifyKind::NoSelfIntersection, &[outline, handles[0]], None),
        ],
    );
    assert!(
        results.iter().all(|r| !r.passed && r.measured.is_none()),
        "{results:?}"
    );
}
