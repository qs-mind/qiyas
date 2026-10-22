//! Human-readable queries plus a lossless JSON record for workbook round trips.
use crate::BoqError;
use calamine::{open_workbook_auto, Reader};
use qiyas_core::Conflict;
use rust_xlsxwriter::{Format, Workbook};
use std::path::Path;
pub(crate) fn write(book: &mut Workbook, queries: &[Conflict]) -> Result<(), BoqError> {
    let sheet = book.add_worksheet().set_name("Queries")?;
    sheet.write_row_with_format(
        0,
        0,
        [
            "Id",
            "Kind",
            "Severity",
            "Question (English)",
            "Question (Arabic)",
            "Sheets",
            "Evidence handles",
            "Grids",
            "Sources",
            "Status",
            "Note",
            "Clouds",
            "Record JSON",
        ],
        &Format::new().set_bold(),
    )?;
    for (i, c) in queries.iter().enumerate() {
        let q = c.query.as_ref();
        let row = [
            c.id.clone(),
            c.kind.name().into(),
            q.map(|q| format!("{:?}", q.severity)).unwrap_or_default(),
            q.map(|q| q.question_en.clone())
                .unwrap_or_else(|| c.reason.clone()),
            q.map(|q| q.question_ar.clone())
                .unwrap_or_else(|| c.reason_ar.clone()),
            c.sheets.join(", "),
            q.map(|q| {
                q.references
                    .iter()
                    .map(|r| {
                        format!(
                            "{}{}{}:{:X}",
                            r.drawing.as_deref().unwrap_or(""),
                            if r.drawing.is_some() { " :: " } else { "" },
                            r.sheet,
                            r.handle
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
            q.map(|q| {
                q.references
                    .iter()
                    .map(|r| r.grid.as_deref().unwrap_or("unassigned"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
            q.map(|q| {
                q.citations
                    .iter()
                    .map(|c| format!("{}: {}", c.source, c.clause))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default(),
            format!("{:?}", c.status),
            c.note.clone(),
            q.map(|q| {
                q.clouds
                    .iter()
                    .map(|h| format!("{h:X}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
            serde_json::to_string(c).map_err(|e| BoqError::Xlsx(e.to_string()))?,
        ];
        sheet.write_row((i + 1) as u32, 0, row)?;
    }
    sheet.set_column_range_width(0, 12, 22.)?;
    sheet.set_column_range_width(3, 4, 60.)?;
    Ok(())
}
pub fn import_queries(path: &Path) -> Result<Vec<Conflict>, BoqError> {
    let mut book = open_workbook_auto(path).map_err(|e| BoqError::Read(e.to_string()))?;
    let range = book
        .worksheet_range("Queries")
        .map_err(|e| BoqError::Read(e.to_string()))?;
    let mut rows = range.rows();
    let header = rows
        .next()
        .ok_or_else(|| BoqError::Read("Empty Queries sheet".into()))?;
    let column = header
        .iter()
        .position(|v| matches!(v, calamine::Data::String(text) if text == "Record JSON"))
        .ok_or_else(|| BoqError::Read("Missing query record column".into()))?;
    rows.map(|r| {
        serde_json::from_str(&r.get(column).map(ToString::to_string).unwrap_or_default())
            .map_err(|e| BoqError::Read(e.to_string()))
    })
    .collect()
}
