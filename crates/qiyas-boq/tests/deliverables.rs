#[path = "../../qiyas-core/tests/support/measurement_fixture.rs"]
mod fixture;
use qiyas_core::{
    deliverables::{self, Export, Format, Kind, Language},
    projects::{aggregate, ProjectData},
    Session, SheetSet,
};
use serde_json::json;
use std::{collections::BTreeMap, io::Read, path::Path};
fn test_session(path: &Path) -> Session {
    let mut s = Session::new_blank(path, 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("deliverables", false));
    let report = s.measurement_report().unwrap();
    let variation = qiyas_core::revisions::build_variation(&[], &report.items, Default::default());
    fixture::run(
        &mut s,
        json!({"type":"set_knowledge","kind":"revisions","name":"result","data":variation}),
    );
    let item = &report.items[0];
    fixture::run(
        &mut s,
        json!({"type":"set_boq","boq":{"source":"client.xlsx","imported":"2026-10-22T00:00:00Z","items":[{"code":item.code,"description":item.description_en,"unit":item.unit,"quantity":item.quantity+100.,"section":item.section,"row":2}]}}),
    );
    fixture::run(&mut s, json!({"type":"check"}));
    assert!(!s.conflicts.is_empty());
    s
}
#[test]
fn all_deliverables_round_trip_from_drawing_and_project_in_both_languages() {
    let dir = tempfile::tempdir().unwrap();
    let s = test_session(&dir.path().join("drawing.dxf"));
    let measured = s.measurement_report().unwrap();
    let set = SheetSet {
        name: "Test".into(),
        drawings: vec![],
        project: Some(ProjectData {
            boq: aggregate(&[("drawing.dxf".into(), measured)]),
            queries: s.conflicts.clone(),
            register: qiyas_core::projects::drawing_register(&s, "drawing.dxf", &[], &[]).unwrap(),
            variations: BTreeMap::from([("drawing.dxf".into(), s.variation().unwrap().unwrap())]),
            ..Default::default()
        }),
    };
    for kind in [
        Kind::Dimensions,
        Kind::Boq,
        Kind::Queries,
        Kind::Variation,
        Kind::Register,
    ] {
        for language in [Language::En, Language::Ar] {
            for project in [false, true] {
                let report = if project {
                    deliverables::project(&set, kind, language, None).unwrap()
                } else {
                    deliverables::drawing(&s, kind, language, None).unwrap()
                };
                assert!(!report.rows.is_empty(), "Empty {kind:?} fixture");
                for format in [Format::Excel, Format::Pdf] {
                    let path = dir.path().join(if format == Format::Excel {
                        "report.xlsx"
                    } else {
                        "report.pdf"
                    });
                    qiyas_boq::deliverables::write(&Export {
                        path: path.clone(),
                        format,
                        report: report.clone(),
                    })
                    .unwrap();
                    let read = if format == Format::Excel {
                        qiyas_boq::deliverables::read_excel(&path).unwrap()
                    } else {
                        qiyas_plot::report::read(&path).unwrap()
                    };
                    assert_eq!(read, report);
                    if format == Format::Pdf {
                        let out = std::process::Command::new("pdftotext")
                            .arg(&path)
                            .arg("-")
                            .output()
                            .expect("pdftotext is required");
                        assert!(out.status.success());
                        let text = String::from_utf8(out.stdout)
                            .unwrap()
                            .chars()
                            .filter(
                                |c| !matches!(*c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'),
                            )
                            .collect::<String>();
                        assert!(
                            text.contains(report.layout.heading.text(language)),
                            "Missing {:?} heading in {text}",
                            kind
                        );
                    }
                }
            }
        }
    }
}
fn entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..archive.len())
        .map(|i| {
            let mut e = archive.by_index(i).unwrap();
            let name = e.name().into();
            let mut data = vec![];
            e.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}
#[test]
fn client_workbook_changes_only_the_mapped_quantity_cell() {
    use calamine::{Data, Reader};
    use rust_xlsxwriter::{Format as Style, Formula, Workbook};
    let dir = tempfile::tempdir().unwrap();
    let s = test_session(&dir.path().join("drawing.dxf"));
    let mut report = deliverables::drawing(&s, Kind::Boq, Language::Ar, None).unwrap();
    let item = &report.rows[0];
    let template = dir.path().join("client.xlsx");
    let output = dir.path().join("copy.xlsx");
    let mut book = Workbook::new();
    let sheet = book.add_worksheet().set_name("Client sheet").unwrap();
    let style = Style::new()
        .set_bold()
        .set_background_color("#123456")
        .set_num_format("0.000");
    sheet
        .merge_range(0, 0, 0, 5, "Client title", &style)
        .unwrap();
    sheet
        .write_row(
            1,
            0,
            ["Item", "Description", "Unit", "Qty", "Rate", "Amount"],
        )
        .unwrap();
    sheet
        .write_string(2, 0, item["code"].as_str().unwrap())
        .unwrap();
    sheet.write_string(2, 1, "Client text stays").unwrap();
    sheet
        .write_string(2, 2, item["unit"].as_str().unwrap())
        .unwrap();
    sheet.write_number_with_format(2, 3, 999., &style).unwrap();
    sheet
        .write_number_with_format(2, 4, 123.45, &style)
        .unwrap();
    sheet
        .write_formula_with_format(2, 5, Formula::new("=D3*E3"), &style)
        .unwrap();
    sheet.set_column_width(1, 42).unwrap();
    sheet.set_row_height(0, 35).unwrap();
    sheet.set_freeze_panes(2, 0).unwrap();
    book.add_worksheet()
        .set_name("Keep")
        .unwrap()
        .write_string(0, 0, "Untouched")
        .unwrap();
    book.save(&template).unwrap();
    report.layout.client=Some(serde_json::from_value(json!({"path":template,"sheets":[{"name":"Client sheet","header_rows":[1,2],"item_column":1,"description_column":2,"unit_column":3,"quantity_column":4,"protected_columns":[5,6],"section_rows":[],"items":[{"row":3,"code":item["code"],"unit":item["unit"],"section":item["section"]}]}]})).unwrap());
    qiyas_boq::deliverables::write(&Export {
        path: output.clone(),
        format: Format::Excel,
        report: report.clone(),
    })
    .unwrap();
    let original = entries(&template);
    let changed = entries(&output);
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        changed.keys().collect::<Vec<_>>()
    );
    for (name, bytes) in original {
        if name == "xl/worksheets/sheet1.xml" {
            let before = String::from_utf8(bytes).unwrap();
            let after = String::from_utf8(changed[&name].clone()).unwrap();
            assert_eq!(
                after,
                before.replace(
                    "<v>999</v>",
                    &format!("<v>{}</v>", item["quantity"].as_f64().unwrap())
                )
            );
        } else {
            assert_eq!(bytes, changed[&name], "Changed {name}");
        }
    }
    let mut read = calamine::open_workbook_auto(output).unwrap();
    let range = read.worksheet_range("Client sheet").unwrap();
    assert_eq!(
        range.get_value((2, 3)),
        Some(&Data::Float(item["quantity"].as_f64().unwrap()))
    );
    assert_eq!(range.get_value((2, 4)), Some(&Data::Float(123.45)));
    report.layout.client.as_mut().unwrap().sheets[0].quantity_column = 5;
    assert!(qiyas_boq::deliverables::write(&Export {
        path: dir.path().join("bad.xlsx"),
        format: Format::Excel,
        report
    })
    .is_err());
}
#[test]
fn report_layout_is_undoable_and_paths_are_confined() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = test_session(&dir.path().join("drawing.dxf"));
    let layout = deliverables::default_layout(Kind::Boq);
    let before = s.history_steps().0.len();
    fixture::run(
        &mut s,
        json!({"type":"set_knowledge","kind":"reports","name":"client","data":layout}),
    );
    assert_eq!(s.history_steps().0.len(), before + 1);
    assert_eq!(
        deliverables::drawing(&s, Kind::Boq, Language::En, Some("client"))
            .unwrap()
            .layout,
        layout
    );
    assert!(s.undo());
    assert!(deliverables::drawing(&s, Kind::Boq, Language::En, Some("client")).is_err());
    let input =
        json!({"type":"deliverable.export","kind":"boq","format":"pdf","path":"../escape.pdf"});
    assert!(s.run_input(&input, qiyas_core::Source::Ai).is_err());
    let input = json!({"type":"deliverable.export","kind":"boq","format":"pdf","path":"out.pdf"});
    let result = s.run_input(&input, qiyas_core::Source::Ai).unwrap();
    qiyas_boq::deliverables::complete(&result.outcome).unwrap();
    assert!(dir.path().join("out.pdf").exists());
}

#[test]
fn visible_variation_is_readable_and_records_remain_lossless() {
    for language in [Language::En, Language::Ar] {
        let report = deliverables::Report {
            kind: Kind::Variation,
            language,
            layout: deliverables::default_layout(Kind::Variation),
            warnings: vec![],
            rows: vec![
                json!({"drawing":"/private/source/flat.dxf","code":"L","description":"Length","unit":"m","a_quantity":0.0,"b_quantity":1.3200000000000005,"delta":1.3200000000000005,"elements":[{"sheet":"*Model_Space","kind":"added","b_handles":[98]}]}),
                json!({"drawing":"/private/source/flat.dxf","code":"P","description":"Method precision","unit":"m","a_quantity":0.0,"b_quantity":1.326,"delta":1.326,"elements":[],"presentation_decimals":2}),
            ],
        };
        let table = report.table().unwrap();
        let visible = table
            .iter()
            .flat_map(|r| &r.cells)
            .map(deliverables::display)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(visible.contains("flat.dxf"));
        assert!(visible.contains("1.32"));
        assert!(visible.contains("1.33"));
        assert!(!visible.contains("1.326"));
        assert!(
            !visible.contains("98"),
            "client references omit internal handles; the lossless record retains them"
        );
        assert!(!visible.contains("/private"));
        assert!(!visible.contains("b_handles"));
        assert!(!visible.contains("000000000000"));
        let dir = tempfile::tempdir().unwrap();
        for format in [Format::Excel, Format::Pdf] {
            let path = dir.path().join(if format == Format::Excel {
                "readable.xlsx"
            } else {
                "readable.pdf"
            });
            qiyas_boq::deliverables::write(&Export {
                path: path.clone(),
                format,
                report: report.clone(),
            })
            .unwrap();
            let recovered = if format == Format::Excel {
                qiyas_boq::deliverables::read_excel(&path).unwrap()
            } else {
                qiyas_plot::report::read(&path).unwrap()
            };
            assert_eq!(recovered, report);
        }
    }
}
