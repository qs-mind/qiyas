//! Bounded, read-only workbook previews.

use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader, SheetVisible};

use crate::BoqError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbookPreview {
    pub sheets: Vec<SheetPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetPreview {
    pub name: String,
    pub hidden: bool,
    pub rows: Vec<Vec<String>>,
    pub rows_total: usize,
    pub cols_total: usize,
    pub truncated: bool,
}

impl WorkbookPreview {
    /// JSON for IPC, using the crate's existing serde_json dependency.
    pub fn into_json(self) -> serde_json::Value {
        let sheets: Vec<_> = self
            .sheets
            .into_iter()
            .map(|sheet| {
                serde_json::json!({
                    "name": sheet.name,
                    "hidden": sheet.hidden,
                    "rows": sheet.rows,
                    "rows_total": sheet.rows_total,
                    "cols_total": sheet.cols_total,
                    "truncated": sheet.truncated,
                })
            })
            .collect();
        serde_json::json!({ "sheets": sheets })
    }
}

/// Reads each sheet in workbook order, including hidden sheets. Coordinates start at A1;
/// totals include leading empty cells through the last used row and column.
pub fn preview_workbook(
    path: &Path,
    max_rows: usize,
    max_cols: usize,
) -> Result<WorkbookPreview, BoqError> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !["xlsx", "xlsm", "xls"]
        .iter()
        .any(|ext| extension.eq_ignore_ascii_case(ext))
    {
        return Err(BoqError::Read(
            "Choose an Excel workbook. اختر مصنف Excel.".into(),
        ));
    }
    let mut book = open_workbook_auto(path).map_err(|e| BoqError::Read(e.to_string()))?;
    let metadata = book.sheets_metadata().to_vec();
    let mut sheets = Vec::with_capacity(metadata.len());
    for sheet in metadata {
        let range = book
            .worksheet_range(&sheet.name)
            .map_err(|e| BoqError::Read(e.to_string()))?;
        let (rows_total, cols_total) = range
            .end()
            .map(|(row, col)| (row as usize + 1, col as usize + 1))
            .unwrap_or((0, 0));
        let rows = (0..rows_total.min(max_rows))
            .map(|row| {
                (0..cols_total.min(max_cols))
                    .map(|col| {
                        range
                            .get_value((row as u32, col as u32))
                            .map(display_cell)
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect();
        sheets.push(SheetPreview {
            name: sheet.name,
            hidden: sheet.visible != SheetVisible::Visible,
            rows,
            rows_total,
            cols_total,
            truncated: rows_total > max_rows || cols_total > max_cols,
        });
    }
    Ok(WorkbookPreview { sheets })
}

fn display_cell(cell: &Data) -> String {
    match cell {
        Data::Float(value) => {
            let text = format!("{value:.6}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text == "-0" {
                "0".into()
            } else {
                text.into()
            }
        }
        Data::Bool(true) => "TRUE".into(),
        Data::Bool(false) => "FALSE".into(),
        Data::Error(_) => "#ERR".into(),
        // Calamine supplies display text for dates and durations.
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{CellErrorType, ExcelDateTime, ExcelDateTimeType};

    #[test]
    fn cell_display_preserves_text_and_formats_values() {
        let date = ExcelDateTime::new(46000.5, ExcelDateTimeType::DateTime, false);
        let duration = ExcelDateTime::new(1.5, ExcelDateTimeType::TimeDelta, false);
        for (cell, expected) in [
            (Data::String("  قياس  ".into()), "  قياس  ".into()),
            (Data::Int(42), "42".into()),
            (Data::Float(42.0), "42".into()),
            (Data::Float(1.23456789), "1.234568".into()),
            (Data::Float(-0.0000001), "0".into()),
            (Data::Float(0.000001), "0.000001".into()),
            (Data::Bool(true), "TRUE".into()),
            (Data::Bool(false), "FALSE".into()),
            (Data::Error(CellErrorType::Div0), "#ERR".into()),
            (Data::Empty, String::new()),
            (Data::DateTime(date), date.to_string()),
            (Data::DateTime(duration), duration.to_string()),
            (Data::DateTimeIso("2026-10-28".into()), "2026-10-28".into()),
            (Data::DurationIso("PT2H".into()), "PT2H".into()),
        ] {
            assert_eq!(display_cell(&cell), expected);
        }
    }
}
