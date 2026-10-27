//! Project workbook and short morning report, shared by CLI, Mizan and the window.
use crate::BoqError;
use qiyas_core::{
    projects::{self, ProjectCommand},
    Command, Outcome, Session, SheetSet, Source,
};
use rust_xlsxwriter::{Format, Workbook};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn export(path: &Path, set: &SheetSet) -> Result<(), BoqError> {
    let project = set
        .project
        .as_ref()
        .ok_or_else(|| BoqError::Xlsx("Not a project".into()))?;
    let mut report = qiyas_core::measurement::MeasurementReport::default();
    for entry in &project.boq {
        let mut item = entry.item.clone();
        for contribution in &entry.contributions {
            for line in &contribution.lines {
                let mut line = line.clone();
                for source in &mut line.sources {
                    source.sheet = format!("{} :: {}", contribution.drawing, source.sheet);
                }
                item.lines.push(line);
            }
        }
        report.items.push(item);
    }
    let mut book = Workbook::new();
    report.warnings = project.warnings.clone();
    crate::dimensions::write_contents(&mut book, &report)?;
    crate::queries::write(&mut book, &project.queries)?;
    let bold = Format::new().set_bold();
    let sheet = book.add_worksheet().set_name("Register")?;
    sheet.write_row_with_format(
        0,
        0,
        [
            "Drawing / الرسم",
            "Sheet / اللوحة",
            "Number / الرقم",
            "Title / العنوان",
            "Revision / المراجعة",
            "Date / التاريخ",
            "Flags / ملاحظات",
        ],
        &bold,
    )?;
    for (i, row) in project.register.iter().enumerate() {
        sheet.write_row(
            (i + 1) as u32,
            0,
            [
                row.drawing.clone(),
                row.sheet.clone(),
                row.number.clone().unwrap_or_default(),
                row.title.clone().unwrap_or_default(),
                row.revision.clone().unwrap_or_default(),
                row.date.clone().unwrap_or_default(),
                row.flags.join("; "),
            ],
        )?;
    }
    sheet
        .set_column_width(0, 55)?
        .set_column_width(3, 40)?
        .set_column_width(6, 55)?;
    let sheet = book.add_worksheet().set_name("Drawings")?;
    sheet.write_row_with_format(
        0,
        0,
        ["Drawing", "Status", "BOQ items", "Warnings / errors"],
        &bold,
    )?;
    for (i, row) in project.drawings.iter().enumerate() {
        let n = (i + 1) as u32;
        sheet
            .write_string(n, 0, &row.drawing)?
            .write_string(n, 1, &row.status)?
            .write_number(n, 2, row.items as f64)?
            .write_string(
                n,
                3,
                row.warnings
                    .iter()
                    .cloned()
                    .chain(row.error.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
            )?;
    }
    // Chunk the lossless project record below Excel's per-cell character limit.
    let sheet = book.add_worksheet().set_name("Project record")?;
    sheet.write_string(0, 0, "Qiyas project JSON chunks")?;
    let record = serde_json::to_string(set).map_err(|e| BoqError::Xlsx(e.to_string()))?;
    let chars: Vec<_> = record.chars().collect();
    for (i, chunk) in chars.chunks(16000).enumerate() {
        sheet.write_string((i + 1) as u32, 0, chunk.iter().collect::<String>())?;
    }
    book.save(path)?;
    Ok(())
}
pub fn import(path: &Path) -> Result<SheetSet, BoqError> {
    use calamine::Reader;
    let mut book = calamine::open_workbook_auto(path).map_err(|e| BoqError::Read(e.to_string()))?;
    let sheet = book
        .worksheet_range("Project record")
        .map_err(|e| BoqError::Read(e.to_string()))?;
    let text: String = sheet
        .rows()
        .skip(1)
        .filter_map(|r| r.first())
        .map(ToString::to_string)
        .collect();
    serde_json::from_str(&text).map_err(|e| BoqError::Read(e.to_string()))
}
pub fn write_outputs(qss: &Path, set: &SheetSet, out: Option<&Path>) -> Result<Value, String> {
    let default = qss.with_extension("output");
    let out = out.unwrap_or(&default);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let workbook = out.join("project.xlsx");
    let report = out.join("morning-report.md");
    export(&workbook, set).map_err(|e| e.to_string())?;
    let project = set.project.as_ref().ok_or("Not a project")?;
    let failed = project
        .drawings
        .iter()
        .filter(|d| d.error.is_some())
        .count();
    let flags = project
        .register
        .iter()
        .map(|r| r.flags.len())
        .sum::<usize>();
    let mut text=format!("# {}\n\n{} drawings; {} failed. {} BOQ items; {} project queries. {} register fields flagged.\n\nCross-drawing checks: {}.\n\nMethod: {}. Quantities only.\n",set.name,project.drawings.len(),failed,project.boq.len(),project.queries.len(),flags,if project.checked{"completed"}else{"incomplete"},project.method.as_ref().map(|m|qiyas_core::measurement::canonical_method_name(&m.title)).as_deref().unwrap_or("drawing methods (see dimension sources)"));
    let confidence = &project.confidence_summary;
    text.push_str(&format!("\nConfidence bands: low {}, medium {}, high {}. Review is optional.\n\n| Item (lowest confidence first) | Confidence | Band |\n|---|---:|---|\n", confidence.low, confidence.medium, confidence.high));
    let mut items = project.boq.iter().collect::<Vec<_>>();
    items.sort_by(|a, b| a.item.confidence.total_cmp(&b.item.confidence));
    for row in items {
        text.push_str(&format!(
            "| {} | {:.0}% | {} |\n",
            row.item.code.replace('|', "\\|"),
            row.item.confidence * 100.,
            qiyas_core::trust::band(row.item.confidence)
        ));
    }
    for warning in &project.warnings {
        text.push_str(&format!("\n- {warning}"));
    }
    for d in &project.drawings {
        for warning in d.warnings.iter().chain(d.error.iter()) {
            text.push_str(&format!("\n- {}: {warning}", d.drawing));
        }
    }
    std::fs::write(&report, text).map_err(|e| e.to_string())?;
    Ok(
        json!({"workbook":workbook,"morning_report":report,"failed":failed,"checked":project.checked,"items":project.boq.len(),"queries":project.queries.len(),"register_flags":flags}),
    )
}
pub fn run(
    session: &mut Session,
    path: &str,
    out: Option<&Path>,
    source: Source,
) -> Result<Value, String> {
    let outcome = session
        .run_as(
            Command::Project(Box::new(ProjectCommand::Run { path: path.into() })),
            source,
        )
        .map_err(|e| e.to_string())?;
    outputs(&outcome, out)
}
pub fn outputs(outcome: &Outcome, out: Option<&Path>) -> Result<Value, String> {
    let Outcome::Project { result } = outcome else {
        return Err("Expected project result".into());
    };
    let path = PathBuf::from(result["path"].as_str().ok_or("Missing project path")?);
    let set: SheetSet = serde_json::from_value(result["set"].clone()).map_err(|e| e.to_string())?;
    write_outputs(&path, &set, out)
}
pub fn session() -> Result<Session, String> {
    Ok(Session::new_blank(
        &qiyas_core::data_home::qiyas_dir()?.join("projects/session.dxf"),
        6,
    ))
}
pub fn resolve(path: &Path) -> Result<PathBuf, String> {
    projects::project_path(&path.to_string_lossy()).map_err(|e| e.to_string())
}
