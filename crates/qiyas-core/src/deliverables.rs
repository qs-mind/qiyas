//! Report presentation is data. All quantities come from the measurement engines.
use crate::{knowledge, CoreError, Session, SheetSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeliverableCommand {
    /// Export dimensions, BOQ, queries, variation or register using a stored layout. The
    /// shorthand deliverable.export accepts kind as the report kind; canonical wire uses report.
    #[schemars(example=serde_json::json!({"kind":"export","report":"boq","format":"excel"}))]
    #[schemars(extend("x-takeoff-kit" = {"order":90}))]
    Export {
        report: Kind,
        format: Format,
        #[serde(default)]
        layout: Option<String>,
        /// Omitted or empty: <drawing name> - <report>.xlsx (or .pdf) in the export folder.
        /// Existing names get a numbered suffix, so an earlier export is kept.
        #[serde(default)]
        path: String,
        /// The report's language; the user's language by default (English without one).
        #[serde(default)]
        language: Option<Language>,
        #[serde(default)]
        project: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Dimensions,
    Boq,
    Queries,
    Variation,
    Register,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Excel,
    Pdf,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    Ar,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Heading {
    pub en: String,
    pub ar: String,
}
impl Heading {
    pub fn text(&self, language: Language) -> &str {
        match language {
            Language::En => &self.en,
            Language::Ar => &self.ar,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub field: String,
    pub heading: Heading,
    pub width: f64,
    #[serde(default)]
    pub total: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub heading: Heading,
    #[serde(default)]
    pub filter: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub width_mm: f64,
    pub height_mm: f64,
    pub margin_mm: f64,
    pub font_size: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub heading: Heading,
    pub columns: Vec<Column>,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub group_by: Vec<String>,
    pub total_heading: Heading,
    pub page: Page,
    /// A system font filename, resolved by the same loader as drawing plots.
    pub font: String,
    #[serde(default)]
    pub client: Option<ClientTemplate>,
    #[serde(default)]
    pub register_rules: Vec<crate::projects::RegisterRule>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClientTemplate {
    pub path: String,
    pub sheets: Vec<TemplateSheet>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateSheet {
    pub name: String,
    /// All rows and columns are one-based Excel coordinates.
    pub header_rows: Vec<u32>,
    pub item_column: u16,
    pub description_column: u16,
    pub unit_column: u16,
    pub quantity_column: u16,
    #[serde(default)]
    pub protected_columns: Vec<u16>,
    #[serde(default)]
    pub section_rows: Vec<u32>,
    pub items: Vec<TemplateItem>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateItem {
    pub row: u32,
    pub code: String,
    pub unit: String,
    #[serde(default)]
    pub section: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub kind: Kind,
    pub language: Language,
    pub layout: Layout,
    pub rows: Vec<Value>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub path: PathBuf,
    pub format: Format,
    pub report: Report,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub cells: Vec<Value>,
    pub heading: bool,
    /// A section or trade title row (the writers draw it as a band across the table).
    #[serde(default)]
    pub section: bool,
}
fn invalid(s: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(s.into())
}
pub fn default_layout(kind: Kind) -> Layout {
    let layouts: BTreeMap<String, Layout> =
        serde_json::from_str(include_str!("deliverables/defaults.json")).expect("report defaults");
    layouts[serde_json::to_value(kind).unwrap().as_str().unwrap()].clone()
}
impl Layout {
    pub fn validate(&self) -> Result<(), CoreError> {
        for rule in &self.register_rules {
            rule.validate()?;
        }
        let p = &self.page;
        if self.columns.is_empty()
            || self.columns.len() > 64
            || self
                .columns
                .iter()
                .any(|c| !c.width.is_finite() || c.width <= 0. || c.field.is_empty())
            || [p.width_mm, p.height_mm, p.margin_mm, p.font_size]
                .iter()
                .any(|v| !v.is_finite())
            || p.width_mm < 80.
            || p.width_mm > 2000.
            || p.height_mm < 80.
            || p.height_mm > 2000.
            || p.margin_mm < 0.
            || p.margin_mm * 2. + 30. > p.width_mm.min(p.height_mm)
            || !(4. ..=36.).contains(&p.font_size)
        {
            return Err(invalid("Invalid report columns or page setup"));
        }
        if let Some(client) = &self.client {
            for s in &client.sheets {
                let mut columns = vec![
                    s.item_column,
                    s.description_column,
                    s.unit_column,
                    s.quantity_column,
                ];
                columns.extend(&s.protected_columns);
                let count = columns.len();
                columns.sort();
                columns.dedup();
                if columns.len() != count
                    || columns.iter().any(|c| *c == 0 || *c > 16384)
                    || s.items.iter().any(|i| {
                        i.row == 0
                            || i.row > 1048576
                            || s.header_rows.contains(&i.row)
                            || s.section_rows.contains(&i.row)
                    })
                {
                    return Err(invalid("Template quantity cells must be separate from headers, sections and other columns"));
                }
            }
        }
        Ok(())
    }
}
/// Resolve existing symlinks and reject traversal outside the source folder, including new files.
pub fn confined(source: &Path, name: &str) -> Result<PathBuf, CoreError> {
    let folder = source
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let full = folder.join(name);
    if full.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid("Export paths must stay in the source folder"));
    }
    let mut ancestor = full.as_path();
    let mut suffix = vec![];
    let real = loop {
        if let Ok(mut real) = ancestor.canonicalize() {
            for p in suffix.iter().rev() {
                real.push(p);
            }
            break real;
        }
        if ancestor.symlink_metadata().is_ok() {
            return Err(invalid("Unresolvable export path"));
        }
        suffix.push(
            ancestor
                .file_name()
                .ok_or_else(|| invalid("Invalid export path"))?,
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| invalid("Invalid export path"))?;
    };
    if !real.starts_with(folder) {
        return Err(invalid("Export paths must stay in the source folder"));
    }
    Ok(real)
}
impl Report {
    /// The line every printed report ends with: Qiyas draws and measures, the engineer approves.
    pub fn status_row(&self) -> TableRow {
        TableRow {
            cells: vec![json!(match self.language {
                Language::En => "Status: For review",
                Language::Ar => "الحالة: للمراجعة",
            })],
            heading: true,
            section: false,
        }
    }

    pub fn table(&self) -> Result<Vec<TableRow>, CoreError> {
        self.layout.validate()?;
        let mut result = vec![];
        let sections = if self.layout.sections.is_empty() {
            vec![Section {
                heading: self.layout.heading.clone(),
                filter: BTreeMap::new(),
            }]
        } else {
            self.layout.sections.clone()
        };
        for section in sections {
            result.push(TableRow {
                cells: vec![json!(section.heading.text(self.language))],
                heading: true,
                section: true,
            });
            let mut groups: BTreeMap<Vec<String>, Vec<&Value>> = BTreeMap::new();
            for row in &self.rows {
                if !section.filter.iter().all(|(k, v)| display(&row[k]) == *v) {
                    continue;
                }
                let mut key: Vec<_> = self
                    .layout
                    .group_by
                    .iter()
                    .map(|f| display(&present(&row[f], f, row, self.language)))
                    .collect();
                // Never add quantities expressed in different units.
                if self.layout.columns.iter().any(|c| c.total) {
                    key.push(display(&present(&row["unit"], "unit", row, self.language)));
                }
                groups.entry(key).or_default().push(row);
            }
            result.push(TableRow {
                cells: self
                    .layout
                    .columns
                    .iter()
                    .map(|c| json!(c.heading.text(self.language)))
                    .collect(),
                heading: true,
                section: false,
            });
            for (key, rows) in groups {
                if !key.is_empty() {
                    result.push(TableRow {
                        cells: vec![json!(key.join(" / "))],
                        heading: true,
                        section: true,
                    });
                }
                let one_item = rows.len() > 1
                    && rows.iter().all(|r| {
                        !r["code"].is_null()
                            && r["code"] == rows[0]["code"]
                            && r["unit"] == rows[0]["unit"]
                    });
                let mut totals = vec![0.; self.layout.columns.len()];
                let mut total_decimals = vec![0; self.layout.columns.len()];
                for row in rows {
                    let mut cells = vec![];
                    for (i, c) in self.layout.columns.iter().enumerate() {
                        let value = row
                            .get(&c.field)
                            .ok_or_else(|| invalid(format!("Unknown report field: {}", c.field)))?;
                        if c.total && !value.is_null() {
                            total_decimals[i] = total_decimals[i].max(precision(row, &c.field));
                            totals[i] += value.as_f64().ok_or_else(|| {
                                invalid("Only computed numeric columns can be totalled")
                            })?;
                        }
                        cells.push(present(value, &c.field, row, self.language));
                    }
                    result.push(TableRow {
                        cells,
                        heading: false,
                        section: false,
                    });
                }
                // A total only adds like with like: rows of one item (one code) in one unit.
                // Different items, even in the same unit, are never summed into one figure.
                if self.layout.columns.iter().any(|c| c.total) && one_item {
                    result.push(TableRow {
                        cells: vec![json!(self.layout.total_heading.text(self.language))],
                        heading: true,
                        section: false,
                    });
                    result.push(TableRow {
                        cells: self
                            .layout
                            .columns
                            .iter()
                            .zip(totals)
                            .zip(total_decimals)
                            .map(|((c, n), decimals)| {
                                if c.total {
                                    rounded(n, decimals)
                                } else {
                                    Value::Null
                                }
                            })
                            .collect(),
                        heading: true,
                        section: false,
                    });
                }
            }
        }
        Ok(result)
    }
}
fn rounded(n: f64, decimals: usize) -> Value {
    // Formatting and parsing avoids binary tails in the visible sheet; records stay untouched.
    serde_json::from_str(&format!("{n:.decimals$}")).unwrap_or(Value::Null)
}
/// SI symbols are consistent in client tables; the stored calculation units stay unchanged.
pub fn display_unit(unit: &str) -> &str {
    match unit {
        "m2" => "m²",
        "m3" => "m³",
        other => other,
    }
}
/// The unit as an Arabic report writes it; a unit it does not know stays as it is.
pub fn arabic_unit(unit: &str) -> &str {
    match unit {
        "m2" | "m²" => "م²",
        "m3" | "m³" => "م³",
        "m" => "م",
        "mm" => "مم",
        "pcs" | "nr" | "count" => "عدد",
        "kg" => "كجم",
        "ton" | "t" => "طن",
        other => other,
    }
}
pub fn display_sheet(sheet: &str, language: Language) -> &str {
    if sheet.eq_ignore_ascii_case("*Model_Space") {
        if language == Language::Ar {
            "النموذج"
        } else {
            "Model"
        }
    } else if sheet.to_ascii_lowercase().starts_with("*paper_space") {
        if language == Language::Ar {
            "لوحة"
        } else {
            "Layout"
        }
    } else {
        sheet
    }
}
fn filename(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}
fn reference(v: &Value, language: Language) -> String {
    match v {
        Value::Array(a) => {
            let mut seen = std::collections::BTreeSet::new();
            a.iter()
                .map(|v| reference(v, language))
                .filter(|s| !s.is_empty() && seen.insert(s.clone()))
                .collect::<Vec<_>>()
                .join("; ")
        }
        Value::Object(o) => {
            if let (Some(source), Some(clause)) = (
                o.get("source").and_then(Value::as_str),
                o.get("clause").and_then(Value::as_str),
            ) {
                return format!("{source} / {clause}");
            }
            let sheet = o.get("sheet").and_then(Value::as_str).unwrap_or("");
            let sheet = display_sheet(sheet, language);
            let label = ["mark", "label", "name", "kind"]
                .iter()
                .find_map(|k| o.get(*k).and_then(Value::as_str))
                .unwrap_or("");
            let drawing = o
                .get("drawing")
                .and_then(Value::as_str)
                .map(filename)
                .unwrap_or("");
            let grid = o.get("grid").and_then(Value::as_str).unwrap_or("");
            [
                drawing.to_string(),
                sheet.to_string(),
                label.to_string(),
                grid.to_string(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" / ")
        }
        _ => display(v),
    }
}
fn precision(row: &Value, field: &str) -> usize {
    let count = matches!(row["unit"].as_str(), Some("pcs" | "count" | "nr"))
        && matches!(
            field,
            "quantity" | "a_quantity" | "b_quantity" | "delta" | "value" | "squaring"
        );
    // Areas and lengths read to the centimetre in a client table, volumes to the litre; a
    // method's own rounding rule wins. The lossless record keeps the full figure.
    let default = match row["unit"].as_str() {
        Some("m2" | "m²" | "m") => 2,
        _ => 3,
    };
    if count {
        0
    } else {
        row["presentation_decimals"]
            .as_u64()
            .unwrap_or(default)
            .min(12) as usize
    }
}
fn present(v: &Value, field: &str, row: &Value, language: Language) -> Value {
    let decimals = precision(row, field);
    if let Some(n) = v.as_f64() {
        return rounded(n, decimals);
    }
    if field == "unit" {
        let unit = v.as_str().unwrap_or_default();
        return json!(if language == Language::Ar {
            arabic_unit(unit)
        } else {
            display_unit(unit)
        });
    }
    if field == "sheet" {
        return json!(display_sheet(v.as_str().unwrap_or_default(), language));
    }
    if matches!(field, "drawing" | "path" | "file" | "source_path") {
        return json!(filename(v.as_str().unwrap_or_default()));
    }
    if matches!(field, "elements" | "sources" | "references" | "reference") && !v.is_string() {
        return json!(reference(v, language));
    }
    if field == "factors" {
        return json!(v
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| v
                .as_f64()
                .map(|n| display(&rounded(n, decimals)))
                .unwrap_or_else(|| display(v)))
            .collect::<Vec<_>>()
            .join(" × "));
    }
    if field == "expression" {
        let mut output = String::new();
        let mut number = String::new();
        for c in v
            .as_str()
            .unwrap_or_default()
            .chars()
            .chain(std::iter::once(' '))
        {
            if c.is_ascii_digit() || c == '.' {
                number.push(c);
            } else {
                if !number.is_empty() {
                    output.push_str(
                        &number
                            .parse::<f64>()
                            .map(|n| display(&rounded(n, decimals)))
                            .unwrap_or_else(|_| number.clone()),
                    );
                    number.clear();
                }
                output.push(c);
            }
        }
        return json!(output.trim());
    }
    if v.is_object() || v.is_array() {
        return json!(reference(v, language));
    }
    v.clone()
}
pub fn display(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(display).collect::<Vec<_>>().join(", "),
        Value::Object(_) => reference(v, Language::En),
        Value::Number(n) => n
            .as_f64()
            .map(|n| {
                if n.fract() == 0.0 {
                    format!("{n:.0}")
                } else {
                    n.to_string()
                }
            })
            .unwrap_or_default(),
        Value::Bool(b) => b.to_string(),
    }
}

/// The method and source columns of a client BOQ are in the report's language or not shown:
/// a column whose every entry is written in the other language (English clauses in an Arabic
/// BOQ) is left out of the printed table, and the report says so. The dimension sheets and the
/// lossless record keep the full citations.
fn drop_foreign_columns(
    layout: &mut Layout,
    rows: &[Value],
    language: Language,
    warnings: &mut Vec<String>,
) {
    if layout.client.is_some() {
        return;
    }
    let wanted_arabic = language == Language::Ar;
    let mut dropped = Vec::new();
    layout.columns.retain(|column| {
        if !matches!(column.field.as_str(), "method" | "citations") {
            return true;
        }
        let texts: Vec<String> = rows
            .iter()
            .map(|r| display(&present(&r[&column.field], &column.field, r, language)))
            .filter(|t| !t.trim().is_empty())
            .collect();
        let foreign = !texts.is_empty()
            && texts
                .iter()
                .all(|t| crate::numbers::has_arabic(t) != wanted_arabic);
        if foreign {
            dropped.push(column.field.clone());
        }
        !foreign
    });
    if !dropped.is_empty() {
        warnings.push(format!(
            "The {} column(s) are written in {}, not in the report's language, so the printed BOQ leaves them out (the dimension sheets keep them). Write the method and its clauses in {} to show them.",
            dropped.join(" and "),
            if wanted_arabic { "English" } else { "Arabic" },
            if wanted_arabic { "Arabic" } else { "English" },
        ));
    }
}

fn localized(rows: &mut [Value], language: Language) {
    for row in rows {
        for name in ["description", "reason"] {
            let key = format!(
                "{name}_{}",
                if language == Language::Ar { "ar" } else { "en" }
            );
            if let Some(v) = row.get(&key).cloned() {
                row[name] = v;
            }
        }
    }
}
fn item_rows(items: &[crate::measurement::GeneratedItem], kind: Kind, drawing: &str) -> Vec<Value> {
    if kind == Kind::Boq {
        return items
            .iter()
            .filter(|i| !i.informational && !i.second_differs())
            .map(|i| {
                let mut v = json!(i);
                v["drawing"] = json!(drawing);
                v["method"] = json!(i.methods());
                v["citations"] = json!(i.rule_citations());
                if i.limited {
                    // Visible on the row itself: the quantity is not a measured one.
                    v["description_en"] = json!(format!(
                        "{} [quantity limited: deductions exceed the gross quantity]",
                        i.description_en
                    ));
                    v["description_ar"] = json!(format!(
                        "{} [الكمية مقيّدة: الخصومات تتجاوز الإجمالي]",
                        i.description_ar
                    ));
                }
                v
            })
            .collect();
    }
    items
        .iter()
        .flat_map(|item| {
            printed_lines(item).into_iter().map(move |line| {
                let mut row = json!(line);
                row["code"] = json!(item.code);
                row["section"] = json!(item.section);
                row["drawing"] = json!(drawing);
                row["reference"] = json!(line
                    .sources
                    .iter()
                    .map(|s| json!({"drawing":filename(drawing),"sheet":s.sheet,"grid":s.grid}))
                    .collect::<Vec<_>>());
                row["squaring"] = json!(if line.included { line.value } else { 0. });
                row
            })
        })
        .collect()
}
/// The dimension lines worth printing: a rule evaluated on an object it does not apply to
/// gives `1 × 0 = 0`, and a sheet of those hides the real working. An item whose lines all
/// give nothing keeps its first line, so the item is still explained.
pub fn printed_lines(
    item: &crate::measurement::GeneratedItem,
) -> Vec<&crate::measurement::DimensionLine> {
    let kept: Vec<_> = item.lines.iter().filter(|l| l.value != 0.0).collect();
    if kept.is_empty() {
        item.lines.iter().take(1).collect()
    } else {
        kept
    }
}
fn query_rows(queries: &[crate::Conflict]) -> Vec<Value> {
    queries
        .iter()
        .map(|q| {
            let mut v = json!(q);
            v["reason_en"] = json!(q.reason);
            v
        })
        .collect()
}
fn layout_missing(name: &str, stored: &[String], session: &Session) -> CoreError {
    let sheet = session
        .drawing
        .sheets()
        .iter()
        .any(|l| l.name.eq_ignore_ascii_case(name));
    invalid(format!(
        "No report layout named {name}{}. Stored report layouts: {}. Leave out layout for the default.",
        if sheet { " (that is a drawing sheet, not a report layout)" } else { "" },
        if stored.is_empty() { "none".into() } else { stored.join(", ") }
    ))
}

pub fn drawing(
    session: &Session,
    kind: Kind,
    language: Language,
    layout: Option<&str>,
) -> Result<Report, CoreError> {
    let mut layout: Layout = match layout {
        Some(name) => serde_json::from_value(
            knowledge::get(&session.drawing.doc, knowledge::Kind::Reports, name)?
                .ok_or_else(|| {
                    let stored = knowledge::list(&session.drawing.doc, knowledge::Kind::Reports)
                        .map(|l| l.into_iter().map(|e| e.name).collect::<Vec<_>>())
                        .unwrap_or_default();
                    layout_missing(name, &stored, session)
                })?
                .data,
        )
        .map_err(|e| invalid(e.to_string()))?,
        None => default_layout(kind),
    };
    let measured = session.measurement_report()?;
    let mut warnings = measured.warnings.clone();
    let path = session.drawing.path.to_string_lossy();
    let mut rows = match kind {
        Kind::Boq | Kind::Dimensions => item_rows(&measured.items, kind, &path),
        Kind::Queries => query_rows(&session.conflicts),
        Kind::Variation => match session.variation()? {
            Some(v) => {
                warnings.extend(v.warnings);
                v.items
                    .iter()
                    .map(|i| {
                        let mut v = json!(i);
                        v["drawing"] = json!(path);
                        v
                    })
                    .collect()
            }
            None => {
                warnings.push("No revision baseline/variation report".into());
                vec![]
            }
        },
        Kind::Register => {
            crate::projects::drawing_register(session, &path, &[], &layout.register_rules)?
                .iter()
                .map(|r| json!(r))
                .collect()
        }
    };
    for row in &mut rows {
        let precision = measured.selection.effective().and_then(|method| {
            let item = measured
                .items
                .iter()
                .find(|i| row["code"].as_str() == Some(i.code.as_str()))?;
            method
                .rules
                .iter()
                .filter(|rule| {
                    measured.quantities.iter().any(|q| {
                        q.rule == rule.id
                            && q.unit == item.unit
                            && item
                                .lines
                                .iter()
                                .flat_map(|l| &l.sources)
                                .any(|r| r.handle == q.object)
                    })
                })
                .filter_map(|r| r.rounding.as_ref().map(|r| r.decimals))
                .max()
        });
        if let Some(precision) = precision {
            row["presentation_decimals"] = json!(precision);
        }
    }
    localized(&mut rows, language);
    if kind == Kind::Boq {
        second_reading_column(
            &mut layout,
            &mut rows,
            &measured.items,
            language,
            &mut warnings,
        );
        drop_foreign_columns(&mut layout, &rows, language, &mut warnings);
    }
    Ok(Report {
        kind,
        language,
        layout,
        rows,
        warnings,
    })
}
/// The BOQ says, line by line, whether a fresh reader measured the same quantity again. The
/// column is left out when nothing was checked. A line whose second reading differs is not a
/// BOQ row at all (it is a query); the report says which.
fn second_reading_column(
    layout: &mut Layout,
    rows: &mut [Value],
    items: &[crate::measurement::GeneratedItem],
    language: Language,
    warnings: &mut Vec<String>,
) {
    let arabic = language == Language::Ar;
    for item in items.iter().filter(|i| i.second_differs()) {
        let (first, second) = item
            .second_reading
            .as_ref()
            .map_or((0.0, 0.0), |s| (s.first, s.second));
        warnings.push(format!(
            "BOQ line {} is left out: the second reading ({second:.2}) differs from the first ({first:.2}). It is a query to answer",
            item.code
        ));
    }
    if layout.client.is_some() || !items.iter().any(|i| i.second_reading.is_some()) {
        return;
    }
    for row in rows.iter_mut() {
        let label = items
            .iter()
            .find(|i| row["code"].as_str() == Some(i.code.as_str()))
            .map_or("", |i| crate::second_reading::item_label(i, arabic));
        row["second_reading_text"] = json!(label);
    }
    layout.columns.push(Column {
        field: "second_reading_text".into(),
        heading: Heading {
            en: "Second reading".into(),
            ar: "القراءة الثانية".into(),
        },
        width: 2.0,
        total: false,
    });
}
pub fn project(
    set: &SheetSet,
    kind: Kind,
    language: Language,
    layout: Option<&str>,
) -> Result<Report, CoreError> {
    let p = set
        .project
        .as_ref()
        .ok_or_else(|| invalid("Not a project"))?;
    let layout: Layout = match layout {
        Some(name) => p
            .reports
            .get(name)
            .cloned()
            .ok_or_else(|| {
                let stored = p.reports.keys().cloned().collect::<Vec<_>>();
                invalid(format!(
                    "No report layout named {name}. Stored report layouts: {}. Leave out layout for the default.",
                    if stored.is_empty() { "none".into() } else { stored.join(", ") }
                ))
            })?,
        None => default_layout(kind),
    };
    let mut rows = match kind {
        Kind::Boq => p
            .boq
            .iter()
            .flat_map(|i| item_rows(std::slice::from_ref(&i.item), kind, ""))
            .collect(),
        Kind::Dimensions => p
            .boq
            .iter()
            .flat_map(|i| {
                i.contributions.iter().flat_map(|c| {
                    let mut item = i.item.clone();
                    item.lines = c.lines.clone();
                    item_rows(&[item], kind, &c.drawing)
                })
            })
            .collect(),
        Kind::Queries => query_rows(&p.queries),
        Kind::Register => p.register.iter().map(|r| json!(r)).collect(),
        Kind::Variation => p
            .variations
            .iter()
            .flat_map(|(drawing, v)| {
                v.items.iter().map(move |i| {
                    let mut r = json!(i);
                    r["drawing"] = json!(drawing);
                    r
                })
            })
            .collect(),
    };
    for row in &mut rows {
        // Project contributions retain the rules used by each drawing's measurement run.
        let decimals = p
            .boq
            .iter()
            .filter(|item| row["code"].as_str() == Some(item.item.code.as_str()))
            .flat_map(|item| &item.contributions)
            .filter(|contribution| {
                row["drawing"]
                    .as_str()
                    .is_none_or(|drawing| drawing.is_empty() || drawing == contribution.drawing)
            })
            .flat_map(|contribution| {
                contribution.explanation["measurement_rules"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(|rule| rule["rounding"]["decimals"].as_u64())
            .max();
        if let Some(decimals) = decimals {
            row["presentation_decimals"] = json!(decimals);
        }
    }
    localized(&mut rows, language);
    let mut warnings = p.warnings.clone();
    for d in &p.drawings {
        warnings.extend(
            d.warnings
                .iter()
                .chain(d.error.iter())
                .map(|s| format!("{}: {s}", d.drawing)),
        );
    }
    Ok(Report {
        kind,
        language,
        layout,
        rows,
        warnings,
    })
}
impl Session {
    pub(crate) fn prepare_deliverable(
        &self,
        kind: Kind,
        format: Format,
        layout: Option<&str>,
        path: &str,
        language: Language,
        project_path: Option<&str>,
    ) -> Result<Export, CoreError> {
        let (source, mut report) = if let Some(name) = project_path {
            let p = crate::projects::project_path(name)?;
            let report = project(&crate::projects::load(&p)?, kind, language, layout)?;
            (p, report)
        } else {
            (self.export_source(), drawing(self, kind, language, layout)?)
        };
        let ext = if format == Format::Excel {
            "xlsx"
        } else {
            "pdf"
        };
        let path = if path.trim().is_empty() {
            // Even a project report defaults beside this drawing's other exports. Explicit
            // project-relative paths and client templates retain their existing anchor.
            let source = self.export_source();
            let drawing = source.file_stem().unwrap_or_default().to_string_lossy();
            let report_name = match kind {
                Kind::Dimensions => "Dimensions",
                Kind::Boq => "BOQ",
                Kind::Queries => "Queries",
                Kind::Variation => "Variation",
                Kind::Register => "Register",
            };
            let base = format!("{drawing} - {report_name}");
            let mut n = 1u32;
            loop {
                let name = if n == 1 {
                    format!("{base}.{ext}")
                } else {
                    format!("{base} {n}.{ext}")
                };
                // A taken symlink name is skipped too; never follow it for a default export.
                if source.with_file_name(&name).symlink_metadata().is_err() {
                    break confined(&source, &name)?;
                }
                n = n
                    .checked_add(1)
                    .ok_or_else(|| invalid("No export name is available"))?;
            }
        } else {
            confined(&source, path)?
        };
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        {
            return Err(invalid(format!("Output must end in .{ext}")));
        }
        report.table()?;
        if let Some(client) = &mut report.layout.client {
            if kind != Kind::Boq || format != Format::Excel {
                return Err(invalid("Client templates require BOQ Excel output"));
            }
            let template = confined(&source, &client.path)?;
            if template == path {
                return Err(invalid(
                    "Write client quantities to a copy, never the source workbook",
                ));
            }
            client.path = template.to_string_lossy().into();
        }
        Ok(Export {
            path,
            format,
            report,
        })
    }
}
