use std::collections::HashMap;
use std::path::{Path, PathBuf};

use opencadcodec::objects::ObjectType;
use opencadcodec::{
    CadDocument, DwgReadOptions, DwgReader, DxfReader, DxfReaderConfiguration, EntityType,
};

use crate::{CoreError, Units};

/// An open DWG or DXF drawing.
pub struct Drawing {
    pub path: PathBuf,
    pub doc: CadDocument,
    /// What the reader had to skip or repair, in its own words.
    pub problems: Vec<String>,
    pub units: Units,
    /// What the failsafe reader read and dropped (see `crate::autosave::ReadReport`).
    pub report: crate::ReadReport,
}

/// Model space or one paper layout.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Sheet {
    pub name: String,
    pub block: String,
    pub is_model: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayerInfo {
    pub name: String,
    pub color: [u8; 3],
    /// Its colour as the palette writes one: "ACI n" or "#rrggbb".
    pub color_name: String,
    pub off: bool,
    pub frozen: bool,
    pub locked: bool,
    pub linetype: String,
    /// Hundredths of a millimetre; -3 Default.
    pub lineweight: i16,
    /// Plotted (AutoCAD's plot setting).
    pub plot: bool,
}

impl Drawing {
    /// Opens a DXF (by extension) or DWG file. A file the reader can't make sense of is `Unreadable`.
    pub fn open(path: &Path) -> Result<Drawing, CoreError> {
        // A missing file is an Io error; anything the reader rejects is Unreadable.
        std::fs::metadata(path)?;
        let is_dxf = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dxf"));
        let mut outcome = if is_dxf {
            // Failsafe, like DWG: a partly broken DXF opens with problems instead of failing.
            let config = DxfReaderConfiguration {
                failsafe: true,
                ..Default::default()
            };
            DxfReader::from_file(path)
                .and_then(|reader| reader.with_configuration(config).read_with_stats())
        } else {
            DwgReader::from_file_with_options(path, DwgReadOptions::failsafe())
                .and_then(|mut reader| reader.read_with_stats())
        }
        .map_err(|e| CoreError::Unreadable(e.to_string()))?;
        if !outcome.stats.has_usable_drawing_data() {
            return Err(CoreError::Unreadable(
                "no usable drawing data found".to_string(),
            ));
        }
        // The DXF reader drops a layout's paper orientation and millimetre units.
        if is_dxf {
            crate::layouts::restore_paper_settings(&mut outcome.document);
        }
        let report = crate::ReadReport::from_stats(&outcome.stats);
        let problems = outcome
            .stats
            .diagnostics
            .into_iter()
            .map(|d| d.message)
            .collect();
        let units = Units::from_header(outcome.document.header.insertion_units);
        Ok(Drawing {
            path: path.to_path_buf(),
            doc: outcome.document,
            problems,
            units,
            report,
        })
    }

    /// A new, empty drawing in memory, not yet on disk: `path` is where it stands until its first
    /// Save As. `insunits` is the header's INSUNITS (6 metres, 5 centimetres, 4 millimetres).
    pub fn blank(path: &Path, insunits: i16) -> Drawing {
        let mut doc = CadDocument::new();
        doc.header.insertion_units = insunits;
        crate::layouts::template_paper(&mut doc, Units::from_header(insunits));
        Drawing {
            path: path.to_path_buf(),
            units: Units::from_header(insunits),
            doc,
            problems: Vec::new(),
            report: crate::ReadReport::default(),
        }
    }

    /// Model first, then each paper layout in tab order.
    pub fn sheets(&self) -> Vec<Sheet> {
        let blocks: HashMap<u64, &str> = self
            .doc
            .block_records
            .iter()
            .map(|b| (b.handle.value(), b.name.as_str()))
            .collect();
        let mut layouts: Vec<(i16, String, String)> = self
            .doc
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::Layout(l) if !l.name.eq_ignore_ascii_case("model") => blocks
                    .get(&l.block_record.value())
                    .map(|block| (l.tab_order, l.name.clone(), block.to_string())),
                _ => None,
            })
            .collect();
        layouts.sort();
        let mut sheets = vec![Sheet {
            name: "Model".to_string(),
            block: "*Model_Space".to_string(),
            is_model: true,
        }];
        sheets.extend(layouts.into_iter().map(|(_, name, block)| Sheet {
            name,
            block,
            is_model: false,
        }));
        sheets
    }

    /// Resolve a layout name, backing block or unique sheet number in this drawing.
    pub fn resolve_sheet(&self, name: &str) -> Result<Sheet, CoreError> {
        let wanted = name.trim();
        let mut sheets = self.sheets();
        if let Some(sheet) = sheets
            .iter()
            .find(|s| s.name == wanted || s.block == wanted)
        {
            return Ok(sheet.clone());
        }
        let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
        let exact: Vec<_> = sheets
            .iter()
            .filter(|s| same(&s.name, wanted) || same(&s.block, wanted))
            .collect();
        if exact.len() == 1 {
            return Ok(exact[0].clone());
        }
        let number = |s: &str| s.split_whitespace().next().unwrap_or("").to_lowercase();
        let prefix = number(wanted);
        let numbered: Vec<_> = sheets
            .iter()
            .filter(|s| {
                // A title without a sheet number is never a fuzzy match for an edit.
                prefix.chars().any(|c| c.is_ascii_digit()) && number(&s.name) == prefix
            })
            .collect();
        if exact.is_empty() && numbered.len() == 1 {
            return Ok(numbered[0].clone());
        }
        if exact.len() > 1 || numbered.len() > 1 {
            let candidates = if exact.len() > 1 { exact } else { numbered };
            return Err(CoreError::InvalidCommand(format!(
                "Ambiguous sheet {wanted}. Choose an exact name or block: {}",
                candidates
                    .iter()
                    .map(|s| format!("{} ({})", s.name, s.block))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        // Rank suggestions by character edits; never select a fuzzy candidate automatically.
        fn distance(a: &str, b: &str) -> usize {
            let b: Vec<_> = b.to_lowercase().chars().collect();
            let mut row: Vec<_> = (0..=b.len()).collect();
            for (i, a) in a.to_lowercase().chars().enumerate() {
                let mut diagonal = row[0];
                row[0] = i + 1;
                for (j, b) in b.iter().enumerate() {
                    let old = row[j + 1];
                    row[j + 1] = (old + 1)
                        .min(row[j] + 1)
                        .min(diagonal + usize::from(a != *b));
                    diagonal = old;
                }
            }
            row[b.len()]
        }
        sheets.sort_by_key(|s| {
            (
                distance(&prefix, &number(&s.name)),
                distance(wanted, &s.name),
            )
        });
        Err(CoreError::InvalidCommand(format!(
            "No sheet {wanted}. Closest sheets: {}. Use records with collection layouts to list sheets.",
            sheets.iter().take(4).map(|s| format!("{} ({})", s.name, s.block)).collect::<Vec<_>>().join(", "))))
    }

    /// Every layer, sorted by name.
    pub fn layers(&self) -> Vec<LayerInfo> {
        let mut layers: Vec<LayerInfo> = self
            .doc
            .layers
            .iter()
            .map(|l| LayerInfo {
                name: l.name.clone(),
                color: l
                    .color
                    .rgb()
                    .map(|(r, g, b)| [r, g, b])
                    .unwrap_or([255, 255, 255]),
                color_name: crate::session::color_name(&l.color),
                off: l.flags.off,
                frozen: l.flags.frozen,
                locked: l.flags.locked,
                linetype: l.line_type.clone(),
                lineweight: l.line_weight.value(),
                plot: l.is_plottable,
            })
            .collect();
        layers.sort_by(|a, b| a.name.cmp(&b.name));
        layers
    }

    /// The entities of one sheet's block (or any block), by block name.
    pub fn entities_in(&self, sheet_block: &str) -> impl Iterator<Item = &EntityType> {
        self.doc.entities_in_block(sheet_block)
    }
}

/// A layer name as a QS reads it: a bound xref's layer `XREF$0$LAYER` (nested ones repeat the
/// `$N$`) gives the part after its last `$N$`, e.g. `A-WALL`. Other names are kept whole.
pub fn short_layer_name(name: &str) -> &str {
    let bytes = name.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'$' {
            continue;
        }
        let digits = bytes[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits > 0 && bytes.get(i + 1 + digits) == Some(&b'$') {
            start = i + digits + 2;
        }
    }
    match &name[start..] {
        "" => name,
        short => short,
    }
}

#[cfg(test)]
mod tests {
    use super::short_layer_name;

    #[test]
    fn a_bound_xref_layer_is_shown_by_its_own_name() {
        assert_eq!(short_layer_name("A-WALL"), "A-WALL");
        assert_eq!(short_layer_name("BASE-B$0$A-WALL"), "A-WALL");
        assert_eq!(
            short_layer_name("02-First floor (arch&furn)$0$02-FIRST FLOOR (ARCH&FURN)$0$A-WALL"),
            "A-WALL"
        );
        assert_eq!(short_layer_name("X$12$Y"), "Y");
        // `$` without the `$N$` pattern stays.
        assert_eq!(short_layer_name("COST$A-WALL"), "COST$A-WALL");
        assert_eq!(short_layer_name("A$$B$x$C"), "A$$B$x$C");
        assert_eq!(short_layer_name("A$0$"), "A$0$");
    }
}
