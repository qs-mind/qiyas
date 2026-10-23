#[path = "../../qiyas-core/tests/support/measurement_fixture.rs"]
mod fixture;
use calamine::{open_workbook, Data, Reader, Xlsx};
use qiyas_core::Session;

#[test]
fn workbook_preserves_bilingual_boq_exact_dimension_arithmetic_and_references() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("test.dxf"), 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("export-test", false));
    let report = s.measurement_report().unwrap();
    let path = dir.path().join("dimensions.xlsx");
    qiyas_boq::export_takeoff_with_report(&path, &[], &[], "Test", &report).unwrap();
    let mut book: Xlsx<_> = open_workbook(&path).unwrap();
    let boq = book.worksheet_range("Generated BOQ").unwrap();
    assert_eq!(boq.get((1, 2)), Some(&Data::String("سطح بعرض 0.2".into())));
    assert_eq!(boq.get((1, 4)), Some(&Data::Float(32.68)));
    assert_eq!(boq.get((1, 9)), Some(&Data::String("export-test".into())));
    assert!(boq
        .get((1, 10))
        .unwrap()
        .to_string()
        .contains("Invented test method; not a published standard /"));
    let dimensions = book.worksheet_range("Dimension sheets").unwrap();
    assert_eq!(dimensions.height(), report.items[0].lines.len() + 1);
    for (i, line) in report.items[0].lines.iter().enumerate() {
        assert_eq!(
            dimensions.get((i + 1, 5)),
            Some(&Data::String(line.expression.clone()))
        );
        assert_eq!(dimensions.get((i + 1, 7)), Some(&Data::Bool(line.included)));
        assert_eq!(
            dimensions.get((i + 1, 9)),
            Some(&Data::String("export-test".into()))
        );
        assert!(dimensions
            .get((i + 1, 11))
            .unwrap()
            .to_string()
            .contains("*Model_Space"));
        assert!(dimensions
            .get((i + 1, 10))
            .unwrap()
            .to_string()
            .contains("test method"));
    }
    assert!(!boq
        .rows()
        .next()
        .unwrap()
        .iter()
        .any(|v| v.to_string().to_lowercase().contains("rate")));
}
