//! Offline stdio and process orchestration. No subscription client or provider is invoked.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use qiyas_core::{Outcome, Session, Source};
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

const CLI: &str = env!("CARGO_BIN_EXE_qiyas-cli");

fn fixture(root: &Path) -> PathBuf {
    let dir = root.join("cases/generated");
    fs::create_dir_all(&dir).unwrap();
    let drawing = dir.join("drawing.dxf");
    let mut session = Session::new_blank(&drawing, 6);
    let outcome = session.run_input(&json!({"type":"draw","kind":"polyline","closed":true,"points":[[0,0],[3,0],[3,2],[0,2]]}),Source::User).unwrap().outcome;
    let Outcome::Added { handles } = outcome else {
        panic!("rectangle");
    };
    session.save_as_headless(&drawing).unwrap();
    fs::write(dir.join("truth.json"),json!({"schema_version":1,"id":"generated","project_type":"office","drawing":"drawing.dxf","items":[{"id":"area","description":"Floor","kind":"area","unit":"m2","quantity":6.0,"source_objects":[format!("{:X}",handles[0])],"basis":"geometric","tolerance_class":"geometry"}]}).to_string()).unwrap();
    drawing
}

async fn call(
    client: &RunningService<RoleClient, ClientConfig>,
    name: &str,
    op: &str,
    args: Value,
) -> Value {
    let key = if name == "qiyas_capture" {
        "source"
    } else {
        "op"
    };
    let answer = client
        .call_tool(
            CallToolRequestParams::new(name.to_owned())
                .with_arguments(json!({key:op,"args":args}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_ne!(answer.is_error, Some(true), "{answer:?}");
    serde_json::from_str(&answer.content[0].as_text().unwrap().text).unwrap()
}

fn adapt(kit: &Value) -> Value {
    let mut batch = kit["batch"].clone();
    for command in batch["commands"].as_array_mut().unwrap() {
        match (command["type"].as_str(), command["kind"].as_str()) {
            (Some("define_object_type"), _) => {
                command["definition"]["geometry"] = json!("region");
                command["definition"]["quantities"] = json!({"surface":"geometry.area"});
            }
            (_, Some("define")) => {
                command["rule"]["match"] = json!({"entity_type":"LwPolyline","closed_region":true})
            }
            (_, Some("set_method")) => {
                command["method"]["rules"][0]["unit"] = json!("m2");
                command["method"]["rules"][0]["kind"] = json!("area");
                command["method"]["rules"][0]["terms"][0]["factors"] = json!(["geometry.area"]);
            }
            (_, Some("set_mappings")) => {
                command["mappings"][0]["unit"] = json!("m2");
                command["mappings"][0]["code"] = json!("surface-{id}");
            }
            _ => {}
        }
    }
    batch
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn assert_artifacts(run: &Path) {
    for name in [
        "takeoff.json",
        "measurement.json",
        "takeoff.xlsx",
        "self-check.json",
        "end-report.json",
        "conflicts.json",
    ] {
        assert!(run.join(name).is_file(), "{name}");
    }
    let submission = read(&run.join("takeoff.json"));
    assert_eq!(submission["items"].as_array().unwrap().len(), 2);
    assert_eq!(submission["items"][0]["quantity"], 6.0);
    assert_eq!(submission["items"][1]["quantity"], 6.0);
    assert_eq!(
        submission["items"][1]["aggregate_of"],
        json!([submission["items"][0]["id"]])
    );
    assert_eq!(
        read(&run.join("end-report.json"))["facts"]["submitted_items"],
        2
    );
}

#[tokio::test]
async fn stdio_client_uses_shared_tools_instructions_finish_and_eof() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = fixture(dir.path());
    for finish in [true, false] {
        let run = dir.path().join(if finish { "finish" } else { "eof" });
        fs::create_dir(&run).unwrap();
        fs::copy(&drawing, run.join("drawing.dxf")).unwrap();
        let mut child = tokio::process::Command::new(CLI)
            .args(["mcp", "--drawing"])
            .arg(run.join("drawing.dxf"))
            .arg("--workdir")
            .arg(&run)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let client = ClientConfig::default()
            .serve((child.stdout.take().unwrap(), child.stdin.take().unwrap()))
            .await
            .unwrap();
        // Outside agents get the same take-off workflow as Mizan, then the registry index.
        let instructions = client.peer_info().unwrap().instructions.clone().unwrap();
        assert!(!instructions.contains("Take-off workflow:"));
        assert!(instructions.contains("deliverables the user requested"));
        assert!(instructions.contains("never add an unrequested BOQ"));
        assert!(instructions.ends_with(qiyas_ai::prompt::command_index().as_str()));
        let tools = client.list_all_tools().await.unwrap();
        assert_eq!(
            tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
            ["qiyas_read", "qiyas_execute", "qiyas_capture"]
        );
        call(&client, "qiyas_read", "digest", json!({})).await;
        let kit = call(&client, "qiyas_read", "takeoff_kit", json!({})).await;
        call(&client, "qiyas_execute", "batch", adapt(&kit)).await;
        let history = call(&client, "qiyas_read", "history", json!({})).await;
        assert!(history.to_string().contains("mcp"));
        assert_eq!(history["undo"].as_array().unwrap().len(), 1);
        call(&client, "qiyas_execute", "undo_own", json!({})).await;
        call(&client, "qiyas_execute", "batch", adapt(&kit)).await;
        let image = client
            .call_tool(
                CallToolRequestParams::new("qiyas_capture").with_arguments(
                    json!({"source":"picture","args":{"width":128}})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_ne!(image.is_error, Some(true));
        for (name, input, expected) in [
            ("qiyas_capture", json!({"source":"screen"}), "needs_window"),
            (
                "qiyas_execute",
                json!({"op":"export_takeoff","args":{"path":"../escape.xlsx"}}),
                "workdir",
            ),
            (
                "qiyas_read",
                json!({"op":"document","args":{"path":"truth.json"}}),
                "unavailable",
            ),
        ] {
            let result = client
                .call_tool(
                    CallToolRequestParams::new(name)
                        .with_arguments(input.as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(true));
            assert!(
                result.content[0].as_text().unwrap().text.contains(expected),
                "{result:?}"
            );
        }
        if finish {
            call(
                &client,
                "qiyas_execute",
                "finish",
                json!({"answer":"Completed supported geometry"}),
            )
            .await;
            assert_artifacts(&run);
        }
        client.cancel().await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(15), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
        assert_artifacts(&run);
        let scored = Command::new(CLI)
            .args(["--json", "bench", "--root"])
            .arg(dir.path())
            .args(["score", "--case", "generated", "--submission"])
            .arg(run.join("takeoff.json"))
            .output()
            .unwrap();
        assert!(
            scored.status.success(),
            "{}",
            String::from_utf8_lossy(&scored.stderr)
        );
        let score: Value = serde_json::from_slice(&scored.stdout).unwrap();
        assert_eq!(score["metrics"]["matched"], 1);
        assert_eq!(score["metrics"]["passed"], 1);
    }
}

#[test]
fn fake_agent_is_scored_and_truth_reads_are_excluded() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_agent.py");
    for contaminated in [false, true] {
        let mut template = format!(
            "python3 '{}' {{cli}} {{drawing}} {{workdir}}",
            script.display()
        );
        if contaminated {
            template.push_str(&format!(
                " '{}'",
                dir.path().join("cases/generated/truth.json").display()
            ));
        }
        let result = Command::new(CLI)
            .args([
                "--json",
                "eval-agent",
                "--cases",
                "generated",
                "--agent",
                "fake",
                "--agent-cmd",
                &template,
                "--root",
            ])
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        let case = &report["reports"][0];
        let run = Path::new(case["report_directory"].as_str().unwrap());
        assert_artifacts(run);
        assert!(!run.join("truth.json").exists());
        assert!(!dir.path().join("budget-ledger.json").exists());
        assert_eq!(case["cost"], "subscription");
        assert_eq!(case["tool_calls"], 4);
        assert_eq!(case["headline"], "AGENT ALONE (fake)");
        if contaminated {
            assert_eq!(case["status"], "contaminated");
            assert!(case["score"].is_null());
            assert_eq!(report["scored_cases"], 0);
            assert_eq!(report["metrics"]["truth_items"], 0);
        } else {
            assert_eq!(case["score"]["metrics"]["passed"], 1);
            assert_eq!(report["metrics"]["matched"], 1);
        }
        let original_takeoff = fs::read(run.join("takeoff.json")).unwrap();
        let replay = dir.path().join(if contaminated {
            "replay-contaminated"
        } else {
            "replay-clean"
        });
        let combined = Path::new(report["report_directory"].as_str().unwrap()).join("report.json");
        let rescored = Command::new(CLI)
            .args(["--json", "bench", "--root"])
            .arg(dir.path())
            .args(["rescore-agent", "--report"])
            .arg(&combined)
            .arg("--output")
            .arg(&replay)
            .output()
            .unwrap();
        assert!(
            rescored.status.success(),
            "{}",
            String::from_utf8_lossy(&rescored.stderr)
        );
        let rescored: Value = serde_json::from_slice(&rescored.stdout).unwrap();
        assert_eq!(
            rescored["metrics"]["matched"],
            if contaminated { 0 } else { 1 }
        );
        assert_eq!(rescored["metrics"]["extras"], 0);
        assert_eq!(
            fs::read(run.join("takeoff.json")).unwrap(),
            original_takeoff
        );
        assert_eq!(rescored["reports"][0]["score"].is_null(), contaminated);
    }
}

#[cfg(unix)]
#[test]
fn eof_save_refuses_symlinked_temporary_output() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = fixture(dir.path());
    let run = dir.path().join("run");
    fs::create_dir(&run).unwrap();
    fs::copy(&drawing, run.join("drawing.dxf")).unwrap();
    let outside = dir.path().join("untouched");
    fs::write(&outside, b"untouched").unwrap();
    std::os::unix::fs::symlink(&outside, run.join("drawing.dxf.tmp")).unwrap();
    let result = Command::new(CLI)
        .args(["mcp", "--drawing"])
        .arg(run.join("drawing.dxf"))
        .arg("--workdir")
        .arg(&run)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("symlinks"));
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
}

struct SealedFixture {
    temp: tempfile::TempDir,
    hashes: Vec<String>,
}

impl SealedFixture {
    fn root(&self) -> &Path {
        self.temp.path()
    }

    fn permissions(&self, mode: u32) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                self.root().join("holdout"),
                fs::Permissions::from_mode(mode),
            )
            .unwrap();
        }
        #[cfg(not(unix))]
        let _ = mode;
    }

    fn assert_closed(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(self.root().join("holdout"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0
            );
        }
        assert!(!self.root().join("holdout-exam.lock").exists());
    }

    fn log(&self) -> Vec<Value> {
        fs::read_to_string(self.root().join("holdout-log.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn command(&self) -> Command {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_agent.py");
        let mut command = Command::new(CLI);
        command
            .args([
                "--json",
                "eval-agent",
                "--holdout",
                "--agent",
                "fake",
                "--agent-cmd",
            ])
            .arg(format!(
                "python3 '{}' {{cli}} {{drawing}} {{workdir}}",
                script.display()
            ))
            .arg("--root")
            .arg(self.root());
        command
    }
}

impl Drop for SealedFixture {
    fn drop(&mut self) {
        self.permissions(0o700);
    }
}

fn sealed_fixture() -> SealedFixture {
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let drawing = fixture(temp.path());
    let drawing_bytes = fs::read(&drawing).unwrap();
    let mut truth = read(&drawing.with_file_name("truth.json"));
    truth["id"] = json!("sealed");
    let mut missing = truth["items"][0].clone();
    missing["id"] = json!("perimeter");
    missing["description"] = json!("Perimeter");
    missing["kind"] = json!("length");
    missing["unit"] = json!("m");
    missing["quantity"] = json!(10);
    truth["items"].as_array_mut().unwrap().push(missing);
    truth["rules"] = json!({"private-rule-name":{"operation":"area_times_height","height_m":0.2475,
        "clause":"project preamble: Use the dimensions shown on the drawing."}});
    truth["seeded_errors"] = json!([{"item_id":"perimeter","kind":"wrong_quantity"}]);
    let mut hashes = Vec::new();
    for typ in ["office", "warehouse"] {
        truth["project_type"] = json!(typ);
        let truth_bytes = serde_json::to_vec_pretty(&truth).unwrap();
        let mut digest = Sha256::new();
        digest.update(&drawing_bytes);
        digest.update(&truth_bytes);
        let hash = format!("{:x}", digest.finalize());
        let case = temp.path().join("holdout").join(&hash);
        fs::create_dir_all(&case).unwrap();
        fs::write(case.join("drawing.dxf"), &drawing_bytes).unwrap();
        fs::write(case.join("truth.json"), truth_bytes).unwrap();
        fs::write(case.join("recognition.json"), "must not reach the agent").unwrap();
        fs::write(case.join("drawing.dxf.qiyas"), "must not reach the agent").unwrap();
        hashes.push(hash);
    }
    hashes.sort();
    fs::write(
        temp.path().join("holdout-manifest.json"),
        json!({"schema_version":1,
        "hash_algorithm":"sha256(drawing.dxf || truth.json)",
        "entries":hashes.iter().map(|hash| json!({"sha256":hash})).collect::<Vec<_>>()})
        .to_string(),
    )
    .unwrap();
    fs::remove_dir_all(temp.path().join("cases")).unwrap();
    let fixture = SealedFixture { temp, hashes };
    fixture.permissions(0);
    fixture
}

#[test]
fn holdout_refuses_without_acknowledgement_and_cannot_mix_dev_selection() {
    let fixture = sealed_fixture();
    let result = fixture.command().output().unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("--i-understand-this-opens-the-holdout")
    );
    for selection in ["--case", "--cases", "--set"] {
        let result = fixture
            .command()
            .args([
                "--i-understand-this-opens-the-holdout",
                selection,
                "live-dev",
            ])
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
    assert!(!fixture.root().join("runs").exists());
    assert!(!fixture.root().join("holdout-log.jsonl").exists());
    fixture.assert_closed();
}

#[test]
fn holdout_verifies_every_seal_before_any_agent_and_logs_failed_openings() {
    for file in ["drawing.dxf", "truth.json"] {
        let fixture = sealed_fixture();
        fixture.permissions(0o700);
        let path = fixture
            .root()
            .join("holdout")
            .join(&fixture.hashes[1])
            .join(file);
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b' ');
        fs::write(path, bytes).unwrap();
        fixture.permissions(0);
        let result = fixture
            .command()
            .arg("--i-understand-this-opens-the-holdout")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("hash mismatch"));
        let log = fixture.log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0]["status"], "opening");
        assert_eq!(log[1]["status"], "failed");
        let run = Path::new(log[0]["report_directory"].as_str().unwrap());
        assert!(!run.join("case-0").exists());
        assert!(run.join("failure.json").exists());
        fixture.assert_closed();
    }
}

#[test]
fn holdout_fake_agent_scores_logs_and_keeps_only_drawing_and_clause_inputs() {
    let fixture = sealed_fixture();
    let result = fixture
        .command()
        .arg("--i-understand-this-opens-the-holdout")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["holdout"], true);
    assert_eq!(report["complete"], true);
    assert_eq!(report["sealed_cases"], 2);
    assert_eq!(report["metrics"]["truth_items"], 4);
    assert_eq!(report["metrics"]["matched"], 2);
    assert_eq!(report["metrics"]["passed"], 2);
    let north = &report["north_star"];
    assert_eq!(north["within_2_percent_all_items"], 0.5);
    assert_eq!(north["within_2_percent_matched_items"], 1.0);
    assert_eq!(north["coverage"], 0.5);
    assert_eq!(north["traceability_submitted"], 1.0);
    assert_eq!(north["seeded_error_recall"], 0.0);
    assert_eq!(north["precision"], 1.0);
    assert!(north["seeded_error_precision"].is_null());
    assert_eq!(north["accuracy_target_met"], false);
    assert_eq!(north["coverage_target_met"], false);
    let run = Path::new(report["report_directory"].as_str().unwrap());
    assert!(run
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("holdout-exam-"));
    assert_eq!(read(&run.join("report.json")), report);
    for case in report["reports"].as_array().unwrap() {
        let workspace = Path::new(case["report_directory"].as_str().unwrap());
        assert!(workspace.starts_with(run));
        assert_artifacts(workspace);
        for forbidden in [
            "truth.json",
            "recognition.json",
            "input-boq.json",
            "schedule.json",
        ] {
            assert!(!workspace.join(forbidden).exists());
        }
        let preamble = fs::read_to_string(workspace.join("preamble.md")).unwrap();
        assert!(preamble.contains("Use the dimensions shown on the drawing."));
        for private in [
            "0.2475",
            "private-rule-name",
            "area_times_height",
            "perimeter",
        ] {
            assert!(!preamble.contains(private));
        }
    }
    let log = fixture.log();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0]["status"], "opening");
    assert_eq!(log[1]["status"], "completed");
    assert_eq!(log[1]["coverage"], 0.5);
    assert_eq!(log[1]["accuracy"], 0.5);
    assert_eq!(log[1]["precision"], 1.0);
    assert_eq!(log[1]["agent"], "fake");
    assert_eq!(log[1]["model"], "gpt-6.1-sol");
    assert_eq!(log[1]["git_commit"].as_str().unwrap().len(), 40);
    assert!(chrono::DateTime::parse_from_rfc3339(log[0]["time"].as_str().unwrap()).is_ok());
    let original_report = fs::read(run.join("report.json")).unwrap();
    let replay = fixture.root().join("saved-rescore");
    let rescored = Command::new(CLI)
        .args(["--json", "bench", "--root"])
        .arg(fixture.root())
        .args(["rescore-agent", "--report"])
        .arg(run.join("report.json"))
        .arg("--output")
        .arg(&replay)
        .output()
        .unwrap();
    assert!(
        rescored.status.success(),
        "{}",
        String::from_utf8_lossy(&rescored.stderr)
    );
    let after = read(&replay.join("report.json"));
    assert_eq!(after["metrics"]["matched"], report["metrics"]["matched"]);
    assert_eq!(after["north_star"]["traceability_submitted"], 1.0);
    assert_eq!(after["north_star"]["traceability_all"], 0.5);
    assert_eq!(fixture.log().len(), 3);
    assert_eq!(fixture.log()[2]["status"], "rescore");
    assert_eq!(fixture.log()[2]["sealed_inputs_opened"], false);
    assert_eq!(fs::read(run.join("report.json")).unwrap(), original_report);
    fixture.assert_closed();
}

#[test]
fn holdout_contaminated_cases_are_excluded_and_exam_is_incomplete() {
    let fixture = sealed_fixture();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_agent.py");
    let forbidden = fixture.root().join("truth.json");
    fs::write(&forbidden, "test contamination bait").unwrap();
    let result = Command::new(CLI)
        .args([
            "--json",
            "eval-agent",
            "--holdout",
            "--i-understand-this-opens-the-holdout",
            "--agent",
            "fake",
            "--agent-cmd",
        ])
        .arg(format!(
            "python3 '{}' {{cli}} {{drawing}} {{workdir}} '{}'",
            script.display(),
            forbidden.display()
        ))
        .arg("--root")
        .arg(fixture.root())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["complete"], false);
    assert_eq!(report["scored_cases"], 0);
    assert!(report["north_star"]["within_2_percent_all_items"].is_null());
    for case in report["reports"].as_array().unwrap() {
        assert_eq!(case["status"], "contaminated");
        assert!(case["score"].is_null());
    }
    assert_eq!(fixture.log()[1]["status"], "incomplete");
    fixture.assert_closed();
}

#[test]
fn eval_agent_passes_the_selected_name_to_mcp_and_saved_boq() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_agent.py");
    let template = format!(
        "python3 '{}' {{cli}} {{drawing}} {{workdir}} --method {{method}}",
        script.display()
    );
    let result = Command::new(CLI)
        .env("QIYAS_HOME", dir.path().join("methods-home"))
        .args([
            "--json",
            "eval-agent",
            "--cases",
            "generated",
            "--agent",
            "fake",
            "--method",
            "POMI (RICS)",
            "--agent-cmd",
            &template,
            "--root",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    let run = Path::new(report["reports"][0]["report_directory"].as_str().unwrap());
    assert_artifacts(run);
    let measurement = read(&run.join("measurement.json"));
    assert_eq!(measurement["selection"]["settings"]["name"], "POMI (RICS)");
    for item in measurement["items"].as_array().unwrap() {
        for line in item["lines"].as_array().unwrap() {
            assert_eq!(line["method"], "POMI (RICS)");
        }
    }
    let config = read(&Path::new(report["report_directory"].as_str().unwrap()).join("mcp-0.json"));
    let args = config["mcpServers"]["qiyas"]["args"].as_array().unwrap();
    assert_eq!(
        &args[args.len() - 2..],
        &[json!("--method"), json!("POMI (RICS)")]
    );
}
