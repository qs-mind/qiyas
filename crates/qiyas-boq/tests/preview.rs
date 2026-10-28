use std::path::{Path, PathBuf};

use qiyas_boq::preview::preview_workbook;
use rust_xlsxwriter::Workbook;

fn write_preview_book(dir: &Path) -> PathBuf {
    let path = dir.join("preview.XLSX");
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name("Take-off").unwrap();
    sheet.write_string(0, 0, "الكمية").unwrap();
    sheet.write_number(0, 1, 42).unwrap();
    // C1 stays empty between two populated cells.
    sheet.write_number(0, 3, 1.23456789).unwrap();
    sheet.write_number(1, 0, 12.340000).unwrap();
    sheet.write_boolean(1, 1, true).unwrap();
    sheet.write_boolean(1, 3, false).unwrap();
    let hidden = book.add_worksheet();
    hidden.set_name("Record").unwrap();
    hidden.set_hidden(true);
    hidden.write_string(0, 0, "internal").unwrap();
    let very_hidden = book.add_worksheet();
    very_hidden.set_name("Very hidden").unwrap();
    very_hidden.set_very_hidden(true);
    very_hidden.write_number(0, 0, 7).unwrap();
    book.add_worksheet().set_name("Empty").unwrap();
    let offset = book.add_worksheet();
    offset.set_name("Offset").unwrap();
    offset.write_string(2, 2, "C3").unwrap();
    book.save(&path).unwrap();
    path
}

#[test]
fn preview_keeps_order_visibility_display_text_and_empty_cells() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_preview_book(dir.path());
    let preview = preview_workbook(&path, 5000, 80).unwrap();
    assert_eq!(
        preview
            .sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["Take-off", "Record", "Very hidden", "Empty", "Offset"]
    );
    let first = &preview.sheets[0];
    assert!(!first.hidden);
    assert_eq!(first.rows_total, 2);
    assert_eq!(first.cols_total, 4);
    assert!(!first.truncated);
    assert_eq!(first.rows[0], ["الكمية", "42", "", "1.234568"]);
    assert_eq!(first.rows[1], ["12.34", "TRUE", "", "FALSE"]);
    assert!(preview.sheets[1].hidden);
    assert!(preview.sheets[2].hidden);
    let empty = &preview.sheets[3];
    assert!(empty.rows.is_empty());
    assert_eq!((empty.rows_total, empty.cols_total), (0, 0));
    assert!(!empty.truncated);
    let offset = &preview.sheets[4];
    assert_eq!((offset.rows_total, offset.cols_total), (3, 3));
    assert_eq!(offset.rows[0], ["", "", ""]);
    assert_eq!(offset.rows[2], ["", "", "C3"]);
    let json = preview.into_json();
    assert_eq!(json["sheets"][0]["rows_total"], 2);
    assert_eq!(json["sheets"][1]["hidden"], true);
}

#[test]
fn preview_caps_each_axis_and_keeps_original_totals() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_preview_book(dir.path());
    for (max_rows, max_cols) in [(1, 80), (5000, 2), (1, 2), (0, 0)] {
        let preview = preview_workbook(&path, max_rows, max_cols).unwrap();
        let first = &preview.sheets[0];
        assert_eq!((first.rows_total, first.cols_total), (2, 4));
        assert!(first.truncated);
        assert_eq!(first.rows.len(), 2.min(max_rows));
        assert!(first.rows.iter().all(|row| row.len() == 4.min(max_cols)));
        assert!(!preview.sheets[3].truncated);
    }
    let exact = preview_workbook(&path, 2, 4).unwrap();
    assert!(!exact.sheets[0].truncated);
}

#[test]
fn preview_refuses_csv_and_reports_unreadable_workbooks() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("bill.csv");
    std::fs::write(&csv, "item,quantity\nwall,2").unwrap();
    assert!(preview_workbook(&csv, 5000, 80).is_err());
    let broken = dir.path().join("broken.xlsx");
    std::fs::write(&broken, b"not a workbook").unwrap();
    assert!(preview_workbook(&broken, 5000, 80).is_err());
    assert!(preview_workbook(&dir.path().join("missing.xlsx"), 5000, 80).is_err());
}
