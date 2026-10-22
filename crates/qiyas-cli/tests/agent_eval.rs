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
        assert_eq!(
            client.peer_info().unwrap().instructions.as_deref(),
            Some(qiyas_core::registry::registry().index_text().as_str())
        );
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
