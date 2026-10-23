use calamine::{open_workbook, Data, Reader, Xlsx};
use qiyas_core::{Conflict, ConflictKind, ConflictStatus};

fn conflict(id: &str, kind: ConflictKind, status: ConflictStatus) -> Conflict {
    Conflict {
        query: None,
        id: id.to_string(),
        kind,
        boq_code: Some("A.1".to_string()),
        measurements: vec!["m1".to_string(), "m2".to_string()],
        schedule: None,
        sheets: vec!["FF Plan".to_string(), "GF Plan".to_string()],
        expected: Some(120.0),
        found: None,
        unit: "m2".to_string(),
        reason: "BOQ says 120.000 m2".to_string(),
        reason_ar: "جدول الكميات يذكر 120.000 م²".to_string(),
        status,
        note: "Checked".to_string(),
        created: "2026-10-09T00:00:00Z".to_string(),
        hash_key: String::new(),
    }
}

#[test]
fn export_conflicts_is_readable_by_calamine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conflicts.xlsx");
    let conflicts = [
        conflict("c1", ConflictKind::QuantityMismatch, ConflictStatus::Open),
        conflict(
            "c2",
            ConflictKind::QuantityMismatch,
            ConflictStatus::Resolved,
        ),
        conflict("c3", ConflictKind::UnitDoubt, ConflictStatus::Open),
    ];
    qiyas_boq::report::export_conflicts(&path, &conflicts).unwrap();

    let mut book: Xlsx<_> = open_workbook(&path).unwrap();
    let sheet = book.worksheet_range("Conflicts").unwrap();
    let row = |r: u32| -> Vec<String> {
        (0..12)
            .map(|c| {
                sheet
                    .get((r as usize, c))
                    .map_or(String::new(), |d| d.to_string())
            })
            .collect()
    };
    assert_eq!(
        row(0),
        [
            "Kind",
            "BOQ code",
            "Schedule",
            "Reason",
            "Reason (Arabic)",
            "Expected",
            "Found",
            "Unit",
            "Sheets",
            "Status",
            "Note",
            "Id"
        ]
    );
    assert_eq!(
        row(2),
        [
            "Quantity mismatch",
            "A.1",
            "",
            "BOQ says 120.000 m2",
            "جدول الكميات يذكر 120.000 م²",
            "120",
            "",
            "m2",
            "FF Plan, GF Plan",
            "Resolved",
            "Checked",
            "c2"
        ]
    );
    assert_eq!(sheet.get((1, 5)), Some(&Data::Float(120.0)));
    assert_eq!(sheet.height(), 4);

    let summary = book.worksheet_range("Summary").unwrap();
    let count = |label: &str| -> f64 {
        let r = (0..summary.height())
            .find(|&r| summary.get((r, 0)) == Some(&Data::String(label.into())))
            .unwrap_or_else(|| panic!("no row {label}"));
        match summary.get((r, 1)) {
            Some(Data::Float(n)) => *n,
            other => panic!("{label}: {other:?}"),
        }
    };
    assert_eq!(count("Quantity mismatch"), 2.0);
    assert_eq!(count("Unit doubt"), 1.0);
    assert_eq!(count("Not in drawings"), 0.0);
    assert_eq!(count("Open"), 2.0);
    assert_eq!(count("Resolved"), 1.0);
    assert_eq!(count("Ignored"), 0.0);
}
