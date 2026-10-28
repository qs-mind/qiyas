use super::*;

#[test]
fn ocr_checks_units_and_keeps_existing_calibration_uncertainty() {
    let primary = Calibration {
        a: [0.0, 0.0],
        b: [100.0, 0.0],
        distance_m: 1.0,
    };
    let mut texts = vec![qiyas_pdf::PageText {
        text: "1200 mm".into(),
        confidence: 0.3,
        bbox: [[40.0, 1.0], [60.0, 11.0]],
    }];
    let (dimensions, evidence) = ocr_dimensions(&texts, &primary, &[], &[]);
    assert_eq!(dimensions.len(), 1);
    assert_eq!(evidence[0].confidence, 0.3);
    let mut calibrated = RasterCalibration {
        calibration: primary.clone(),
        uncertainty_percent: 2.0,
        refined: false,
        checked_spans: 1,
        warnings: vec!["Original warning".into()],
    };
    cross_check_ocr(&mut calibrated, &dimensions).unwrap();
    assert!((calibrated.uncertainty_percent - 20.0).abs() < 1e-9);
    assert_eq!(calibrated.calibration.distance_m, 1.0);
    assert_eq!(calibrated.checked_spans, 1);
    assert_eq!(calibrated.warnings[0], "Original warning");
    assert!(ocr_dimensions(&texts, &primary, &[], &dimensions)
        .0
        .is_empty());
    texts[0].text = "1200".into();
    assert!(ocr_dimensions(&texts, &primary, &[], &[]).0.is_empty());
    texts[0].text = "1000 mm".into();
    let (dimensions, _) = ocr_dimensions(&texts, &primary, &[], &[]);
    cross_check_ocr(&mut calibrated, &dimensions).unwrap();
    assert!(calibrated.uncertainty_percent > 19.9);
}
