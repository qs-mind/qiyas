//! Bilingual quantities and lossless variation records. Long JSON is split across cells.
use crate::BoqError;
use calamine::{open_workbook_auto, Reader};
use qiyas_core::revisions::VariationReport;
use rust_xlsxwriter::{Format, Workbook};
use std::path::Path;

pub(crate) fn write(book: &mut Workbook, report: Option<&VariationReport>) -> Result<(), BoqError> {
    let Some(report) = report else { return Ok(()) };
    let sheet = book.add_worksheet().set_name("Variation")?;
    sheet.write_row_with_format(
        0,
        0,
        [
            "Code / البند",
            "Description (English)",
            "الوصف بالعربية",
            "Unit / الوحدة",
            "A quantity / كمية أ",
            "B quantity / كمية ب",
            "Delta / الفرق",
            "Evidence / المصادر",
            "Record JSON",
        ],
        &Format::new().set_bold(),
    )?;
    sheet.set_column_range_width(0, 7, 22.)?;
    sheet.set_column_range_width(1, 2, 50.)?;
    for (i, item) in report.items.iter().enumerate() {
        let row = (i + 1) as u32;
        sheet.write_string(row, 0, &item.code)?;
        sheet.write_string(row, 1, &item.description_en)?;
        sheet.write_string(row, 2, &item.description_ar)?;
        sheet.write_string(row, 3, &item.unit)?;
        for (column, value) in [(4, item.a_quantity), (5, item.b_quantity), (6, item.delta)] {
            sheet.write_number(row, column, value)?;
        }
        let evidence = item
            .elements
            .iter()
            .map(|e| {
                format!(
                    "{}: A {:?}; B {:?}; {}",
                    e.identity, e.a_handles, e.b_handles, e.delta
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        // Full evidence is retained in the record cells even for very large statements.
        sheet.write_string(row, 7, evidence.chars().take(30000).collect::<String>())?;
    }
    let text = serde_json::to_string(report).map_err(|e| BoqError::Xlsx(e.to_string()))?;
    let chars: Vec<_> = text.chars().collect();
    for (i, chunk) in chars.chunks(8000).enumerate() {
        sheet.write_string((i + 1) as u32, 8, chunk.iter().collect::<String>())?;
    }
    Ok(())
}
pub fn import_variation(path: &Path) -> Result<VariationReport, BoqError> {
    let mut book = open_workbook_auto(path).map_err(|e| BoqError::Read(e.to_string()))?;
    let range = book
        .worksheet_range("Variation")
        .map_err(|e| BoqError::Read(e.to_string()))?;
    let text = range
        .rows()
        .skip(1)
        .filter_map(|r| r.get(8))
        .map(ToString::to_string)
        .collect::<String>();
    serde_json::from_str(&text).map_err(|e| BoqError::Read(e.to_string()))
}
