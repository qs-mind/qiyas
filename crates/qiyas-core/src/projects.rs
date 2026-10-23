//! Project quantities and evidence live in the sheet set; drawing knowledge stays in drawings.
mod register;
use crate::{
    discrepancy::{self, CheckRule},
    measurement::{DimensionLine, GeneratedItem, MeasurementCommand, MethodScope, RuleSet},
    Command, Conflict, CoreError, Outcome, Session, SheetSet, Source,
};
pub use register::read as drawing_register;
pub use register::{RegisterField, RegisterRow, RegisterRule};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectData {
    pub confidence_summary: crate::trust::ConfidenceSummary,
    pub reports: BTreeMap<String, crate::deliverables::Layout>,
    pub variations: BTreeMap<String, crate::revisions::VariationReport>,
    pub method: Option<RuleSet>,
    pub checks: BTreeMap<String, CheckRule>,
    pub register_rules: Vec<RegisterRule>,
    pub boq: Vec<ProjectItem>,
    pub queries: Vec<Conflict>,
    pub register: Vec<RegisterRow>,
    pub drawings: Vec<DrawingStatus>,
    pub warnings: Vec<String>,
    pub checked: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    #[serde(default = "crate::trust::unscored")]
    pub confidence: f64,
    #[serde(default)]
    pub explanation: Value,
    pub drawing: String,
    pub code: String,
    pub quantity: f64,
    pub lines: Vec<DimensionLine>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectItem {
    pub item: GeneratedItem,
    pub contributions: Vec<Contribution>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingStatus {
    pub drawing: String,
    pub status: String,
    pub items: usize,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectCommand {
    /// Create a project in the Qiyas data home's projects folder. Never overwrites a project.
    #[schemars(example = json!({"kind":"create","name":"Project"}))]
    Create { name: String },
    /// Read a project and its drawings and status. Does not change the drawing on screen.
    #[schemars(extend("x-effect" = "read"))]
    #[schemars(example = json!({"kind":"open","path":"Project.qss"}))]
    Open { path: String },
    /// List projects in the Qiyas data home, including errors for unreadable files.
    #[schemars(extend("x-effect" = "read"))]
    #[schemars(example = json!({"kind":"list"}))]
    List,
    /// Add a drawing by path; duplicate files are refused. Empty layouts includes every sheet.
    #[schemars(example = json!({"kind":"add_drawing","path":"Project.qss","drawing":"plan.dxf"}))]
    AddDrawing {
        path: String,
        drawing: String,
        #[serde(default)]
        layouts: Vec<String>,
    },
    /// Remove a drawing and invalidate the previous aggregate. One undo step.
    #[schemars(example = json!({"kind":"remove_drawing","path":"Project.qss","drawing":"plan.dxf"}))]
    RemoveDrawing { path: String, drawing: String },
    /// Set the method override for every project drawing; null restores each drawing's Settings method.
    #[schemars(example = json!({"kind":"set_method","path":"Project.qss","method":null}))]
    SetMethod {
        path: String,
        method: Option<Box<RuleSet>>,
    },
    /// Replace project checks with cited Phase 19 rules. Set.drawings selects project paths;
    /// table.drawing names the drawing containing the schedule or extracted document table.
    #[schemars(example = json!({"kind":"set_checks","path":"Project.qss","checks":{}}))]
    SetChecks {
        path: String,
        checks: BTreeMap<String, CheckRule>,
    },
    /// Supply title-block attribute/text extraction rules. Missing or ambiguous fields are flagged.
    #[schemars(example = json!({"kind":"set_register_rules","path":"Project.qss","rules":[{"field":"number","attribute_tag":"SHEET"}]}))]
    SetRegisterRules {
        path: String,
        rules: Vec<RegisterRule>,
    },
    /// Replace project report layouts. One undo step; layouts contain no trade rules.
    #[schemars(example = json!({"kind":"set_reports","path":"Project.qss","reports":{}}))]
    SetReports {
        path: String,
        reports: BTreeMap<String, crate::deliverables::Layout>,
    },
    /// Run every drawing through the existing script take-off machinery, aggregate and check.
    /// Uses drawing-owned recognition/mappings; missing interpretation is reported, never invented.
    #[schemars(extend("x-takeoff-step" = "project / open, interpret each drawing, run aggregate and cross-drawing checks"))]
    #[schemars(example = json!({"kind":"run","path":"Project.qss"}))]
    Run { path: String },
}
impl ProjectCommand {
    pub fn is_read(&self) -> bool {
        matches!(self, Self::Open { .. } | Self::List)
    }
    fn path(&self) -> Result<Option<PathBuf>, CoreError> {
        let name = match self {
            Self::List => return Ok(None),
            Self::Create { name } => {
                if name.trim().is_empty()
                    || name.contains(['/', '\\', ':'])
                    || name == "."
                    || name == ".."
                {
                    return Err(invalid("Use a project name without path separators"));
                }
                return project_path(&format!("{name}.qss")).map(Some);
            }
            Self::Open { path }
            | Self::AddDrawing { path, .. }
            | Self::RemoveDrawing { path, .. }
            | Self::SetMethod { path, .. }
            | Self::SetChecks { path, .. }
            | Self::SetRegisterRules { path, .. }
            | Self::SetReports { path, .. }
            | Self::Run { path } => path,
        };
        project_path(name).map(Some)
    }
}
pub fn project_path(path: &str) -> Result<PathBuf, CoreError> {
    let home = crate::data_home::qiyas_dir().map_err(invalid)?;
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        home.join("projects").join(path)
    };
    if !crate::data_home::is_project_path(&path) {
        return Err(invalid("Projects live in qiyas_dir()/projects/*.qss"));
    }
    Ok(path)
}
pub fn load(path: &Path) -> Result<SheetSet, CoreError> {
    let set = SheetSet::load(path)?;
    if set.project.is_none() {
        return Err(invalid("This sheet set is not a project"));
    }
    Ok(set)
}
pub fn list() -> Result<Value, CoreError> {
    let dir = crate::data_home::qiyas_dir()
        .map_err(invalid)?
        .join("projects");
    let mut files = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    let mut projects = vec![];
    for path in files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "qss"))
    {
        match load(&path) {
            Ok(set) => projects.push(json!({"path":path,"name":set.name,"drawings":set.drawings.len(),"checked":set.project.as_ref().is_some_and(|p|p.checked)})),
            Err(e) => projects.push(json!({"path":path,"name":path.file_stem().unwrap_or_default(),"error":e.to_string()})),
        }
    }
    Ok(json!({"projects":projects}))
}
fn invalidate(project: &mut ProjectData) {
    project.variations.clear();
    project.boq.clear();
    project.confidence_summary = Default::default();
    project.queries.clear();
    project.register.clear();
    project.drawings.clear();
    project.warnings.clear();
    project.checked = false;
}
impl Session {
    pub(crate) fn project_command(
        &mut self,
        command: ProjectCommand,
        source: Source,
    ) -> Result<Outcome, CoreError> {
        if self.tx_depth > 0 {
            return Err(invalid(
                "Project commands run on their own, outside drawing batches",
            ));
        }
        let label = Command::Project(Box::new(command.clone())).label();
        let Some(path) = command.path()? else {
            return Ok(Outcome::Project { result: list()? });
        };
        let before = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let mut set = match &command {
            ProjectCommand::Create { name } => {
                if before.is_some() {
                    return Err(invalid("Project already exists"));
                }
                SheetSet {
                    name: name.clone(),
                    drawings: vec![],
                    project: Some(ProjectData::default()),
                }
            }
            _ => load(&path)?,
        };
        let mutating = !command.is_read();
        match command {
            ProjectCommand::Create { .. } | ProjectCommand::Open { .. } => {}
            ProjectCommand::AddDrawing {
                drawing, layouts, ..
            } => {
                let entry = crate::sheet_set_entry(&path, Path::new(&drawing), layouts)?;
                let full = Path::new(&entry.path);
                if !full
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
                {
                    return Err(invalid(
                        "Add a DWG or DXF drawing; import PDF pages into a drawing first",
                    ));
                }
                if crate::sheet_set_entries(&path, &set)
                    .iter()
                    .any(|e| Path::new(&e.full).canonicalize().ok().as_deref() == Some(full))
                {
                    return Err(invalid("Drawing already belongs to this project"));
                }
                set.drawings.push(entry);
                invalidate(set.project.as_mut().unwrap());
            }
            ProjectCommand::RemoveDrawing { drawing, .. } => {
                let index = crate::sheet_set_entries(&path, &set)
                    .iter()
                    .position(|e| {
                        e.path == drawing
                            || e.full == drawing
                            || Path::new(&drawing)
                                .canonicalize()
                                .ok()
                                .is_some_and(|p| Path::new(&e.full).canonicalize().ok() == Some(p))
                    })
                    .ok_or_else(|| invalid("Drawing is not in the project"))?;
                set.drawings.remove(index);
                invalidate(set.project.as_mut().unwrap());
            }
            ProjectCommand::SetMethod { method, .. } => {
                if let Some(method) = &method {
                    method.validate()?;
                }
                let project = set.project.as_mut().unwrap();
                project.method = method.map(|m| *m);
                invalidate(project);
            }
            ProjectCommand::SetChecks { checks, .. } => {
                for rule in checks.values() {
                    rule.validate()?;
                }
                let project = set.project.as_mut().unwrap();
                project.checks = checks;
                invalidate(project);
            }
            ProjectCommand::SetRegisterRules { rules, .. } => {
                for rule in &rules {
                    rule.validate()?;
                }
                let project = set.project.as_mut().unwrap();
                project.register_rules = rules;
                invalidate(project);
            }
            ProjectCommand::SetReports { reports, .. } => {
                for report in reports.values() {
                    report.validate()?;
                }
                set.project.as_mut().unwrap().reports = reports;
            }
            ProjectCommand::Run { .. } => {
                evaluate(&path, &mut set)?;
            }
            ProjectCommand::List => unreachable!(),
        }
        if mutating {
            set.save(&path)?;
            let after = std::fs::read(&path)?;
            self.history.record(
                crate::undo::Step::ProjectFile(FileChange {
                    path: path.clone(),
                    before,
                    after: Some(after),
                }),
                source,
                &label,
            );
        }
        Ok(Outcome::Project {
            result: json!({"path":path,"entries":crate::sheet_set_entries(&path,&set),"set":set}),
        })
    }
}
/// One project edit. Undo refuses to overwrite a project edited elsewhere.
#[derive(Debug)]
pub(crate) struct FileChange {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}
impl FileChange {
    pub fn apply(&self) -> Result<(), CoreError> {
        let current = match std::fs::read(&self.path) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if current != self.after {
            return Err(invalid("Project changed since this command"));
        }
        if let Some(bytes) = &self.before {
            crate::project::write_atomic(&self.path, bytes)?;
        } else if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        Ok(())
    }
    pub fn reversed(self) -> Self {
        Self {
            path: self.path,
            before: self.after,
            after: self.before,
        }
    }
}

pub fn aggregate(reports: &[(String, crate::measurement::MeasurementReport)]) -> Vec<ProjectItem> {
    let key = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let mut items: BTreeMap<(String, String, String, String), ProjectItem> = BTreeMap::new();
    for (drawing, report) in reports {
        for item in &report.items {
            let identity = if item.code.trim().is_empty() {
                format!("description:{}", key(&item.description_en))
            } else {
                format!("code:{}", key(&item.code))
            };
            let entry = items
                .entry((
                    key(&item.section),
                    key(&item.trade),
                    item.unit.clone(),
                    identity,
                ))
                .or_insert_with(|| {
                    let mut item = item.clone();
                    item.quantity = 0.;
                    item.lines.clear();
                    ProjectItem {
                        item,
                        contributions: vec![],
                    }
                });
            entry.item.confidence = entry.item.confidence.min(item.confidence);
            entry.item.quantity += item.quantity;
            for citation in &item.mapping_citations {
                if !entry.item.mapping_citations.contains(citation) {
                    entry.item.mapping_citations.push(citation.clone());
                }
            }
            for note in &item.coverage {
                if !entry.item.coverage.contains(note) {
                    entry.item.coverage.push(note.clone());
                }
            }
            entry.contributions.push(Contribution {
                confidence: item.confidence,
                explanation: Value::Null,
                drawing: drawing.clone(),
                code: item.code.clone(),
                quantity: item.quantity,
                lines: item.lines.clone(),
            });
        }
    }
    items.into_values().collect()
}
struct CheckDrawings<'a>(&'a [(String, Session)]);
impl discrepancy::rules::CheckContext for CheckDrawings<'_> {
    fn elements(
        &self,
        set: &discrepancy::Set,
    ) -> Result<Vec<discrepancy::rules::CheckObject>, CoreError> {
        for path in &set.drawings {
            if !self.0.iter().any(|(p, _)| p == path) {
                return Err(invalid(format!("Check drawing is missing: {path}")));
            }
        }
        let mut objects = vec![];
        for (path, session) in self
            .0
            .iter()
            .filter(|(p, _)| set.drawings.is_empty() || set.drawings.contains(p))
        {
            objects.extend(
                discrepancy::rules::local_elements(session, set)?
                    .into_iter()
                    .map(|object| discrepancy::rules::CheckObject {
                        object,
                        drawing: Some(path.clone()),
                    }),
            );
        }
        Ok(objects)
    }
    fn table(
        &self,
        source: &discrepancy::TableSource,
    ) -> Result<discrepancy::rules::ReadTable, CoreError> {
        let drawing = match source {
            discrepancy::TableSource::Drawing { drawing, .. }
            | discrepancy::TableSource::Document { drawing, .. } => drawing,
        };
        let path = drawing
            .as_deref()
            .ok_or_else(|| invalid("Project tables need a drawing path"))?;
        let (_, session) = self
            .0
            .iter()
            .find(|(p, _)| p == path)
            .ok_or_else(|| invalid(format!("Table drawing is missing: {path}")))?;
        let (rows, mut refs, mut citation) = discrepancy::rules::local_table(session, source)?;
        citation.source = format!("{path} :: {}", citation.source);
        for r in &mut refs {
            r.drawing = Some(path.into());
        }
        Ok((rows, refs, citation))
    }
}
struct BatchHost;
impl crate::script::Host for BatchHost {
    fn export_takeoff(&mut self, _: &Session, _: &Path) -> Result<(), String> {
        Err("Project exports after aggregation".into())
    }
    fn import_boq(&mut self, _: &Path) -> Result<crate::Boq, String> {
        Err("Project batch uses drawing-owned inputs".into())
    }
    fn compare(&mut self, s: &mut Session) -> Result<usize, String> {
        s.run(Command::Check { tolerance: None })
            .map_err(|e| e.to_string())?;
        Ok(s.conflicts.len())
    }
}
/// Reuses script parsing/execution from batch, one isolated drawing session at a time.
pub fn evaluate(path: &Path, set: &mut SheetSet) -> Result<(), CoreError> {
    if set.drawings.is_empty() {
        return Err(invalid("Project has no drawings"));
    }
    let old = set
        .project
        .clone()
        .ok_or_else(|| invalid("Not a project"))?;
    let mut result = old.clone();
    invalidate(&mut result);
    let mut sessions = vec![];
    let mut reports = vec![];
    for entry in crate::sheet_set_entries(path, set) {
        let process = || -> Result<(Session, Vec<String>), CoreError> {
            let mut session = Session::open(Path::new(&entry.full))?;
            let mut lines = vec![];
            if old.method.is_some() {
                lines.push(
                    json!(Command::Measurement(Box::new(
                        MeasurementCommand::SetMethod {
                            scope: MethodScope::Project,
                            method: old.method.clone().map(Box::new)
                        }
                    )))
                    .to_string(),
                );
            }
            for sheet in session.drawing.sheets() {
                lines.push(json!({"type":"set_sheet","block":sheet.block}).to_string());
                lines.push(json!({"type":"recognition","kind":"apply"}).to_string());
            }
            lines.push(json!({"type":"measurement","kind":"report"}).to_string());
            let script = crate::script::parse(
                &lines.join("\n"),
                &crate::script::Context {
                    script: path,
                    drawing: Some(Path::new(&entry.full)),
                    out_dir: None,
                },
            )
            .map_err(|e| invalid(e.to_string()))?;
            let run = crate::script::run(&script, Some(session), &mut BatchHost)
                .map_err(|e| invalid(e.to_string()))?;
            session = run
                .session
                .ok_or_else(|| invalid("Project batch lost drawing"))?;
            Ok((session, run.warnings))
        };
        let processed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(process))
            .unwrap_or_else(|_| Err(invalid("Drawing reader crashed")));
        match processed {
            Ok((session, batch_warnings)) => {
                let report = session.measurement_report()?;
                if let Some(variation) = session.variation()? {
                    result.variations.insert(entry.path.clone(), variation);
                }
                for mut query in session.conflicts.iter().cloned() {
                    query.id = format!("{}::{}", entry.path, query.id);
                    query.sheets = query
                        .sheets
                        .iter()
                        .map(|sheet| format!("{} :: {sheet}", entry.path))
                        .collect();
                    if let Some(details) = &mut query.query {
                        for reference in &mut details.references {
                            reference.drawing = Some(entry.path.clone());
                        }
                    }
                    result.queries.push(query);
                }
                let mut warnings = report.warnings.clone();
                warnings.extend(batch_warnings);
                if report.items.is_empty() {
                    warnings.push(
                        "No generated BOQ: interpret the drawing and supply method/mappings".into(),
                    );
                }
                result.register.extend(register::read(
                    &session,
                    &entry.path,
                    &entry.layouts,
                    &old.register_rules,
                )?);
                result.drawings.push(DrawingStatus {
                    drawing: entry.path.clone(),
                    status: if warnings.is_empty() { "ok" } else { "warning" }.into(),
                    items: report.items.len(),
                    warnings,
                    error: None,
                });
                reports.push((entry.path.clone(), report));
                sessions.push((entry.path, session));
            }
            Err(error) => result.drawings.push(DrawingStatus {
                drawing: entry.path,
                status: "error".into(),
                items: 0,
                warnings: vec![],
                error: Some(error.to_string()),
            }),
        }
    }
    result.boq = aggregate(&reports);
    for row in &mut result.boq {
        for contribution in &mut row.contributions {
            if let Some((_, session)) = sessions
                .iter()
                .find(|(path, _)| path == &contribution.drawing)
            {
                contribution.explanation =
                    session.explain(&format!("generated:{}", contribution.code))?;
            }
        }
    }
    result.boq.sort_by(|a, b| {
        a.item
            .confidence
            .total_cmp(&b.item.confidence)
            .then(a.item.code.cmp(&b.item.code))
    });
    result.confidence_summary =
        crate::trust::ConfidenceSummary::of(result.boq.iter().map(|r| r.item.confidence));
    if result.drawings.iter().any(|d| d.error.is_some()) {
        result
            .warnings
            .push("Cross-drawing checks incomplete: one or more drawings failed".into());
    } else {
        for (name, rule) in &old.checks {
            match discrepancy::rules::evaluate_named(
                &CheckDrawings(&sessions),
                &[(name.clone(), rule.clone())],
            ) {
                Ok(queries) => result.queries.extend(queries),
                Err(e) => result
                    .warnings
                    .push(format!("Check {name} could not run: {e}")),
            }
        }
        result.checked = result.warnings.is_empty();
    }
    for query in &mut result.queries {
        if let Some(previous) = old.queries.iter().find(|q| q.id == query.id) {
            query.status = previous.status;
            query.note = previous.note.clone();
            query.created = previous.created.clone();
        }
    }
    set.project = Some(result);
    Ok(())
}
