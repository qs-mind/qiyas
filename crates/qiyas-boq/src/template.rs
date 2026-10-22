//! Patch only mapped quantity cells in the client's OOXML package. Other entries are copied raw.
use calamine::{Data, Reader as _};
use qiyas_core::deliverables::{ClientTemplate, Report};
use quick_xml::{events::Event, Reader};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
};

fn attrs(e: &quick_xml::events::BytesStart<'_>) -> Result<BTreeMap<String, String>, String> {
    e.attributes()
        .map(|a| {
            let a = a.map_err(|e| e.to_string())?;
            Ok((
                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                quick_xml::escape::unescape(
                    std::str::from_utf8(a.value.as_ref()).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?
                .into_owned(),
            ))
        })
        .collect()
}
fn tags(xml: &str, tag: &[u8]) -> Result<Vec<BTreeMap<String, String>>, String> {
    let mut r = Reader::from_str(xml);
    let mut out = vec![];
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == tag => {
                out.push(attrs(&e)?)
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}
fn zip_text<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, String> {
    let mut out = String::new();
    archive
        .by_name(name)
        .map_err(|e| e.to_string())?
        .take(64 * 1024 * 1024)
        .read_to_string(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}
fn cell_name(row: u32, col: u16) -> String {
    let mut c = u32::from(col);
    let mut text = String::new();
    while c > 0 {
        c -= 1;
        text.insert(0, (b'A' + (c % 26) as u8) as char);
        c /= 26;
    }
    format!("{text}{row}")
}
/// Byte ranges retain cell attributes (including style) and every byte outside quantity cells.
fn patch(xml: &str, values: &BTreeMap<String, f64>) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    let mut edits = vec![];
    let mut found = BTreeSet::new();
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|e| e.to_string())?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"c" => {
                let a = attrs(&e)?;
                let Some((cell, value)) = a.get("r").and_then(|r| values.get_key_value(r)) else {
                    continue;
                };
                if !found.insert(cell.clone()) {
                    return Err("Duplicate quantity cell".into());
                }
                let end_tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut open = format!("<{end_tag}");
                // Only the quantity cell changes. Keep style and all other attribute values.
                for (key, value) in &a {
                    if key != "t" {
                        open.push_str(&format!(" {key}=\"{}\"", quick_xml::escape::escape(value)));
                    }
                }
                open.push('>');
                if !empty {
                    reader.read_to_end(e.name()).map_err(|e| e.to_string())?;
                }
                let prefix = end_tag
                    .rsplit_once(':')
                    .map(|(p, _)| format!("{p}:"))
                    .unwrap_or_default();
                edits.push((
                    start,
                    reader.buffer_position() as usize,
                    format!("{open}<{prefix}v>{value}</{prefix}v></{end_tag}>"),
                ));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if found.len() != values.len() {
        return Err(
            "A mapped quantity cell is absent from the template; map an existing cell".into(),
        );
    }
    let mut result = xml.to_string();
    for (start, end, replacement) in edits.into_iter().rev() {
        result.replace_range(start..end, &replacement);
    }
    Ok(result)
}
fn quantities(
    template: &ClientTemplate,
    report: &Report,
) -> Result<BTreeMap<String, BTreeMap<String, f64>>, String> {
    let mut book = calamine::open_workbook_auto(&template.path).map_err(|e| e.to_string())?;
    let mut sheets = BTreeMap::new();
    let mut used = BTreeSet::new();
    for sheet in &template.sheets {
        if sheets.contains_key(&sheet.name) {
            return Err("Duplicate template sheet mapping".into());
        }
        let range = book
            .worksheet_range(&sheet.name)
            .map_err(|e| e.to_string())?;
        let mut cells = BTreeMap::new();
        for item in &sheet.items {
            let read = |col: u16| {
                range
                    .get_value((item.row - 1, u32::from(col - 1)))
                    .unwrap_or(&Data::Empty)
                    .to_string()
            };
            if read(sheet.item_column) != item.code || read(sheet.unit_column) != item.unit {
                return Err(format!(
                    "Template identity/unit changed at {} row {}",
                    sheet.name, item.row
                ));
            }
            let matches: Vec<_> = report
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    r["code"].as_str() == Some(&item.code)
                        && r["unit"].as_str() == Some(&item.unit)
                        && r["section"].as_str() == Some(&item.section)
                })
                .collect();
            if matches.len() != 1 {
                return Err(format!(
                    "Template item {} is missing or ambiguous",
                    item.code
                ));
            }
            let (index, row) = matches[0];
            if !used.insert(index) {
                return Err("A BOQ item is mapped more than once".into());
            }
            let value = row["quantity"]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Quantity is not finite")?;
            if cells
                .insert(cell_name(item.row, sheet.quantity_column), value)
                .is_some()
            {
                return Err("Duplicate quantity destination".into());
            }
        }
        sheets.insert(sheet.name.clone(), cells);
    }
    if used.len() != report.rows.len() {
        return Err("Map every generated BOQ item before template export".into());
    }
    Ok(sheets)
}
pub fn write(path: &Path, report: &Report) -> Result<(), String> {
    report.layout.validate().map_err(|e| e.to_string())?;
    let template = report
        .layout
        .client
        .as_ref()
        .ok_or("Missing client template")?;
    let quantities = quantities(template, report)?;
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&template.path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let sheets = tags(&zip_text(&mut archive, "xl/workbook.xml")?, b"sheet")?;
    let relationships = tags(
        &zip_text(&mut archive, "xl/_rels/workbook.xml.rels")?,
        b"Relationship",
    )?;
    let mut replacements = BTreeMap::new();
    for (sheet, values) in quantities {
        let metadata = sheets
            .iter()
            .find(|s| s.get("name") == Some(&sheet))
            .ok_or("Template sheet missing")?;
        let id = metadata.get("r:id").ok_or("Sheet relationship missing")?;
        let rel = relationships
            .iter()
            .find(|r| r.get("Id") == Some(id))
            .ok_or("Sheet relationship missing")?;
        if rel.get("TargetMode").is_some_and(|v| v == "External") {
            return Err("External worksheet is not supported".into());
        }
        let target = rel.get("Target").ok_or("Sheet target missing")?;
        let target = if target.starts_with('/') {
            target.trim_start_matches('/').into()
        } else {
            format!("xl/{target}")
        };
        let xml = zip_text(&mut archive, &target)?;
        replacements.insert(target, patch(&xml, &values)?);
    }
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if let Some(xml) = replacements.get(entry.name()) {
            let options =
                zip::write::SimpleFileOptions::default().compression_method(entry.compression());
            writer
                .start_file(entry.name(), options)
                .map_err(|e| e.to_string())?;
            writer
                .write_all(xml.as_bytes())
                .map_err(|e| e.to_string())?;
        } else {
            writer.raw_copy_file(entry).map_err(|e| e.to_string())?;
        }
    }
    writer.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// rust_xlsxwriter exposes numbered papers; OOXML also supports exact physical dimensions.
pub(crate) fn page_setup(
    bytes: Vec<u8>,
    page: &qiyas_core::deliverables::Page,
) -> Result<Vec<u8>, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.name() == "xl/worksheets/sheet1.xml" {
            let mut xml = String::new();
            entry.read_to_string(&mut xml).map_err(|e| e.to_string())?;
            let setup = format!(
                "paperWidth=\"{}mm\" paperHeight=\"{}mm\"",
                page.width_mm.min(page.height_mm),
                page.width_mm.max(page.height_mm)
            );
            let xml = xml.replace("paperSize=\"9\"", &setup);
            writer
                .start_file(
                    entry.name(),
                    zip::write::SimpleFileOptions::default()
                        .compression_method(entry.compression()),
                )
                .map_err(|e| e.to_string())?;
            writer
                .write_all(xml.as_bytes())
                .map_err(|e| e.to_string())?;
        } else {
            writer.raw_copy_file(entry).map_err(|e| e.to_string())?;
        }
    }
    Ok(writer.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantity_cells_accept_empty_and_string_cells_with_xml_whitespace() {
        let xml="<worksheet><c r='A1' s='7'\n t = 's'><v>4</v></c><c r='B1' s='8'/><c r='C1'><f>A1*B1</f><v>20</v></c></worksheet>";
        let result = patch(
            xml,
            &BTreeMap::from([("A1".into(), 2.5), ("B1".into(), 3.)]),
        )
        .unwrap();
        assert!(result.contains("<c r=\"A1\" s=\"7\"><v>2.5</v></c>"));
        assert!(result.contains("<c r=\"B1\" s=\"8\"><v>3</v></c>"));
        assert!(result.contains("<c r='C1'><f>A1*B1</f><v>20</v></c>"));
    }
}
