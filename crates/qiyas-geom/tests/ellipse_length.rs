use opencadcodec::{Ellipse, EntityType, Vector3};
use qiyas_geom::{exact_length, Xform};
use std::f64::consts::TAU;

// Independent reference: dense chords of the parametric curve, not speed integration.
fn reference(e: &Ellipse) -> f64 {
    let span = (e.end_parameter - e.start_parameter).rem_euclid(TAU);
    let point = |t: f64| {
        [
            e.major_axis.x * t.cos(),
            e.major_axis.x * e.minor_axis_ratio * t.sin(),
        ]
    };
    let mut prev = point(e.start_parameter);
    let mut length = 0.0;
    for i in 1..=100_000 {
        let p = point(e.start_parameter + span * i as f64 / 100_000.0);
        length += (p[0] - prev[0]).hypot(p[1] - prev[1]);
        prev = p;
    }
    length
}

#[test]
fn small_ellipse_arcs_keep_relative_accuracy_under_mirrors_and_scales() {
    for ratio in [0.0, 0.0001, 0.1, 0.5, 1.0] {
        for (start, end) in [(0.0, 0.15), (1.2, 1.2001), (6.2, 0.1), (0.1, 6.2)] {
            let mut e = Ellipse::from_center_axes(
                Vector3::new(1e9, -1e9, 0.0),
                Vector3::new(0.02, 0.0, 0.0),
                ratio,
            );
            e.start_parameter = start;
            e.end_parameter = end;
            let expected = reference(&e);
            for normal in [-1.0, 1.0] {
                e.normal.z = normal;
                for scale in [0.001, 1.0, 1000.0] {
                    let xf = Xform::scale(-scale, scale)
                        .then(&Xform::rotate(0.73))
                        .then(&Xform::translate(1e8, -1e8));
                    let measured = exact_length(&EntityType::Ellipse(e.clone()), &xf).unwrap();
                    let relative = (measured / (expected * scale) - 1.0).abs();
                    assert!(
                        relative < 0.00001,
                        "ratio={ratio} start={start} end={end}: {relative}"
                    );
                }
            }
        }
    }
}

#[test]
fn stretched_or_tilted_ellipse_still_uses_fallback() {
    let mut e = Ellipse::new();
    e.end_parameter = 0.2;
    assert!(exact_length(&EntityType::Ellipse(e.clone()), &Xform::scale(2.0, 1.0)).is_none());
    e.normal.x = 1.0;
    assert!(exact_length(&EntityType::Ellipse(e), &Xform::identity()).is_none());
}
