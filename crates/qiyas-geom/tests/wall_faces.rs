use qiyas_geom::walls::{boundary_faces, build_walls, Opening, Run};

fn run(points: &[[f64; 2]], closed: bool) -> Run {
    Run {
        points: points.to_vec(),
        closed,
        thickness: None,
        justification: None,
    }
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

#[test]
fn gross_rings_keep_their_classification_when_a_door_opens_a_room() {
    let runs = [run(&[[0., 0.], [4., 0.], [4., 3.], [0., 3.]], true)];
    let walls = build_walls(
        &runs,
        0.2,
        &[Opening {
            run: 0,
            at: 2.,
            width: 1.,
        }],
        1e-7,
    )
    .unwrap();
    let faces = boundary_faces(&runs, 0.2, 1e-7).unwrap();
    for (external, gross, net) in [(true, 14.8, 13.8), (false, 13.2, 12.2)] {
        near(
            faces
                .iter()
                .filter(|f| f.external == external)
                .map(|f| f.length())
                .sum(),
            gross,
        );
        near(
            faces
                .iter()
                .filter(|f| f.external == external)
                .map(|f| f.net_length(&walls.openings))
                .sum(),
            net,
        );
    }
    // Duplicate cutters cannot deduct a face twice.
    let cuts = [walls.openings[0], walls.openings[0]];
    near(faces.iter().map(|f| f.net_length(&cuts)).sum(), 26.);
}

#[test]
fn collinear_ownership_mitres_steps_and_junction_hulls_cover_the_boundary() {
    let cases = [
        vec![
            run(&[[0., 0.], [3., 0.]], false),
            run(&[[3., 0.], [6., 0.]], false),
        ],
        vec![
            run(&[[0., 0.], [3., 0.]], false),
            run(&[[3., 0.], [3., 4.]], false),
        ],
        vec![
            run(&[[0., 0.], [3., 0.]], false),
            run(&[[3., 0.], [5., 2.]], false),
            run(&[[3., 0.], [4., -2.]], false),
        ],
        vec![
            run(&[[0., 0.], [6., 0.]], false),
            run(&[[3., -2.], [3., 2.]], false),
        ],
    ];
    for mut runs in cases {
        for unequal in [false, true] {
            if unequal {
                runs[1].thickness = Some(0.4);
            }
            let walls = build_walls(&runs, 0.2, &[], 1e-7).unwrap();
            let faces = boundary_faces(&runs, 0.2, 1e-7).unwrap();
            let perimeter: f64 = walls
                .regions
                .iter()
                .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
                .map(|r| {
                    r.iter()
                        .zip(r.iter().cycle().skip(1))
                        .take(r.len())
                        .map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
                        .sum::<f64>()
                })
                .sum();
            near(faces.iter().map(|f| f.length()).sum(), perimeter);
            assert!(faces.iter().all(|f| f.run < runs.len() && f.length() > 0.));
        }
    }
    let runs = [
        run(&[[0., 0.], [3., 0.]], false),
        run(&[[3., 0.], [6., 0.]], false),
    ];
    let faces = boundary_faces(&runs, 0.2, 1e-7).unwrap();
    for owner in [0, 1] {
        near(
            faces
                .iter()
                .filter(|f| f.run == owner)
                .map(|f| f.length())
                .sum(),
            6.2,
        );
    }
}
