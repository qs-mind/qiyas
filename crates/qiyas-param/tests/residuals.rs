//! Read-only residuals must use the same equations as solving, including arc closure.

use qiyas_param::solver::residuals;
use qiyas_param::{Constraint, Curve, Sketch};

#[test]
fn residuals_report_the_measured_error_without_solving() {
    for scale in [1.0, 100.0, 1000.0, 1.0 / 0.0254] {
        let mut sketch = Sketch::default();
        let a = sketch.line([0.0, 0.0], [4.0 * scale, 0.0]);
        let b = sketch.line([0.0, scale], [4.0 * scale, scale]);
        let (p, q) = sketch.line_ends(a).unwrap();
        sketch.constrain(Constraint::Distance {
            a: p,
            b: q,
            axis: None,
            value: 3.0 * scale,
        });
        sketch.constrain(Constraint::Parallel(a, b));
        let before = sketch.clone();
        let values = residuals(&sketch, 1.0).unwrap();
        assert!((values[0] - scale).abs() < scale * 1e-12);
        assert_eq!(values[1], 0.0);
        assert_eq!(before, sketch);
    }
}

#[test]
fn arc_internal_residual_precedes_tangent_residual() {
    let mut sketch = Sketch::default();
    let centre = sketch.point([0.0, 0.0]);
    let start = sketch.point([2.0, 0.0]);
    let end = sketch.point([0.0, 2.0]);
    let arc = sketch.curve(Curve::Arc { centre, start, end });
    let line = sketch.line([-4.0, 2.0], [4.0, 2.0]);
    sketch.constrain(Constraint::Tangent(arc, line));
    assert_eq!(residuals(&sketch, 1.0).unwrap(), vec![0.0, 0.0]);
    sketch.points[end] = [0.0, 3.0];
    assert_eq!(residuals(&sketch, 1.0).unwrap(), vec![1.0, 0.0]);
}

#[test]
fn invalid_references_and_nonfinite_numbers_are_errors() {
    let mut sketch = Sketch::default();
    sketch.constrain(Constraint::Coincident(0, 1));
    assert!(residuals(&sketch, 1.0).is_err());
    sketch.points = vec![[0.0, 0.0], [f64::NAN, 0.0]];
    assert!(residuals(&sketch, 1.0).is_err());
    sketch.points[1] = [0.0, 0.0];
    assert!(residuals(&sketch, 0.0).is_err());
}
