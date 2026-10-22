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
        let previous = directory.join("takeoff.json");
        let saved: Option<Value> = if previous.exists() {
            super::reject_symlink(&previous)?;
            Some(
                serde_json::from_slice(&std::fs::read(&previous).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let measurement = directory.join("measurement.json");
        let mut submitted = if measurement.exists() {
            super::reject_symlink(&measurement)?;
            let report =
                serde_json::from_slice(&std::fs::read(measurement).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let drawing = Path::new(
                saved
                    .as_ref()
                    .and_then(|v| v["drawing"].as_str())
                    .or_else(|| case["drawing"].as_str())
                    .ok_or("Missing saved drawing")?,
            );
            super::refuse_holdout_path(drawing)?;
            super::reject_symlink(drawing)?;
            // Read metadata directly: saved PDF drawings may contain an underlay. Opening
            // the document does not resolve that resource, load sidecars or derive quantities.
            let drawing = qiyas_core::Drawing::open(drawing).map_err(|e| e.to_string())?;
            let evidence: Vec<qiyas_core::recognition::Candidate> = drawing
                .doc
                .entities()
                .filter_map(|entity| {
                    qiyas_core::xdata::read_text(entity.common(), qiyas_core::recognition::APP)
                })
                .map(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
                .collect::<Result<_, _>>()?;
            super::submission::from_report(&report, &evidence, &drawing)?
        } else {
            Submission {
                warnings: vec!["No saved measurement artefact; empty submission retained".into()],
                ..Default::default()
            }
        };
        if previous.exists() {
            let previous = super::submission::read(&previous)?;
            submitted.elapsed_ms = previous.elapsed_ms;
            submitted.findings = previous.findings;
        }
        let path = output.join(format!("{index}-takeoff.json"));
        super::write_json(&path, &submitted)?;
        // Prediction is now fully materialised. No gold values have entered it.
        let score =
            super::score_submission(root, case["case"].as_str().ok_or("Missing case")?, &path)?;
        let case_metrics: Metrics =
            serde_json::from_value(score["metrics"].clone()).map_err(|e| e.to_string())?;
        metrics.merge(&case_metrics);
        result["before_score"] = case["score"].clone();
        result["score"] = score;
        result["takeoff"] = json!(path);
        results.push(result);
    }
    let result = json!({"headline":original["headline"],"source_report":report_path,
        "replay":"Saved artefacts only; no agent calls or changed quantities",
        "before_metrics":original["metrics"],"before_rates":original["rates"],
        "metrics":metrics,"rates":metrics.rates(),"reports":results});
    super::write_json(&output.join("report.json"), &result)?;
    Ok(result)
}
