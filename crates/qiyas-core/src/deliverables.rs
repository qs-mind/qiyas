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
    #[schemars(example=serde_json::json!({"kind":"export","report":"boq","format":"excel","path":"boq.xlsx"}))]
    #[schemars(extend("x-takeoff-kit" = {"order":90}))]
    Export {
        report: Kind,
        format: Format,
        #[serde(default)]
        layout: Option<String>,
        path: String,
        #[serde(default)]
        language: Language,
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
                    .map(|f| display(&row[f]))
                    .collect();
                // Never add quantities expressed in different units.
                if self.layout.columns.iter().any(|c| c.total) {
                    key.push(display(&row["unit"]));
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
            });
            for (key, rows) in groups {
                if !key.is_empty() {
                    result.push(TableRow {
                        cells: vec![json!(key.join(" / "))],
                        heading: true,
                    });
                }
                let mut totals = vec![0.; self.layout.columns.len()];
                for row in rows {
                    let mut cells = vec![];
                    for (i, c) in self.layout.columns.iter().enumerate() {
                        let value = row
                            .get(&c.field)
                            .ok_or_else(|| invalid(format!("Unknown report field: {}", c.field)))?;
                        if c.total {
                            totals[i] += value.as_f64().ok_or_else(|| {
                                invalid("Only computed numeric columns can be totalled")
                            })?;
                        }
                        cells.push(value.clone());
                    }
                    result.push(TableRow {
                        cells,
                        heading: false,
                    });
                }
                if self.layout.columns.iter().any(|c| c.total) {
                    result.push(TableRow {
                        cells: vec![json!(self.layout.total_heading.text(self.language))],
                        heading: true,
                    });
                    result.push(TableRow {
                        cells: self
                            .layout
                            .columns
                            .iter()
                            .zip(totals)
                            .map(|(c, n)| if c.total { json!(n) } else { Value::Null })
                            .collect(),
                        heading: true,
                    });
                }
            }
        }
        Ok(result)
    }
}
pub fn display(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
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
            .map(|i| {
                let mut v = json!(i);
                v["drawing"] = json!(drawing);
                v
            })
            .collect();
    }
    items
        .iter()
        .flat_map(|item| {
            item.lines.iter().map(move |line| {
                let mut row = json!(line);
                row["code"] = json!(item.code);
                row["section"] = json!(item.section);
                row["drawing"] = json!(drawing);
                row["reference"] = json!(line
                    .sources
                    .iter()
                    .map(|s| format!(
                        "{drawing} / {} / {} / {}",
                        s.handle,
                        s.sheet,
                        s.grid.as_deref().unwrap_or("?")
                    ))
                    .collect::<Vec<_>>()
                    .join("; "));
                row["squaring"] = json!(if line.included { line.value } else { 0. });
                row
            })
        })
        .collect()
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
pub fn drawing(
    session: &Session,
    kind: Kind,
    language: Language,
    layout: Option<&str>,
) -> Result<Report, CoreError> {
    let layout: Layout = match layout {
        Some(name) => serde_json::from_value(
            knowledge::get(&session.drawing.doc, knowledge::Kind::Reports, name)?
                .ok_or_else(|| invalid("Report layout not found"))?
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
    localized(&mut rows, language);
    Ok(Report {
        kind,
        language,
        layout,
        rows,
        warnings,
    })
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
            .ok_or_else(|| invalid("Report layout not found"))?,
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
            (
                self.drawing.path.clone(),
                drawing(self, kind, language, layout)?,
            )
        };
        let path = confined(&source, path)?;
        let ext = if format == Format::Excel {
            "xlsx"
        } else {
            "pdf"
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
