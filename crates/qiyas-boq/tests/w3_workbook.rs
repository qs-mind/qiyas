//! W3: a client workbook shows sheets a client reads. No visible sheet holds JSON, sections are
//! bands across the table, the BOQ lists doors by size and totals nothing across items.

use std::io::Read;

use calamine::Reader;
use qiyas_core::deliverables::{self, Export, Format, Kind, Language};
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Outcome {
    s.run_input(&command, Source::User).unwrap().outcome
}

fn villa() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("villa.dwg"), 6);
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[20,0]]}],"thickness":0.2,
            "openings":[{"run":0,"at":2,"width":0.9,"kind":"door"},
                        {"run":0,"at":5,"width":0.9,"kind":"door"},
                        {"run":0,"at":8,"width":1.0,"kind":"door"},
                        {"run":0,"at":12,"width":1.2,"kind":"window"}]}),
    );
    let cite = json!({"source":"Test method","clause":"1"});
    let count = |id: &str, element: &str| {
        json!({"id":id,"kind":"count","basis":"geometric","element_type":element,"unit":"pcs",
            "terms":[{"label":"Count","operation":"add","factors":["pieces"],"citation":cite}],
            "citation":cite})
    };
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"doors","code":"A-D01","description_en":"Doors, various sizes","description_ar":"أبواب بمقاسات متنوعة","section":"Openings","trade":"Doors","unit":"pcs","citation":cite},
            {"rule":"windows","code":"A-W01","description_en":"Windows","description_ar":"شبابيك","section":"Openings","trade":"Windows","unit":"pcs","citation":cite}]}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
            "version":1,"name":"Test method","title":"Test","based_on":"test","source_note":"test",
            "rules":[count("doors","Legacy door"),count("windows","Legacy window")]}}),
    );
    (dir, s)
}

/// (name, hidden) of each sheet in the package, in order.
fn sheets(path: &std::path::Path) -> Vec<(String, bool)> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml.split("<sheet ")
        .skip(1)
        .map(|tag| {
            let name = tag
                .split("name=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            (name.to_string(), tag.contains("state=\"hidden\""))
        })
        .collect()
}

/// Every visible sheet's cells as text.
fn visible_text(path: &std::path::Path) -> Vec<(String, String)> {
    let mut book = calamine::open_workbook_auto(path).unwrap();
    sheets(path)
        .into_iter()
        .filter(|(_, hidden)| !hidden)
        .map(|(name, _)| {
            let text = book
                .worksheet_range(&name)
                .unwrap()
                .cells()
                .map(|(_, _, v)| v.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            (name, text)
        })
        .collect()
}

fn no_json(sheet: &str, text: &str) {
    for cell in text.lines() {
        let cell = cell.trim();
        let looks = (cell.starts_with('{') && cell.ends_with('}'))
            || (cell.starts_with("[{") && cell.ends_with(']'));
        assert!(
            !(looks && serde_json::from_str::<Value>(cell).is_ok()),
            "sheet {sheet} shows JSON: {}",
            &cell[..cell.len().min(80)]
        );
        assert!(
            !cell.contains("takeoff:") && !cell.contains("boq-row:"),
            "sheet {sheet} shows an id: {cell}"
        );
    }
}

#[test]
fn no_visible_sheet_of_a_client_workbook_holds_json_or_ids() {
    let (dir, s) = villa();
    for language in [Language::En, Language::Ar] {
        for kind in [Kind::Boq, Kind::Dimensions] {
            let report = deliverables::drawing(&s, kind, language, None).unwrap();
            let path = dir.path().join(format!("{kind:?}-{language:?}.xlsx"));
            qiyas_boq::deliverables::write(&Export {
                path: path.clone(),
                format: Format::Excel,
                report,
            })
            .unwrap();
            let visible = visible_text(&path);
            assert_eq!(visible.len(), 1, "one visible sheet: {:?}", sheets(&path));
            // The lossless record is there, hidden.
            assert!(sheets(&path)
                .iter()
                .any(|(n, hidden)| n == "Qiyas record" && *hidden));
            for (name, text) in &visible {
                no_json(name, text);
            }
        }
    }
    // The take-off workbook: every visible sheet is plain too.
    let path = dir.path().join("takeoff.xlsx");
    qiyas_boq::export_takeoff_with_queries(
        &path,
        &s.takeoff.items,
        &s.takeoff.totals(),
        "villa",
        &s.measurement_report().unwrap(),
        &s.conflicts,
    )
    .unwrap();
    for (name, text) in visible_text(&path) {
        no_json(&name, &text);
    }
}

#[test]
fn the_boq_is_itemised_by_size_in_sections_with_no_cross_item_total() {
    let (dir, s) = villa();
    let report = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
    let path = dir.path().join("boq.xlsx");
    qiyas_boq::deliverables::write(&Export {
        path: path.clone(),
        format: Format::Excel,
        report,
    })
    .unwrap();
    let text = &visible_text(&path)[0].1;
    assert!(text.contains("Openings"), "a section row: {text}");
    assert!(
        text.contains("900 × 2100 mm") && text.contains("1000 × 2100 mm"),
        "{text}"
    );
    assert!(!text.contains("various sizes\n"), "{text}");
    assert!(!text.contains("Total"), "{text}");
    // The same in the PDF a client reads.
    let pdf = dir.path().join("boq.pdf");
    let report = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
    qiyas_boq::deliverables::write(&Export {
        path: pdf.clone(),
        format: Format::Pdf,
        report,
    })
    .unwrap();
    let out = std::process::Command::new("pdftotext")
        .arg(&pdf)
        .arg("-")
        .output()
        .unwrap();
    let words = String::from_utf8(out.stdout).unwrap();
    assert!(
        words.contains("Openings") && words.contains("900"),
        "{words}"
    );
}
