//! Shared host writer: a prepared report contains only quantities and presentation data.
use qiyas_core::{
    deliverables::{Export, Format, Language, Report},
    Outcome,
};
use rust_xlsxwriter::{Format as CellFormat, Workbook};
use std::path::Path;

pub fn write(export: &Export) -> Result<(), String> {
    export.report.table().map_err(|e| e.to_string())?;
    if let Some(client) = &export.report.layout.client {
        let original = Path::new(&client.path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if export.path.canonicalize().ok().as_ref() == Some(&original) {
            return Err("Write client quantities to a copy, never the source workbook".into());
        }
    }
    let parent = export.path.parent().ok_or("Output needs a parent folder")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    match export.format {
        Format::Pdf => qiyas_plot::report::write(temporary.path(), &export.report)?,
        Format::Excel => {
            if export.report.layout.client.is_some() {
                crate::template::write(temporary.path(), &export.report)?;
            } else {
                excel(temporary.path(), &export.report)?;
            }
        }
    }
    temporary.persist(&export.path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn complete(outcome: &Outcome) -> Result<(), String> {
    if let Outcome::Deliverable { export } = outcome {
        write(export)?;
    }
    Ok(())
}
fn excel(path: &Path, report: &Report) -> Result<(), String> {
    let save = || -> Result<(), crate::BoqError> {
        let mut book = Workbook::new();
        let sheet = book.add_worksheet().set_name("Report")?;
        if report.language == Language::Ar {
            sheet.set_right_to_left(true);
        }
        let base = CellFormat::new()
            .set_font_name("Thmanyah Sans")
            .set_font_size(report.layout.page.font_size)
            .set_text_wrap();
        let bold = base.clone().set_bold();
        let total: f64 = report.layout.columns.iter().map(|c| c.width).sum();
        for (i, c) in report.layout.columns.iter().enumerate() {
            sheet.set_column_width(i as u16, 150. * c.width / total)?;
        }
        let p = &report.layout.page;
        if p.width_mm > p.height_mm {
            sheet.set_landscape();
        } else {
            sheet.set_portrait();
        }
        sheet.set_paper_size(9);
        sheet.set_margins(
            p.margin_mm / 25.4,
            p.margin_mm / 25.4,
            p.margin_mm / 25.4,
            p.margin_mm / 25.4,
            0.,
            0.,
        );
        sheet.set_print_scale(100).set_print_fit_to_pages(1, 0);
        let rows = report
            .table()
            .map_err(|e| crate::BoqError::Xlsx(e.to_string()))?;
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.cells.iter().enumerate() {
                let format = if row.heading { &bold } else { &base };
                if let Some(n) = v.as_f64() {
                    sheet.write_number_with_format(r as u32, c as u16, n, format)?;
                } else {
                    sheet.write_string_with_format(
                        r as u32,
                        c as u16,
                        qiyas_core::deliverables::display(v),
                        format,
                    )?;
                }
            }
        }
        for (i, w) in report.warnings.iter().enumerate() {
            sheet.write_string((rows.len() + i + 1) as u32, 0, w)?;
        }
        let record = book.add_worksheet().set_name("Qiyas record")?;
        let text =
            serde_json::to_string(report).map_err(|e| crate::BoqError::Xlsx(e.to_string()))?;
        for (i, chunk) in text.chars().collect::<Vec<_>>().chunks(16000).enumerate() {
            record.write_string(i as u32, 0, chunk.iter().collect::<String>())?;
        }
        let bytes = crate::template::page_setup(book.save_to_buffer()?, p)
            .map_err(crate::BoqError::Xlsx)?;
        std::fs::write(path, bytes)?;
        Ok(())
    };
    save().map_err(|e| e.to_string())
}
pub fn read_excel(path: &Path) -> Result<Report, String> {
    use calamine::Reader;
    let mut book = calamine::open_workbook_auto(path).map_err(|e| e.to_string())?;
    let range = book
        .worksheet_range("Qiyas record")
        .map_err(|e| e.to_string())?;
    let text: String = range
        .rows()
        .filter_map(|r| r.first())
        .map(ToString::to_string)
        .collect();
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
