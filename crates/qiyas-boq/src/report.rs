//! Writes the conflicts of a comparison to an Excel workbook.

use std::path::Path;

use qiyas_core::{Conflict, ConflictKind, ConflictStatus};
use rust_xlsxwriter::{Format, RowNum, Workbook};

use crate::BoqError;

const HEADER: [&str; 12] = [
    "Kind",
    "BOQ code",
    "Schedule",
    "Reason",
    "Reason (Arabic)",
    "Expected",
    "Found",
    "Unit",
    "Sheets",
    "Status",
    "Note",
    "Id",
];

/// Writes `conflicts` to the workbook at `path`: sheet "Conflicts" with one row per conflict, and
/// sheet "Summary" with the count of each kind, then of each status.
pub fn export_conflicts(path: &Path, conflicts: &[Conflict]) -> Result<(), BoqError> {
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    let number = Format::new().set_num_format("0.000");

    let sheet = book.add_worksheet().set_name("Conflicts")?;
    sheet.write_row_with_format(0, 0, HEADER, &bold)?;
    for (col, width) in [18, 10, 20, 60, 60, 12, 12, 8, 24, 10, 30, 18]
        .into_iter()
        .enumerate()
    {
        sheet.set_column_width(col as u16, width)?;
    }
    for (c, row) in conflicts.iter().zip(1..) {
        sheet.write_string(row, 0, kind_name(c.kind))?;
        sheet.write_string(row, 1, c.boq_code.as_deref().unwrap_or(""))?;
        sheet.write_string(row, 2, c.schedule.as_deref().unwrap_or(""))?;
        sheet.write_string(row, 3, &c.reason)?;
        sheet.write_string(row, 4, &c.reason_ar)?;
        for (col, value) in [(5, c.expected), (6, c.found)] {
            if let Some(v) = value {
                sheet.write_number_with_format(row, col, v, &number)?;
            }
        }
        sheet.write_string(row, 7, &c.unit)?;
        sheet.write_string(row, 8, c.sheets.join(", "))?;
        sheet.write_string(row, 9, status_name(c.status))?;
        sheet.write_string(row, 10, &c.note)?;
        sheet.write_string(row, 11, &c.id)?;
    }

    let sheet = book.add_worksheet().set_name("Summary")?;
    sheet.set_column_width(0, 20)?;
    sheet.write_row_with_format(0, 0, ["Kind", "Count"], &bold)?;
    let mut row: RowNum = 1;
    for kind in ConflictKind::ALL {
        sheet.write_string(row, 0, kind_name(kind))?;
        sheet.write_number(
            row,
            1,
            conflicts.iter().filter(|c| c.kind == kind).count() as f64,
        )?;
        row += 1;
    }
    row += 1;
    sheet.write_row_with_format(row, 0, ["Status", "Count"], &bold)?;
    for status in ConflictStatus::ALL {
        row += 1;
        sheet.write_string(row, 0, status_name(status))?;
        let count = conflicts.iter().filter(|c| c.status == status).count();
        sheet.write_number(row, 1, count as f64)?;
    }

    crate::queries::write(&mut book, conflicts)?;
    book.save(path)?;
    Ok(())
}

fn kind_name(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::DuplicateItem => "Duplicate item",
        ConflictKind::RelationMismatch => "Element relation",
        ConflictKind::DocumentMismatch => "Document vs drawing",
        ConflictKind::QuantityMismatch => "Quantity mismatch",
        ConflictKind::MissingFromBoq => "Missing from BOQ",
        ConflictKind::NotInDrawings => "Not in drawings",
        ConflictKind::ScheduleVsPlan => "Schedule vs plan",
        ConflictKind::DrawingVsDrawing => "Drawing vs drawing",
        ConflictKind::UnitDoubt => "Unit doubt",
    }
}

fn status_name(status: ConflictStatus) -> &'static str {
    match status {
        ConflictStatus::Open => "Open",
        ConflictStatus::Resolved => "Resolved",
        ConflictStatus::Ignored => "Ignored",
    }
}
