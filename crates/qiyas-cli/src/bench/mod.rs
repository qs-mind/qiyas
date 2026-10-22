//! Local development benchmark. Enumeration is rooted in `cases`, never `holdout`.
mod baseline;
pub mod generate;
mod model;
mod recognition;
mod score;
mod submission;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use clap::{Args, Subcommand};
use qiyas_core::Session;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub use model::Truth;
use score::{CaseScore, Metrics};

#[derive(Args)]
pub struct BenchArgs {
    #[arg(long, global = true, default_value = ".local/benchmark")]
    pub root: PathBuf,
    #[command(subcommand)]
    pub command: BenchCommand,
}

#[derive(Subcommand)]
pub enum BenchCommand {
    /// Generate deterministic development drawings and an unread holdout seal.
    Generate {
        /// Seeds per project type (six project types).
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=100))]
        seeds: u16,
    },
    /// Run case-local recognition recipes, with geometric baseline fallback, on development cases.
    Run {
        #[arg(long)]
        case: Option<String>,
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u16).range(1..=100))]
        repeats: u16,
        /// Score only the clean cases; by default all registered variants are scored too.
        #[arg(long)]
        clean_only: bool,
    },
    /// Run a data-only recognition recipe and write computed quantities for scoring.
    Recognize {
        #[arg(long)]
        case: String,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Score a JSON take-off/BOQ, Qiyas Excel take-off or BOQ workbook.
    Score {
        #[arg(long)]
        case: String,
        #[arg(long)]
        submission: PathBuf,
    },
    /// List development IDs and the published holdout hashes (never opens the holdout).
    List,
}

pub fn run(args: &BenchArgs) -> Result<Value, String> {
    refuse_holdout_path(&args.root)?;
    match &args.command {
        BenchCommand::Generate { seeds } => generate::generate(&args.root, usize::from(*seeds)),
        BenchCommand::Run {
            case,
            repeats,
            clean_only,
        } => baseline_run(
            &args.root,
            case.as_deref(),
            usize::from(*repeats),
            *clean_only,
        ),
        BenchCommand::Recognize { case, plan, output } => {
            let (truth, directory) = load_case(&args.root, case)?;
            reject_symlink(plan)?;
            refuse_holdout_path(output)?;
            if output.exists() {
                reject_symlink(output)?;
            }
            if let Some(parent) = output.parent() {
                reject_symlink(parent)?;
            }
            let submission = recognition::measure(&directory.join(truth.drawing), plan)?;
            write_json(output, &submission)?;
            Ok(
                json!({"written":output,"items":submission.items.len(),"warnings":submission.warnings}),
            )
        }
        BenchCommand::Score { case, submission } => score_submission(&args.root, case, submission),
        BenchCommand::List => {
            Ok(json!({"dev_cases":dev_ids(&args.root,false)?,"holdout":seal(&args.root)?}))
        }
    }
}

fn safe_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.components().any(|c| {
            c.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("holdout")
        })
    {
        return Err("only relative development case paths are allowed; holdout is sealed".into());
    }
    Ok(())
}

/// Refuse symlinks before opening any file, including aliases that could reach the holdout.
pub fn safe_file(base: &Path, relative: &Path) -> Result<PathBuf, String> {
    safe_relative(relative)?;
    let mut path = base.to_path_buf();
    reject_symlink(&path)?;
    for part in relative.components() {
        path.push(part);
        reject_symlink(&path)?;
    }
    Ok(path)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    refuse_holdout_path(path)?;
    // Inspect every ancestor too: a linked benchmark root must not bypass containment.
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta = std::fs::symlink_metadata(ancestor)
            .map_err(|e| format!("{}: {e}", ancestor.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "benchmark paths cannot be symlinks: {}",
                ancestor.display()
            ));
        }
    }
    Ok(())
}

pub fn refuse_holdout_path(path: &Path) -> Result<(), String> {
    if path.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("holdout")
    }) {
        return Err("holdout is sealed; no development command may open it".into());
    }
    Ok(())
}

pub fn load_case(root: &Path, id: &str) -> Result<(Truth, PathBuf), String> {
    let parts: Vec<_> = Path::new(id).components().collect();
    if !(parts.len() == 1 || (parts.len() == 3 && parts[1].as_os_str() == "variants")) {
        return Err("case id must be <id> or <id>/variants/<variant>".into());
    }
    let directory = safe_file(root, &Path::new("cases").join(id))?;
    let path = safe_file(&directory, Path::new("truth.json"))?;
    let truth: Truth = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    truth.validate()?;
    if truth.id != id {
        return Err(format!("truth id {} does not match case {id}", truth.id));
    }
    safe_file(&directory, Path::new(&truth.drawing))?;
    Ok((truth, directory))
}

fn directories(path: &Path) -> Result<Vec<String>, String> {
    reject_symlink(path)?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            return Err("symlink in development cases refused".into());
        }
        if file_type.is_dir() {
            names.push(
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "case id is not UTF-8")?,
            );
        }
    }
    names.sort();
    Ok(names)
}

fn dev_ids(root: &Path, clean_only: bool) -> Result<Vec<String>, String> {
    let cases = safe_file(root, Path::new("cases"))?;
    let mut ids = Vec::new();
    for id in directories(&cases)? {
        load_case(root, &id)?;
        ids.push(id.clone());
        if !clean_only
            && cases
                .join(&id)
                .join("variants")
                .try_exists()
                .map_err(|e| e.to_string())?
        {
            let variants = safe_file(&cases, &Path::new(&id).join("variants"))?;
            for variant in directories(&variants)? {
                ids.push(format!("{id}/variants/{variant}"));
            }
        }
    }
    if ids.is_empty() {
        return Err("no development cases; run bench generate first".into());
    }
    Ok(ids)
}

fn seal(root: &Path) -> Result<Value, String> {
    let path = root.join("holdout-manifest.json");
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(json!({"status":"not generated"}));
    }
    let path = safe_file(root, Path::new("holdout-manifest.json"))?;
    let value: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    // Project only hashes even if a future seal manifest carries private metadata.
    let hashes: Vec<_> = value["entries"]
        .as_array()
        .ok_or("invalid holdout manifest")?
        .iter()
        .map(|entry| entry["sha256"].clone())
        .collect();
    if hashes.iter().any(|v| {
        !v.as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    }) {
        return Err("invalid holdout SHA-256".into());
    }
    Ok(json!({"status":"sealed; contents never opened","sha256":hashes}))
}

pub fn score_submission(
    root: &Path,
    case_id: &str,
    submission_path: &Path,
) -> Result<Value, String> {
    let (truth, directory) = load_case(root, case_id)?;
    // User-supplied submissions must not provide another route to the sealed directory.
    reject_symlink(submission_path)?;
    let canonical = submission_path.canonicalize().map_err(|e| e.to_string())?;
    if canonical.components().any(|c| c.as_os_str() == "holdout") {
        return Err("holdout submissions are sealed".into());
    }
    let submission = submission::read(submission_path)?;
    let session = Session::open_standalone(&directory.join(&truth.drawing)).ok();
    let scored = score::score(&truth, &submission, session.as_ref());
    serde_json::to_value(scored).map_err(|e| e.to_string())
}

fn fingerprint(submission: &model::Submission) -> Result<String, String> {
    let stable = json!({"items":submission.items,"findings":submission.findings});
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&stable).map_err(|e| e.to_string())?)
    ))
}

fn baseline_run(
    root: &Path,
    selected: Option<&str>,
    repeats: usize,
    clean_only: bool,
) -> Result<Value, String> {
    let started = Instant::now();
    let previous_rates = if root.join("baseline.json").exists() {
        let path = safe_file(root, Path::new("baseline.json"))?;
        let previous: Value =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        previous["rates"].clone()
    } else {
        Value::Null
    };
    let ids = selected
        .map(|s| vec![s.to_string()])
        .map(Ok)
        .unwrap_or_else(|| dev_ids(root, clean_only))?;
    let run_dir = root.join("runs").join(format!(
        "baseline-{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%f"),
        std::process::id()
    ));
    if root.join("runs").try_exists().map_err(|e| e.to_string())? {
        reject_symlink(&root.join("runs"))?;
    }
    reject_symlink(root)?;
    std::fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;
    reject_symlink(&run_dir)?;
    let mut reports = Vec::<CaseScore>::new();
    let mut metrics = Metrics::default();
    let mut types = BTreeMap::<String, Metrics>::new();
    let mut repeatability = Vec::new();
    for id in ids {
        let (truth, directory) = load_case(root, &id)?;
        let drawing = directory.join(&truth.drawing);
        let mut hashes = Vec::new();
        let mut times = Vec::new();
        for repeat in 0..repeats {
            let attempt = Instant::now();
            let plan = directory.join("recognition.json");
            let attempt_result = if plan.exists() {
                let plan = safe_file(&directory, Path::new("recognition.json"))?;
                recognition::measure(&drawing, &plan)
            } else {
                baseline::measure(&drawing)
            };
            let submission = attempt_result.unwrap_or_else(|e| model::Submission {
                warnings: vec![format!("automatic recognition unavailable: {e}")],
                elapsed_ms: attempt.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            });
            hashes.push(fingerprint(&submission)?);
            times.push(submission.elapsed_ms);
            let artifact = run_dir.join(id.replace('/', "__"));
            std::fs::create_dir_all(&artifact).map_err(|e| e.to_string())?;
            write_json(
                &artifact.join(format!("submission-{}.json", repeat + 1)),
                &submission,
            )?;
            if repeat == 0 {
                let session = Session::open_standalone(&drawing).ok();
                let report = score::score(&truth, &submission, session.as_ref());
                metrics.merge(&report.metrics);
                types
                    .entry(truth.project_type.clone())
                    .or_default()
                    .merge(&report.metrics);
                reports.push(report);
            }
        }
        repeatability.push(json!({"id":id,"identical":hashes.windows(2).all(|w|w[0]==w[1]),"sha256":hashes,"elapsed_ms":times}));
    }
    let mut classes = BTreeMap::<String, Metrics>::new();
    let mut errors: Vec<f64> = Vec::new();
    for report in &reports {
        for item in &report.items {
            let group = classes.entry(item.tolerance_class.clone()).or_default();
            group.truth_items += 1;
            group.matched += usize::from(item.submitted.is_some());
            group.passed += usize::from(item.passed);
            group.traceable += usize::from(item.traceable);
            if let Some(error) = item.absolute_percent_error {
                errors.push(error);
                group.within_1_percent += usize::from(error <= 1.0);
                group.within_2_percent += usize::from(error <= 2.0);
                group.within_5_percent += usize::from(error <= 5.0);
                group.within_10_percent += usize::from(error <= 10.0);
                group.within_25_percent += usize::from(error <= 25.0);
            }
        }
    }
    errors.sort_by(f64::total_cmp);
    let median = if errors.is_empty() {
        None
    } else {
        Some((errors[(errors.len() - 1) / 2] + errors[errors.len() / 2]) / 2.0)
    };
    let next=vec![
        "Extend drawing-evidence recognition recipes to the remaining development cases, including model provenance and unresolved property evidence.",
        "Validate Phase 17 on independently reviewed consultant drawings; controlled PDF hints do not establish live vision recall.",
        "Phase 18: apply declared measurement rules with reproducible geometry and clause references; improve coverage without duplicating room and region rows.",
        "Phase 19: detect seeded missing, quantity, unit and schedule discrepancies; baseline automatic measurement makes no discrepancy claims.",
        "Phase 20: compare revision A/B and report the known quantity deltas.",
        "Add independently sourced CC BY cases after verified downloads and QS spot checks. Synthetic-only scores do not establish performance on consultant drawings.",
    ];
    let result = json!({"schema_version":1,"baseline":"Qiyas recognition recipes with geometric baseline fallback","previous_rates":previous_rates,"cases":reports.len(),"metrics":metrics,"rates":metrics.rates(),"median_absolute_percent_error":median,"by_project_type":types,"by_tolerance_class":classes,"reports":reports,"repeatability":repeatability,"repeats":repeats,"elapsed_ms":started.elapsed().as_secs_f64()*1000.0,"holdout":seal(root)?,"next_phases":next,"report_directory":run_dir});
    write_json(&run_dir.join("report.json"), &result)?;
    let md = markdown(&result);
    std::fs::write(run_dir.join("report.md"), &md).map_err(|e| e.to_string())?;
    write_json(&root.join("baseline.json"), &result)?;
    std::fs::write(root.join("baseline.md"), md).map_err(|e| e.to_string())?;
    Ok(
        json!({"cases":result["cases"],"metrics":result["metrics"],"rates":result["rates"],"elapsed_ms":result["elapsed_ms"],"repeats":repeats,"report_directory":run_dir,"previous_rates":previous_rates}),
    )
}

pub fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn markdown(report: &Value) -> String {
    let mut text=format!("# Qiyas QS baseline\n\n{} dev drawings, {} repetitions. Drawing-owned recognition recipes plus the geometric baseline; no model calls.\n\n| Measure | Result |\n|---|---:|\n",report["cases"],report["repeats"]);
    if let Some(rates) = report["rates"].as_object() {
        for (name, value) in rates {
            text.push_str(&format!(
                "| {} | {} |\n",
                name,
                value
                    .as_f64()
                    .map(|v| format!("{v:.6}"))
                    .unwrap_or_else(|| "not applicable".into())
            ));
        }
    }
    text.push_str("\nRates use 0..1. Accuracy bands use matched items; pass_rate_all and traceability_all include missing items. No errors predicted gives undefined precision, not perfect precision. See report.json for item-level misses/extras, citation failures, errors and timings.\n\n## What later phases must improve\n\n");
    if let Some(next) = report["next_phases"].as_array() {
        for line in next {
            text.push_str(&format!("- {}\n", line.as_str().unwrap_or("")));
        }
    }
    text
}

#[cfg(test)]
mod tests;
