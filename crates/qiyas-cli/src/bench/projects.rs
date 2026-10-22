//! Multi-drawing engine diagnostic. Input construction, prediction and truth scoring are separate.
use super::{
    model::{Item, Submission, Truth},
    RunMode,
};
use qiyas_core::{
    projects::{ProjectCommand, ProjectData},
    Command, Session, SheetSet, SheetSetDrawing, Source,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

pub(super) fn selected(root: &Path, set: Option<&str>, mode: RunMode) -> Result<Value, String> {
    if set.is_some_and(|s| s != "quick") {
        return Err("Project set must be quick".into());
    }
    let out = root.join("runs").join(format!(
        "projects-{}-{}",
        mode.name(),
        chrono::Utc::now().format("%Y%m%dT%H%M%S%f")
    ));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let report = run(root, mode, &out, set == Some("quick"))?;
    Ok(json!({"projects":report,"report_directory":out}))
}
fn groups(root: &Path, quick: bool) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut names = super::directories(&root.join("cases"))?;
    names.sort();
    let mut groups = vec![];
    // These are existing development source families, never product interpretation rules.
    for (name, prefix) in [
        (
            "clinic-storeys",
            "buildingsmart-clinic-Clinic_Architectural-",
        ),
        ("duplex-storeys", "buildingsmart-duplex-Duplex_A_20110907-"),
        ("apartment-floors", "apartment-"),
    ] {
        let ids = names
            .iter()
            .filter(|s| s.starts_with(prefix))
            .take(if quick { 3 } else { usize::MAX })
            .cloned()
            .collect::<Vec<_>>();
        if ids.len() >= 2 {
            groups.push((name.into(), ids));
        }
    }
    Ok(groups)
}
/// Add explicit check evidence to private copies. No gold quantity is read or exposed here.
fn build_inputs(
    root: &Path,
    ids: &[String],
    out: &Path,
    clean: bool,
) -> Result<(SheetSet, Vec<PathBuf>), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut set = SheetSet {
        name: "Project benchmark".into(),
        drawings: vec![],
        project: Some(ProjectData::default()),
    };
    let mut copies = vec![];
    for (index, id) in ids.iter().enumerate() {
        let source = super::safe_file(root, &Path::new("cases").join(id).join("drawing.dxf"))?;
        let path = out.join(format!("{id}.dxf"));
        let mut session = Session::open_standalone(&source).map_err(|e| e.to_string())?;
        if index < 2 {
            session.run_input(&json!({"type":"define_object_type","definition":{"name":"ProjectProbe","category":"Benchmark check evidence","geometry":"point","properties":{"key":{"kind":"text","default":""}},"quantities":{"n":"geometry.count"}}}),Source::User).map_err(|e|e.to_string())?;
            session.run_input(&json!({"type":"place_objects","objects":[{"type":"ProjectProbe","points":[[0,0]],"props":{"key":if clean&&index==1{"B"}else{"A"}}}]}),Source::User).map_err(|e|e.to_string())?;
        }
        if index == 0 {
            let rows = if clean {
                json!([["A", "1"], ["B", "1"]])
            } else {
                json!([["A", "3"]])
            };
            session.run_input(&json!({"type":"set_knowledge","kind":"tables","name":"project-schedule","data":{"rows":rows,"citation":{"source":"Synthetic cross-drawing schedule","clause":"Declared input rows"}}}),Source::User).map_err(|e|e.to_string())?;
        }
        session.save_as_headless(&path).map_err(|e| e.to_string())?;
        copies.push(path.canonicalize().map_err(|e| e.to_string())?);
        set.drawings.push(SheetSetDrawing {
            path: copies.last().unwrap().to_string_lossy().into(),
            layouts: vec![],
        });
    }
    let cite = json!({"source":"Synthetic benchmark check specification","clause":"Identity and schedule consistency"});
    let checks = json!({
        "schedule":{"citation":cite,"question_en":"Confirm schedule count?","question_ar":"تأكيد عدد الجدول؟","test":{"kind":"schedule","table":{"kind":"document","drawing":set.drawings[0].path,"name":"project-schedule"},"key_column":0,"value_column":1,"elements":{"filter":{"type":"ProjectProbe"}},"property":"key"}},
        "duplicate":{"citation":cite,"question_en":"Confirm duplicate identity?","question_ar":"تأكيد الهوية المكررة؟","test":{"kind":"relation","left":{"filter":{"type":"ProjectProbe"}},"right":{"filter":{"type":"ProjectProbe"}},"join":"key","right_property":"key","minimum":1,"maximum":1}}
    });
    set.project.as_mut().unwrap().checks =
        serde_json::from_value(checks).map_err(|e| e.to_string())?;
    // This file belongs to the scorer. Prediction only gets the set and drawing paths.
    super::write_json(
        &out.join("check-truth.json"),
        &json!({"rules":if clean{vec![]}else{vec!["schedule","duplicate"]}}),
    )?;
    Ok((set, copies))
}
fn predict(
    root: &Path,
    ids: &[String],
    mut set: SheetSet,
    copies: &[PathBuf],
    mode: RunMode,
    qss: &Path,
    out: &Path,
) -> Result<SheetSet, String> {
    for (id, path) in ids.iter().zip(copies) {
        let case = root.join("cases").join(id);
        let general = mode == RunMode::Autonomy || !case.join("recognition.json").is_file();
        let recipe = if general {
            root.join("general/recognition.json")
        } else {
            case.join("recognition.json")
        };
        let mut session = super::recognition::revision_base(path, &recipe, general, None)?;
        // Each mapping remains drawing-owned. Source ids/handles survive saving the private copy.
        session.save_as_headless(path).map_err(|e| e.to_string())?;
    }
    set.name = qss.file_stem().unwrap_or_default().to_string_lossy().into();
    set.save(qss).map_err(|e| e.to_string())?;
    let mut workspace = qiyas_boq::project::session()?;
    workspace
        .run(Command::Project(Box::new(ProjectCommand::Run {
            path: qss.to_string_lossy().into(),
        })))
        .map_err(|e| e.to_string())?;
    let set = qiyas_core::projects::load(qss).map_err(|e| e.to_string())?;
    qiyas_boq::project::write_outputs(qss, &set, Some(out))?;
    if qiyas_boq::project::import(&out.join("project.xlsx")).map_err(|e| e.to_string())? != set {
        return Err("Project workbook did not round trip".into());
    }
    super::write_json(&out.join("prediction.json"), &set)?;
    Ok(set)
}
/// Preserve the interpretation recipe's declared basis; never replace it with a gold label.
fn submission(prediction: Option<&SheetSet>) -> Result<Submission, String> {
    let mut submission = Submission::default();
    let mut recipes = BTreeMap::<String, Value>::new();
    if let Some(project) = prediction.and_then(|s| s.project.as_ref()) {
        for row in &project.boq {
            let mut handles = std::collections::BTreeSet::new();
            let mut bases = std::collections::BTreeSet::new();
            for c in &row.contributions {
                for l in c.lines.iter().filter(|l| l.included) {
                    for citation in &l.citations {
                        let path = Path::new(&citation.source);
                        if path.file_name().is_none_or(|n| n != "recognition.json") {
                            continue;
                        }
                        if !recipes.contains_key(&citation.source) {
                            let recipe: Value = serde_json::from_slice(
                                &std::fs::read(path).map_err(|e| e.to_string())?,
                            )
                            .map_err(|e| e.to_string())?;
                            recipes.insert(citation.source.clone(), recipe);
                        }
                        if let Some(measurements) =
                            recipes[&citation.source]["measurements"].as_array()
                        {
                            for measurement in measurements
                                .iter()
                                .filter(|m| m["quantity"] == l.label && m["unit"] == row.item.unit)
                            {
                                bases.insert(
                                    measurement["basis"]
                                        .as_str()
                                        .unwrap_or("geometric")
                                        .to_owned(),
                                );
                            }
                        }
                    }
                    for r in &l.sources {
                        handles.insert(format!("{}:{:X}", c.drawing, r.handle));
                    }
                }
            }
            submission.items.push(Item {
                id: row.item.code.clone(),
                description: row.item.description_en.clone(),
                unit: row.item.unit.clone(),
                kind: super::model::kind_from_unit(&row.item.unit)?,
                quantity: row.item.quantity,
                source_objects: handles.into_iter().collect(),
                basis: if bases.len() == 1 {
                    bases.into_iter().next().unwrap()
                } else {
                    String::new()
                },
                ..Default::default()
            });
        }
    }
    Ok(submission)
}
fn score(
    root: &Path,
    ids: &[String],
    copies: &[PathBuf],
    prediction: Option<&SheetSet>,
    out: &Path,
) -> Result<Value, String> {
    let submission = submission(prediction)?;
    super::write_json(&out.join("submission.json"), &submission)?;
    let mut items: BTreeMap<(String, String, String), Item> = BTreeMap::new();
    for (id, path) in ids.iter().zip(copies) {
        let (truth, _) = super::load_case(root, id)?;
        for item in truth.items {
            let entry = items
                .entry((item.id.clone(), item.unit.clone(), item.kind.clone()))
                .or_insert_with(|| {
                    let mut item = item.clone();
                    item.quantity = 0.;
                    item.source_objects.clear();
                    item.position = None;
                    item
                });
            entry.quantity += item.quantity;
            entry.source_objects.extend(
                item.source_objects
                    .iter()
                    .map(|h| format!("{}:{h}", path.display())),
            );
        }
    }
    let truth = Truth {
        schema_version: 1,
        id: "project".into(),
        project_type: "project".into(),
        drawing: String::new(),
        items: items.into_values().collect(),
        rules: Default::default(),
        seeded_errors: vec![],
    };
    let metrics = super::score::score(&truth, &submission, None);
    let check_truth: Value = serde_json::from_slice(
        &std::fs::read(out.join("check-truth.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let expected = check_truth["rules"].as_array().unwrap();
    let predicted: std::collections::BTreeSet<_> = prediction
        .and_then(|s| s.project.as_ref())
        .into_iter()
        .flat_map(|p| &p.queries)
        .filter_map(|q| q.query.as_ref().and_then(|q| q.rule.clone()))
        .collect();
    let found = expected
        .iter()
        .filter(|r| r.as_str().is_some_and(|r| predicted.contains(r)))
        .count();
    Ok(
        json!({"boq":metrics,"checks":{"expected":expected.len(),"found":found,"predicted":predicted.len()},"aggregate_items":submission.items.len()}),
    )
}
pub(super) fn run(root: &Path, mode: RunMode, out: &Path, quick: bool) -> Result<Value, String> {
    let groups = groups(root, quick)?;
    let mut reports = vec![];
    for (name, ids) in groups {
        for clean in [false, true] {
            let name = format!("{name}-{}", if clean { "clean" } else { "seeded" });
            let folder = out.join(&name);
            let (set, copies) = build_inputs(root, &ids, &folder, clean)?;
            let qss = qiyas_core::projects::project_path(&format!(
                "bench-{}-{name}-{}.qss",
                mode.name(),
                chrono::Utc::now().format("%Y%m%dT%H%M%S%f")
            ))
            .map_err(|e| e.to_string())?;
            let start = Instant::now();
            let prediction = predict(root, &ids, set, &copies, mode, &qss, &folder);
            let mut result = score(root, &ids, &copies, prediction.as_ref().ok(), &folder)?;
            result["name"] = json!(name);
            result["drawings"] = json!(ids);
            result["project"] = json!(qss);
            result["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
            result["error"] = json!(prediction.err());
            reports.push(result);
        }
    }
    let result = summarize(reports, mode.name())?;
    super::write_json(&out.join("project-report.json"), &result)?;
    Ok(result)
}
fn summarize(reports: Vec<Value>, mode: &str) -> Result<Value, String> {
    let mut metrics = super::score::Metrics::default();
    let (mut expected, mut found, mut predicted) = (0u64, 0u64, 0u64);
    for result in &reports {
        let m: super::score::Metrics =
            serde_json::from_value(result["boq"]["metrics"].clone()).map_err(|e| e.to_string())?;
        metrics.merge(&m);
        expected += result["checks"]["expected"].as_u64().unwrap();
        found += result["checks"]["found"].as_u64().unwrap();
        predicted += result["checks"]["predicted"].as_u64().unwrap();
    }
    let result = json!({"cases":reports,"mode":mode,"diagnostic":"Engine diagnostic; not live MIZAN ALONE. Check probes are synthetic, BOQ truth is from the original development cases.","boq":metrics,"rates":metrics.rates(),"checks":{"expected":expected,"found":found,"predicted":predicted,"recall":(expected>0).then(||found as f64/expected as f64),"precision":(predicted>0).then(||found as f64/predicted as f64)}});
    Ok(result)
}

/// Only the scorer runs: prediction snapshots and workbooks stay unchanged.
pub(super) fn rescore(root: &Path, out: &Path) -> Result<Value, String> {
    let old: Value = serde_json::from_slice(
        &std::fs::read(out.join("project-report.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut reports = vec![];
    for case in old["cases"].as_array().ok_or("Missing project cases")? {
        let name = case["name"].as_str().ok_or("Missing project name")?;
        let folder = out.join(name);
        let ids: Vec<String> =
            serde_json::from_value(case["drawings"].clone()).map_err(|e| e.to_string())?;
        let prediction: Option<SheetSet> = if case["error"].is_null() {
            Some(
                serde_json::from_slice(
                    &std::fs::read(folder.join("prediction.json")).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let copies: Vec<_> = ids
            .iter()
            .map(|id| folder.join(format!("{id}.dxf")))
            .map(|p| p.canonicalize().map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let mut result = score(root, &ids, &copies, prediction.as_ref(), &folder)?;
        for key in ["name", "drawings", "project", "elapsed_ms", "error"] {
            result[key] = case[key].clone();
        }
        reports.push(result);
    }
    let result = summarize(reports, old["mode"].as_str().ok_or("Missing project mode")?)?;
    super::write_json(&out.join("project-rescored.json"), &result)?;
    Ok(json!({"projects":result,"report_directory":out,"rescored":true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_inputs_are_valid_and_scoring_preserves_declared_basis_without_truth() {
        let dir = tempfile::tempdir().unwrap();
        let ids = vec!["a".to_owned(), "b".to_owned()];
        for id in &ids {
            let folder = dir.path().join("cases").join(id);
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join("drawing.dxf");
            Session::new_blank(&path, 6)
                .save_as_headless(&path)
                .unwrap();
        }
        for clean in [false, true] {
            let (set, copies) = build_inputs(
                dir.path(),
                &ids,
                &dir.path().join(format!("inputs-{clean}")),
                clean,
            )
            .unwrap();
            assert_eq!(set.drawings.len(), 2);
            assert_eq!(copies.len(), 2);
            assert_eq!(set.project.unwrap().checks.len(), 2);
        }
        let recipe = dir.path().join("recognition.json");
        super::super::write_json(
            &recipe,
            &json!({"measurements":[
                {"quantity":"net","unit":"m2","basis":"rule:declared"},
                {"quantity":"other","unit":"m2","basis":"rule:other"}
            ]}),
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("truth.json")).unwrap();
        let mut set = SheetSet {
            name: "Test".into(),
            drawings: vec![],
            project: Some(ProjectData::default()),
        };
        set.project.as_mut().unwrap().boq = serde_json::from_value(json!([{
            "item":{"code":"N","description_en":"Net","description_ar":"Net","section":"","trade":"","unit":"m2","quantity":3,"mapping_citations":[],"coverage":[],"lines":[]},
            "contributions":[{"drawing":"a.dxf","code":"N","quantity":3,"lines":[{
                "label":"net","operation":"add","timesing":1,"factors":[3],"source_expressions":["area"],"expression":"3","value":3,"unit":"m2","method":"supplied","citations":[{"source":recipe,"clause":"Input mapping"}],"sources":[{"handle":16,"sheet":"*Model_Space","grid":null,"evidence":[]}],"included":true,"decision":null
            }]}]
        }])).unwrap();
        let submitted = submission(Some(&set)).unwrap();
        assert_eq!(submitted.items[0].basis, "rule:declared");
        assert_eq!(submitted.items[0].quantity, 3.);
        assert_eq!(submitted.items[0].source_objects, ["a.dxf:10"]);
    }
}
