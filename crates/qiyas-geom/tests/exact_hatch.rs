use std::f64::consts::PI;

use opencadcodec::entities::{
    BoundaryEdge, BoundaryPath, CircularArcEdge, EllipticArcEdge, Hatch, LineEdge,
};
use opencadcodec::{EntityType, Vector2};
use qiyas_geom::{exact_hatch_areas, Xform};

#[test]
fn curved_hatch_edges_use_true_area_in_both_directions_far_from_origin() {
    for ccw in [true, false] {
        for elliptical in [true, false] {
            for offset in [0.0, 1e9] {
                let center = Vector2::new(offset + 25.0, offset);
                let mut path = BoundaryPath::new();
                path.add_edge(if elliptical {
                    BoundaryEdge::EllipticArc(EllipticArcEdge {
                        center,
                        major_axis_endpoint: Vector2::new(5.0, 0.0),
                        minor_axis_ratio: 0.4,
                        start_angle: 0.0,
                        end_angle: PI,
                        counter_clockwise: ccw,
                    })
                } else {
                    BoundaryEdge::CircularArc(CircularArcEdge {
                        center,
                        radius: 5.0,
                        start_angle: 0.0,
                        end_angle: PI,
                        counter_clockwise: ccw,
                    })
                });
                path.add_edge(BoundaryEdge::Line(LineEdge {
                    start: Vector2::new(offset + 20.0, offset),
                    end: Vector2::new(offset + 30.0, offset),
                }));
                let mut hatch = Hatch::new();
                hatch.paths.push(path);
                let areas =
                    exact_hatch_areas(&EntityType::Hatch(hatch), &Xform::identity()).unwrap();
                let expected = PI * 25.0 / 2.0 * if elliptical { 0.4 } else { 1.0 };
                assert!(
                    (areas[0] - expected).abs() < 1e-9,
                    "{areas:?} vs {expected}"
                );
            }
        }
    }
}

#[test]
fn quarter_circle_segment_includes_the_chord_and_mirrored_scale() {
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::Line(LineEdge {
        start: Vector2::new(10.0, 25.0),
        end: Vector2::new(15.0, 20.0),
    }));
    path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
        center: Vector2::new(10.0, 20.0),
        radius: 5.0,
        start_angle: 0.0,
        end_angle: PI / 2.0,
        counter_clockwise: true,
    }));
    let mut hatch = Hatch::new();
    hatch.paths.push(path);
    let areas = exact_hatch_areas(&EntityType::Hatch(hatch), &Xform::scale(-2.0, 2.0)).unwrap();
    let expected = (PI * 25.0 / 4.0 - 12.5) * 4.0;
    assert!((areas[0] - expected).abs() < 1e-9);
}
