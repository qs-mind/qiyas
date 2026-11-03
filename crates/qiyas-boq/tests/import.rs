use std::path::{Path, PathBuf};

use qiyas_boq::{import_boq, BoqError};
use qiyas_core::{project_path, Boq, Command, Session};
use rust_xlsxwriter::{Format, Workbook, Worksheet};

/// A cell to write: text, a number, or nothing.
enum C {
    T(&'static str),
    N(f64),
    E,
}

fn write_rows(sheet: &mut Worksheet, first_row: u32, rows: &[Vec<C>]) {
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let (r, c) = (first_row + r as u32, c as u16);
            match cell {
                C::T(s) => sheet.write_string(r, c, *s).map(|_| ()),
                C::N(n) => sheet.write_number(r, c, *n).map(|_| ()),
                C::E => Ok(()),
            }
            .unwrap();
        }
    }
}

/// A cover sheet first, then "BOQ" with an English header on row 1.
fn english(dir: &Path) -> PathBuf {
    use C::*;
    let path = dir.join("english.xlsx");
    let mut book = Workbook::new();
    let cover = book.add_worksheet().set_name("Cover").unwrap();
    cover
        .write_string(0, 0, "Terminal project, tender BOQ")
        .unwrap();
    let sheet = book.add_worksheet().set_name("BOQ").unwrap();
    write_rows(
        sheet,
        0,
        &[
            vec![T("Code"), T("Description"), T("Unit"), T("Qty"), T("Rate")],
            vec![E, T("Concrete works"), E, E, E],
            vec![T("A.1"), T("Blinding concrete"), T("m3"), N(12.5), N(850.0)],
            vec![T("A.2"), T("Ground slab"), T("m²"), T("1,200"), T("950")],
            vec![E, E, E, E, E],
            vec![E, T("Masonry"), E, E, E],
            vec![T("B.1"), T("Block wall 200 mm"), T("SQM"), N(340.0), E],
            vec![
                T("B.2"),
                T("Block wall 100 mm"),
                T("m2"),
                T("Rate only"),
                N(100.0),
            ],
        ],
    );
    book.save(&path).unwrap();
    path
}

/// Two merged title rows, a blank row, then the Arabic header on row 4.
fn arabic(dir: &Path) -> PathBuf {
    use C::*;
    let path = dir.join("arabic.xlsx");
    let mut book = Workbook::new();
    let sheet = book.add_worksheet().set_name("جدول الكميات").unwrap();
    let title = Format::new().set_bold();
    sheet.merge_range(0, 0, 0, 4, "مشروع مطار", &title).unwrap();
    sheet
        .merge_range(1, 0, 1, 4, "جدول الكميات - أعمال مدنية", &title)
        .unwrap();
    write_rows(
        sheet,
        3,
        &[
            vec![
                T("بند"),
                T("الوصف"),
                T("الوحدة"),
                T("الكمية"),
                T("سعر الوحدة"),
            ],
            vec![E, T("أعمال الخرسانة"), E, E, E],
            vec![T("1"), T("خرسانة عادية"), T("م3"), N(10.0), N(700.0)],
            vec![T("2"), T("بلاط أرضيات"), T("م2"), N(50.0), N(120.0)],
            vec![E, T("أعمال النجارة"), E, E, E],
            vec![T("3"), T("أبواب خشب"), T("عدد"), N(4.0), N(1500.0)],
            vec![T("4"), T("وزرة"), T("م.ط"), N(30.0), N(45.0)],
        ],
    );
    book.save(&path).unwrap();
    path
}

#[test]
fn english_boq_imports_items_and_sections() {
    let dir = tempfile::tempdir().unwrap();
    let path = english(dir.path());
    let boq = import_boq(&path).unwrap();

    assert_eq!(boq.source, path.display().to_string());
    assert!(!boq.imported.is_empty());
    let rows: Vec<_> = boq
        .items
        .iter()
        .map(|i| {
            (
                i.code.as_str(),
                i.description.as_str(),
                i.unit.as_str(),
                i.quantity,
                i.rate,
                i.section.as_str(),
                i.row,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "A.1",
                "Blinding concrete",
                "m3",
                12.5,
                Some(850.0),
                "Concrete works",
                3
            ),
            (
                "A.2",
                "Ground slab",
                "m2",
                1200.0,
                Some(950.0),
                "Concrete works",
                4
            ),
            ("B.1", "Block wall 200 mm", "m2", 340.0, None, "Masonry", 7),
        ]
    );
    // B.2's quantity is text, not a number.
    assert_eq!(boq.skipped_rows, 1);
    assert!(boq.items.iter().all(|i| i.tolerance.is_none()));
}

#[test]
fn arabic_boq_with_merged_title_rows_imports() {
    let dir = tempfile::tempdir().unwrap();
    let boq = import_boq(&arabic(dir.path())).unwrap();

    let rows: Vec<_> = boq
        .items
        .iter()
        .map(|i| {
            (
                i.code.as_str(),
                i.description.as_str(),
                i.quantity,
                i.rate,
                i.section.as_str(),
                i.row,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("1", "خرسانة عادية", 10.0, Some(700.0), "أعمال الخرسانة", 6),
            ("2", "بلاط أرضيات", 50.0, Some(120.0), "أعمال الخرسانة", 7),
            ("3", "أبواب خشب", 4.0, Some(1500.0), "أعمال النجارة", 9),
            ("4", "وزرة", 30.0, Some(45.0), "أعمال النجارة", 10),
        ]
    );
    assert_eq!(boq.skipped_rows, 0);
}

#[test]
fn units_are_normalised() {
    let dir = tempfile::tempdir().unwrap();
    let units = |boq: Boq| boq.items.into_iter().map(|i| i.unit).collect::<Vec<_>>();
    assert_eq!(
        units(import_boq(&arabic(dir.path())).unwrap()),
        ["m3", "m2", "pcs", "m"]
    );
    assert_eq!(
        units(import_boq(&english(dir.path())).unwrap()),
        ["m3", "m2", "m2"]
    );
}

#[test]
fn no_header_is_a_clear_error() {
    use C::*;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.xlsx");
    let mut book = Workbook::new();
    let notes = book.add_worksheet().set_name("Notes").unwrap();
    write_rows(
        notes,
        0,
        &[
            vec![T("Site visit notes")],
            // Description and quantity, but no unit column: not a BOQ header.
            vec![T("Description"), T("Qty")],
            vec![T("Excavation"), N(10.0)],
        ],
    );
    book.add_worksheet().set_name("Data").unwrap();
    book.save(&path).unwrap();

    let err = import_boq(&path).unwrap_err();
    match &err {
        BoqError::NoHeader { sheets_tried } => assert_eq!(sheets_tried, &["Notes", "Data"]),
        other => panic!("{other:?}"),
    }
    let message = err.to_string();
    assert!(
        message.starts_with("No BOQ header found. Sheets tried: Notes, Data."),
        "{message}"
    );

    let missing = import_boq(&dir.path().join("missing.xlsx")).unwrap_err();
    assert!(matches!(missing, BoqError::Read(_)), "{missing:?}");
}

#[test]
fn arabic_indic_quantities_parse() {
    use C::*;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("indic.xlsx");
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    write_rows(
        sheet,
        0,
        &[
            vec![T("رقم"), T("البيان"), T("الوحدة"), T("الكمية"), T("السعر")],
            vec![T("١"), T("حفر"), T("م3"), T("١٢٫٥"), T("٨٥")],
            vec![T("٢"), T("ردم"), T("م3"), T("١٬٢٠٠"), E],
            vec![T("٣"), T("دهانات"), T("م2"), T("٣٤٠"), T("٢٢٫٧٥")],
            vec![T("٤"), T("مقطوعية"), T("مقطوعية"), T("حسب الطلب"), E],
        ],
    );
    book.save(&path).unwrap();

    let boq = import_boq(&path).unwrap();
    let got: Vec<_> = boq.items.iter().map(|i| (i.quantity, i.rate)).collect();
    assert_eq!(
        got,
        [(12.5, Some(85.0)), (1200.0, None), (340.0, Some(22.75))]
    );
    assert_eq!(boq.skipped_rows, 1);
}

#[test]
fn import_boq_command_stores_in_project() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let boq = import_boq(&english(dir.path())).unwrap();

    let mut session = Session::open(&drawing).unwrap();
    session.run(Command::SetBoq { boq: boq.clone() }).unwrap();
    session
        .run(Command::SetBoqTolerance {
            code: "A.2".to_string(),
            percent: Some(3.0),
        })
        .unwrap();
    let saved = session.save_project().unwrap();

    let reopened = Session::open(&drawing).unwrap();
    let stored = reopened.boq.clone().unwrap();
    assert_eq!(stored.items.len(), boq.items.len());
    assert_eq!(stored.items[1].tolerance, Some(3.0));
    assert_eq!(stored, session.boq.clone().unwrap());
    assert!(reopened.drawing.problems.is_empty());

    // A project file from before BOQ import (no "boq") still loads, with no BOQ.
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("boq");
    std::fs::write(project_path(&drawing), json.to_string()).unwrap();
    let old = Session::open(&drawing).unwrap();
    assert_eq!(old.boq, None);
    assert!(
        old.drawing.problems.is_empty(),
        "{:?}",
        old.drawing.problems
    );
}

/// Writes a workbook of one sheet holding `rows` from row 1.
fn one_sheet(dir: &Path, name: &str, rows: &[Vec<C>]) -> PathBuf {
    let path = dir.join(name);
    let mut book = Workbook::new();
    write_rows(book.add_worksheet(), 0, rows);
    book.save(&path).unwrap();
    path
}

#[test]
fn unit_price_is_the_rate_not_the_unit() {
    use C::*;
    let dir = tempfile::tempdir().unwrap();
    let after = one_sheet(
        dir.path(),
        "after.xlsx",
        &[
            vec![
                T("Item"),
                T("Description"),
                T("Unit"),
                T("Qty"),
                T("Unit Price"),
            ],
            vec![T("1"), T("Tiles"), T("m2"), N(20.0), N(150.0)],
        ],
    );
    let before = one_sheet(
        dir.path(),
        "before.xlsx",
        &[
            vec![
                T("Item"),
                T("Description"),
                T("Unit Price"),
                T("Unit"),
                T("Qty"),
            ],
            vec![T("1"), T("Tiles"), N(150.0), T("m2"), N(20.0)],
        ],
    );
    for path in [after, before] {
        let item = &import_boq(&path).unwrap().items[0];
        assert_eq!(
            (item.unit.as_str(), item.rate),
            ("m2", Some(150.0)),
            "{path:?}"
        );
    }
}

#[test]
fn arabic_headers_without_the_article_match() {
    use C::*;
    let dir = tempfile::tempdir().unwrap();
    let path = one_sheet(
        dir.path(),
        "bare.xlsx",
        &[
            vec![T("بند"), T("وصف"), T("وحدة"), T("كمية"), T("سعر")],
            vec![T("1"), T("لياسة"), T("م2"), N(80.0), N(35.0)],
        ],
    );
    let item = &import_boq(&path).unwrap().items[0];
    assert_eq!(
        (
            item.code.as_str(),
            item.unit.as_str(),
            item.quantity,
            item.rate
        ),
        ("1", "m2", 80.0, Some(35.0))
    );
}

#[test]
fn a_quantity_without_a_description_is_skipped() {
    use C::*;
    let dir = tempfile::tempdir().unwrap();
    let path = one_sheet(
        dir.path(),
        "total.xlsx",
        &[
            vec![T("Code"), T("Description"), T("Unit"), T("Qty")],
            vec![T("A.1"), T("Plaster"), T("m2"), N(450.0)],
            vec![T("Total"), E, E, N(450.0)],
            vec![E, E, E, N(450.0)],
        ],
    );
    let boq = import_boq(&path).unwrap();
    assert_eq!(boq.items.len(), 1);
    assert_eq!(boq.skipped_rows, 2);
}
