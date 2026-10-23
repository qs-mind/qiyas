use qiyas_core::{Session, Source};
use serde_json::json;
#[path = "support/measurement_fixture.rs"]
mod fixture;

#[test]
fn explain_preserves_cited_dimensions_confidence_and_undo_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("trust.dxf"), 6);
    let (host, _) = fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("Test", false));
    let before = (
        s.document_revision(),
        s.history_steps().0.len(),
        s.sheet.clone(),
    );
    let explanation = s.explain("generated:S-m2").unwrap();
    assert_eq!(explanation["confidence"], 0.5);
    assert_eq!(
        explanation["measurement_rules"][0]["citation"]["clause"],
        "Q"
    );
    assert!(explanation["dimension_lines"].as_array().unwrap().len() >= 4);
    assert!(explanation["handles"]
        .as_array()
        .unwrap()
        .contains(&json!(host)));
    assert_eq!(explanation["elements"][0]["id"], host);
    assert_eq!(
        before,
        (
            s.document_revision(),
            s.history_steps().0.len(),
            s.sheet.clone()
        )
    );
    assert_eq!(s.measurement_report().unwrap().confidence_summary.low, 1);
    assert_eq!(
        s.explain(&format!("dimension:{host}:surface")).unwrap()["confidence"],
        0.5
    );
    assert!(s.explain("generated:missing").is_err());
    let mut method = fixture::method("Lower confidence", false);
    method["rules"][0]["confidence"] = json!(0.2);
    s.run_input(
        &json!({"type":"measurement","kind":"set_method","scope":"settings","method":method}),
        Source::Ai,
    )
    .unwrap();
    assert_eq!(s.explain("generated:S-m2").unwrap()["confidence"], 0.2);
    assert!(s.undo());
    assert_eq!(s.explain("generated:S-m2").unwrap()["confidence"], 0.5);
    s.save_as(&dir.path().join("saved.dxf")).unwrap();
    let reopened = Session::open(&dir.path().join("saved.dxf")).unwrap();
    assert_eq!(
        reopened.explain("generated:S-m2").unwrap()["dimension_lines"],
        explanation["dimension_lines"]
    );
}

#[test]
fn bands_have_exact_boundaries_and_invalid_rule_confidence_is_rejected() {
    use qiyas_core::trust::{band, ConfidenceSummary};
    assert_eq!(
        [
            band(0.),
            band(0.599),
            band(0.6),
            band(0.899),
            band(0.9),
            band(1.)
        ],
        ["low", "low", "medium", "medium", "high", "high"]
    );
    assert_eq!(ConfidenceSummary::of([0.1, 0.6, 0.9]).high, 1);
    let mut method = fixture::method("Bad", false);
    method["rules"][0]["confidence"] = json!(1.1);
    let method: qiyas_core::measurement::RuleSet = serde_json::from_value(method).unwrap();
    assert!(method.validate().is_err());
}
