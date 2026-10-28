use calamine::Reader;
use qiyas_core::deliverables::{default_layout, Export, Format, Kind, Language, Report};
use serde_json::json;

#[test]
fn u3_u4_client_excel_keeps_long_cells_and_leaves_diagnostics_in_the_record() {
    check_export(Format::Excel);
}
#[test]
fn u3_u4_client_pdf_keeps_long_cells_and_leaves_diagnostics_in_the_record() {
    check_export(Format::Pdf);
}

fn check_export(format: Format) {
    let dir = tempfile::tempdir().unwrap();
    let description =
        "Wall finish measured net of openings with all deductions checked. ".repeat(40);
    let report = Report {
        drawing: None,
        kind: Kind::Boq,
        language: Language::En,
        layout: default_layout(Kind::Boq),
        rows: vec![
            json!({"code":"WF","section":"Finishes","description":description,"unit":"m2","quantity":12.5,"method":"measured net","citations":[{"source":"POMI (RICS)","clause":"Section D"},{"source":"POMI (RICS)","clause":"Section D"}]}),
        ],
        warnings: vec!["Object 987654321 on *Model_Space has no assigned grid reference".into()],
    };
    {
        let path = dir.path().join(if format == Format::Excel {
            "client.xlsx"
        } else {
            "client.pdf"
        });
        qiyas_boq::deliverables::write(&Export {
            path: path.clone(),
            format,
            report: report.clone(),
        })
        .unwrap();
        if let Ok(folder) = std::env::var("QIYAS_GOAL_U_EVIDENCE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::copy(
                &path,
                std::path::Path::new(&folder).join(path.file_name().unwrap()),
            )
            .unwrap();
        }
        let (visible, lossless) = if format == Format::Excel {
            let mut workbook = calamine::open_workbook_auto(&path).unwrap();
            let text = workbook
                .worksheet_range("Report")
                .unwrap()
                .cells()
                .map(|(_, _, v)| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let record = qiyas_boq::deliverables::read_excel(&path).unwrap();
            (text, record)
        } else {
            let output = std::process::Command::new("pdftotext")
                .arg(&path)
                .arg("-")
                .output()
                .unwrap();
            assert!(output.status.success());
            (
                String::from_utf8(output.stdout).unwrap(),
                qiyas_plot::report::read(&path).unwrap(),
            )
        };
        assert_eq!(lossless, report);
        assert!(
            !visible.contains("987654321")
                && !visible.contains("*Model_Space")
                && !visible.contains("assigned grid"),
            "{visible}"
        );
        assert_eq!(visible.matches("POMI (RICS)").count(), 1, "{visible}");
        assert!(visible.contains("m²"), "{visible}");
        // Every word of a cell that spans several pages must survive wrapping.
        for word in ["finish", "deductions", "checked."] {
            assert_eq!(
                visible.matches(word).count(),
                40,
                "missing {word}: {visible}"
            );
        }
    }
}

#[test]
fn u3_u4_ordinary_workbook_has_plain_sheets_units_and_no_printed_record_columns() {
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("takeoff.xlsx");
    let mut row = qiyas_core::Measurement::new(
        qiyas_core::Kind::Area,
        12.5,
        qiyas_core::Units::Metres,
        "Wall finish".into(),
        "Walls".into(),
        "*Model_Space".into(),
    );
    row.handles = vec![987654321];
    qiyas_boq::export_takeoff(&path, &[row], &[], "Quantities").unwrap();
    let mut book = calamine::open_workbook_auto(&path).unwrap();
    let sheet = book.worksheet_range("Take-off").unwrap();
    assert_eq!(sheet.get_value((1, 3)).unwrap().to_string(), "m²");
    assert_eq!(sheet.get_value((1, 5)).unwrap().to_string(), "Model");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("xl/worksheets/sheet1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(
        xml.contains("hidden=\"1\""),
        "internal row ids must be hidden"
    );
    let mut workbook = String::new();
    archive
        .by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut workbook)
        .unwrap();
    assert!(
        workbook.contains("Print_Area"),
        "only the client columns belong to the print area"
    );
}
