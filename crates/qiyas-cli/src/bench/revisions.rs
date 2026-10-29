//! Revision engine diagnostic. Prediction finishes and writes artifacts before truth is read.
use super::{model::Truth, RunMode};
use qiyas_core::{revisions::VariationReport, Command, Session};
use serde_json::{json, Value};
use std::{path::Path, time::Instant};

pub(super) fn selected(root: &Path, set: Option<&str>, mode: RunMode) -> Result<Value, String> {
    let ids = if let Some(set) = set {
        super::sets::select(root, set)?
    } else {
        super::dev_ids(root, false)?
    };
    let out = root.join("runs").join(format!(
        "revisions-{}-{}-{}",
        mode.name(),
        chrono::Utc::now().format("%Y%m%dT%H%M%S%f"),
        std::process::id()
    ));
    super::reject_symlink(root)?;
    if root.join("runs").exists() {
        super::reject_symlink(&root.join("runs"))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let result = run(root, &ids, mode, &out)?;
    Ok(json!({"mode":mode.name(),"set":set,"report_directory":out,"revisions":result}))
}

pub(super) fn predict(
    a: &Path,
    b: &Path,
    recipe: &Path,
    general: bool,
    supplement: Option<&Path>,
    out: &Path,
) -> Result<(VariationReport, f64), String> {
    let started = Instant::now();
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut a = super::recognition::revision_base(a, recipe, general, supplement)?;
    let path = out.join("A.dxf");
    a.save_as_headless(&path).map_err(|e| e.to_string())?;
    let mut b = Session::open_standalone(b).map_err(|e| e.to_string())?;
    b.run(Command::CarryKnowledge {
        from_path: path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into(),
        tolerance: None,
    })
    .map_err(|e| e.to_string())?;
    b.run(Command::VariationReport {
        layer: None,
        revision: None,
    })
    .map_err(|e| e.to_string())?;
    let report = b
        .variation()
        .map_err(|e| e.to_string())?
        .ok_or("Missing variation")?;
    let workbook = out.join("variation.xlsx");
    qiyas_boq::export_takeoff_with_report(
        &workbook,
        &b.takeoff.items,
        &b.takeoff.totals(),
        "Variation",
        &b.measurement_report().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if qiyas_boq::variation::import_variation(&workbook).map_err(|e| e.to_string())? != report {
        return Err("Variation workbook did not round trip".into());
    }
    let saved = out.join("B.dxf");
    b.save_as_headless(&saved).map_err(|e| e.to_string())?;
    let reopened = Session::open_standalone(&saved).map_err(|e| e.to_string())?;
    if reopened.variation().map_err(|e| e.to_string())? != Some(report.clone())
        || reopened.list_comments().len() != b.list_comments().len()
    {
        return Err("Variation/clouds did not round trip".into());
    }
    super::write_json(&out.join("prediction.json"), &report)?;
    Ok((report, started.elapsed().as_secs_f64() * 1000.))
}
fn handles(item: &qiyas_core::revisions::VariationItem) -> Vec<String> {
    let mut h: Vec<_> = item
        .elements
        .iter()
        .flat_map(|e| e.a_handles.iter().chain(&e.b_handles))
        .map(|h| format!("{h:X}"))
        .collect();
    h.sort();
    h.dedup();
    h
}
fn score(a: &Truth, b: &Truth, prediction: &VariationReport) -> Value {
    let keys: std::collections::BTreeSet<_> = a
        .items
        .iter()
        .chain(&b.items)
        .map(|i| i.id.clone())
        .collect();
    let mut used = std::collections::BTreeSet::new();
    let mut rows = vec![];
    let mut expected = 0;
    let mut found = 0;
    let mut accurate = 0;
    for id in keys {
        let left = a.items.iter().find(|i| i.id == id);
        let right = b.items.iter().find(|i| i.id == id);
        let item = right.or(left).unwrap();
        let delta = right.map_or(0., |i| i.quantity) - left.map_or(0., |i| i.quantity);
        if delta.abs() < 1e-9 {
            continue;
        }
        expected += 1;
        let sources: Vec<_> = left
            .into_iter()
            .chain(right)
            .flat_map(|i| &i.source_objects)
            .collect();
        let candidate = prediction
            .items
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                !used.contains(i)
                    && p.delta.abs() > 1e-9
                    && super::model::unit(&p.unit) == super::model::unit(&item.unit)
            })
            .filter_map(|(i, p)| {
                let overlap = handles(p).iter().filter(|h| sources.contains(h)).count();
                (overlap > 0).then_some((i, p, overlap))
            })
            .max_by_key(|(_, _, n)| *n);
        let tolerance = if item.kind == "count" {
            1e-9
        } else {
            delta.abs()
                * match item.tolerance_class.as_str() {
                    "rule" => item.tolerance_percent.unwrap_or(2.) / 100.,
                    "judgement" => 0.1,
                    _ => 0.005,
                }
        };
        let mut passed = false;
        let mut submitted = None;
        if let Some((i, p, _)) = candidate {
            used.insert(i);
            found += 1;
            submitted = Some(p.delta);
            passed = (p.delta - delta).abs() <= tolerance;
            accurate += usize::from(passed);
        }
        rows.push(json!({"item":id,"expected_delta":delta,"submitted_delta":submitted,"tolerance":tolerance,"passed":passed}));
    }
    let predicted = prediction
        .items
        .iter()
        .filter(|i| i.delta.abs() > 1e-9)
        .count();
    json!({"expected":expected,"found":found,"predicted":predicted,"accurate":accurate,"items":rows})
}
pub(super) fn run(root: &Path, ids: &[String], mode: RunMode, out: &Path) -> Result<Value, String> {
    let mut cases = vec![];
    let (mut expected, mut found, mut predicted, mut accurate) = (0u64, 0u64, 0u64, 0u64);
    for id in ids.iter().filter(|id| id.ends_with("/variants/revision-b")) {
        let base = id.strip_suffix("/variants/revision-b").unwrap();
        let directory = super::safe_file(root, &Path::new("cases").join(base))?;
        // Existing generated revision variants use drawing.dxf. This is input enumeration,
        // independent of the truth file and of the expected item or mutation.
        let a = super::safe_file(&directory, Path::new("drawing.dxf"))?;
        let b = super::safe_file(&directory, Path::new("variants/revision-b/drawing.dxf"))?;
        let general = mode == RunMode::Autonomy || !directory.join("recognition.json").exists();
        let recipe = if general {
            super::safe_file(root, Path::new("general/recognition.json"))?
        } else {
            super::safe_file(&directory, Path::new("recognition.json"))?
        };
        let case_out = out.join(id.replace('/', "__"));
        let started = Instant::now();
        let supplement = if general {
            None
        } else {
            Some(super::safe_file(
                root,
                Path::new("general/recognition.json"),
            )?)
        };
        let prediction = predict(&a, &b, &recipe, general, supplement.as_deref(), &case_out);
        let (ta, _) = super::load_case(root, base)?;
        let (tb, _) = super::load_case(root, id)?;
        let (report, elapsed, error) = match prediction {
            Ok((p, t)) => (p, t, None),
            Err(e) => (
                VariationReport::default(),
                started.elapsed().as_secs_f64() * 1000.,
                Some(e),
            ),
        };
        let mut result = score(&ta, &tb, &report);
        result["id"] = json!(id);
        result["elapsed_ms"] = json!(elapsed);
        result["error"] = json!(error);
        expected += result["expected"].as_u64().unwrap();
        found += result["found"].as_u64().unwrap();
        predicted += result["predicted"].as_u64().unwrap();
        accurate += result["accurate"].as_u64().unwrap();
        cases.push(result);
    }
    let ratio = |n: u64, d: u64| {
        if d == 0 {
            None
        } else {
            Some(n as f64 / d as f64)
        }
    };
    let result = json!({"cases":cases,"expected":expected,"found":found,"predicted":predicted,"accurate":accurate,"recall":ratio(found,expected),"precision":ratio(found,predicted),"delta_accuracy":ratio(accurate,found),"accurate_all":ratio(accurate,expected)});
    super::write_json(&out.join("revision-report.json"), &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prediction_uses_the_registry_and_round_trips_without_readable_truth() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("truth.json")).unwrap();
        let a = dir.path().join("a.dxf");
        let b = dir.path().join("b.dxf");
        let mut s = Session::new_blank(&a, 6);
        s.run_input(
            &json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
            qiyas_core::Source::User,
        )
        .unwrap();
        s.save_as_headless(&a).unwrap();
        s.run_input(
            &json!({"type":"draw","kind":"line","a":[10,0],"b":[13,0]}),
            qiyas_core::Source::User,
        )
        .unwrap();
        s.save_as_headless(&b).unwrap();
        let recipe = dir.path().join("interpretation.json");
        super::super::write_json(&recipe,&json!({"commands":[{"type":"define_object_type","definition":{"name":"Path","category":"drawing","geometry":"path","quantities":{"measured":"geometry.length"}}},{"type":"recognition","kind":"define","rule":{"name":"paths","object_type":"Path","match":{"entity_type":"Line"},"confidence":1.0}},{"type":"recognition","kind":"apply"}],"measurements":[{"object_type":"Path","quantity":"measured","kind":"length","unit":"m","aggregate":true}]})).unwrap();
        let (report, elapsed) =
            predict(&a, &b, &recipe, true, None, &dir.path().join("output")).unwrap();
        assert_eq!(report.items[0].delta, 3.);
        assert!(!report.clouds.is_empty());
        assert!(elapsed < 60000.);
        // Prepared mode also needs shared interpretation for scopes the case recipe omits.
        let extra = dir.path().join("shared.json");
        super::super::write_json(&extra, &json!({"commands":[
            {"type":"define_object_type","definition":{"name":"Marks","category":"drawing","geometry":"point","quantities":{"measured":"geometry.count"}}},
            {"type":"recognition","kind":"define","rule":{"name":"marks","object_type":"Marks","match":{"entity_type":"Point"},"confidence":1.0}},
            {"type":"define_object_type","definition":{"name":"FallbackPath","category":"drawing","geometry":"path","quantities":{"measured":"geometry.length"}}},
            {"type":"recognition","kind":"define","rule":{"name":"fallback-paths","object_type":"FallbackPath","match":{"entity_type":"Line"},"confidence":1.0}},
            {"type":"recognition","kind":"apply"}],
            "measurements":[{"object_type":"Marks","quantity":"measured","kind":"count","unit":"pcs","aggregate":true},{"object_type":"FallbackPath","quantity":"measured","kind":"length","unit":"m","aggregate":true}]})).unwrap();
        s.run_input(
            &json!({"type":"draw","kind":"point","p":[20,20]}),
            qiyas_core::Source::User,
        )
        .unwrap();
        s.save_as_headless(&b).unwrap();
        let (report, _) = predict(
            &a,
            &b,
            &recipe,
            false,
            Some(&extra),
            &dir.path().join("prepared"),
        )
        .unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(
            report.items.iter().find(|i| i.unit == "pcs").unwrap().delta,
            1.
        );
        assert_eq!(
            report.items.iter().find(|i| i.unit == "m").unwrap().delta,
            3.
        );
    }
}
