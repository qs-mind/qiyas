//! Regression cases for the shared geometry library adapters, in the existing test binary.
use qiyas_geom::{predicates as geom, walls, Path};

fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<[f64; 2]> {
    vec![
        [x, y],
        [x + width, y],
        [x + width, y + height],
        [x, y + height],
    ]
}

#[test]
fn half_open_boundaries_and_fan_seams_keep_their_policy() {
    for origin in [[0.0, 0.0], [3_300_000_000.0, 2_700_000_000.0]] {
        let mut ring = rectangle(origin[0], origin[1], 10.0, 10.0);
        for _ in 0..2 {
            for (p, expected) in [
                ([5.0, 5.0], true), // Internal fan seam.
                ([0.0, 5.0], true),
                ([5.0, 0.0], true),
                ([10.0, 5.0], false),
                ([5.0, 10.0], false),
                ([0.0, 0.0], true),
                ([10.0, 10.0], false),
                ([11.0, 5.0], false),
            ] {
                let p = [p[0] + origin[0], p[1] + origin[1]];
                assert_eq!(geom::inside(p, &ring), expected, "{p:?}");
            }
            ring.reverse();
        }
        for p in &ring {
            assert!(geom::in_ring(*p, &ring));
        }
    }
}

#[test]
fn boundary_tolerances_stay_in_drawing_units() {
    let ring = rectangle(0.0, 0.0, 10.0, 10.0);
    assert!(geom::inside_with_tolerance(&ring, [10.0001, 5.0], 0.001));
    assert!(!geom::inside_with_tolerance(&ring, [10.002, 5.0], 0.001));
    // Selection's square endpoint padding is deliberately different from a distance disk.
    assert!(geom::on_edge([10.0 + 9e-9, 9e-9], [0.0, 0.0], [10.0, 0.0]));
    assert!(!geom::on_edge([10.0 + 2e-8, 0.0], [0.0, 0.0], [10.0, 0.0]));
    assert!(geom::on_edge([1.0, 1.0], [1.0, 1.0], [1.0, 1.0]));
}

#[test]
fn even_odd_cancels_retraced_coverage_but_keeps_bowtie_lobes() {
    let ring = rectangle(0.0, 0.0, 10.0, 10.0);
    let twice: Vec<_> = ring.iter().chain(&ring).copied().collect();
    assert!(!geom::inside([3.0, 4.0], &twice));
    let twice = Path {
        points: twice,
        closed: true,
    };
    assert!(twice.self_intersects());
    assert_eq!(twice.area(), 0.0);
    assert!(twice.loops().unwrap().is_empty());
    let bowtie = Path {
        points: vec![[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]],
        closed: true,
    };
    assert!((bowtie.area() - 2.0).abs() < 1e-9);
    assert_eq!(bowtie.loops().unwrap().len(), 2);
}

#[test]
fn pinched_hole_and_island_follow_even_odd_ownership_at_survey_coordinates() {
    let points = vec![
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
        [0.0, 0.0],
        [2.0, 2.0],
        [2.0, 8.0],
        [8.0, 8.0],
        [8.0, 2.0],
        [2.0, 2.0],
        [4.0, 4.0],
        [6.0, 4.0],
        [6.0, 6.0],
        [4.0, 6.0],
        [4.0, 4.0],
        [2.0, 2.0],
        [0.0, 0.0],
    ];
    for offset in [0.0, 3_300_000_000.0] {
        let path = Path {
            points: points
                .iter()
                .map(|p| [p[0] + offset, p[1] + offset])
                .collect(),
            closed: true,
        };
        assert!((path.area() - 68.0).abs() < 1e-9);
        let loops = path.loops().unwrap();
        assert_eq!(loops.len(), 3);
        assert!((loops.iter().map(|r| geom::ring_area(r)).sum::<f64>() - 68.0).abs() < 1e-9);
    }
}

#[test]
fn intersections_include_collinear_contacts_and_small_determinants() {
    assert!(geom::intersects(
        [0.0, 0.0],
        [3.0, 0.0],
        [1.0, 0.0],
        [4.0, 0.0]
    ));
    assert_eq!(
        geom::crossing([0.0, 0.0], [3.0, 0.0], [1.0, 0.0], [4.0, 0.0]),
        None
    );
    assert!(!geom::cross_properly(
        [0.0, 0.0],
        [3.0, 0.0],
        [3.0, 0.0],
        [3.0, 2.0]
    ));
    let p = geom::crossing([0.0, 0.0], [1.0, 1e-16], [0.0, 1e-16], [1.0, 0.0]).unwrap();
    assert!((p[0] - 0.5).abs() < 1e-12);
    assert!((p[1] - 5e-17).abs() < 1e-28);
    assert!(geom::intersects(
        [1.0, 0.0],
        [1.0, 0.0],
        [0.0, 0.0],
        [2.0, 0.0]
    ));
}

#[test]
fn concave_segment_containment_checks_every_boundary_interval() {
    let ring = vec![
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 4.0],
        [3.0, 4.0],
        [3.0, 1.0],
        [1.0, 1.0],
        [1.0, 4.0],
        [0.0, 4.0],
    ];
    assert!(!geom::segment_inside(&ring, [0.5, 3.0], [3.5, 3.0], 1e-9));
    assert!(geom::segment_inside(&ring, [0.0, 0.0], [4.0, 0.0], 1e-9));
    assert!(geom::segment_inside(&ring, [0.5, 0.5], [3.5, 0.5], 1e-9));
}

#[test]
fn hull_area_and_centroid_keep_local_precision() {
    let ring = rectangle(3_300_000_000.0, 2_700_000_000.0, 4.0, 3.0);
    let mut points = ring.clone();
    points.extend([ring[0], [ring[0][0] + 1.0, ring[0][1] + 1.0]]);
    let hull = geom::convex_hull(&points);
    assert_eq!(hull, ring);
    assert!((geom::ring_area(&hull) - 12.0).abs() < 1e-9);
    assert_eq!(
        geom::ring_centroid(&hull),
        Some([3_300_000_002.0, 2_700_000_001.5])
    );
}

#[test]
fn wall_room_hole_island_and_opening_remain_separate_parts() {
    let run = |points, closed| walls::Run {
        points,
        closed,
        thickness: None,
        justification: None,
    };
    let runs = [
        run(rectangle(0.0, 0.0, 10.0, 10.0), true),
        run(vec![[3.0, 5.0], [7.0, 5.0]], false),
    ];
    let gross = walls::build_walls(&runs, 0.2, &[], 1e-6).unwrap();
    assert_eq!(gross.regions.len(), 2);
    assert_eq!(
        gross.regions.iter().map(|r| r.holes.len()).sum::<usize>(),
        1
    );
    assert!((gross.gross_area - 8.8).abs() < 1e-9);
    let cut = walls::build_walls(
        &runs,
        0.2,
        &[walls::Opening {
            run: 0,
            at: 5.0,
            width: 1.0,
        }],
        1e-6,
    )
    .unwrap();
    assert!((cut.net_area - 8.6).abs() < 1e-9);
    assert_eq!(cut.regions.iter().map(|r| r.holes.len()).sum::<usize>(), 0);
    assert_eq!(cut.regions.len(), 2);
    assert_eq!(
        cut,
        walls::build_walls(
            &runs,
            0.2,
            &[walls::Opening {
                run: 0,
                at: 5.0,
                width: 1.0
            }],
            1e-6
        )
        .unwrap()
    );
}

#[test]
fn spatial_queries_include_tolerance_offsets_and_keep_report_order() {
    use qiyas_core::{check::CheckScope, Command, DrawSpec, Session};
    let dir = tempfile::tempdir().unwrap();
    for units in [6, 5, 4, 1] {
        let mut session = Session::new_blank(&dir.path().join("spatial.dwg"), units);
        let k = 1.0 / session.drawing.units.to_metres();
        let origin = [3_300_000.0, 2_700_000.0];
        for (a, b) in [
            ([0.0, 0.0], [10_000.0, 0.0]),
            ([9_998.0, 0.0004], [9_999.0, 0.0004]),
            ([2.0, 0.0004], [3.0, 0.0004]),
            ([5.0, 0.002], [6.0, 0.002]), // Outside the exact overlap tolerance.
        ] {
            session
                .run(Command::Draw(DrawSpec::Line {
                    a: [origin[0] + a[0] * k, origin[1] + a[1] * k],
                    b: [origin[0] + b[0] * k, origin[1] + b[1] * k],
                }))
                .unwrap();
        }
        let scope = CheckScope {
            rules: vec!["overlap".into()],
            ..Default::default()
        };
        let first = session.check_drawing(&scope);
        assert_eq!(first.findings.len(), 2, "{units}: {:?}", first.findings);
        assert_eq!(first, session.check_drawing(&scope));
    }
}
