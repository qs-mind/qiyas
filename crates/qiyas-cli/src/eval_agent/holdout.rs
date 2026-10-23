//! The explicit exam boundary. Development loaders retain their blanket holdout refusal.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{CaseInput, EvalAgentArgs};
use crate::bench::score::Metrics;

struct Opening {
    lock: PathBuf,
    permissions: Option<(PathBuf, Permissions)>,
}

impl Opening {
    fn restore(&mut self) -> Result<(), String> {
        if let Some((path, permissions)) = &self.permissions {
            fs::set_permissions(path, permissions.clone()).map_err(|e| e.to_string())?;
            self.permissions = None;
        }
        Ok(())
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        let _ = self.restore();
        let _ = fs::remove_file(&self.lock);
    }
}

pub(super) fn run(args: &EvalAgentArgs, root: &Path) -> Result<Value, String> {
    let manifest = crate::bench::safe_file(root, Path::new("holdout-manifest.json"))?;
    let manifest: Value = serde_json::from_slice(&fs::read(manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if manifest["schema_version"] != 1
        || manifest["hash_algorithm"] != "sha256(drawing.dxf || truth.json)"
    {
        return Err("Unsupported holdout seal format".into());
    }
    let mut hashes = BTreeSet::new();
    for entry in manifest["entries"]
        .as_array()
        .ok_or("Invalid holdout manifest")?
    {
        let hash = entry["sha256"].as_str().ok_or("Invalid holdout SHA-256")?;
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !hashes.insert(hash.to_string())
        {
            return Err("Invalid or duplicate holdout SHA-256".into());
        }
    }
    if hashes.is_empty() {
        return Err("The holdout manifest has no cases".into());
    }
    let lock = root.join("holdout-exam.lock");
    crate::eval::validate_path(&lock)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|e| {
            format!("Cannot lock holdout exam (inspect any stale lock before retrying): {e}")
        })?;
    let mut opening = Opening {
        lock,
        permissions: None,
    };
    let combined = super::folder(root, "holdout-exam")?;
    let log_path = root.join("holdout-log.jsonl");
    crate::eval::validate_path(&log_path)?;
    let mut log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path)
        .map_err(|e| e.to_string())?;
    let git_commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());
    let mut entry = json!({"time":Utc::now().to_rfc3339(), "agent":args.agent,
        "model":args.model,"effort":args.effort,"git_commit":git_commit,
        "report_directory":combined,"status":"opening","coverage":null,"accuracy":null,"precision":null});
    append(&mut log, &entry)?;
    // Record first: even a bad seal, failed permission change or interrupted exam is visible.
    let result = (|| {
        let cases = load_verified(root, &hashes, &mut opening);
        opening.restore()?;
        let cases = cases?;
        super::run_cases(args, root, &cases, &combined)
    })();
    entry["time"] = json!(Utc::now().to_rfc3339());
    let result = match result {
        Ok(mut report) => {
            report["git_commit"] = json!(git_commit);
            report["sealed_cases"] = json!(hashes.len());
            report["complete"] = json!(report["reports"]
                .as_array()
                .is_some_and(|reports| reports.len() == hashes.len()
                    && reports.iter().all(|r| r["status"] == "done")));
            entry["status"] = json!(if report["complete"] == true {
                "completed"
            } else {
                "incomplete"
            });
            entry["coverage"] = report["north_star"]["coverage"].clone();
            entry["accuracy"] = report["north_star"]["within_2_percent_all_items"].clone();
            entry["precision"] = report["north_star"]["precision"].clone();
            entry["north_star"] = report["north_star"].clone();
            crate::bench::write_json(&combined.join("report.json"), &report).map(|()| report)
        }
        Err(error) => {
            entry["status"] = json!("failed");
            entry["error"] = json!(error);
            Err(error)
        }
    };
    if let Err(error) = &result {
        entry["status"] = json!("failed");
        entry["error"] = json!(error);
        // Keep any partial per-case report when a later case fails.
        crate::bench::write_json(&combined.join("failure.json"), &entry)?;
    }
    append(&mut log, &entry)?;
    result
}

fn append(log: &mut fs::File, entry: &Value) -> Result<(), String> {
    let mut line = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
    line.push(b'\n');
    log.write_all(&line)
        .and_then(|()| log.sync_all())
        .map_err(|e| e.to_string())
}

// Only direct children named by the seal are allowed; no path supplied by truth is followed.
fn require_type(path: &Path, directory: bool) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink()
        || (if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        })
    {
        return Err("Holdout paths must be ordinary directories and files, never symlinks".into());
    }
    Ok(())
}

fn load_verified(
    root: &Path,
    hashes: &BTreeSet<String>,
    opening: &mut Opening,
) -> Result<Vec<CaseInput>, String> {
    let holdout = root.join("holdout");
    require_type(&holdout, true)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::metadata(&holdout)
            .map_err(|e| e.to_string())?
            .permissions();
        let readable = Permissions::from_mode(permissions.mode() | 0o500);
        opening.permissions = Some((holdout.clone(), permissions));
        fs::set_permissions(&holdout, readable).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = opening;
    let actual: BTreeSet<String> = fs::read_dir(&holdout)
        .map_err(|e| e.to_string())?
        .map(|e| {
            e.map_err(|e| e.to_string()).and_then(|e| {
                e.file_name()
                    .into_string()
                    .map_err(|_| "Invalid holdout name".into())
            })
        })
        .collect::<Result<_, String>>()?;
    if &actual != hashes {
        return Err("Holdout cases do not match the published seal manifest".into());
    }
    let mut cases = Vec::new();
    for hash in hashes {
        let directory = holdout.join(hash);
        require_type(&directory, true)?;
        let drawing_path = directory.join("drawing.dxf");
        let truth_path = directory.join("truth.json");
        require_type(&drawing_path, false)?;
        require_type(&truth_path, false)?;
        let drawing = fs::read(drawing_path).map_err(|e| e.to_string())?;
        let truth_bytes = fs::read(truth_path).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        digest.update(&drawing);
        digest.update(&truth_bytes);
        if format!("{:x}", digest.finalize()) != *hash {
            return Err(format!(
                "Holdout hash mismatch for seal {hash}; no agent was run"
            ));
        }
        let truth: crate::bench::Truth =
            serde_json::from_slice(&truth_bytes).map_err(|e| e.to_string())?;
        truth.validate()?;
        if truth.drawing != "drawing.dxf" || truth.id != "sealed" {
            return Err("Holdout truth must describe the sealed drawing.dxf".into());
        }
        cases.push(CaseInput {
            id: hash.clone(),
            truth,
            drawing,
        });
    }
    Ok(cases)
}

pub(crate) fn north_star(metrics: &Metrics) -> Value {
    let rates = metrics.rates();
    let accuracy = (metrics.truth_items != 0)
        .then(|| metrics.within_2_percent as f64 / metrics.truth_items as f64);
    json!({"within_2_percent_all_items":accuracy,"accuracy_target":0.95,
        "accuracy_target_met":accuracy.map(|v| v >= 0.95),
        "within_2_percent_matched_items":rates["p_at_2_percent"],
        "coverage":rates["coverage"],"coverage_target_exclusive":0.90,
        "coverage_target_met":rates["coverage"].as_f64().map(|v| v > 0.90),
        "traceability_submitted":rates["traceability_submitted"],
        "traceability_all":rates["traceability_all"],"traceability_target":1.0,
        "traceability_target_met":rates["traceability_submitted"].as_f64().map(|v| v == 1.0),
        "seeded_error_recall":rates["error_recall"],"precision":rates["item_precision"],
        "seeded_error_precision":rates["error_precision"]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn north_star_targets_denominators_and_not_applicable_errors() {
        let mut metrics = Metrics {
            truth_items: 100,
            matched: 95,
            within_2_percent: 95,
            traceable: 80,
            extras: 5,
            true_positives: 3,
            false_positives: 1,
            false_negatives: 3,
            ..Default::default()
        };
        let north = north_star(&metrics);
        assert_eq!(north["within_2_percent_all_items"], 0.95);
        assert_eq!(north["accuracy_target_met"], true);
        assert_eq!(north["coverage_target_met"], true);
        assert_eq!(north["traceability_submitted"], 0.8);
        assert_eq!(north["precision"], 0.95);
        assert_eq!(north["seeded_error_recall"], 0.5);
        assert_eq!(north["seeded_error_precision"], 0.75);
        metrics.matched = 90;
        assert_eq!(north_star(&metrics)["coverage_target_met"], false);
        let empty = north_star(&Metrics::default());
        for key in [
            "within_2_percent_all_items",
            "coverage",
            "traceability_submitted",
            "seeded_error_recall",
            "precision",
            "seeded_error_precision",
        ] {
            assert!(empty[key].is_null(), "{key}");
        }
    }
}
