//! Writes the take-off to an Excel workbook.

use std::path::Path;

use qiyas_core::deliverables::{display_sheet, display_unit, Language};
use qiyas_core::{Kind, Measurement, Total};
use rust_xlsxwriter::{DocProperties, Format, RowNum, Workbook, Worksheet, XlsxError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoqError {
    #[error("The workbook could not be written: {0}")]
    Xlsx(String),
    #[error("The workbook could not be read: {0}")]
    Read(String),
    /// No sheet has a row naming description, unit and quantity columns.
    #[error(
        "No BOQ header found. Sheets tried: {}. The header row needs Description, Unit and \
         Quantity columns (or الوصف، الوحدة، الكمية).",
        sheets_tried.join(", ")
    )]
    NoHeader { sheets_tried: Vec<String> },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<XlsxError> for BoqError {
    fn from(e: XlsxError) -> BoqError {
        match e {
            XlsxError::IoError(e) => BoqError::Io(e),
            other => BoqError::Xlsx(other.to_string()),
        }
    }
}

const ITEM_HEADER: [&str; 11] = [
    "Description",
    "Kind",
    "Quantity",
    "Unit",
    "Layer",
    "Sheet",
    "Note",
    "Id",
    "Area origin",
    "Measurement basis",
    "Count scope",
];
const TOTAL_HEADER: [&str; 5] = ["Kind", "Unit", "Item", "Count", "Sum"];
/// Every output Qiyas makes says it is not approved.
const STATUS: &str = "Status: For review";

/// Writes `items` to the workbook at `path`. Sheet "Take-off" has one row per item, a blank row,
/// then the totals; sheet "Totals" has the totals alone. A total adds like with like (one item of
/// one kind and unit), so the `totals` of the whole take-off, which add different items, are not
/// written. `title` becomes the workbook's document title.
pub fn export_takeoff(
    path: &Path,
    items: &[Measurement],
    totals: &[Total],
    title: &str,
) -> Result<(), BoqError> {
    export_takeoff_with_report(path, items, totals, title, &Default::default())
}

/// The ordinary take-off plus the live generated BOQ and its fully resolved dimension lines.
pub fn export_takeoff_with_report(
    path: &Path,
    items: &[Measurement],
    totals: &[Total],
    title: &str,
    report: &qiyas_core::measurement::MeasurementReport,
) -> Result<(), BoqError> {
    export_takeoff_with_queries(path, items, totals, title, report, &[])
}

/// Export measured quantities and the complete bilingual query list, without rates.
pub fn export_takeoff_with_queries(
    path: &Path,
    items: &[Measurement],
    _totals: &[Total],
    title: &str,
    report: &qiyas_core::measurement::MeasurementReport,
    queries: &[qiyas_core::Conflict],
) -> Result<(), BoqError> {
    let totals = item_totals(items);
    let mut book = Workbook::new();
    book.set_properties(
        &DocProperties::new()
            .set_title(title)
            .set_status("For review"),
    );
    let bold = Format::new().set_bold();

    let sheet = book.add_worksheet().set_name("Take-off")?;
    sheet.write_row_with_format(0, 0, ITEM_HEADER, &bold)?;
    sheet.set_column_width(0, 40)?;
    for col in 1..ITEM_HEADER.len() as u16 {
        sheet.set_column_width(col, 12)?;
    }
    let mut row: RowNum = 1;
    for m in items {
        sheet.write_string(row, 0, &m.description)?;
        sheet.write_string(row, 1, kind_name(m.kind))?;
        if m.available() {
            sheet.write_number_with_format(row, 2, m.shown, &qty(m.kind))?;
        } else {
            sheet.write_string(
                row,
                2,
                if m.view_only {
                    "View only"
                } else {
                    "Unavailable"
                },
            )?;
        }
        sheet.write_string(row, 3, display_unit(&m.unit))?;
        sheet.write_string(row, 4, &m.layer)?;
        sheet.write_string(row, 5, display_sheet(&m.sheet, Language::En))?;
        sheet.write_string(row, 6, &m.note)?;
        sheet.write_string(row, 7, &m.id)?;
        sheet.write_string(row, 8, m.area_origin.name())?;
        sheet.write_string(row, 9, m.measurement_basis.name())?;
        sheet.write_string(row, 10, m.count_scope.as_deref().unwrap_or("unspecified"))?;
        row += 1;
    }
    write_totals(sheet, row + 1, &totals, &bold)?;
    let status_row = row + totals.len() as u32 + 3;
    sheet.write_string_with_format(status_row, 0, STATUS, &bold)?;
    for column in 7..ITEM_HEADER.len() as u16 {
        sheet.set_column_hidden(column)?;
    }
    sheet.set_column_format(0, &Format::new().set_text_wrap())?;
    sheet.set_column_format(6, &Format::new().set_text_wrap())?;
    sheet.set_print_area(0, 0, status_row, 6)?;

    let sheet = book.add_worksheet().set_name("Totals")?;
    for col in 0..TOTAL_HEADER.len() as u16 {
        sheet.set_column_width(col, 12)?;
    }
    write_totals(sheet, 0, &totals, &bold)?;
    sheet.write_string_with_format(totals.len() as u32 + 2, 0, STATUS, &bold)?;

    crate::dimensions::write(&mut book, report)?;
    crate::queries::write(&mut book, queries)?;
    crate::variation::write(&mut book, report.variation.as_ref())?;
    book.save(path)?;
    Ok(())
}

/// Widest a column of `export_table` gets, in characters.
const MAX_WIDTH: usize = 60;

/// Writes a table (data extracted from the drawing) to the workbook at `path`: one sheet "Data",
/// `columns` as a bold header row, then `rows`: a cell that is a plain decimal (see
/// `plain_decimal`) as a number, the rest as text. Each column is as wide as its longest cell, up to 60
/// characters.
pub fn export_table(path: &Path, columns: &[String], rows: &[Vec<String>]) -> Result<(), BoqError> {
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    let sheet = book.add_worksheet().set_name("Data")?;
    sheet.write_row_with_format(0, 0, columns, &bold)?;
    for (r, cells) in rows.iter().enumerate() {
        let r = RowNum::try_from(r + 1).map_err(|_| BoqError::Xlsx("Too many rows".into()))?;
        for (c, cell) in cells.iter().enumerate() {
            let c = u16::try_from(c).map_err(|_| BoqError::Xlsx("Too many columns".into()))?;
            match plain_decimal(cell) {
                Some(n) => sheet.write_number(r, c, n)?,
                None => sheet.write_string(r, c, cell)?,
            };
        }
    }
    for (c, heading) in columns.iter().enumerate() {
        let widest = std::iter::once(heading)
            .chain(rows.iter().filter_map(|r| r.get(c)))
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0);
        let col = u16::try_from(c).map_err(|_| BoqError::Xlsx("Too many columns".into()))?;
        sheet.set_column_width(col, (widest + 1).clamp(4, MAX_WIDTH) as f64)?;
    }
    book.save(path)?;
    Ok(())
}

/// The number `s` spells as a plain decimal (`-?(0|[1-9][0-9]*)(\.[0-9]+)?`): no leading zero,
/// exponent or plus sign, so a code like "007" or "1e3" stays text.
fn plain_decimal(s: &str) -> Option<f64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let (whole, fraction) = match digits.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (digits, None),
    };
    let all_digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let whole_ok = all_digits(whole) && (whole == "0" || !whole.starts_with('0'));
    if !whole_ok || fraction.is_some_and(|f| !all_digits(f)) {
        return None;
    }
    s.parse().ok()
}

/// One item's total: its rows of one kind and unit, added. Different items are never added
/// together, even in the same unit (floor tiles and ceiling are both m²).
struct ItemTotal {
    kind: Kind,
    unit: String,
    item: String,
    count: usize,
    sum: f64,
}

/// Totals by kind, unit and description, in order of first appearance; only available rows.
fn item_totals(items: &[Measurement]) -> Vec<ItemTotal> {
    let mut totals: Vec<ItemTotal> = Vec::new();
    for m in items.iter().filter(|m| m.available()) {
        match totals
            .iter_mut()
            .find(|t| t.kind == m.kind && t.unit == m.unit && t.item == m.description)
        {
            Some(t) => {
                t.count += 1;
                t.sum += m.shown;
            }
            None => totals.push(ItemTotal {
                kind: m.kind,
                unit: m.unit.clone(),
                item: m.description.clone(),
                count: 1,
                sum: m.shown,
            }),
        }
    }
    totals
}

/// Writes the totals header at `row` and one row per total below it.
fn write_totals(
    sheet: &mut Worksheet,
    row: RowNum,
    totals: &[ItemTotal],
    bold: &Format,
) -> Result<(), XlsxError> {
    sheet.write_row_with_format(row, 0, TOTAL_HEADER, bold)?;
    for (t, row) in totals.iter().zip(row + 1..) {
        sheet.write_string(row, 0, kind_name(t.kind))?;
        sheet.write_string(row, 1, display_unit(&t.unit))?;
        sheet.write_string(row, 2, &t.item)?;
        sheet.write_number(row, 3, t.count as f64)?;
        sheet.write_number_with_format(row, 4, t.sum, &qty(t.kind))?;
    }
    Ok(())
}

/// Counts are whole numbers; everything else keeps three decimals.
fn num_format(kind: Kind) -> &'static str {
    match kind {
        Kind::Count => "0",
        Kind::Length | Kind::Area | Kind::Volume | Kind::Item => "0.000",
    }
}

fn qty(kind: Kind) -> Format {
    Format::new().set_num_format(num_format(kind))
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Length => "length",
        Kind::Area => "area",
        Kind::Count => "count",
        Kind::Volume => "volume",
        Kind::Item => "item",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_whole_numbers() {
        assert_eq!(num_format(Kind::Count), "0");
        assert_eq!(num_format(Kind::Length), "0.000");
        assert_eq!(num_format(Kind::Area), "0.000");
    }
}
