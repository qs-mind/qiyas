//! Values are calculated by qiyas-param, never by an Excel locale or a handwritten quantity.
use qiyas_core::measurement::MeasurementReport;
use rust_xlsxwriter::{Format, Workbook, XlsxError};

pub(crate) fn write(book: &mut Workbook, report: &MeasurementReport) -> Result<(), XlsxError> {
    if report.selection.effective().is_none() {
        return Ok(());
    }
    let bold = Format::new().set_bold();
    let sheet = book.add_worksheet().set_name("Generated BOQ")?;
    sheet.write_row_with_format(
        0,
        0,
        [
            "Code",
            "Description",
            "الوصف",
            "Unit",
            "Quantity",
            "Section",
            "Trade",
            "Mapping source",
            "Coverage",
        ],
        &bold,
    )?;
    sheet.set_column_width(1, 45)?;
    sheet.set_column_width(2, 45)?;
    for (index, item) in report.items.iter().enumerate() {
        let row = (index + 1) as u32;
        sheet.write_string(row, 0, &item.code)?;
        sheet.write_string(row, 1, &item.description_en)?;
        sheet.write_string(row, 2, &item.description_ar)?;
        sheet.write_string(row, 3, &item.unit)?;
        sheet.write_number(row, 4, item.quantity)?;
        sheet.write_string(row, 5, &item.section)?;
        sheet.write_string(row, 6, &item.trade)?;
        sheet.write_string(
            row,
            7,
            item.mapping_citations
                .iter()
                .map(|c| format!("{} / {}", c.source, c.clause))
                .collect::<Vec<_>>()
                .join("; "),
        )?;
        sheet.write_string(
            row,
            8,
            item.coverage
                .iter()
                .map(|n| format!("{} [{} / {}]", n.text, n.citation.source, n.citation.clause))
                .collect::<Vec<_>>()
                .join("; "),
        )?;
    }
    let sheet = book.add_worksheet().set_name("Dimension sheets")?;
    sheet.write_row_with_format(
        0,
        0,
        [
            "Item",
            "Line",
            "Operation",
            "Timesing",
            "Factors",
            "Expression",
            "Value",
            "Included",
            "Unit",
            "Method",
            "Source / clause",
            "Handles / sheet / grid",
            "Decision",
            "Source expressions",
            "Drawing evidence",
        ],
        &bold,
    )?;
    sheet.set_column_width(5, 45)?;
    sheet.set_column_width(10, 45)?;
    sheet.set_column_width(11, 45)?;
    let mut row = 1;
    for item in &report.items {
        for line in &item.lines {
            sheet.write_string(row, 0, &item.code)?;
            sheet.write_string(row, 1, &line.label)?;
            sheet.write_string(row, 2, format!("{:?}", line.operation))?;
            sheet.write_number(row, 3, line.timesing)?;
            sheet.write_string(
                row,
                4,
                line.factors
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(" × "),
            )?;
            sheet.write_string(row, 5, &line.expression)?;
            sheet.write_number(row, 6, line.value)?;
            sheet.write_boolean(row, 7, line.included)?;
            sheet.write_string(row, 8, &line.unit)?;
            sheet.write_string(row, 9, &line.method)?;
            sheet.write_string(
                row,
                10,
                line.citations
                    .iter()
                    .map(|c| format!("{} / {}", c.source, c.clause))
                    .collect::<Vec<_>>()
                    .join("; "),
            )?;
            sheet.write_string(
                row,
                11,
                line.sources
                    .iter()
                    .map(|s| {
                        format!(
                            "{:X} / {} / {}",
                            s.handle,
                            s.sheet,
                            s.grid.as_deref().unwrap_or("unassigned")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            )?;
            sheet.write_string(row, 12, line.decision.as_deref().unwrap_or(""))?;
            sheet.write_string(row, 13, line.source_expressions.join(" × "))?;
            sheet.write_string(
                row,
                14,
                line.sources
                    .iter()
                    .flat_map(|s| &s.evidence)
                    .map(|e| {
                        format!(
                            "{} page {}: {}",
                            e.path.as_deref().unwrap_or("drawing"),
                            e.page.map(|p| p.to_string()).unwrap_or_default(),
                            e.detail
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            )?;
            row += 1;
        }
    }
    let sheet = book.add_worksheet().set_name("Measurement review")?;
    sheet.write_string_with_format(0, 0, "Review notes", &bold)?;
    for (i, warning) in report.warnings.iter().enumerate() {
        sheet.write_string((i + 1) as u32, 0, warning)?;
    }
    Ok(())
}
