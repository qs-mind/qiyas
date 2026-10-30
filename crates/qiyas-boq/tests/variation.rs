use calamine::{open_workbook_auto, Reader};
use qiyas_core::{
    measurement::MeasurementReport,
    revisions::{VariationItem, VariationReport},
};
#[test]
fn bilingual_quantities_and_large_evidence_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("variation.xlsx");
    let variation = VariationReport {
        items: vec![VariationItem {
            code: "X".into(),
            description_en: "Surface".into(),
            description_ar: "مساحة".into(),
            unit: "m2".into(),
            a_quantity: 12.,
            b_quantity: 14.,
            delta: 2.,
            elements: vec![],
        }],
        warnings: vec!["دليل ".repeat(10000)],
        ..Default::default()
    };
    let report = MeasurementReport {
        variation: Some(variation.clone()),
        ..Default::default()
    };
    qiyas_boq::export_takeoff_with_report(&path, &[], &[], "Variation", &report).unwrap();
    // Deliverable labels use the same engineering terms as the window.
    let mut workbook = open_workbook_auto(&path).unwrap();
    let sheet = workbook.worksheet_range("Variation").unwrap();
    assert_eq!(
        sheet.get_value((0, 0)).unwrap().to_string(),
        "Code / كود البند"
    );
    assert_eq!(
        sheet.get_value((0, 7)).unwrap().to_string(),
        "Evidence / الأدلة"
    );
    assert_eq!(
        qiyas_boq::variation::import_variation(&path).unwrap(),
        variation
    );
}
