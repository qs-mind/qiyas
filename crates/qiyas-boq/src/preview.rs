//! Bounded, read-only workbook previews.

use std::collections::BTreeMap;
use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader, SheetVisible};

use crate::BoqError;
use qiyas_core::deliverables::TraceTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbookPreview {
    pub sheets: Vec<SheetPreview>,
    pub drawing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetPreview {
    pub name: String,
    pub hidden: bool,
    pub rows: Vec<Vec<String>>,
    pub rows_total: usize,
    pub cols_total: usize,
    pub truncated: bool,
    /// One-based worksheet row numbers, never indices into a filtered preview.
    pub targets: Option<BTreeMap<u32, TraceTarget>>,
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
                    "targets": sheet.targets,
                })
            })
            .collect();
        serde_json::json!({ "sheets": sheets, "drawing": self.drawing })
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
    let trace = book.worksheet_range("Qiyas record").ok().and_then(|range| {
        let text: String = range
            .rows()
            .filter_map(|row| row.first())
            .map(ToString::to_string)
            .collect();
        let record: serde_json::Value = serde_json::from_str(&text).ok()?;
        (record["trace"]["version"] == 1).then(|| record["trace"].clone())
    });
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
        let targets = trace.as_ref().and_then(|trace| {
            let maps = trace["sheets"].as_array()?;
            let mut matches = maps
                .iter()
                .filter(|map| map["name"].as_str() == Some(sheet.name.as_str()));
            let map = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            let formulas = book.worksheet_formula(&sheet.name).ok()?;
            if formulas.cells().any(|(_, _, formula)| !formula.is_empty()) {
                return None;
            }
            validated_targets(map, &range)
        });
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
            targets,
        });
    }
    let drawing = if sheets.iter().any(|sheet| sheet.targets.is_some()) {
        trace
            .as_ref()
            .and_then(|trace| trace["drawing"].as_str())
            .map(str::to_owned)
    } else {
        None
    };
    Ok(WorkbookPreview { sheets, drawing })
}

fn validated_targets(
    map: &serde_json::Value,
    range: &calamine::Range<Data>,
) -> Option<BTreeMap<u32, TraceTarget>> {
    let cells = map["cells"].as_array()?;
    let (last_row, last_col) = range.end()?;
    if last_row as usize + 1 != cells.len() {
        return None;
    }
    for (row, expected) in cells.iter().enumerate() {
        let expected = expected.as_array()?;
        if last_col as usize >= expected.len() {
            return None;
        }
        for (col, value) in expected.iter().enumerate() {
            let actual = range
                .get_value((row as u32, col as u32))
                .unwrap_or(&Data::Empty);
            let matches = if let Some(number) = value.as_f64() {
                match actual {
                    Data::Float(n) => *n == number,
                    Data::Int(n) => *n as f64 == number,
                    _ => false,
                }
            } else {
                match actual {
                    Data::String(text) => value.as_str() == Some(text.as_str()),
                    Data::Empty => value.as_str() == Some(""),
                    _ => false,
                }
            };
            if !matches {
                return None;
            }
        }
    }
    let targets: BTreeMap<u32, TraceTarget> = serde_json::from_value(map["rows"].clone()).ok()?;
    if targets
        .iter()
        .any(|(row, target)| *row == 0 || *row as usize > cells.len() || target.item.is_empty())
    {
        return None;
    }
    Some(targets)
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

#[cfg(test)]
mod trace_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn map_validation_uses_full_precision_and_exact_rows() {
        let mut range = calamine::Range::new((0, 0), (2, 1));
        range.set_value((0, 0), Data::String("Title".into()));
        range.set_value((1, 0), Data::String("Same label".into()));
        range.set_value((1, 1), Data::Float(1.00000001));
        range.set_value((2, 0), Data::String("Same label".into()));
        range.set_value((2, 1), Data::Float(2.0));
        let map = json!({
            "cells": [["Title", ""], ["Same label", 1.00000001], ["Same label", 2.0]],
            "rows": {"2":{"item":"boq-row:0"},"3":{"item":"boq-row:1"}},
        });
        let targets = validated_targets(&map, &range).unwrap();
        assert_eq!(targets[&2].item, "boq-row:0");
        assert_eq!(targets[&3].item, "boq-row:1");
        range.set_value((1, 1), Data::Float(1.00000002));
        assert_eq!(
            display_cell(&Data::Float(1.00000001)),
            display_cell(&Data::Float(1.00000002))
        );
        assert!(validated_targets(&map, &range).is_none());
        range.set_value((1, 1), Data::Float(2.0));
        range.set_value((2, 1), Data::Float(1.00000001));
        assert!(validated_targets(&map, &range).is_none());
        range.set_value((1, 1), Data::Float(1.00000001));
        range.set_value((2, 1), Data::Float(2.0));
        let mut invalid = map.clone();
        invalid["rows"]["0"] = json!({"item":"generated:bad"});
        assert!(validated_targets(&invalid, &range).is_none());
    }
}
