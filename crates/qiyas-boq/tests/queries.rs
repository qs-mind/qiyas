use qiyas_core::{Session, Source};
use serde_json::json;
#[test]
fn queries_excel_preserves_bilingual_evidence_and_status() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&d.path().join("p.dxf"), 6);
    s.run_input(&json!({"type":"set_boq","boq":{"source":"client.xlsx","imported":"","items":[{"code":"A","description":"Scope","quantity":2,"unit":"pcs","row":1}]}}),Source::User).unwrap();
    s.run_input(&json!({"type":"check"}), Source::Ai).unwrap();
    let path = d.path().join("takeoff.xlsx");
    qiyas_boq::export_takeoff_with_queries(
        &path,
        &[],
        &[],
        "Queries",
        &s.measurement_report().unwrap(),
        &s.conflicts,
    )
    .unwrap();
    let read = qiyas_boq::queries::import_queries(&path).unwrap();
    assert_eq!(read, s.conflicts);
    assert!(!read[0].query.as_ref().unwrap().question_ar.is_empty());
    let standalone = d.path().join("queries.xlsx");
    qiyas_boq::report::export_conflicts(&standalone, &s.conflicts).unwrap();
    assert_eq!(
        qiyas_boq::queries::import_queries(&standalone).unwrap(),
        s.conflicts
    );
}
