//! Offline replay has a strict prediction boundary: saved measurements and drawing evidence
//! are converted first; only then may the outer scorer open the benchmark truth.
use std::path::Path;

use serde_json::{json, Value};

use super::{model::Submission, score::Metrics};

pub fn agent(root: &Path, report_path: &Path, output: &Path) -> Result<Value, String> {
    super::refuse_holdout_path(report_path)?;
    super::reject_symlink(report_path)?;
    super::refuse_holdout_path(output)?;
    if output.exists() {
        return Err("Replay output already exists; preserve the original artefacts and choose a new directory".into());
    }
    super::reject_symlink(output.parent().unwrap_or(Path::new(".")))?;
    let original: Value =
        serde_json::from_slice(&std::fs::read(report_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let reports = original["reports"]
        .as_array()
        .ok_or("Expected combined agent report")?;
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    let mut metrics = Metrics::default();
    for (index, case) in reports.iter().enumerate() {
        let mut result = case.clone();
        if case["status"] == "contaminated"
            || case["contamination"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        {
            result["score"] = Value::Null;
            results.push(result);
            continue;
        }
        let directory = Path::new(
            case["report_directory"]
                .as_str()
                .ok_or("Missing report directory")?,
        );
        super::reject_symlink(directory)?;
        super::refuse_holdout_path(directory)?;
        let previous = directory.join("takeoff.json");
        let submitted = if previous.exists() {
            super::reject_symlink(&previous)?;
            super::submission::read(&previous)?
        } else {
            Submission {
                warnings: vec!["No saved submission; empty submission retained".into()],
                ..Default::default()
            }
        };
        let drawing = directory.join("drawing.dxf");
        super::reject_symlink(&drawing)?;
        let session = qiyas_core::Session::open_embedded(&drawing).map_err(|e| e.to_string())?;
        let path = output.join(format!("{index}-takeoff.json"));
        if previous.exists() {
            // Preserve even the original decimal spelling of metadata and quantities.
            std::fs::copy(&previous, &path).map_err(|e| e.to_string())?;
        } else {
            super::write_json(&path, &submitted)?;
        }
        // A saved holdout score already owns its correctness and correspondence results.
        // Traceability can be updated without reading any sealed input or truth quantity.
        let score = if original["holdout"] == true {
            let previous =
                serde_json::from_value(case["score"].clone()).map_err(|e| e.to_string())?;
            json!(saved_trace_score(previous, &submitted, &session)?)
        } else {
            let (truth, _) = super::load_case(root, case["case"].as_str().ok_or("Missing case")?)?;
            json!(super::score::score(&truth, &submitted, Some(&session)))
        };
        let case_metrics: Metrics =
            serde_json::from_value(score["metrics"].clone()).map_err(|e| e.to_string())?;
        metrics.merge(&case_metrics);
        result["before_score"] = case["score"].clone();
        result["score"] = score;
        result["takeoff"] = json!(path);
        results.push(result);
    }
    let mut result = json!({"headline":original["headline"],"source_report":report_path,
        "replay":"Saved artefacts only; no agent calls or changed quantities",
        "before_metrics":original["metrics"],"before_rates":original["rates"],
        "metrics":metrics,"rates":metrics.rates(),"reports":results});
    result["report_directory"] = json!(output);
    if original["holdout"] == true {
        for key in [
            "holdout",
            "complete",
            "sealed_cases",
            "scored_cases",
            "cases",
            "git_commit",
        ] {
            result[key] = original[key].clone();
        }
        result["north_star"] = crate::eval_agent::holdout::north_star(&metrics);
        result["before_north_star"] = original["north_star"].clone();
        result["replay"] = json!("Saved holdout artefacts only; correctness and correspondence retained; no sealed inputs opened");
    }
    super::write_json(&output.join("report.json"), &result)?;
    if original["holdout"] == true {
        use std::io::Write;
        let log = root.join("holdout-log.jsonl");
        super::reject_symlink(&log)?;
        let entry = json!({"status":"rescore", "time":chrono::Utc::now().to_rfc3339(),
            "source_report":report_path,"report_directory":output,"sealed_inputs_opened":false,
            "agent":original["reports"][0]["agent"],"model":original["reports"][0]["model"],
            "git_commit":original["git_commit"],"coverage":metrics.rates()["coverage"],
            "accuracy":result["north_star"]["within_2_percent_all_items"],
            "precision":metrics.rates()["item_precision"],"north_star":result["north_star"],
            "before_rates":original["rates"],"rates":metrics.rates()});
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{entry}")
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(result)
}

/// Recover the scored set from saved extras and complete aggregate memberships, never values.
/// The old report may not identify which member matched which truth row.
fn legacy_scored_set(
    score: &super::score::CaseScore,
    submitted: &Submission,
) -> Result<std::collections::BTreeSet<usize>, String> {
    use std::collections::BTreeSet;
    let aggregates: Vec<_> = (0..submitted.items.len())
        .filter_map(|i| {
            super::submission::aggregate_components(&submitted.items, i).map(|parts| (i, parts))
        })
        .collect();
    let base: BTreeSet<_> = submitted
        .items
        .iter()
        .enumerate()
        .filter(|(i, item)| {
            !aggregates.iter().any(|(a, _)| a == i) && !score.extra_items.contains(&item.id)
        })
        .map(|(i, _)| i)
        .collect();
    let groups: Vec<_> = aggregates
        .into_iter()
        .filter(|(_, p)| p.len() > 1 && p.iter().all(|i| base.contains(i)))
        .collect();
    fn partitions(
        groups: &[(usize, Vec<usize>)],
        at: usize,
        current: BTreeSet<usize>,
        target: usize,
        results: &mut Vec<BTreeSet<usize>>,
    ) {
        if results.len() > 1 || current.len() < target {
            return;
        }
        if current.len() == target {
            results.push(current);
            return;
        }
        let Some((index, parts)) = groups.get(at) else {
            return;
        };
        partitions(groups, at + 1, current.clone(), target, results);
        if parts.iter().all(|p| current.contains(p)) {
            let mut grouped = current;
            for p in parts {
                grouped.remove(p);
            }
            grouped.insert(*index);
            partitions(groups, at + 1, grouped, target, results);
        }
    }
    let mut results = Vec::new();
    partitions(&groups, 0, base, score.metrics.matched, &mut results);
    if results.len() != 1 {
        return Err("Legacy score has ambiguous aggregate membership; cannot recover trace counts without guessing correspondence".into());
    }
    Ok(results.remove(0))
}

fn saved_trace_score(
    mut score: super::score::CaseScore,
    submitted: &Submission,
    session: &qiyas_core::Session,
) -> Result<Value, String> {
    let traces = super::trace::submission(submitted, Some(session));
    let all_traceable = traces.iter().all(|t| t.traceable);
    let legacy = score
        .items
        .iter()
        .any(|r| r.submitted.is_some() && r.submitted_index.is_none());
    let traced_matches = if legacy {
        if all_traceable {
            score.metrics.matched
        } else {
            legacy_scored_set(&score, submitted)?
                .iter()
                .filter(|i| traces[**i].traceable)
                .count()
        }
    } else {
        score
            .items
            .iter()
            .filter_map(|r| r.submitted_index)
            .map(|i| {
                traces
                    .get(i)
                    .ok_or("Saved submission index is out of range")
            })
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .filter(|t| t.traceable)
            .count()
    };
    for row in &mut score.items {
        if row.submitted.is_none() {
            continue;
        }
        if let Some(index) = row.submitted_index {
            let trace = traces
                .get(index)
                .ok_or("Saved submission index is out of range")?;
            row.traceable = trace.traceable;
            row.trace_reason = trace.reason.clone();
        } else if all_traceable {
            row.traceable = true;
            row.trace_reason =
                "Every saved submission and aggregate reproduced; retained legacy correspondence"
                    .into();
        }
    }
    let mut extra_traced = 0;
    for id in &score.extra_items {
        let matching: Vec<_> = traces.iter().filter(|t| &t.id == id).collect();
        if matching.is_empty()
            || matching
                .iter()
                .any(|t| t.traceable != matching[0].traceable)
        {
            return Err(format!("Ambiguous saved extra item {id}"));
        }
        extra_traced += usize::from(matching[0].traceable);
    }
    score.metrics.traceable = traced_matches;
    score.metrics.traceable_submitted = Some(traced_matches + extra_traced);
    score.rates = score.metrics.rates();
    score.submission_traces = traces;
    let mut value = json!(score);
    if legacy && !all_traceable {
        // Do not invent a link between a truth row and a submitted row. The exact failures
        // and their reasons are reported by submitted identity in submission_traces.
        for row in value["items"].as_array_mut().unwrap() {
            if !row["submitted"].is_null() && row["submitted_index"].is_null() {
                let row = row.as_object_mut().unwrap();
                row.remove("traceable");
                row.remove("trace_reason");
            }
        }
        value["trace_correspondence"] = json!("Legacy report lacks per-truth-row submission indices; trace counts use its unique scored set, and submission_traces identifies every failure");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::model::{Item, Truth};

    #[test]
    fn legacy_scored_membership_uses_extras_and_aggregates_not_quantities() {
        let count = |id: &str, handle: &str| Item {
            id: id.into(),
            kind: "count".into(),
            unit: "pcs".into(),
            quantity: 1.0,
            basis: "geometric".into(),
            source_objects: vec![handle.into()],
            ..Default::default()
        };
        let a = count("a", "10");
        let b = count("b", "11");
        let c = count("c", "12");
        let total = Item {
            id: "group".into(),
            quantity: 2.0,
            source_objects: vec!["10".into(), "11".into()],
            aggregate_of: vec!["a".into(), "b".into()],
            ..a.clone()
        };
        let truth = Truth {
            schema_version: 1,
            id: "test".into(),
            project_type: "test".into(),
            drawing: "drawing.dxf".into(),
            items: vec![total.clone(), c.clone()],
            rules: Default::default(),
            seeded_errors: vec![],
        };
        let mut submitted = Submission {
            items: vec![a, b, total, c, count("extra", "13")],
            ..Default::default()
        };
        let score = super::super::score::score(&truth, &submitted, None);
        assert_eq!(score.metrics.matched, 2);
        assert_eq!(score.extra_items, ["extra"]);
        let indices = std::collections::BTreeSet::from([2, 3]);
        assert_eq!(legacy_scored_set(&score, &submitted).unwrap(), indices);
        for item in &mut submitted.items {
            item.quantity *= 100.0;
        }
        assert_eq!(legacy_scored_set(&score, &submitted).unwrap(), indices);
    }
}
