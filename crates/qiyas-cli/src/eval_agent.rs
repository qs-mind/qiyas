//! Subscription clients drive a drawing-only workspace. Only this outer harness scores truth.
pub(crate) mod holdout;

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use clap::{ArgGroup, Args};
use serde_json::{json, Value};

#[derive(Args)]
#[command(group(ArgGroup::new("selection").args(["case", "cases", "set", "holdout"]).required(true)))]
pub struct EvalAgentArgs {
    /// Open the sealed final exam instead of selecting development cases.
    #[arg(long)]
    pub holdout: bool,
    /// Acknowledge that this run opens the holdout and is recorded permanently.
    #[arg(long, requires = "holdout")]
    pub i_understand_this_opens_the_holdout: bool,
    #[arg(long)]
    pub case: Option<String>,
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub cases: Vec<String>,
    #[arg(long, value_parser = ["live-dev"])]
    pub set: Option<String>,
    #[arg(long, default_value = "codex", value_parser = agent_name)]
    pub agent: String,
    /// Shell template; placeholders are shell-quoted arguments, not raw shell fragments.
    #[arg(long)]
    pub agent_cmd: Option<String>,
    #[arg(long, default_value = "gpt-6.1-sol")]
    pub model: String,
    #[arg(long, default_value = "medium")]
    pub effort: String,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1440))]
    pub timeout_min: u32,
    #[arg(long, default_value = ".local/benchmark")]
    pub root: PathBuf,
}

const CLAUDE_EVAL_REFUSED: &str = "Claude subscriptions are for individual Mizan messages, not benchmarks or evaluations. Use ChatGPT, Codex or an API key for benchmarks.";

fn agent_name(value: &str) -> Result<String, String> {
    if value.eq_ignore_ascii_case("claude") {
        Err(CLAUDE_EVAL_REFUSED.into())
    } else {
        Ok(value.into())
    }
}

fn folder(root: &Path, prefix: &str) -> Result<PathBuf, String> {
    let path = root.join("runs").join(format!(
        "{prefix}-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%f"),
        std::process::id()
    ));
    crate::eval::validate_path(&path)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn run(args: &EvalAgentArgs) -> Result<Value, String> {
    if args.agent.eq_ignore_ascii_case("claude") {
        return Err(CLAUDE_EVAL_REFUSED.into());
    }
    if args.holdout && !args.i_understand_this_opens_the_holdout {
        return Err("The holdout is sealed. Opening it requires --i-understand-this-opens-the-holdout and is logged.".into());
    }
    if args.agent != "codex" && args.agent_cmd.is_none() {
        return Err("Provide --agent-cmd for clients other than codex".into());
    }
    crate::eval::validate_path(&args.root)?;
    let root = args.root.canonicalize().map_err(|e| e.to_string())?;
    if args.holdout {
        return holdout::run(args, &root);
    }
    let cases = if let Some(set) = &args.set {
        crate::bench::sets::select(&root, set)?
    } else if let Some(case) = &args.case {
        vec![case.clone()]
    } else {
        args.cases.clone()
    };
    if cases.is_empty() || cases.iter().collect::<BTreeSet<_>>().len() != cases.len() {
        return Err("Choose distinct development cases".into());
    }
    let inputs = cases
        .iter()
        .map(|id| {
            let (truth, directory) = crate::bench::load_case(&root, id)?;
            let drawing = fs::read(crate::bench::safe_file(
                &directory,
                Path::new(&truth.drawing),
            )?)
            .map_err(|e| e.to_string())?;
            Ok(CaseInput {
                id: id.clone(),
                truth,
                drawing,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let combined = folder(&root, "eval-agent-set")?;
    run_cases(args, &root, &inputs, &combined)
}

#[cfg(test)]
mod subscription_tests {
    use super::*;

    #[test]
    fn claude_evaluation_is_refused_before_files_or_custom_commands() {
        for name in ["claude", "CLAUDE", "Claude"] {
            assert_eq!(agent_name(name).unwrap_err(), CLAUDE_EVAL_REFUSED);
            let args = EvalAgentArgs {
                holdout: true,
                i_understand_this_opens_the_holdout: false,
                case: None,
                cases: Vec::new(),
                set: None,
                agent: name.into(),
                agent_cmd: Some("must-never-execute".into()),
                model: String::new(),
                effort: String::new(),
                timeout_min: 1,
                root: PathBuf::from("/must-not-be-read"),
            };
            assert_eq!(run(&args).unwrap_err(), CLAUDE_EVAL_REFUSED);
        }
        assert_eq!(agent_name("codex").unwrap(), "codex");
    }
}

struct CaseInput {
    id: String,
    truth: crate::bench::Truth,
    drawing: Vec<u8>,
}

fn run_cases(
    args: &EvalAgentArgs,
    root: &Path,
    cases: &[CaseInput],
    combined: &Path,
) -> Result<Value, String> {
    let cli = std::env::current_exe().map_err(|e| e.to_string())?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut reports = Vec::new();
    let mut metrics = crate::bench::score::Metrics::default();
    for (index, case) in cases.iter().enumerate() {
        let output = if args.holdout {
            let path = combined.join(format!("case-{index}"));
            fs::create_dir(&path).map_err(|e| e.to_string())?;
            path
        } else {
            folder(root, "eval-agent")?
        };
        let report = run_case(
            args,
            root,
            case,
            &cli,
            &repo,
            &combined.join(format!("mcp-{index}.json")),
            &output,
        )?;
        if report["status"] != "contaminated" {
            let m = serde_json::from_value(report["score"]["metrics"].clone())
                .map_err(|e| e.to_string())?;
            metrics.merge(&m);
        }
        reports.push(report);
        let report = combined_report(args, &reports, &metrics, combined);
        crate::bench::write_json(&combined.join("report.json"), &report)?;
    }
    if args.case.is_some() {
        Ok(reports.remove(0))
    } else {
        Ok(combined_report(args, &reports, &metrics, combined))
    }
}

fn combined_report(
    args: &EvalAgentArgs,
    reports: &[Value],
    metrics: &crate::bench::score::Metrics,
    output: &Path,
) -> Value {
    let mut report = json!({"headline":format!("AGENT ALONE ({})",args.agent),"cases":reports.len(),
        "scored_cases":reports.iter().filter(|r|r["status"] != "contaminated").count(),
        "reports":reports,"metrics":metrics,"rates":metrics.rates(),"cost":"subscription",
        "elapsed_ms":reports.iter().filter_map(|r|r["elapsed_ms"].as_u64()).sum::<u64>(),
        "tool_calls":reports.iter().filter_map(|r|r["tool_calls"].as_u64()).sum::<u64>(),"report_directory":output});
    if args.holdout {
        report["holdout"] = json!(true);
        report["north_star"] = holdout::north_star(metrics);
        report["metric_scope"] = json!("Uncontaminated scored cases only; contaminated cases are excluded, not a complete exam result.");
    }
    report
}

fn run_case(
    args: &EvalAgentArgs,
    root: &Path,
    input: &CaseInput,
    cli: &Path,
    repo: &Path,
    config: &Path,
    output: &Path,
) -> Result<Value, String> {
    let truth = &input.truth;
    let case = &input.id;
    let extension = Path::new(&truth.drawing)
        .extension()
        .and_then(|s| s.to_str())
        .ok_or("Drawing has no extension")?;
    let drawing = output.join(format!("drawing.{extension}"));
    fs::write(&drawing, &input.drawing).map_err(|e| e.to_string())?;
    // Keep the verified original geometry in memory before the agent can change its copy.
    let original = qiyas_core::Session::open_standalone(&drawing).ok();
    // The drawing is the only case input copied, plus the project preamble a QS receives with a
    // tender: each rule's specification wording, never its numbers' results, quantities or truth.
    let preamble = preamble_text(truth);
    if let Some(text) = &preamble {
        fs::write(output.join("preamble.md"), text).map_err(|e| e.to_string())?;
    }
    let server_args = json!(["mcp", "--drawing", drawing, "--workdir", output]);
    crate::bench::write_json(
        config,
        &json!({"mcpServers":{"qiyas":{"command":cli,"args":server_args}}}),
    )?;
    let mut instructions = "Use the Qiyas MCP tools. Call qiyas_execute finish with your end report in args.answer before ending. Do not inspect files outside this run folder or use repository code, benchmark data, truth or recipes. The MCP server prepares PDF page 1 when applicable; its digest exposes visible source documents.".to_string();
    if preamble.is_some() {
        instructions.push_str(" The project preamble (specification excerpt issued with this drawing) is preamble.md in this run folder: read it and apply its measurement rules to the elements it names.");
    }
    let request = crate::eval::task_request(&instructions);
    let mut command = if let Some(template) = &args.agent_cmd {
        let expanded = expand(
            template,
            &[
                ("cli", cli.to_string_lossy().as_ref()),
                ("drawing", drawing.to_string_lossy().as_ref()),
                ("workdir", output.to_string_lossy().as_ref()),
                ("request", &request),
                ("model", &args.model),
                ("effort", &args.effort),
                ("mcp_config", config.to_string_lossy().as_ref()),
            ],
        )?;
        let mut command = Command::new("sh");
        command.arg("-c").arg(expanded);
        command
    } else {
        codex_command(cli, &drawing, output, &request, &args.model, &args.effort)
    };
    command.current_dir(output).stdin(Stdio::null());
    // Remove common API credential variables without inspecting their contents. Codex is also
    // forced to subscription login, so missing subscription auth fails instead of billing an API.
    for name in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        command.env_remove(name);
    }
    command
        .stdout(File::create(output.join("agent-events.jsonl")).map_err(|e| e.to_string())?)
        .stderr(File::create(output.join("agent-stderr.log")).map_err(|e| e.to_string())?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let mut process_id = None;
    let status = match command.spawn() {
        Err(error) => format!("failed to start: {error}"),
        Ok(mut child) => {
            process_id = Some(child.id());
            loop {
                match child.try_wait().map_err(|e| e.to_string())? {
                    Some(status) => {
                        break if status.success() {
                            "done".into()
                        } else {
                            format!("agent failed: {status}")
                        }
                    }
                    None if started.elapsed()
                        >= Duration::from_secs(u64::from(args.timeout_min) * 60) =>
                    {
                        #[cfg(unix)]
                        {
                            let _ = Command::new("kill")
                                .args(["-KILL", "--", &format!("-{}", child.id())])
                                .status();
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                        break "timeout".into();
                    }
                    None => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        }
    };
    // A well-behaved client closes its server stdin on exit. Give that final save a bounded wait.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !output.join("mcp-exit.json").exists() && Instant::now() < deadline && status == "done" {
        std::thread::sleep(Duration::from_millis(50));
    }
    // Freeze the run before scoring even if a custom client left descendants alive.
    #[cfg(unix)]
    if let Some(id) = process_id {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{id}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = process_id;
    let elapsed_ms = started.elapsed().as_millis();
    let log = fs::read_to_string(output.join("agent-events.jsonl")).map_err(|e| e.to_string())?;
    let mut contamination = contaminated(&log, output, root, repo);
    let mcp_log = fs::read_to_string(output.join("mcp-events.jsonl")).unwrap_or_default();
    contamination.extend(contaminated(&mcp_log, output, root, repo));
    let submission = output.join("takeoff.json");
    let mut status = status;
    let score = if !contamination.is_empty() {
        status = "contaminated".into();
        Value::Null
    } else if submission.exists() {
        match crate::eval::validate_path(&submission)
            .and_then(|()| crate::bench::submission::read(&submission))
            .map(|submission| {
                json!(crate::bench::score::score(
                    truth,
                    &submission,
                    qiyas_core::Session::open_embedded(&drawing)
                        .ok()
                        .as_ref()
                        .or(original.as_ref())
                ))
            }) {
            Ok(score) => score,
            Err(error) => {
                status = format!("failed to score submission: {error}");
                json!(crate::bench::score::score(
                    truth,
                    &crate::bench::model::Submission::default(),
                    None
                ))
            }
        }
    } else {
        status = format!("{status}; no submission");
        json!(crate::bench::score::score(
            truth,
            &crate::bench::model::Submission::default(),
            None
        ))
    };
    if status == "done"
        && score["metrics"]["matched"].as_u64().unwrap_or(0)
            + score["metrics"]["extras"].as_u64().unwrap_or(0)
            == 0
    {
        status = "failed: done with 0 submitted items".into();
    }
    let report = json!({"run":output.file_name().unwrap().to_string_lossy(),"case":case,
        "headline":format!("AGENT ALONE ({})",args.agent),"agent":args.agent,"model":args.model,"effort":args.effort,
        "status":status,"elapsed_ms":elapsed_ms,"tool_calls":mcp_log.lines().count(),"cost":"subscription",
        "usage":{"cost":"subscription"},"agent_log":output.join("agent-events.jsonl"),"step_log":output.join("mcp-events.jsonl"),
        "drawing":drawing,"takeoff":submission,"score":score,"contamination":contamination,"report_directory":output});
    crate::bench::write_json(&output.join("report.json"), &report)?;
    Ok(report)
}

fn codex_command(
    cli: &Path,
    drawing: &Path,
    output: &Path,
    request: &str,
    model: &str,
    effort: &str,
) -> Command {
    let mut command = Command::new("codex");
    command
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--ignore-user-config",
            "--ignore-rules",
        ])
        .arg("-C")
        .arg(output)
        .arg("-c")
        .arg(format!("mcp_servers.qiyas.command={}", json!(cli)))
        .arg("-c")
        .arg(format!(
            "mcp_servers.qiyas.args={}",
            json!(["mcp", "--drawing", drawing, "--workdir", output])
        ))
        // Without this, `codex exec` refuses every MCP call ("requires approval").
        .arg("-c")
        .arg("mcp_servers.qiyas.default_tools_approval_mode=\"approve\"")
        .arg("-c")
        .arg("forced_login_method=\"chatgpt\"")
        .arg("-c")
        .arg("project_doc_max_bytes=0")
        .arg("-m")
        .arg(model)
        .arg("-c")
        .arg(format!("model_reasoning_effort={}", json!(effort)))
        .arg(request);
    command
}

/// Substitute once, quoting each whole argument. Quotes, dollars and backticks stay literal.
fn expand(template: &str, values: &[(&str, &str)]) -> Result<String, String> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail
            .find('}')
            .ok_or("Unclosed agent template placeholder")?;
        let name = &tail[..end];
        let value = values
            .iter()
            .find(|(key, _)| *key == name)
            .ok_or_else(|| format!("Unknown agent template placeholder {{{name}}}"))?
            .1;
        output.push('\'');
        output.push_str(&value.replace('\'', "'\\''"));
        output.push('\'');
        rest = &tail[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

/// Audit commands and file tool arguments, not prose or the agent's final answer. This is an
/// evidence guard, not an OS filesystem sandbox: clients must emit their complete action log.
fn contaminated(log: &str, run: &Path, root: &Path, repo: &Path) -> Vec<String> {
    fn scan(value: &Value, active: bool, texts: &mut Vec<String>) {
        match value {
            Value::Object(fields) => {
                let kind = fields
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let tool = fields
                    .get("tool")
                    .or_else(|| fields.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let active = active
                    || kind.contains("command")
                    || kind.contains("file")
                    || kind == "tool_use"
                    || tool.contains("read")
                    || tool.contains("exec")
                    || tool.contains("shell")
                    || tool.contains("capture");
                for (key, v) in fields {
                    if matches!(
                        key.as_str(),
                        "output" | "aggregated_output" | "result" | "text"
                    ) {
                        continue;
                    }
                    scan(
                        v,
                        active || matches!(key.as_str(), "command" | "file_path" | "path" | "cwd"),
                        texts,
                    );
                }
            }
            Value::Array(values) => {
                for value in values {
                    scan(value, active, texts);
                }
            }
            Value::String(text) if active => texts.push(text.clone()),
            _ => {}
        }
    }
    let mut hits = BTreeSet::new();
    for line in log.lines() {
        let mut texts = Vec::new();
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            scan(&value, false, &mut texts);
        } else {
            texts.push(line.into());
        }
        for text in texts {
            // A run directory sits under both benchmark and repo; only that exact subtree is allowed.
            let text = mask_run(&text, run);
            let lower = text.to_ascii_lowercase();
            if [
                "truth.json",
                "recipe",
                "../",
                ".git",
                "claude.md",
                "agents.md",
                ".local/benchmark",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
                || text.contains(root.to_string_lossy().as_ref())
                || text.contains(repo.to_string_lossy().as_ref())
            {
                hits.insert(text);
            }
        }
    }
    hits.into_iter().collect()
}

fn mask_run(text: &str, run: &Path) -> String {
    let path = run.to_string_lossy();
    let mut output = String::new();
    let mut start = 0;
    for (at, _) in text.match_indices(path.as_ref()) {
        let end = at + path.len();
        let boundary = text[end..]
            .chars()
            .next()
            .is_none_or(|c| c == '/' || c.is_whitespace() || matches!(c, '\'' | '"' | ';' | ')'));
        if boundary {
            output.push_str(&text[start..at]);
            output.push_str("<run>");
            start = end;
        }
    }
    output.push_str(&text[start..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contamination_looks_at_actions_and_exempts_only_the_run_subtree() {
        let run = Path::new("/repo/.local/benchmark/runs/run");
        let root = Path::new("/repo/.local/benchmark");
        let repo = Path::new("/repo");
        for event in [
            json!({"item":{"type":"command_execution","command":"cat ../truth.json"}}),
            json!({"type":"tool_use","name":"Read","input":{"file_path":"/repo/CLAUDE.md"}}),
            json!({"command":"cat /repo/.local/benchmark/cases/a/drawing.dxf"}),
            json!({"command":"cat recipes/a.json"}),
            json!({"command":"cat /repo/.local/benchmark/runs/run-other/drawing.dxf"}),
        ] {
            assert!(!contaminated(&event.to_string(), run, root, repo).is_empty());
        }
        for event in [
            json!({"item":{"type":"agent_message","text":"Do not read truth.json"}}),
            json!({"command":"cat /repo/.local/benchmark/runs/run/takeoff.json"}),
            json!({"cwd":"/repo/.local/benchmark/runs/run","command":"pwd"}),
            json!({"command":"cd /repo/.local/benchmark/runs/run && pwd"}),
        ] {
            assert!(contaminated(&event.to_string(), run, root, repo).is_empty());
        }
    }
    #[test]
    fn template_values_are_literal_and_not_expanded_twice() {
        let value = "it's $(false) `false` {model}";
        let command = expand("printf %s {request}", &[("request", value)]).unwrap();
        let out = Command::new("sh").args(["-c", &command]).output().unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), value);
    }
    #[test]
    fn codex_uses_subscription_and_requested_budget_model() {
        let cmd = codex_command(
            Path::new("/cli"),
            Path::new("/run/drawing.dxf"),
            Path::new("/run"),
            "request",
            "gpt-6.1-sol",
            "medium",
        );
        let args: Vec<_> = cmd.get_args().map(|s| s.to_string_lossy()).collect();
        assert!(args.iter().any(|s| s == "gpt-6.1-sol"));
        assert!(args.iter().any(|s| s == "forced_login_method=\"chatgpt\""));
        assert!(args
            .iter()
            .any(|s| s == "mcp_servers.qiyas.default_tools_approval_mode=\"approve\""));
        assert!(args
            .iter()
            .any(|s| s == "model_reasoning_effort=\"medium\""));
    }
}

/// The project preamble a QS receives with a tender: the specification wording of each rule
/// the case declares ("material depth plus 10% provisional allowance"). Only the clause text is
/// written; heights, factors held as data, quantities and item ids stay with the scorer.
fn preamble_text(truth: &crate::bench::model::Truth) -> Option<String> {
    let mut clauses: Vec<String> = truth
        .rules
        .values()
        .map(|rule| {
            let clause = rule.clause.trim();
            clause
                .strip_prefix("project preamble:")
                .unwrap_or(clause)
                .trim()
                .to_string()
        })
        .filter(|c| !c.is_empty())
        .collect();
    clauses.sort();
    clauses.dedup();
    if clauses.is_empty() {
        return None;
    }
    let mut text = String::from("# Project preamble (specification excerpt)\n\n");
    for clause in clauses {
        text.push_str(&format!("- {clause}\n"));
    }
    Some(text)
}

#[cfg(test)]
mod preamble_tests {
    use super::preamble_text;
    use crate::bench::model::{Rule, Truth};
    use std::collections::BTreeMap;

    #[test]
    fn preamble_carries_clause_wording_only() {
        let mut rules = BTreeMap::new();
        rules.insert(
            "working-allowance".to_string(),
            Rule {
                operation: "area_times_height".into(),
                clause: "project preamble: material depth plus 10% provisional allowance".into(),
                height_m: Some(0.2475),
                factor: None,
                deduction: None,
            },
        );
        let truth = Truth {
            schema_version: 1,
            id: "t".into(),
            project_type: "t".into(),
            drawing: "drawing.dxf".into(),
            items: Vec::new(),
            rules,
            seeded_errors: Vec::new(),
        };
        let text = preamble_text(&truth).unwrap();
        assert!(text.contains("- material depth plus 10% provisional allowance"));
        assert!(!text.contains("0.2475") && !text.contains("area_times_height"));
        let empty = Truth {
            rules: BTreeMap::new(),
            ..truth
        };
        assert!(preamble_text(&empty).is_none());
    }
}
