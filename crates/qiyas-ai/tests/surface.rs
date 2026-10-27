//! The model-facing surface, with scripted providers and no network.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use qiyas_ai::prompt::{system_prompt, DrawingFacts, Lang};
use qiyas_ai::surface::{self, CAPTURE, EXECUTE, READ};
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, ContentBlock, ResultPart, ToolError, ToolOutput, TurnRequest,
};
use qiyas_core::{Command, EntityFilter, Session, Source};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[path = "support/scripted.rs"]
pub mod scripted;
use scripted::Script;

fn sample() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

#[test]
fn takeoff_kit_binds_drawing_names_runs_and_fits_one_read() {
    let (_dir, mut s) = sample();
    let digest = call(&mut s, READ, json!({"op":"digest","args":{}})).unwrap();
    assert_eq!(digest["takeoff_kit"]["op"], "takeoff_kit");
    let kit = call(&mut s, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    assert_eq!(
        kit,
        call(&mut s, READ, json!({"op":"takeoff_kit","args":{}})).unwrap()
    );
    let bytes = serde_json::to_vec(&kit).unwrap().len();
    let artifacts =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/mizan-discovery");
    std::fs::create_dir_all(&artifacts).unwrap();
    std::fs::write(
        artifacts.join("takeoff-kit.json"),
        serde_json::to_vec(&kit).unwrap(),
    )
    .unwrap();
    for info in qiyas_core::registry::registry()
        .commands()
        .iter()
        .filter(|c| c.takeoff_kit.is_some())
    {
        assert_eq!(
            kit["commands"][&info.name]["enums"],
            qiyas_core::takeoff_kit::fields(&info.schema)["enums"]
        );
    }
    assert!(bytes <= qiyas_core::takeoff_kit::MAX_BYTES, "{bytes}");
    assert_eq!(
        kit["batch"]["commands"][1]["rule"]["match"]["block"],
        digest["example_names"]["block"]
    );
    assert_eq!(
        kit["batch"]["commands"][1]["rule"]["match"]["layer"],
        digest["example_names"]["layer"]
    );
    call(&mut s, EXECUTE, json!({"op":"batch","args":kit["batch"]})).unwrap();
    assert!(!s.measurement_report().unwrap().items.is_empty());
}

#[test]
fn validate_and_run_return_full_repairs_for_empty_commands_and_batch_steps() {
    let (_dir, mut s) = sample();
    for command in [
        json!({"type":"define_object_type"}),
        json!({"type":"recognition","kind":"define"}),
        json!({"type":"measurement","kind":"set_mappings","mappings":[{}]}),
    ] {
        let validated = call(
            &mut s,
            READ,
            json!({"op":"validate","args":{"command":command}}),
        )
        .unwrap();
        let error = call(
            &mut s,
            EXECUTE,
            json!({"op":"run","args":{"command":command}}),
        )
        .unwrap_err()
        .json();
        assert_eq!(error["repairs"], validated["repairs"]);
        assert!(validated["repairs"][0]["required"].is_object());
        assert!(validated["repairs"][0]["example"].is_object());
        let error = call(
            &mut s,
            EXECUTE,
            json!({"op":"batch","args":{"commands":[command]}}),
        )
        .unwrap_err()
        .json();
        assert_eq!(error["repairs"][0]["pointer"], "/commands/0");
    }
}

fn call(s: &mut Session, tool: &str, input: Value) -> Result<Value, ToolError> {
    let surface::Route::Session { tool, op, args } =
        surface::route(tool, &input).map_err(ToolError::BadInput)?
    else {
        panic!("a session operation")
    };
    match surface::run_session(s, &tool, &op, &args, Source::Ai)? {
        ToolOutput::Json(value) => Ok(value),
        ToolOutput::Image { .. } => panic!("a JSON result"),
    }
}

fn strict(schema: &Value) {
    match schema {
        Value::Object(map) => {
            assert!(!map.contains_key("$ref"), "{schema}");
            if map.get("type") == Some(&json!("object")) || map.contains_key("properties") {
                assert_eq!(schema["additionalProperties"], false, "{schema}");
                let properties = schema["properties"].as_object().unwrap();
                assert!(!properties.is_empty(), "{schema}");
                let required: BTreeSet<_> = schema["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(
                    properties
                        .keys()
                        .map(String::as_str)
                        .collect::<BTreeSet<_>>(),
                    required,
                    "{schema}"
                );
            }
            map.values().for_each(strict);
        }
        Value::Array(values) => values.iter().for_each(strict),
        _ => {}
    }
}

#[test]
fn the_three_schemas_are_closed_and_require_every_property() {
    let tools = surface::tools();
    assert_eq!(
        tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
        [READ, EXECUTE, CAPTURE]
    );
    for tool in tools {
        strict(&tool.input_schema);
        assert_eq!(tool.input_schema["properties"]["args"]["type"], "string");
    }
}

#[test]
fn commands_without_arguments_return_the_registry_index() {
    let (_dir, mut s) = sample();
    for input in [
        json!({"op": "commands", "args": "{}"}),
        json!({"op": "commands", "args": {}}),
        json!({"op": "commands"}),
    ] {
        let answer = call(&mut s, READ, input).unwrap();
        assert_eq!(answer["index"], qiyas_ai::prompt::command_index());
    }
    assert!(!s.can_undo());
}

#[test]
fn typed_commands_are_discoverable_and_run_with_the_same_undo_and_reveal() {
    let (_dir, mut s) = sample();
    let capabilities = call(&mut s, READ, json!({"op": "capabilities"})).unwrap();
    assert!(capabilities["execute_ops"]
        .as_array()
        .unwrap()
        .contains(&json!("type")));
    let info = call(
        &mut s,
        READ,
        json!({"op": "commands", "args": {"name": "draw.polyline"}}),
    )
    .unwrap();
    assert!(info["words"].as_array().unwrap().contains(&json!("PLINE")));
    let before = count(&s);
    let input = json!({"op": "type", "args": {"line": "PLINE 0,0 10,0 C"}});
    assert_eq!(
        surface::label(EXECUTE, &input).as_deref(),
        Some("draw.polyline")
    );
    let out = call(&mut s, EXECUTE, input).unwrap();
    assert_eq!(out["command"], "draw.polyline");
    assert_eq!(out["added"].as_array().unwrap().len(), 1);
    assert_eq!(out["step"]["source"], "ai");
    assert_eq!(out["ui"]["action"], "reveal");
    assert_eq!(count(&s), before + 1);
    assert!(s.undo_if(Source::Ai));
    assert_eq!(count(&s), before);
}

#[test]
fn typed_commands_use_the_ucs_and_do_not_run_incomplete_input() {
    let (_dir, mut s) = sample();
    s.ucs = qiyas_core::Ucs::new([10.0, 20.0], [0.0, 1.0]).unwrap();
    let before = count(&s);
    let pending = call(
        &mut s,
        EXECUTE,
        json!({"op": "type", "args": {"line": "LINE 0,0"}}),
    )
    .unwrap();
    assert_eq!(pending["complete"], false);
    assert!(pending["prompt"].is_string());
    assert_eq!(count(&s), before);
    assert!(!s.can_undo());
    let out = call(
        &mut s,
        EXECUTE,
        json!({"op": "type", "args": "{\"line\":\"L ١،٢ @٣،٠\"}"}),
    )
    .unwrap();
    let handle = out["added"][0].as_u64().unwrap();
    let entity = s
        .find_entities(&EntityFilter {
            limit: 500,
            ..Default::default()
        })
        .rows
        .into_iter()
        .find(|row| row.handle == handle)
        .unwrap();
    assert_eq!(entity.bounds, [[8.0, 21.0], [8.0, 24.0]]);
}

#[test]
fn new_registry_words_need_no_ai_dispatch_and_keep_destructive_metadata() {
    let (_dir, mut s) = sample();
    let out = call(
        &mut s,
        EXECUTE,
        json!({"op": "type", "args": {"line": "NEWLAYER \"Typed layer\""}}),
    )
    .unwrap();
    assert_eq!(out["command"], "new_layer");
    assert!(s.drawing.doc.layers.get("Typed layer").is_some());
    assert_eq!(
        surface::destructive_count(EXECUTE, &json!({"op": "type", "args": {"line": "ERASE"}})),
        1
    );
    let error = call(
        &mut s,
        EXECUTE,
        json!({"op": "type", "args": {"line": "WBLOCK DOOR ../outside.dwg"}}),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("Only files beside the open drawing"),
        "{error}"
    );
}

#[test]
fn a_wrong_command_names_the_field_and_gives_its_schema_and_example() {
    let (_dir, mut s) = sample();
    let error = call(
        &mut s,
        EXECUTE,
        json!({"op": "run", "args": {"command": {
            "type": "draw", "kind": "line", "a": [0, 0], "b": ["wrong", 1]
        }}}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("/b/0"), "{error}");
    assert!(error.contains("schema:"), "{error}");
    assert!(error.contains("Example:"), "{error}");
    assert!(error.contains("draw.line"), "{error}");
    assert!(!s.can_undo());
}

#[test]
fn an_unknown_command_type_suggests_close_names() {
    let (_dir, mut s) = sample();
    let error = call(
        &mut s,
        EXECUTE,
        json!({"op": "run", "args": {"command": {"type": "measure_distanc"}}}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("close ones"), "{error}");
    assert!(error.contains("measure_distance"), "{error}");
}

fn count(s: &Session) -> usize {
    s.find_entities(&EntityFilter {
        limit: 0,
        ..Default::default()
    })
    .total
}

#[test]
fn batch_references_use_the_earlier_result_and_undo_as_one_step() {
    let (_dir, mut s) = sample();
    let before = count(&s);
    let out = call(
        &mut s,
        EXECUTE,
        json!({"op": "batch", "args": {
            "layer": "BATCH", "color": "#ff0000",
            "commands": [
                {"type": "draw", "kind": "line", "a": [30, 0], "b": [35, 0]},
                {"type": "modify", "kind": "move",
                 "targets": [{"handle": {"$step": 0, "pointer": "/added/0"}}], "delta": [0, 5]}
            ]
        }}),
    )
    .unwrap();
    let handle = out["outcome"]["steps"][0]["added"][0].as_u64().unwrap();
    assert_eq!(out["outcome"]["steps"][1]["changed"], json!([handle]));
    assert_eq!(out["added"], json!([handle]));
    assert_eq!(count(&s), before + 1);
    let info = s.entity_info(handle, &[]).unwrap();
    let bounds = s
        .find_entities(&EntityFilter {
            limit: 500,
            ..Default::default()
        })
        .rows
        .into_iter()
        .find(|row| row.handle == handle)
        .unwrap()
        .bounds;
    assert_eq!(bounds, [[30.0, 5.0], [35.0, 5.0]]);
    assert_eq!(info.layer_short, "BATCH");
    assert_eq!(info.color, "#ff0000");
    assert_eq!(s.current.layer, "0");
    assert_eq!(s.current.color, "ByLayer");
    assert_eq!(s.history_steps().0.len(), 1);
    assert!(s.undo_if(Source::Ai));
    assert_eq!(count(&s), before);
    assert!(s.drawing.doc.layers.get("BATCH").is_none());
    assert!(s.redo());
    assert_eq!(count(&s), before + 1);
}

#[test]
fn a_failing_referenced_step_puts_the_drawing_and_state_back() {
    let (_dir, mut s) = sample();
    let before = count(&s);
    let revision = s.document_revision();
    let current = s.current.clone();
    let error = call(
        &mut s,
        EXECUTE,
        json!({"op": "batch", "args": {
            "layer": "ROLLBACK", "color": "#ff0000",
            "commands": [
                {"type": "draw", "kind": "circle", "centre": [30, 0], "radius": 1},
                {"type": "set_property", "target": {"handle": {"$step": 0, "pointer": "/added/0"}},
                 "prop": "radius", "value": -1}
            ]
        }}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Batch step 1 (set_property)"), "{error}");
    assert!(error.contains("The value must be more than 0"), "{error}");
    assert_eq!(count(&s), before);
    assert_eq!(s.current, current);
    assert_eq!(s.document_revision(), revision);
    assert!(s.drawing.doc.layers.get("ROLLBACK").is_none());
    assert!(!s.can_undo());
}

#[test]
fn records_can_create_and_edit_an_entity_through_execute() {
    let (_dir, mut s) = sample();
    let out = call(
        &mut s,
        EXECUTE,
        json!({"op": "create_entities", "args": {
            "entities": [{"type": "Circle", "data": {"radius": 2,
                "center": {"x": 30, "y": 20, "z": 0}}}]
        }}),
    )
    .unwrap();
    let handle = out["added"][0].as_u64().unwrap();
    assert_eq!(s.entity_info(handle, &[]).unwrap().radius, Some(2.0));
    let edited = call(
        &mut s,
        EXECUTE,
        json!({"op": "set_records", "args": {
            "collection": "entities",
            "updates": [{"key": handle, "path": "/radius", "value": 3, "expected": 2.0}]
        }}),
    )
    .unwrap();
    assert_eq!(edited["changed"], json!([handle]));
    assert_eq!(s.entity_info(handle, &[]).unwrap().radius, Some(3.0));
    assert!(s.undo_if(Source::Ai));
    assert_eq!(s.entity_info(handle, &[]).unwrap().radius, Some(2.0));
    assert!(s.undo_if(Source::Ai));
    assert!(s.entity_info(handle, &[]).is_err());
}

#[test]
fn read_run_refuses_changes_and_leaves_the_drawing_alone() {
    let (_dir, mut s) = sample();
    let before = count(&s);
    let error = call(
        &mut s,
        READ,
        json!({"op": "run", "args": {"command": {
            "type": "draw", "kind": "line", "a": [0, 0], "b": [5, 0]
        }}}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("qiyas_execute"), "{error}");
    assert_eq!(count(&s), before);
    assert!(!s.can_undo());
}

#[test]
fn verify_and_conventions_use_the_existing_three_tools() {
    let (_dir, mut s) = sample();
    for name in [
        "verify",
        "center_mark",
        "center_line",
        "symmetry_axis",
        "grid",
    ] {
        let info = call(
            &mut s,
            READ,
            json!({"op": "commands", "args": {"name": name}}),
        )
        .unwrap();
        assert_eq!(info["name"], name);
        assert_eq!(
            info["effect"],
            if name == "verify" { "read" } else { "edit" }
        );
    }
    let line = call(
        &mut s,
        EXECUTE,
        json!({"op": "run", "args": {"command": {
            "type": "draw", "kind": "line", "a": [30, 0], "b": [35, 0]
        }}}),
    )
    .unwrap();
    let handle = line["added"][0].as_u64().unwrap();
    let revision = s.document_revision();
    let history = s.history_steps();
    let answer = call(
        &mut s,
        READ,
        json!({"op": "run", "args": {"command": {
            "type": "verify", "expectations": [
                {"kind": "length", "targets": [{"handle": handle}], "value": 5},
                {"kind": "length", "targets": [{"handle": handle}], "value": 7}
            ]
        }}}),
    )
    .unwrap();
    assert_eq!(answer["outcome"]["type"], "verified");
    assert_eq!(answer["outcome"]["results"][0]["passed"], true);
    assert_eq!(answer["outcome"]["results"][1]["passed"], false);
    assert_eq!(answer["outcome"]["results"][1]["measured"], 5.0);
    assert_eq!(s.document_revision(), revision);
    assert_eq!(s.history_steps(), history);
    assert_eq!(surface::tools().len(), 3);
}

#[test]
fn run_batch_refuses_internal_commands_before_any_step_runs() {
    let (_dir, mut s) = sample();
    let before = count(&s);
    let error = call(
        &mut s,
        EXECUTE,
        json!({"op": "run", "args": {"command": {
            "type": "batch", "commands": [
                {"type": "draw", "kind": "line", "a": [30, 0], "b": [35, 0]},
                {"type": "set_boq", "boq": {
                    "source": "test.xlsx", "imported": "2026-10-20", "items": []
                }}
            ]
        }}}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("run by Qiyas itself"), "{error}");
    assert!(error.contains("set_boq"), "{error}");
    assert_eq!(count(&s), before);
    assert!(s.boq.is_none());
    assert!(!s.can_undo());
}

#[test]
fn destructive_commands_written_as_strings_count_inside_both_batch_forms() {
    let command = json!({"type": "delete_entities", "targets": [{"handle": 1234}]}).to_string();
    assert!(
        qiyas_core::registry::registry()
            .get("delete_entities")
            .unwrap()
            .destructive
    );
    for input in [
        json!({"op": "batch", "args": {"commands": [command.clone()]}}),
        json!({"op": "run", "args": {"command": {"type": "batch", "commands": [command]}}}),
    ] {
        assert_eq!(surface::destructive_count(EXECUTE, &input), 1, "{input}");
    }
}

#[test]
fn a_no_op_reports_no_step_instead_of_the_previous_users_step() {
    let (_dir, mut s) = sample();
    s.run(Command::SaveView {
        name: "Test".into(),
        bounds: [[0.0, 0.0], [5.0, 5.0]],
    })
    .unwrap();
    s.run(Command::DeleteView {
        name: "Test".into(),
    })
    .unwrap();
    let before = s.history_steps();
    assert_eq!(before.0.last().unwrap().source, Source::User);
    assert_eq!(before.0.last().unwrap().label, "delete_view");
    let out = call(
        &mut s,
        EXECUTE,
        json!({"op": "run", "args": {"command": {"type": "delete_view", "name": "Test"}}}),
    )
    .unwrap();
    assert!(out["step"].is_null(), "{out}");
    assert_eq!(s.history_steps(), before);
}

#[cfg(unix)]
#[test]
fn exports_refuse_new_files_beneath_a_symlink_outside_the_drawing_folder() {
    let (dir, mut s) = sample();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("outside")).unwrap();
    for (op, filename) in [
        ("export_takeoff", "takeoff.xlsx"),
        ("export_conflicts", "conflicts.xlsx"),
    ] {
        let error = call(
            &mut s,
            EXECUTE,
            json!({"op": op, "args": {"path": format!("outside/{filename}")}}),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("Only files beside the open drawing"),
            "{error}"
        );
        assert!(!outside.path().join(filename).exists());
        let written = call(
            &mut s,
            EXECUTE,
            json!({"op": op, "args": {"path": filename}}),
        )
        .unwrap();
        let path = dir.path().join(filename);
        assert_eq!(written["written"], path.to_string_lossy().as_ref());
        assert!(path.is_file());
    }
}

#[test]
fn every_name_in_the_prompt_index_exists_in_the_registry() {
    let registry = qiyas_core::registry::registry();
    let index = qiyas_ai::prompt::command_index();
    for language in [Lang::En, Lang::Ar] {
        let prompt = system_prompt(language, &DrawingFacts::none());
        assert!(prompt.contains(&format!("## Commands by category\n{index}\n")));
        assert_eq!(prompt, system_prompt(language, &DrawingFacts::none()));
    }
    for line in index.lines() {
        let (category, names) = line.split_once(": ").unwrap();
        if category.ends_with("(qiyas_read)") {
            for name in names.split(", ") {
                assert!(
                    surface::READ_OPS.contains(&name),
                    "unknown read operation {name}"
                );
            }
            continue;
        }
        let mut parent = "";
        let mut kinds = false;
        for word in names.split_whitespace() {
            let name = word.trim_matches([',', '(', ')']);
            if word.starts_with('(') {
                kinds = true;
            }
            let full = if kinds {
                format!("{parent}.{name}")
            } else {
                name.to_string()
            };
            let command = registry
                .get(&full)
                .unwrap_or_else(|| panic!("unknown command {full}"));
            assert!(!command.internal, "{full}");
            if !kinds {
                parent = name;
            }
            if word.contains(')') {
                kinds = false;
            }
        }
    }
}

async fn scripted(
    session: Arc<Mutex<Option<Session>>>,
    calls: Vec<(&'static str, Value)>,
) -> (Agent, Vec<AgentEvent>, Arc<Mutex<Vec<TurnRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = Script::new(calls, requests.clone());
    let mut agent = Agent::new(Box::new(provider), session, AgentSettings::default());
    let (tx, mut rx) = mpsc::channel(1_000);
    agent
        .run_turn("Do the requested work".into(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    (agent, events, requests)
}

fn results(agent: &Agent) -> Vec<(bool, Value)> {
    message_results(agent.history())
}

fn observed_results(agent: &Agent, requests: &[TurnRequest]) -> Vec<(bool, Value)> {
    // A capped run has no next request for its last result; include the final local history.
    let mut requests = requests.to_vec();
    let mut final_request = requests.last().unwrap().clone();
    final_request.messages = agent.history().to_vec();
    requests.push(final_request);
    message_results(&scripted::observed_messages(&requests))
}

fn message_results(messages: &[qiyas_ai::Message]) -> Vec<(bool, Value)> {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult {
                is_error, content, ..
            } => {
                let ResultPart::Text(text) = &content[0] else {
                    panic!("text result")
                };
                Some((
                    *is_error,
                    serde_json::from_str(text).unwrap_or_else(|_| json!({"error": text})),
                ))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn successful_reads_continue_with_cost_advice() {
    let (_dir, s) = sample();
    let calls = vec![(READ, json!({"op":"state","args":"{}"})); 50];
    let (agent, events, requests) = scripted(Arc::new(Mutex::new(Some(s))), calls).await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 51);
    for (offered, expected) in requests[0].tools.iter().zip(surface::tools()) {
        assert_eq!(offered.name, expected.name);
        assert_eq!(offered.input_schema, expected.input_schema);
    }
    let results = observed_results(&agent, &requests);
    assert_eq!(results.len(), 50);
    assert!(results.iter().all(|(error, _)| !error));
    // The efficiency advice is given once, not on every call after the threshold.
    assert_eq!(
        results
            .iter()
            .filter(|(_, r)| r["advice"].to_string().contains("tool_cost"))
            .count(),
        1
    );
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
}

#[tokio::test]
async fn drawing_work_continues_beyond_the_cost_advice_threshold() {
    let (_dir, s) = sample();
    let mut calls = vec![(
        EXECUTE,
        json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[30,0],"b":[35,0]}}}),
    )];
    calls.extend(vec![(READ, json!({"op":"state"})); 130]);
    let (agent, events, requests) = scripted(Arc::new(Mutex::new(Some(s))), calls).await;
    assert_eq!(requests.lock().unwrap().len(), 132);
    let results = observed_results(&agent, &requests.lock().unwrap());
    assert_eq!(results.len(), 131);
    assert!(results.iter().all(|(error, _)| !error));
    // The efficiency advice is given once, not on every call after the threshold.
    assert_eq!(
        results
            .iter()
            .filter(|(_, r)| r["advice"].to_string().contains("tool_cost"))
            .count(),
        1
    );
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
}

#[tokio::test]
async fn destructive_commands_in_run_and_batch_report_counts_and_apply() {
    let (_dir, mut s) = sample();
    let ids: Vec<_> = (0..12)
        .map(|_| {
            match s
                .run(Command::MeasureDistance {
                    points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
                })
                .unwrap()
            {
                qiyas_core::Outcome::Measured { measurement } => measurement.id,
                other => panic!("{other:?}"),
            }
        })
        .collect();
    let remove = |id: &str| json!({"type":"remove_measurement","id":id});
    let mut calls: Vec<_> = ids[..6]
        .iter()
        .map(|id| (EXECUTE, json!({"op":"run","args":{"command":remove(id)}})))
        .collect();
    let over = json!({"op":"batch","args":{"commands":ids[6..11].iter().map(|id|remove(id)).collect::<Vec<_>>()}});
    assert_eq!(surface::destructive_count(EXECUTE, &over), 5);
    calls.push((EXECUTE, over));
    let session = Arc::new(Mutex::new(Some(s)));
    let (agent, events, _) = scripted(session.clone(), calls).await;
    let results = results(&agent);
    assert_eq!(results.len(), 7);
    assert!(results.iter().all(|(error, _)| !error));
    assert!(results[6].1["advice"].to_string().contains("many_removals"));
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    let guard = session.lock().unwrap();
    let remaining = &guard.as_ref().unwrap().takeoff.items;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, ids[11]);
}

#[test]
fn operation_needs_cover_every_public_op_and_registry_reads_need_nothing() {
    let (_drawing, mut session) = sample();
    let kit = call(&mut session, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":kit["batch"]}),
    )
    .unwrap();
    let item = session.measurement_report().unwrap().items[0].clone();
    let project = tempfile::Builder::new()
        .suffix(".qss")
        .tempfile_in(qiyas_core::data_home::qiyas_dir().unwrap().join("projects"))
        .unwrap();
    std::fs::write(project.path(), serde_json::to_vec(&json!({"name":"Trust fixture","drawings":[],"project":{"boq":[{"item":item,"contributions":[]}]}})).unwrap()).unwrap();
    let documents = tempfile::tempdir().unwrap();
    let pdf = qiyas_pdf::fixtures::write_wall(documents.path());
    let mut declared = BTreeSet::new();
    for entry in surface::OPERATIONS {
        assert!(
            declared.insert((entry.tool, entry.op)),
            "duplicate operation"
        );
        for (drawing, window, host) in [
            (true, false, true),
            (false, true, false),
            (true, true, false),
            (true, true, true),
        ] {
            let missing = entry.needs.iter().find_map(|need| match need {
                surface::Need::Drawing if !drawing => Some("no_drawing"),
                surface::Need::Window if !window => Some("needs_window"),
                surface::Need::Host if !host => Some("needs_host"),
                _ => None,
            });
            let answer = surface::check_need(entry.tool, entry.op, drawing, window, host);
            match missing {
                None => assert!(answer.is_ok(), "{} {}", entry.tool, entry.op),
                Some(code) => assert_eq!(answer.unwrap_err().json()["code"], code),
            }
        }
        let need = surface::check_need(entry.tool, entry.op, false, false, false);
        let expected = match entry.needs[0] {
            surface::Need::Nothing => {
                let args = match entry.op {
                    "explain" => json!({"item":item.code,"project":project.path()}),
                    "document" if entry.tool == CAPTURE => json!({"path":pdf,"page":1}),
                    "document" => json!({"path":pdf,"pages":[1]}),
                    "plan" => {
                        json!({"phases":[{"id":"sizes","title":"Check sizes","status":"active"},{"id":"review","title":"Review","status":"pending"}]})
                    }
                    "artifact" => {
                        let raw = surface::run_without_drawing(READ, "commands", &json!({}), None)
                            .unwrap()
                            .unwrap();
                        let qiyas_ai::ToolOutput::Json(envelope) =
                            surface::format_result(READ, "commands", &json!({}), raw).unwrap()
                        else {
                            panic!("JSON envelope")
                        };
                        json!({"id": envelope["artifact"], "offset": 0})
                    }
                    "record_schema" => json!({"name":"line"}),
                    "ask" => {
                        json!({"questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]})
                    }
                    "validate" => {
                        json!({"command":{"type":"draw","kind":"line","a":[0,0],"b":[1,0]}})
                    }
                    _ => json!({}),
                };
                assert!(need.is_ok(), "{}", entry.op);
                let result =
                    surface::run_without_drawing(entry.tool, entry.op, &args, None).unwrap();
                if entry.op == "document" && !qiyas_pdf::available() {
                    assert!(result.unwrap_err().to_string().contains("PDF support"));
                } else {
                    result.unwrap();
                }
                continue;
            }
            surface::Need::Drawing => "no_drawing",
            surface::Need::Window => "needs_window",
            surface::Need::Host => "needs_host",
        };
        assert_eq!(
            need.unwrap_err().json()["code"],
            expected,
            "{} {}",
            entry.tool,
            entry.op
        );
    }
    for (tool, ops) in [
        (READ, surface::READ_OPS.as_slice()),
        (EXECUTE, surface::EXECUTE_OPS.as_slice()),
        (CAPTURE, &["screen", "picture", "document"]),
    ] {
        for op in ops {
            assert!(declared.contains(&(tool, op)));
        }
    }
}

#[test]
fn plan_is_discoverable_validated_and_never_changes_the_drawing_or_undo() {
    let (_dir, mut session) = sample();
    let before = count(&session);
    let revision = session.history_revision();
    let phases = json!([
        {"id":"sizes","title":"Check sizes","status":"done"},
        {"id":"geometry","title":"Draw geometry","status":"active"},
        {"id":"labels","title":"Labels","status":"pending"},
        {"id":"sheet","title":"Sheet","status":"skipped"}
    ]);
    let input = json!({"op":"plan","args":{"phases":phases}});
    assert!(!surface::mutating(EXECUTE, &input));
    surface::check_need(EXECUTE, "plan", false, false, false).unwrap();
    let out = call(&mut session, EXECUTE, input).unwrap();
    assert_eq!(out["plan"], json!({"phases":phases}));
    assert!(out.get("revision").is_some());
    assert_eq!(count(&session), before);
    assert_eq!(session.history_revision(), revision);
    assert!(!session.can_undo());
    let mut info = call(
        &mut session,
        READ,
        json!({"op":"commands","args":{"name":"plan"}}),
    )
    .unwrap();
    assert_eq!(info["op"], "plan");
    assert_eq!(info["undo"], false);
    let caps = call(&mut session, READ, json!({"op":"capabilities"})).unwrap();
    assert_eq!(info["revision"], caps["revision"]);
    info.as_object_mut().unwrap().remove("revision");
    assert_eq!(caps["plan"], info);
    let index = call(&mut session, READ, json!({"op":"commands"})).unwrap();
    assert_eq!(index["operations"][0], info);
    for bad in [
        json!({"phases":[]}),
        json!({"phases":[{"id":"a","title":"A","status":"invalid"}]}),
        json!({"phases":[{"id":"a","title":"A","status":"active"},{"id":"b","title":"B","status":"active"}]}),
        json!({"phases":[{"id":"a","title":"A","status":"done"},{"id":"a","title":"B","status":"pending"}]}),
    ] {
        assert!(surface::run_without_drawing(EXECUTE, "plan", &bad, None)
            .unwrap()
            .is_err());
    }
}

#[tokio::test]
async fn plan_events_follow_valid_snapshots_and_the_provider_observes_them_without_a_drawing() {
    let pending = json!({"phases":[{"id":"sizes","title":"Check sizes","status":"pending"},{"id":"review","title":"Review","status":"pending"}]});
    let active = json!({"phases":[{"id":"sizes","title":"Check sizes","status":"active"},{"id":"review","title":"Review","status":"pending"}]});
    let done = json!({"phases":[{"id":"sizes","title":"Check sizes","status":"done"},{"id":"review","title":"Review","status":"pending"}]});
    let (agent, events, requests) = scripted(
        Arc::new(Mutex::new(None)),
        vec![
            (EXECUTE, json!({"op":"plan","args":pending})),
            (EXECUTE, json!({"op":"plan","args":{"phases":[]}})),
            (EXECUTE, json!({"op":"plan","args":active.to_string()})),
            (EXECUTE, json!({"op":"plan","args":done})),
        ],
    )
    .await;
    let plans: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Plan { phases } => Some(json!({"phases":phases})),
            _ => None,
        })
        .collect();
    assert_eq!(plans, [pending.clone(), active.clone(), done.clone()]);
    assert!(events
        .iter()
        .all(|event| !matches!(event, AgentEvent::ToolCall { mutating: true, .. })));
    let history = observed_results(&agent, &requests.lock().unwrap());
    assert_eq!(
        history.iter().map(|(error, _)| *error).collect::<Vec<_>>(),
        [false, true, false, false]
    );
    let saved: Vec<_> = history
        .into_iter()
        .filter(|(error, _)| !error)
        .map(|(_, result)| result["plan"].clone())
        .collect();
    assert_eq!(saved, [pending, active, done]);
}

async fn events_for(agent: &mut Agent, message: &str) -> Vec<AgentEvent> {
    let (tx, mut rx) = mpsc::channel(1024);
    agent
        .run_turn(message.into(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn approval_modes_pause_at_the_job_and_phase_boundaries() {
    use qiyas_ai::Autonomy;
    for mode in [Autonomy::PerJob, Autonomy::PerPhase, Autonomy::Autonomous] {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let initial = json!({"phases":[{"id":"one","title":"First phase","status":"active"},{"id":"two","title":"Second phase","status":"pending"}]});
        let done = json!({"phases":[{"id":"one","title":"First phase","status":"done"},{"id":"two","title":"Second phase","status":"active"}]});
        let provider = Script::new(
            vec![
                (EXECUTE, json!({"op":"plan","args":initial})),
                (EXECUTE, json!({"op":"plan","args":done})),
            ],
            requests.clone(),
        );
        let mut agent = Agent::new(
            Box::new(provider),
            Arc::new(Mutex::new(None)),
            AgentSettings {
                autonomy: mode,
                ..Default::default()
            },
        );
        let events = events_for(&mut agent, "Start the job").await;
        if mode == Autonomy::Autonomous {
            assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
            assert!(!events
                .iter()
                .any(|event| matches!(event, AgentEvent::AwaitingApproval { .. })));
            // The finish gate sends the early ending back twice (phase two is still active).
            assert_eq!(requests.lock().unwrap().len(), 5);
        } else {
            assert!(
                matches!(events.last(), Some(AgentEvent::AwaitingApproval { plan }) if plan.phases.len() == 2)
            );
            assert_eq!(
                requests.lock().unwrap().len(),
                1,
                "no request after the plan result until approval"
            );
            assert!(agent.history().last().unwrap().has_tool_results());
            let events = events_for(&mut agent, "Approved, continue.").await;
            if mode == Autonomy::PerPhase {
                assert!(matches!(
                    events.last(),
                    Some(AgentEvent::AwaitingApproval { .. })
                ));
                assert_eq!(requests.lock().unwrap().len(), 2);
            } else {
                assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
                // The finish gate sends the early ending back twice (phase two is still active).
                assert_eq!(requests.lock().unwrap().len(), 5);
            }
        }
    }
}

#[test]
fn state_read_reports_method_name_and_whether_rules_are_loaded() {
    let (_dir, mut session) = sample();
    for loaded in [false, true] {
        let method = qiyas_ai::MeasurementMethod {
            name: Some("office-method".into()),
            rules_loaded: loaded,
        };
        let ToolOutput::Json(state) = surface::run_session_with_method(
            &mut session,
            READ,
            "state",
            &json!({}),
            Source::Ai,
            &method,
        )
        .unwrap() else {
            panic!("JSON state");
        };
        assert_eq!(
            state["measurement_method"],
            json!({
                "name":"office-method", "rules_loaded":loaded, "instruction":method.instruction()
            })
        );
    }
}

#[tokio::test]
async fn open_findings_survive_restart_until_repaired() {
    let (_dir, mut session) = sample();
    session.current.layer = "A-WALL".into();
    let shared = Arc::new(Mutex::new(Some(session)));
    let settings = AgentSettings {
        autonomy: qiyas_ai::Autonomy::PerJob,
        ..Default::default()
    };
    let line = json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[100,100],"b":[110,100]}}});
    let mut agent = Agent::new(
        Box::new(Script::new(
            vec![(EXECUTE, line.clone()), (EXECUTE, line)],
            Default::default(),
        )),
        shared.clone(),
        settings.clone(),
    );
    let events = events_for(&mut agent, "Draw").await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e,AgentEvent::OpenFindings{findings,..} if !findings.is_empty())));
    let saved = serde_json::to_vec(&agent.snapshot()).unwrap();
    let conversation = serde_json::from_slice(&saved).unwrap();
    let mut resumed = Agent::resume(
        conversation,
        Box::new(Script::new(vec![], Default::default())),
        shared.clone(),
        settings.clone(),
    );
    let events = events_for(&mut resumed, "Continue.").await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e,AgentEvent::OpenFindings{findings,..} if !findings.is_empty())));
    let mut repairing = Agent::resume(
        resumed.snapshot(),
        Box::new(Script::new(
            vec![(EXECUTE, json!({"op":"undo_own","args":{}}))],
            Default::default(),
        )),
        shared,
        settings,
    );
    assert!(matches!(
        events_for(&mut repairing, "Fix the error").await.last(),
        Some(AgentEvent::Done { .. })
    ));
}

#[tokio::test]
async fn preexisting_errors_after_the_display_limit_are_not_attributed_to_mizan() {
    use opencadcodec::{EntityType, MText, Vector3};
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("Untitled.dwg"), 6);
    let owner = session
        .drawing
        .doc
        .block_records
        .get("*Model_Space")
        .unwrap()
        .handle;
    let mut last = 0;
    for index in 0..205 {
        let mut text = MText::with_value(
            "BED\\nROOM",
            Vector3::new(f64::from(index) * 100.0, 0.0, 0.0),
        );
        text.common.owner_handle = owner;
        last = session
            .drawing
            .doc
            .add_entity(EntityType::MText(text))
            .unwrap()
            .value();
    }
    let report = session.check_drawing(&Default::default());
    assert!(report.errors > 200);
    assert_eq!(report.findings.len(), 200);
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.handles.contains(&last)));
    let edit = json!({"op":"run","args":{"command":{"type":"set_property","target":{"handle":last},"prop":"text_height","value":0.3}}});
    let mut agent = Agent::new(
        Box::new(Script::new(vec![(EXECUTE, edit)], Default::default())),
        Arc::new(Mutex::new(Some(session))),
        AgentSettings::default(),
    );
    let events = events_for(&mut agent, "Change the height only").await;
    assert!(
        matches!(events.last(), Some(AgentEvent::Done { .. })),
        "the existing text error beyond the display cap was not made by Mizan: {events:?}"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolResult {
            is_error: false,
            ..
        }
    )));
}

#[test]
fn validate_reports_all_input_errors_and_checks_step_indexes_without_a_drawing() {
    let input = json!({"commands":[
        {"type":"draw","kind":"line","a":[0,0],"extra":true},
        {"type":"draw","kind":"circle","center":[0,0],"radius":"bad"},
        {"type":"delete_entities","targets":[{"handle":{"$step":2,"pointer":"/added/0"}}]}
    ]});
    let ToolOutput::Json(out) = surface::run_without_drawing(READ, "validate", &input, None)
        .unwrap()
        .unwrap()
    else {
        panic!("JSON")
    };
    assert_eq!(out["valid"], false);
    assert!(out["errors"].as_array().unwrap().len() >= 3, "{out}");
    assert!(
        out["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "unknown_field"),
        "{out}"
    );
    assert!(out["not_checked"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let (_dir, mut session) = sample();
    let before = count(&session);
    let revision = session.history_revision();
    let out = call(
        &mut session,
        READ,
        json!({"op":"validate","args":{"commands":[
            {"type":"draw","kind":"line","a":[0,0],"b":[10,0]},
            {"type":"delete_entities","targets":[{"handle":{"$step":0,"pointer":"/added/0"}}]}
        ]}}),
    )
    .unwrap();
    assert_eq!(out["valid"], true, "{out}");
    assert_eq!(count(&session), before);
    assert_eq!(session.history_revision(), revision);
}

#[test]
fn executing_bad_inputs_reports_all_fields_and_keeps_schema_and_example() {
    let (_dir, mut session) = sample();
    let error = call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":{
            "type":"draw","kind":"line","a":"wrong","typo":12
        }}}),
    )
    .unwrap_err()
    .json();
    let errors = error["errors"].as_array().unwrap();
    assert!(
        errors.iter().any(|issue| issue["pointer"] == "/a"),
        "{error}"
    );
    assert!(
        errors.iter().any(|issue| issue["pointer"] == "/b"),
        "{error}"
    );
    assert!(errors
        .iter()
        .any(|issue| issue["pointer"] == "/typo" && issue["code"] == "unknown_field"));
    assert!(error["schema"].is_object());
    assert!(error["example"].is_object());
}

#[tokio::test]
async fn mizan_reads_commands_and_validates_before_a_drawing_is_open() {
    let (agent, events, _) = scripted(Arc::new(Mutex::new(None)), vec![
        (READ, json!({"op":"commands"})),
        (READ, json!({"op":"validate","args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[1,0]}}})),
        (READ, json!({"op":"state"})),
    ]).await;
    let outputs = results(&agent);
    assert!(!outputs[0].0, "{:?}", outputs[0]);
    assert!(outputs[0].1["index"].is_string());
    assert_eq!(outputs[1].1["valid"], true);
    assert_eq!(outputs[2].1["code"], "no_drawing");
    assert!(events
        .iter()
        .any(|event| matches!(event, AgentEvent::ToolResult { is_error: true, .. })));
}

#[test]
fn valid_inputs_keep_defaults_and_edits_refuse_unknown_fields() {
    let (_dir, mut session) = sample();
    let mut command =
        json!({"type":"draw","kind":"polyline","points":[[20000,0],[21000,0]],"typo":true});
    let before = count(&session);
    let error = call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":command}}),
    )
    .unwrap_err()
    .json();
    assert!(error["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["pointer"] == "/typo"));
    assert_eq!(count(&session), before);
    command.as_object_mut().unwrap().remove("typo");
    let single = call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":command}}),
    )
    .unwrap();
    assert_eq!(single["warnings"], json!([]));
    assert!(
        single["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["field"] == "/closed" && entry["value"] == false),
        "{single}"
    );
    let batch = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":[{
            "type":"draw","kind":"polyline","points":[[22000,0],[23000,0]]
        }]}}),
    )
    .unwrap();
    assert_eq!(batch["warnings"], json!([]));
    assert!(
        batch["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["field"] == "/commands/0/closed"),
        "{batch}"
    );

    let typed = call(
        &mut session,
        EXECUTE,
        json!({"op":"type","args":{"line":"PLINE 24000,0 25000,0"}}),
    )
    .unwrap();
    assert!(
        typed["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["field"] == "/closed" && entry["value"] == false),
        "{typed}"
    );
}

#[test]
fn bad_command_shapes_have_stable_codes_and_json_pointers() {
    let (_dir, mut session) = sample();
    for (command, code, pointer) in [
        (json!(12), "invalid_value", ""),
        (json!({"kind":"line"}), "missing_field", "/type"),
    ] {
        let error = call(
            &mut session,
            EXECUTE,
            json!({"op":"run", "args":{"command":command}}),
        )
        .unwrap_err()
        .json();
        assert_eq!(error["code"], code, "{error}");
        assert!(
            error["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["pointer"] == pointer),
            "{error}"
        );
    }
}

#[test]
fn validate_and_execute_share_the_batch_size_schema() {
    let (_dir, mut session) = sample();
    let commands = json!([]);
    let validated = call(
        &mut session,
        READ,
        json!({"op":"validate","args":{"commands":commands}}),
    )
    .unwrap();
    assert_eq!(validated["valid"], false);
    assert!(validated["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["pointer"] == "/commands"));
    let error = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":commands}}),
    )
    .unwrap_err()
    .json();
    assert!(error["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["pointer"] == "/commands"));
    let capabilities = call(&mut session, READ, json!({"op":"capabilities"})).unwrap();
    assert_eq!(capabilities["validate"]["commands_schema"]["minItems"], 1);
    assert_eq!(capabilities["validate"]["commands_schema"]["maxItems"], 200);
}

#[test]
fn unknown_reference_fields_are_refused_before_binding() {
    let (_dir, mut session) = sample();
    let commands = json!([
        {"type":"draw","kind":"line","a":[30000,0],"b":[31000,0]},
        {"type":"modify","kind":"move","targets":[{"handle":{"$step":0,"pointer":"/added/0","typo":true}}],"delta":[1000,0]}
    ]);
    let validation = call(
        &mut session,
        READ,
        json!({"op":"validate","args":{"commands":commands}}),
    )
    .unwrap();
    assert_eq!(validation["valid"], false, "{validation}");
    let out = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":commands}}),
    )
    .unwrap_err()
    .json();
    assert!(
        out["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |warning| warning["pointer"] == "/commands/1/targets/0/handle/typo"
                    && warning["code"] == "unknown_field"
            ),
        "{out}"
    );
}

#[test]
fn unknown_edit_fields_are_refused_before_geometry_runs() {
    let (_dir, mut session) = sample();
    let error = call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":{
        "type":"draw","kind":"circle","centre":[0,0],"radius":-1,"typo":true
        }}}),
    )
    .unwrap_err()
    .json();
    assert!(error["code"].is_string(), "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("typo"),
        "{error}"
    );
    assert!(
        error["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["pointer"] == "/typo" && warning["code"] == "unknown_field"),
        "{error}"
    );
}

#[test]
fn failed_batches_report_unknown_fields_and_bound_value_errors_without_mutation() {
    let (_dir, mut session) = sample();
    let before = count(&session);
    let error = call(&mut session, EXECUTE, json!({"op":"batch","args":{"commands":[
        {"type":"draw","kind":"polyline","points":[[30000,0],[31000,0]],"typo":true},
        {"type":"draw","kind":"line","a":{"$step":0,"pointer":"/added/0"},"b":[32000,0],"extra":true}
    ],"batch_typo":true}})).unwrap_err().json();
    assert_eq!(error["code"], "unknown_field", "{error}");
    let errors = error["errors"].as_array().unwrap();
    for pointer in ["/batch_typo", "/commands/0/typo", "/commands/1/extra"] {
        assert!(
            errors.iter().any(|issue| issue["pointer"] == pointer),
            "{error}"
        );
    }
    assert_eq!(count(&session), before);
    let error = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":[
            {"type":"draw","kind":"polyline","points":[[30000,0],[31000,0]]},
            {"type":"draw","kind":"line","a":{"$step":0,"pointer":"/added/0"},"b":[32000,0]}
        ]}}),
    )
    .unwrap_err()
    .json();
    assert_eq!(error["code"], "invalid_value", "{error}");
    assert!(
        error["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["pointer"] == "/commands/1/a"),
        "{error}"
    );
    assert!(error["schema"].is_object());
    assert_eq!(count(&session), before, "the earlier step was rolled back");
}

#[test]
fn batch_defaults_follow_the_context_when_each_step_runs() {
    let (_dir, mut session) = sample();
    let out = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":[
            {"type":"annotation_scale","name":"1:200"},
            {"type":"text","at":[30000,0],"value":"Scaled"}
        ]}}),
    )
    .unwrap();
    let height = out["defaults_used"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["field"] == "/commands/1/height")
        .unwrap();
    assert_eq!(height["value"], session.default_text_height(), "{out}");
}

#[test]
fn surface_errors_show_pointers_expected_forms_and_examples() {
    let (_dir, mut session) = sample();
    for (tool, op, args, pointer, form) in [
        (
            READ,
            "records",
            json!({"detail":true}),
            "/args/detail",
            "summary",
        ),
        (
            CAPTURE,
            "picture",
            json!({"bounds":[-2,-2,22,27]}),
            "/args/bounds",
            "[[x1, y1], [x2, y2]]",
        ),
        (
            EXECUTE,
            "run",
            json!({"commands":[]}),
            "/args/command",
            "For several commands use op batch",
        ),
        (
            READ,
            "query",
            json!({"kind":"near","point":[0]}),
            "/args/point",
            "[x, y]",
        ),
        (
            EXECUTE,
            "type",
            json!({"line":true}),
            "/args/line",
            "string",
        ),
        (
            EXECUTE,
            "new_drawing",
            json!({"units":true}),
            "/args/units",
            "mm",
        ),
        (
            EXECUTE,
            "select",
            json!({"handles":"all"}),
            "/args/handles",
            "array",
        ),
    ] {
        let error = call(&mut session, tool, json!({"op":op,"source":op,"args":args}))
            .unwrap_err()
            .json();
        assert!(
            error["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["pointer"].as_str().unwrap().starts_with(pointer)),
            "{error}"
        );
        assert!(error["message"].as_str().unwrap().contains(form), "{error}");
        assert!(
            error.get("schema").is_some() && error.get("example").is_some(),
            "{error}"
        );
    }
}

#[test]
fn state_reports_local_date_and_layout_sizes_in_paper_units() {
    let (_dir, mut session) = sample();
    let before = chrono::Local::now().format("%Y-%m-%d").to_string();
    let state = call(&mut session, READ, json!({"op":"state"})).unwrap();
    let after = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!([before, after].contains(&state["local_date"].as_str().unwrap().to_string()));
    let layout = session
        .info()
        .sheets
        .iter()
        .find(|sheet| !sheet.is_model)
        .unwrap()
        .block
        .clone();
    session.drawing.units = qiyas_core::Units::Metres;
    session.set_sheet(&layout).unwrap();
    let measured = call(
        &mut session,
        READ,
        json!({"op":"measure","args":{"points":[[0,0],[100,0]]}}),
    )
    .unwrap();
    assert_eq!(measured["length_m"], 0.1);
    let state = call(&mut session, READ, json!({"op":"state"})).unwrap();
    assert_eq!(state["sheet_units"], "paper millimetres");
    assert_eq!(state["text_height_2_5mm"], 2.5);
    let ToolOutput::Image { caption, .. } = surface::run_session(
        &mut session,
        CAPTURE,
        "picture",
        &json!({"bounds":[[0,0],[100,100]],"width":1000}),
        Source::Ai,
    )
    .unwrap() else {
        panic!("picture")
    };
    assert!(caption.contains("in paper millimetres"), "{caption}");
    assert!(
        caption.contains("1 px = 0.1000 paper millimetres"),
        "{caption}"
    );
    assert!(!caption.contains("drawing units"), "{caption}");
}

#[test]
fn collection_schemas_answer_or_explicitly_name_supported_schemas() {
    let (_dir, mut session) = sample();
    for name in [
        "layout",
        "viewport",
        "block",
        "dim_style",
        "linetype",
        "view",
        "takeoff",
        "schedule",
        "boq",
        "conflict",
    ] {
        let schema = call(
            &mut session,
            READ,
            json!({"op":"record_schema","args":{"name":name}}),
        )
        .unwrap();
        assert!(
            !schema["paths"].as_array().unwrap().is_empty(),
            "{name}: {schema}"
        );
    }
    for name in [
        "group",
        "layer_state",
        "xrefs",
        "comments",
        "constraints",
        "parameters",
    ] {
        let error = call(
            &mut session,
            READ,
            json!({"op":"record_schema","args":{"name":name}}),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Supported record schemas:"), "{error}");
        assert!(error.contains("records detail full"), "{error}");
    }
}

#[test]
fn registry_read_shortcuts_retain_unknown_argument_warnings() {
    let (_dir, mut session) = sample();
    for (op, mut args) in [
        ("records", json!({"collection":"layers"})),
        ("check", json!({"rules":["layer_zero"]})),
        (
            "rooms",
            json!({"seeds":[{"name":"Test room","point":[2,2]}]}),
        ),
    ] {
        args["misspelled"] = json!(true);
        let result = call(&mut session, READ, json!({"op":op,"args":args})).unwrap();
        if op == "rooms" {
            assert_eq!(result["rooms"].as_array().unwrap().len(), 1, "{result}");
        }
        assert!(
            result["warnings"].as_array().unwrap().iter().any(|issue| {
                issue["code"] == "unknown_field" && issue["pointer"] == "/args/misspelled"
            }),
            "{op}: {result}"
        );
    }
}

#[test]
fn pdf_document_text_and_picture_preserve_the_open_drawing() {
    if !qiyas_pdf::available() {
        return;
    }
    let (dir, mut session) = sample();
    let pdf = qiyas_pdf::fixtures::write_wall(dir.path());
    let before = count(&session);
    let revision = session.batch_version();
    let history = session.history_steps().0.len();
    let text = call(
        &mut session,
        READ,
        json!({"op":"document","args":{"path":pdf,"pages":[1]}}),
    )
    .unwrap();
    assert_eq!(text["pages"][0]["page"], 1);
    assert!(text["pages"][0]["text"].as_str().unwrap().contains("WALL"));
    assert_eq!(text["pages"][0]["needs_vision"], false);
    let picture = surface::run_session(
        &mut session,
        CAPTURE,
        "document",
        &json!({"path":pdf,"page":1}),
        Source::Ai,
    )
    .unwrap();
    let ToolOutput::Image { png, caption } = picture else {
        panic!("document picture")
    };
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(caption.contains("bottom-left origin"));
    assert_eq!(count(&session), before);
    assert_eq!(session.batch_version(), revision);
    assert_eq!(session.history_steps().0.len(), history);
}

#[test]
fn pdf_only_sheet_captures_and_exposes_the_source_for_vision() {
    if !qiyas_pdf::available() {
        return;
    }
    let cwd = std::env::current_dir().unwrap();
    let dir = tempfile::tempdir_in(&cwd).unwrap();
    let pdf = qiyas_pdf::fixtures::write_wall(dir.path());
    let mut session = Session::new_blank(&dir.path().join("scan.dxf"), 6);
    session
        .attach_pdf(
            &pdf,
            1,
            [0., 0.],
            0.0254 / 72.,
            qiyas_core::AttachMode::Underlay,
        )
        .unwrap();
    let before = session.history_steps();
    let digest = session.drawing_digest(0).unwrap();
    let document = digest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["section"] == "documents")
        .unwrap();
    assert_eq!(document["path"], json!(pdf));
    let ToolOutput::Image { png, caption } =
        surface::run_session(&mut session, CAPTURE, "picture", &json!({}), Source::Ai).unwrap()
    else {
        panic!("picture")
    };
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(caption.contains(pdf.file_name().unwrap().to_str().unwrap()));
    let ToolOutput::Image { caption, .. } = surface::run_session(
        &mut session,
        CAPTURE,
        "document",
        &json!({"path":pdf.file_name().unwrap().to_str().unwrap(),"page":1}),
        Source::Ai,
    )
    .unwrap() else {
        panic!("document")
    };
    assert!(caption.contains("bottom-left origin"));
    assert_eq!(session.history_steps(), before);

    // A relative drawing and relative underlay must still advertise an absolute capture path.
    session.drawing.path = session
        .drawing
        .path
        .strip_prefix(&cwd)
        .unwrap()
        .to_path_buf();
    for object in session.drawing.doc.objects.values_mut() {
        if let opencadcodec::objects::ObjectType::UnderlayDefinition(def) = object {
            def.file_path = pdf.file_name().unwrap().to_str().unwrap().into();
        }
    }
    let digest = session.drawing_digest(0).unwrap();
    let document = digest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["section"] == "documents")
        .unwrap();
    assert_eq!(document["path"], json!(pdf));
    assert!(matches!(
        surface::run_session(
            &mut session,
            CAPTURE,
            "document",
            &document["capture"]["args"],
            Source::Ai
        )
        .unwrap(),
        ToolOutput::Image { .. }
    ));
}

#[test]
fn digest_is_bounded_paged_and_read_only_and_workflow_schemas_are_batched() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("digest.dxf"), 6);
    let commands:Vec<_>=(0..80).map(|i|json!({"type":"text","at":[i,0],"height":0.1,"value":format!("Label {i}: {}","測".repeat(180))})).collect();
    session
        .run_input(&json!({"type":"batch","commands":commands}), Source::User)
        .unwrap();
    let steps = session.history_steps();
    let mut offset = 0;
    let mut seen = 0;
    loop {
        let page = call(
            &mut session,
            READ,
            json!({"op":"digest","args":{"offset":offset}}),
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= qiyas_core::digest::MAX_BYTES);
        assert!(page["picture"].is_object());
        seen += page["entries"].as_array().unwrap().len();
        let Some(next) = page["next_offset"].as_u64() else {
            assert_eq!(seen, page["total_entries"].as_u64().unwrap() as usize);
            break;
        };
        assert!(next > offset);
        offset = next;
    }
    assert!(offset > 0, "Evidence must be paged, not silently dropped");
    assert_eq!(json!(steps), json!(session.history_steps()));
    let schemas=call(&mut session,READ,json!({"op":"commands","args":{"names":["define_object_type","recognition.apply","measurement.set_mappings"]}})).unwrap();
    assert_eq!(schemas["commands"].as_array().unwrap().len(), 3);
    let index = qiyas_core::registry::registry().index_text();
    assert!(
        index.contains("Take-off workflow") && index.contains("3 recognise: recognition.apply")
    );
}

#[test]
fn revision_kit_runs_through_generic_tools_and_typed_commands() {
    let (dir, mut a) = sample();
    let kit = call(&mut a, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    call(&mut a, EXECUTE, json!({"op":"batch","args":kit["batch"]})).unwrap();
    let path = dir.path().join("A.dxf");
    a.save_as(&path).unwrap();
    let mut b = Session::open_standalone(&path).unwrap();
    let mut carry = kit["revision_commands"][1].clone();
    carry["from_path"] = json!(path);
    let n = b.history_steps().0.len();
    call(
        &mut b,
        EXECUTE,
        json!({"op":"run","args":{"command":carry}}),
    )
    .unwrap();
    assert_eq!(b.history_steps().0.len(), n + 1);
    let comparison = call(
        &mut b,
        READ,
        json!({"op":"run","args":{"command":{"type":"compare_revisions","other_path":path}}}),
    )
    .unwrap();
    assert!(comparison["outcome"]["result"]["sheets"].is_array());
    call(
        &mut b,
        EXECUTE,
        json!({"op":"type","args":{"line":"VARIATIONREPORT"}}),
    )
    .unwrap();
    assert!(b
        .variation()
        .unwrap()
        .unwrap()
        .items
        .iter()
        .all(|i| i.delta == 0.));
    assert!(qiyas_core::registry::registry()
        .index_text()
        .contains("revision / open B"));
}

#[test]
fn kit_commands_return_the_complete_short_entry_without_rediscovery() {
    let (_, mut session) = sample();
    let kit = call(&mut session, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    assert_eq!(kit["complete"], true);
    for name in [
        "define_object_type",
        "recognition.apply",
        "measurement.set_mappings",
        "project.run",
    ] {
        let result = call(
            &mut session,
            READ,
            json!({"op":"commands","args":{"name":name}}),
        )
        .unwrap();
        assert_eq!(result["in_takeoff_kit"], true);
        assert!(result.get("schema").is_none());
        assert!(result["entry"]["example"].is_object());
        if !name.starts_with("project.") {
            assert_eq!(result["entry"], kit["commands"][name]);
        }
    }
    let family = call(
        &mut session,
        READ,
        json!({"op":"commands","args":{"name":"measurement"}}),
    )
    .unwrap();
    assert_eq!(family["in_takeoff_kit"], true);
    assert_eq!(
        family["entry"]["commands"]["measurement.set_mappings"],
        kit["commands"]["measurement.set_mappings"]
    );
    assert!(matches!(
        qiyas_ai::surface::route(
            READ,
            &json!({"op":"run","args":{"command":{"type":"project","kind":"list"}}})
        )
        .unwrap(),
        qiyas_ai::surface::Route::Host {
            tool: "project",
            ..
        }
    ));
}

#[test]
fn deliverable_export_preserves_its_kind_and_writes_through_the_registry() {
    let (dir, mut session) = sample();
    for (format, extension) in [("excel", "xlsx"), ("pdf", "pdf")] {
        let name = format!("deliverable.{extension}");
        call(&mut session,EXECUTE,json!({"op":"run","args":{"command":{"type":"deliverable.export","kind":"register","format":format,"path":name,"language":"ar"}}})).unwrap();
        assert!(dir.path().join(name).exists());
    }
    assert!(call(&mut session,EXECUTE,json!({"op":"run","args":{"command":{"type":"deliverable.export","kind":"boq","format":"pdf","path":"../escape.pdf"}}})).is_err());
}

#[test]
fn takeoff_self_check_preserves_unicode_note_unit_dimensions() {
    let mut s = Session::new_blank(std::path::Path::new("unused.dxf"), 6);
    let added = call(
        &mut s,
        EXECUTE,
        json!({"op":"run","args":{"command":{
            "type":"text","at":[0,0],"height":0.1,"value":"10% of 20 m³"
        }}}),
    )
    .unwrap();
    let note = added["added"][0].as_u64().unwrap();
    let citation = json!({"source":"drawing","clause":format!("note {note}")});
    call(&mut s, EXECUTE, json!({"op":"run","args":{"command":{
        "type":"define_object_type","definition":{"name":"Observed","geometry":"region","category":"Test"}
    }}})).unwrap();
    for (unit, kind, warnings) in [("m3", "volume", 0), ("m2", "area", 1)] {
        call(&mut s, EXECUTE, json!({"op":"run","args":{"command":{
            "type":"measurement","kind":"set_method","scope":"project","method":{
                "name":"test","title":"Test","based_on":"drawing","source_note":"Test",
                "rules":[{"id":"scaled","element_type":"Observed","kind":kind,"basis":"rule:sample-note","unit":unit,
                    "terms":[{"label":"Scaled","operation":"add","factors":["geometry.area","0.1"],"citation":citation}],"citation":citation}]
            }
        }}})).unwrap();
        let checked = call(&mut s, READ, json!({"op":"takeoff_check","args":{}})).unwrap();
        assert_eq!(checked["quantity_basis_warnings"], warnings);
    }
}

#[test]
fn explain_is_one_read_with_recognition_rules_and_no_mutation() {
    let (_dir, mut s) = sample();
    let mut kit = call(&mut s, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    kit["batch"]["commands"][1]["rule"]["confidence"] = json!(0.3);
    kit["batch"]["commands"][1]["rule"]["review_below"] = json!(0.0);
    call(&mut s, EXECUTE, json!({"op":"batch","args":kit["batch"]})).unwrap();
    let item = s.measurement_report().unwrap().items[0].code.clone();
    let before = (
        s.document_revision(),
        s.history_steps().0.len(),
        s.sheet.clone(),
    );
    let result = call(
        &mut s,
        READ,
        json!({"op":"explain","args":{"item":format!("generated:{item}")}}),
    )
    .unwrap();
    assert!(!result["recognition_rules"].as_array().unwrap().is_empty());
    assert!(!result["measurement_rules"].as_array().unwrap().is_empty());
    assert!(!result["evidence"].as_array().unwrap().is_empty());
    assert_eq!(result["confidence"], 0.3);
    assert_eq!(
        before,
        (
            s.document_revision(),
            s.history_steps().0.len(),
            s.sheet.clone()
        )
    );
}

#[test]
fn takeoff_check_reports_count_and_global_allowance_errors_until_repaired() {
    let (_dir, mut session) = sample();
    let mut kit = call(&mut session, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    assert!(kit["measurement_rules"]
        .as_str()
        .unwrap()
        .contains("Counts are whole numbers"));
    let commands = kit["batch"]["commands"].as_array_mut().unwrap();
    let method = commands
        .iter_mut()
        .find(|c| c["kind"] == "set_method")
        .unwrap();
    let rules = method["method"]["rules"].as_array_mut().unwrap();
    let mut rule = rules[0].clone();
    rule["kind"] = json!("count");
    rule["unit"] = json!("pcs");
    rule["terms"] = json!([{"label":"Count","operation":"add","factors":["geometry.count","1.1"],
        "citation":{"source":"drawing","clause":"provisional allowance 10%"}}]);
    rules[0] = rule.clone();
    rule["id"] = json!("second");
    rules.push(rule);
    call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":kit["batch"]}),
    )
    .unwrap();
    let checked = call(&mut session, READ, json!({"op":"takeoff_check","args":{}})).unwrap();
    assert!(checked["measurement_errors"].as_u64().unwrap() >= 3);
    assert_eq!(checked["passed"], false);
    let detail = checked.to_string();
    assert!(detail.contains("fractional_count"));
    assert!(detail.contains("allowance applied globally; confirm its scope from the note"));
    let mut method = session
        .measurement_report()
        .unwrap()
        .selection
        .project
        .unwrap();
    for rule in &mut method.rules {
        rule.terms[0].factors.pop();
    }
    call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":{
            "type":"measurement","kind":"set_method","scope":"project","method":method
        }}}),
    )
    .unwrap();
    let checked = call(&mut session, READ, json!({"op":"takeoff_check","args":{}})).unwrap();
    assert_eq!(checked["measurement_errors"], 0);
}

#[test]
fn digest_named_handles_and_scope_checks_share_drawing_evidence() {
    let mut s = Session::new_blank(std::path::Path::new("unused.dxf"), 6);
    let mut handles = Vec::new();
    for y in [0, 10] {
        let added = call(
            &mut s,
            EXECUTE,
            json!({"op":"run","args":{"command":{
                "type":"draw","kind":"line","a":[0,y],"b":[7.75,y]
            }}}),
        )
        .unwrap();
        handles.push(added["added"][0].as_u64().unwrap());
    }
    let (named, other) = (handles[0], handles[1]);
    let note_text = format!("Element {named}; factor 2.4; absent 999999");
    let added = call(
        &mut s,
        EXECUTE,
        json!({"op":"run","args":{"command":{
            "type":"text","at":[0,20],"height":0.1,"value":note_text
        }}}),
    )
    .unwrap();
    let note = added["added"][0].as_u64().unwrap();
    let digest = call(&mut s, READ, json!({"op":"digest","args":{}})).unwrap();
    let entry = digest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["section"] == "text_clusters" && e["handle"] == note)
        .unwrap();
    assert_eq!(entry["named_handles"], json!([named]));
    assert!(!entry["closest"].as_array().unwrap().is_empty());
    let citation = json!({"source":"drawing","clause":format!("note {note}")});
    call(&mut s, EXECUTE, json!({"op":"batch","args":{"commands":[
        {"type":"define_object_type","definition":{"name":"Observed","geometry":"path","category":"Test"}},
        {"type":"recognition","kind":"define","rule":{"name":"observed","object_type":"Observed","match":{"handles":[other]},"confidence":1.0}},
        {"type":"recognition","kind":"apply"},
        {"type":"measurement","kind":"set_method","scope":"project","method":{
            "name":"test","title":"Test","based_on":"drawing","source_note":"test",
            "rules":[{"id":"scaled","element_type":"Observed","kind":"area","unit":"m2",
                "terms":[{"label":"Scaled","operation":"add","factors":["geometry.length","2.4"],"citation":citation}],"citation":citation}]}}
    ]}})).unwrap();
    let checked = call(&mut s, READ, json!({"op":"takeoff_check","args":{}})).unwrap();
    assert_eq!(checked["measurement_errors"], 2);
    assert_eq!(checked["passed"], false);
    assert!(checked.to_string().contains("named_handle_omitted"));
    assert!(checked.to_string().contains("note_factor_scope"));
    let checked = call(
        &mut s,
        READ,
        json!({"op":"takeoff_check","args":{
            "end_report":format!("Exclude {named}: duplicate (note {note})")
        }}),
    )
    .unwrap();
    assert_eq!(checked["measurement_errors"], 1);
    assert!(checked.to_string().contains("note_factor_scope"));
    // Measure the named source too; the rule's elements now intersect its note's scope.
    call(&mut s, EXECUTE, json!({"op":"batch","args":{"commands":[
        {"type":"recognition","kind":"define","rule":{"name":"observed","object_type":"Observed","match":{"handles":[named,other]},"confidence":1.0}},
        {"type":"recognition","kind":"apply"}
    ]}})).unwrap();
    let checked = call(&mut s, READ, json!({"op":"takeoff_check","args":{}})).unwrap();
    assert_eq!(checked["measurement_errors"], 0);
    assert!(s
        .measurement_report()
        .unwrap()
        .quantities
        .iter()
        .any(|q| q.object == named && (q.quantity - 18.6).abs() < 1e-9));
}

#[test]
fn wall_opening_batch_keeps_earlier_step_handles_and_custom_ids() {
    let (_dir, mut session) = sample();
    let result = call(&mut session, EXECUTE, json!({"op":"batch","args":{"commands":[
        {"type":"walls","runs":[{"points":[[30000,0],[36000,0]]},{"points":[[36000,0],[36000,6000]]}],"thickness":200},
        {"type":"add_wall_openings","centreline":{"$step":0,"pointer":"/outcome/centrelines/0"},"openings":[{"at":2000,"width":900,"kind":"door"}]},
        {"type":"add_wall_openings","centreline":{"$step":0,"pointer":"/outcome/centrelines/1"},"openings":[{"at":2000,"width":1200,"kind":"window"}]},
        {"type":"set_object_props","targets":[{"$step":1,"pointer":"/outcome/inserts/0"}],"props":{"mark":"D1"}}
    ]}})).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    let groups = session.wall_groups(None).unwrap();
    let group = groups
        .iter()
        .find(|g| g.runs.iter().any(|r| r.points[0][0] == 30000.0))
        .unwrap();
    assert_eq!(
        group.runs.iter().map(|r| r.openings.len()).sum::<usize>(),
        2
    );
    let door = group
        .runs
        .iter()
        .flat_map(|r| &r.openings)
        .find(|o| matches!(o.opening.kind, qiyas_core::OpeningKind::Door))
        .unwrap()
        .insert;
    let objects = call(
        &mut session,
        READ,
        json!({"op":"records","args":{"collection":"objects"}}),
    )
    .unwrap();
    let door_key = door.to_string();
    assert!(
        objects["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == door_key && r["properties"]["mark"] == "D1"),
        "{objects}"
    );
}

#[test]
fn bound_step_handles_follow_a_real_replacement_after_joining() {
    let (_dir, mut session) = sample();
    let result = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":[
            {"type":"draw","kind":"line","a":[30000,0],"b":[31000,0]},
            {"type":"draw","kind":"line","a":[31000,0],"b":[32000,0]},
            {"type":"edit","kind":"join","targets":[{"handle":{"$step":0}},{"handle":{"$step":1}}]},
            {"type":"set_property","target":{"handle":{"$step":1}},"prop":"color","value":"ACI 1"}
        ]}}),
    )
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    let records = call(&mut session, READ, json!({"op":"records","args":{"collection":"entities","type":"LwPolyline","bounds":[[29999,-1],[32001,1]],"fields":["/common/color"]}})).unwrap();
    assert!(
        records["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["fields"]["/common/color"] == json!({"Index":1})),
        "{records}"
    );
}

#[test]
fn read_shortcuts_accept_the_same_handle_strings_as_commands() {
    let (_dir, mut session) = sample();
    let metre = 1.0 / session.drawing.units.to_metres();
    let added = call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":{
            "type":"draw","kind":"line","a":[30.0*metre,0],"b":[31.0*metre,0]
        }}}),
    )
    .unwrap();
    let handle = added["added"][0].as_u64().unwrap();
    for value in [
        json!(handle),
        json!(handle.to_string()),
        json!(format!("0x{handle:X}")),
    ] {
        let records = call(
            &mut session,
            READ,
            json!({"op":"records","args":{"collection":"entities","handles":[value]}}),
        )
        .unwrap();
        assert_eq!(records["records"][0]["key"], handle);
        let entity = call(
            &mut session,
            READ,
            json!({"op":"query","args":{"kind":"entity","handle":value}}),
        )
        .unwrap();
        assert_eq!(entity["entity"]["handle"], handle);
        let measured = call(
            &mut session,
            READ,
            json!({"op":"measure","args":{"handle":value}}),
        )
        .unwrap();
        assert_eq!(measured["handle"], handle);
        assert_eq!(measured["length_m"], 1.0);
    }
}

#[tokio::test]
async fn new_layout_warnings_are_visible_until_repaired() {
    for repair in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("labels.dxf"), 6);
        session
            .run_input(
                &json!({"type":"text","at":[0,0],"height":0.25,"value":"First label"}),
                Source::User,
            )
            .unwrap();
        let mut calls = vec![(
            EXECUTE,
            json!({"op":"run","args":{"command":{"type":"text","at":[0.02,0],"height":0.25,"value":"Second label"}}}),
        )];
        if repair {
            calls.push((EXECUTE, json!({"op":"undo_own","args":{}})));
        }
        let mut agent = Agent::new(
            Box::new(Script::new(calls, Arc::new(Mutex::new(Vec::new())))),
            Arc::new(Mutex::new(Some(session))),
            AgentSettings {
                autonomy: qiyas_ai::Autonomy::PerJob,
                ..Default::default()
            },
        );
        let events = events_for(&mut agent, "Add a label").await;
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "{events:?}"
        );
        assert_eq!(events.iter().any(|e|matches!(e,AgentEvent::OpenFindings{findings,..} if findings.iter().any(|f|f["rule"]=="text_overlaps_text"))),!repair);
        let serialized = serde_json::to_string(agent.history()).unwrap();
        assert!(
            serialized.contains("text_overlaps_text"),
            "The model must see what needs fixing"
        );
        assert!(serialized.contains("layout_findings"));
    }
}

#[test]
fn named_method_state_digest_and_kit_request_authored_rules_without_bundling_them() {
    let (_dir, mut session) = sample();
    let selected = qiyas_ai::MeasurementMethod {
        name: Some("POMI (RICS)".into()),
        rules_loaded: false,
    };
    for op in ["state", "digest", "takeoff_kit"] {
        let ToolOutput::Json(value) = surface::run_session_with_method(
            &mut session,
            READ,
            op,
            &json!({}),
            Source::Ai,
            &selected,
        )
        .unwrap() else {
            panic!("JSON")
        };
        let method = &value["measurement_method"];
        // A saved "POMI (RICS)" is told to the model, and shown, as "POMI".
        assert_eq!(method["name"], "POMI");
        assert_eq!(method["rules_loaded"], false);
        if op != "digest" {
            assert!(method["instruction"]
                .as_str()
                .unwrap()
                .contains("Write rules from your knowledge"));
            assert!(method["instruction"]
                .as_str()
                .unwrap()
                .contains("<standard> clause <n>"));
        }
        if op == "takeoff_kit" {
            let bytes = serde_json::to_vec(&value).unwrap().len();
            assert!(bytes <= qiyas_core::takeoff_kit::MAX_BYTES, "{bytes}");
            let command = value["batch"]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["kind"] == "set_method")
                .unwrap();
            assert_eq!(command["method"]["name"], "POMI");
            assert_eq!(command["method"]["rules"], json!([]));
            assert_eq!(command["scope"], "settings");
        }
    }
}

#[tokio::test]
async fn scripted_named_takeoff_writes_cited_rules_and_retains_method_on_every_quantity() {
    let (_dir, mut session) = sample();
    // The saved name is the old one; everything shown and exported is bare.
    session
        .apply_settings_method(Some(&qiyas_core::measurement::RuleSet::named(
            "POMI (RICS)",
        )))
        .unwrap();
    let name = "POMI";
    fn author_from_kit(request: &TurnRequest) -> (&'static str, Value) {
        let kit = request
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .filter_map(|block| {
                if let ContentBlock::ToolResult {
                    content, is_error, ..
                } = block
                {
                    assert!(!is_error);
                    content.iter().find_map(|part| match part {
                        ResultPart::Text(text) => serde_json::from_str::<Value>(text)
                            .ok()
                            .filter(|v| v["batch"]["commands"].is_array()),
                        _ => None,
                    })
                } else {
                    None
                }
            })
            .next()
            .expect("kit from conversation");
        assert!(kit["measurement_method"]["instruction"]
            .as_str()
            .unwrap()
            .contains("Write rules from your knowledge"));
        let mut batch = kit["batch"].clone();
        // This exercises transport with invented geometry and a visibly fictitious reference.
        // It neither asserts nor supplies any rule of the named standard.
        let name = kit["measurement_method"]["name"].as_str().unwrap();
        let citation = json!({"source":name,"clause":format!("{name} clause TEST-ONLY (not a standard clause)")});
        for command in batch["commands"].as_array_mut().unwrap() {
            if command["kind"] == "set_method" {
                let mut rule = kit["commands"]["measurement.set_method"]["example"]["method"]
                    ["rules"][0]
                    .clone();
                rule["citation"] = citation.clone();
                rule["terms"][0]["citation"] = citation.clone();
                command["method"]["rules"] = json!([rule]);
                command["method"]["source_note"] =
                    json!("Synthetic transport test only; not standard guidance");
            } else if command["kind"] == "set_mappings" {
                command["mappings"][0]["citation"] = citation.clone();
            }
        }
        (EXECUTE, json!({"op":"batch","args":batch}))
    }
    let citation =
        json!({"source":name,"clause":format!("{name} clause TEST-ONLY (not a standard clause)")});
    let requests = Arc::new(Mutex::new(Vec::new()));
    let session = Arc::new(Mutex::new(Some(session)));
    let mut agent = Agent::new(
        Box::new(Script::steps(
            vec![
                (READ, json!({"op":"state","args":{}})).into(),
                (READ, json!({"op":"takeoff_kit","args":{"detail":"full"}})).into(),
                scripted::ScriptCall::FromHistory(author_from_kit),
                (
                    READ,
                    json!({"op":"run","args":{"command":{"type":"measurement","kind":"report"}}}),
                )
                    .into(),
                (READ, json!({"op":"takeoff_check","args":{}})).into(),
            ],
            requests.clone(),
        )),
        session.clone(),
        AgentSettings::default(),
    );
    let events = events_for(
        &mut agent,
        "Take off this drawing using the selected method",
    )
    .await;
    assert!(
        matches!(events.last(), Some(AgentEvent::Done { .. })),
        "{events:?}"
    );
    let requests = requests.lock().unwrap();
    assert!(format!("{:?}", requests[0].messages)
        .contains("Measurement method: POMI; rules loaded: false"));
    assert!(format!("{:?}", requests[0].messages).contains("Write rules from your knowledge"));
    let mut guard = session.lock().unwrap();
    let session = guard.as_mut().unwrap();
    let report = serde_json::to_value(session.measurement_report().unwrap()).unwrap();
    assert!(!report["items"].as_array().unwrap().is_empty());
    for quantity in report["quantities"].as_array().unwrap() {
        assert_eq!(quantity["method"], name);
        assert_eq!(quantity["citation"], citation);
    }
    for item in report["items"].as_array().unwrap() {
        assert_eq!(item["mapping_citations"], json!([citation]));
        assert!(!item["lines"].as_array().unwrap().is_empty());
        for line in item["lines"].as_array().unwrap() {
            assert_eq!(line["method"], name);
            assert!(line["citations"].as_array().unwrap().contains(&citation));
        }
    }
    let before = session.measurement_selection().unwrap();
    let kit = call(session, READ, json!({"op":"takeoff_kit","args":{}})).unwrap();
    assert_eq!(kit["measurement_method"]["rules_loaded"], true);
    assert!(!kit["batch"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["kind"] == "set_method"));
    session
        .apply_settings_method(Some(&qiyas_core::measurement::RuleSet::named(name)))
        .unwrap();
    assert_eq!(session.measurement_selection().unwrap(), before);
}

#[test]
fn project_method_overrides_the_settings_name_in_state_and_kit() {
    let (_dir, mut session) = sample();
    call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":{
            "type":"measurement","kind":"set_method","scope":"project",
            "method":qiyas_core::measurement::RuleSet::named("Client override")
        }}}),
    )
    .unwrap();
    let selected = qiyas_ai::MeasurementMethod {
        name: Some("POMI (RICS)".into()),
        rules_loaded: false,
    };
    for op in ["state", "takeoff_kit"] {
        let ToolOutput::Json(value) = surface::run_session_with_method(
            &mut session,
            READ,
            op,
            &json!({}),
            Source::Ai,
            &selected,
        )
        .unwrap() else {
            panic!("JSON")
        };
        assert_eq!(value["measurement_method"]["name"], "Client override");
        if op == "takeoff_kit" {
            assert_eq!(value["measurement_method"]["scope"], "project");
            let command = value["batch"]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["kind"] == "set_method")
                .unwrap();
            assert_eq!(command["scope"], "project");
            assert_eq!(command["method"]["name"], "Client override");
        }
    }
}

#[tokio::test]
async fn all_modes_allow_completion_and_attach_open_findings() {
    use qiyas_ai::Autonomy;
    let line = json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[100,100],"b":[110,100]}}});
    for mode in [Autonomy::PerJob, Autonomy::PerPhase, Autonomy::Autonomous] {
        for repair in [false, true] {
            let (_dir, mut session) = sample();
            session.current.layer = "A-WALL".into();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let mut calls = vec![(EXECUTE, line.clone()), (EXECUTE, line.clone())];
            if repair {
                calls.push((EXECUTE, json!({"op":"undo_own","args":{}})));
            }
            let mut agent = Agent::new(
                Box::new(Script::new(calls, requests)),
                Arc::new(Mutex::new(Some(session))),
                AgentSettings {
                    autonomy: mode,
                    ..Default::default()
                },
            );
            let events = events_for(&mut agent, "Draw").await;
            assert!(
                matches!(events.last(), Some(AgentEvent::Done { .. })),
                "{mode:?}, repair={repair}: {events:?}"
            );
            assert_eq!(
                events.iter().any(
                    |e| matches!(e,AgentEvent::OpenFindings{findings,..} if !findings.is_empty())
                ),
                !repair
            );
        }
        let (_dir, mut session) = sample();
        session.current.layer = "A-WALL".into();
        call(&mut session, EXECUTE, line.clone()).unwrap();
        call(&mut session, EXECUTE, line.clone()).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let elsewhere = json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[200,200],"b":[210,200]}}});
        let mut agent = Agent::new(
            Box::new(Script::new(vec![(EXECUTE, elsewhere)], requests)),
            Arc::new(Mutex::new(Some(session))),
            AgentSettings {
                autonomy: mode,
                ..Default::default()
            },
        );
        let events = events_for(&mut agent, "Draw elsewhere").await;
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "pre-existing errors must not be attributed to Mizan: {events:?}"
        );
    }
}

#[tokio::test]
async fn identical_ineffective_edits_advise_then_stop_only_the_sixth_attempt() {
    for attempts in [3, 6] {
        let (_dir, mut session) = sample();
        let input = json!({"op":"run","args":{"command":{"type":"new_layer","name":"Repeated"}}});
        call(&mut session, EXECUTE, input.clone()).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut calls = vec![(EXECUTE, input.clone())];
        calls.push((READ, json!({"op":"state"})));
        calls.extend((1..attempts).map(|_| (EXECUTE, input.clone())));
        let mut agent = Agent::new(
            Box::new(Script::new(calls, requests.clone())),
            Arc::new(Mutex::new(Some(session))),
            AgentSettings::default(),
        );
        let events = events_for(&mut agent, "Create the requested layer").await;
        let results = results(&agent);
        for n in [2, 3] {
            assert!(
                results.iter().any(|(_, v)| v["advice"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|a| a["attempts"] == n))),
                "{results:?}"
            );
        }
        if attempts == 6 {
            assert!(
                matches!(events.last(),Some(AgentEvent::Stopped{reason}) if reason=="repeated_ineffective_edit")
            );
            assert!(events
                .iter()
                .any(|e| matches!(e,AgentEvent::Text{text} if text.contains("five times"))));
            assert_eq!(requests.lock().unwrap().len(), 7);
        } else {
            assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
        }
    }
}

#[tokio::test]
async fn goal_d_budget_and_workflow_follow_the_job_and_chat_language() {
    for (request, takeoff, arabic) in [
        ("Draw a mounting plate in this drawing", false, false),
        ("Take off the quantities", true, false),
        ("احصر كميات المخطط", true, true),
    ] {
        let (_dir, session) = sample();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut calls = vec![(READ, json!({"op":"commands","args":{"name":"draw.line"}}))];
        calls.extend((0..33).map(|_| (READ, json!({"op":"state","args":{}}))));
        let mut agent = Agent::new(
            Box::new(Script::new(calls, requests.clone())),
            Arc::new(Mutex::new(Some(session))),
            AgentSettings::default(),
        );
        let events = events_for(&mut agent, request).await;
        assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
        {
            let reqs = requests.lock().unwrap();
            if let Some(root) = std::env::var_os("QIYAS_GOAL_D_EVIDENCE") {
                let root = std::path::PathBuf::from(root);
                std::fs::create_dir_all(&root).unwrap();
                let label = if arabic {
                    "takeoff-ar"
                } else if takeoff {
                    "takeoff-en"
                } else {
                    "drafting-en"
                };
                std::fs::write(
                    root.join(format!("{label}-requests.json")),
                    serde_json::to_vec_pretty(
                        &reqs
                            .iter()
                            .map(|r| json!({"system":r.system,"messages":r.messages}))
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                )
                .unwrap();
                std::fs::write(
                    root.join(format!("{label}-events.json")),
                    serde_json::to_vec_pretty(&events).unwrap(),
                )
                .unwrap();
            }
            let last = reqs.last().unwrap();
            assert!(!last.system.contains("Take-off workflow"));
            let texts = last
                .messages
                .iter()
                .flat_map(|m| &m.content)
                .filter_map(|b| match b {
                    qiyas_ai::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                texts.contains("Take-off workflow:") || texts.contains("سير عمل الحصر:"),
                takeoff
            );
            // The note is given once in the turn, as a message of its own, and says to go on.
            let notices: Vec<_> = last
                .messages
                .iter()
                .flat_map(|m| &m.content)
                .filter_map(|b| match b {
                    qiyas_ai::ContentBlock::Text { text }
                        if text.starts_with("[efficiency note]") =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(notices.len(), 1);
            let notice = notices[0];
            assert_eq!(
                notice.contains("quantity work") || notice.contains("عمل الكميات"),
                takeoff
            );
            assert_eq!(notice.contains("استُخدمت"), arabic);
            assert!(notice.contains("not a reason to stop") || notice.contains("ليس سببًا للتوقف"));
            // The original schema remains, after far more than three results.
            let contract = last.messages.iter().flat_map(|m| &m.content).find(|b| matches!(b,qiyas_ai::ContentBlock::ToolResult{tool_use_id,..} if tool_use_id == "call_0")).unwrap();
            assert!(serde_json::to_string(contract).unwrap().contains("schema"));
            assert!(!serde_json::to_string(contract)
                .unwrap()
                .contains("tool result summary"));
        }
        if takeoff {
            events_for(&mut agent, "Draw a bracket in this drawing, drawing only.").await;
            let reqs = requests.lock().unwrap();
            let latest = reqs.last().unwrap();
            let previous = &reqs[reqs.len() - 2];
            assert_eq!(
                &latest.messages[..previous.messages.len()],
                previous.messages.as_slice(),
                "earlier guidance stays byte-stable until a token trim"
            );
            assert!(
                !latest.messages[previous.messages.len()..]
                    .iter()
                    .flat_map(|m| &m.content)
                    .any(|b| matches!(b, qiyas_ai::ContentBlock::Text { text }
                    if text == qiyas_ai::prompt::takeoff_workflow(qiyas_ai::prompt::Lang::En)
                        || text == qiyas_ai::prompt::takeoff_workflow(qiyas_ai::prompt::Lang::Ar))),
                "a drawing-only turn does not append take-off guidance"
            );
        }
    }
}

#[test]
fn goal_d_redo_restores_the_original_handles_and_respects_ownership() {
    let (_dir, mut session) = sample();
    let result = call(&mut session, EXECUTE,json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[100,100],"b":[110,100]}}})).unwrap();
    let handle = result["added"][0].as_u64().unwrap();
    assert!(session.entity_info(handle, &[]).is_ok());
    assert_eq!(
        call(&mut session, EXECUTE, json!({"op":"undo_own","args":{}})).unwrap()["undone"],
        true
    );
    assert!(!session.entity_info(handle, &[]).is_ok());
    assert_eq!(
        call(
            &mut session,
            EXECUTE,
            json!({"op":"type","args":{"line":"REDO"}})
        )
        .unwrap()["redone"],
        true
    );
    assert!(session.entity_info(handle, &[]).is_ok());
    session
        .run(qiyas_core::Command::NewLayer {
            name: "User layer".into(),
        })
        .unwrap();
    session.undo();
    assert_eq!(
        call(&mut session, EXECUTE, json!({"op":"redo_own","args":{}})).unwrap()["redone"],
        false
    );
    assert!(session.can_redo());
    session.undo();
    assert_eq!(
        call(&mut session, EXECUTE, json!({"op":"redo_own","args":{}})).unwrap()["redone"],
        true
    );
    assert!(session.entity_info(handle, &[]).is_ok());
    assert_eq!(
        call(&mut session, EXECUTE, json!({"op":"redo_own","args":{}})).unwrap()["redone"],
        false
    );
}

#[tokio::test]
async fn new_warnings_are_attached_to_completion_in_all_drawing_units() {
    for (unit, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::new_blank(&dir.path().join("warning.dxf"), unit);
        // The warning remains visible; Mizan decides whether the work is complete.
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = Agent::new(
            Box::new(Script::new(
                vec![(
                    EXECUTE,
                    json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[5.0/factor,0]}}}),
                )],
                requests.clone(),
            )),
            Arc::new(Mutex::new(Some(session))),
            AgentSettings::default(),
        );
        let events = events_for(&mut agent, "Draw a line").await;
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "{unit}: {events:?}"
        );
        assert!(events.iter().any(|e|matches!(e,AgentEvent::OpenFindings{findings,revision} if !revision.is_null() && findings.iter().any(|f|f["rule"]=="layer_zero"))));
        assert_eq!(
            requests.lock().unwrap().len(),
            2,
            "one edit and the model's final answer"
        );
    }
}

#[test]
fn goal_d_workflow_is_optional_localised_and_separate_from_the_bounded_kit() {
    for (language, heading) in [("en", "Take-off workflow:"), ("ar", "سير عمل الحصر:")] {
        let ToolOutput::Json(workflow) =
            surface::run_without_drawing(READ, "workflow", &json!({"language":language}), None)
                .unwrap()
                .unwrap()
        else {
            panic!()
        };
        assert!(workflow["workflow"].as_str().unwrap().contains(heading));
        let ToolOutput::Json(kit) =
            surface::run_without_drawing(READ, "takeoff_kit", &json!({}), None)
                .unwrap()
                .unwrap()
        else {
            panic!()
        };
        assert!(kit["batch"]["commands"].is_array());
        assert!(kit.get("workflow").is_none());
        assert!(serde_json::to_vec(&kit).unwrap().len() <= qiyas_core::takeoff_kit::MAX_BYTES);
    }
}

#[tokio::test]
async fn goal_d_switching_jobs_preserves_requests_and_evidence() {
    let (_dir, session) = sample();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut agent = Agent::new(
        Box::new(Script::new(
            vec![(READ, json!({"op":"state"}))],
            requests.clone(),
        )),
        Arc::new(Mutex::new(Some(session))),
        AgentSettings::default(),
    );
    events_for(&mut agent, "Take off the quantities").await;
    events_for(&mut agent, "Draw a bracket in this drawing, drawing only.").await;
    let requests = requests.lock().unwrap();
    let evidence = |messages: &[qiyas_ai::Message]| {
        let mut messages = messages.to_vec();
        for message in &mut messages {
            message.content.retain(|b| {
                !matches!(b, qiyas_ai::ContentBlock::Text { text }
                if text == qiyas_ai::prompt::takeoff_workflow(qiyas_ai::prompt::Lang::En)
                    || text == qiyas_ai::prompt::takeoff_workflow(qiyas_ai::prompt::Lang::Ar))
            });
        }
        serde_json::to_value(messages).unwrap()
    };
    let before = evidence(&requests[1].messages);
    let after = evidence(&requests[2].messages);
    assert_eq!(
        before.as_array().unwrap(),
        &after.as_array().unwrap()[..requests[1].messages.len()]
    );
    assert_eq!(requests[0].system, requests[2].system);
}

#[tokio::test]
async fn goal_d_interrupted_quantity_work_reports_in_the_chat_language() {
    let (_dir, session) = sample();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let reads = (0..6)
        .map(|_| (EXECUTE, json!({"op":"run","args":{"command":{"type":"set_property","target":{"handle":999999},"prop":"color","value":"red"}}})))
        .collect();
    let mut agent = Agent::new(
        Box::new(Script::new(reads, requests)),
        Arc::new(Mutex::new(Some(session))),
        AgentSettings::default(),
    );
    let events = events_for(&mut agent, "احصر كميات المخطط").await;
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Text { text } if text.contains("ملخص العمل المتوقف:"))));
    assert!(!events.iter().any(|e| matches!(e, AgentEvent::Text { text } if text.contains("End report:") || text.contains("generated BOQ items"))));
}

#[test]
fn goal_c_operation_discovery_and_scoped_text_queries() {
    for unit in [4, 5, 6, 1] {
        let mut s = Session::new_blank(std::path::Path::new("text-query.dxf"), unit);
        let k = 1.0 / s.drawing.units.to_metres();
        for (layer, point, text) in [
            ("Notes", [k, k], "Inside"),
            ("Notes", [5.0 * k, k], "Outside"),
            ("Other", [k, k], "Other layer"),
        ] {
            let v=call(&mut s,EXECUTE,json!({"op":"run","args":{"layer":layer,"command":{"type":"text","at":point,"value":text}}})).unwrap();
            assert!(v["ok"].as_bool().unwrap(), "{v}");
        }
        for bounds in ["bounds", "within"] {
            let mut args = json!({"kind":"texts","layer":"notes"});
            args[bounds] = json!([[0, 0], [2.0 * k, 2.0 * k]]);
            let v = call(&mut s, READ, json!({"op":"query","args":args})).unwrap();
            assert!(v.to_string().contains("Inside"), "{v}");
            assert!(
                !v.to_string().contains("Outside") && !v.to_string().contains("Other layer"),
                "{v}"
            );
        }
        assert_eq!(s.current.layer, "0");
        for name in [
            "query",
            "finish",
            "qiyas_execute.run",
            "qiyas_read.query",
            "qiyas_capture.picture",
        ] {
            let v = call(&mut s, READ, json!({"op":"commands","args":{"name":name}})).unwrap();
            assert!(v["schema"].is_object(), "{name}: {v}");
            assert!(v["example"].is_object(), "{name}: {v}");
        }
    }
}

#[test]
fn u11_read_check_refusal_points_to_drawing_check_and_the_comparison_name() {
    let (_dir, mut s) = sample();
    let error = call(
        &mut s,
        READ,
        json!({"op":"run","args":{"command":{"type":"check"}}}),
    )
    .unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("qiyas_read")
            && text.contains("op check")
            && text.contains("discrepancy_check"),
        "{text}"
    );
}

#[test]
fn u15_surface_batch_targets_another_sheet_and_rolls_back_without_switching_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("sheets.dxf"), 6);
    let made = call(
        &mut s,
        EXECUTE,
        json!({"op":"run","args":{"command":{"type":"text","value":"Before","at":[0,0]}}}),
    )
    .unwrap();
    let handle = made["added"][0].as_u64().unwrap();
    call(&mut s, EXECUTE, json!({"op":"run","args":{"command":{"type":"new_layout","name":"Issued","paper":"ISO A3"}}})).unwrap();
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.name == "Issued")
        .unwrap()
        .block;
    s.set_sheet(&block).unwrap();
    let command = json!({"type":"edit_text","target":{"handle":handle},"value":"After"});
    call(
        &mut s,
        EXECUTE,
        json!({"op":"batch","args":{"sheet":"Model","commands":[command.clone()]}}),
    )
    .unwrap();
    assert_eq!(s.sheet, block);
    assert_eq!(
        s.entity_info(handle, &[]).unwrap().text.as_deref(),
        Some("After")
    );
    assert!(s.undo());
    assert_eq!(s.sheet, block);
    let before = s.history_steps();
    assert!(call(&mut s, EXECUTE, json!({"op":"batch","args":{"sheet":"Model","commands":[command,{"type":"edit_text","target":{"handle":999999999},"value":"Missing"}]}})).is_err());
    assert_eq!(s.sheet, block);
    assert_eq!(s.history_steps(), before);
    assert_eq!(
        s.entity_info(handle, &[]).unwrap().text.as_deref(),
        Some("Before")
    );
}

#[test]
fn u5_change_reports_all_label_collisions_and_u15_checks_the_edited_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("labels.dxf"), 6);
    let commands: Vec<_> = (0..12)
        .map(|_| json!({"type":"text","value":"Room name","at":[0,0],"height":0.2}))
        .collect();
    let made = call(
        &mut s,
        EXECUTE,
        json!({"op":"batch","args":{"commands":commands}}),
    )
    .unwrap();
    assert!(
        made["check"]["counts"]["text_overlaps_text"]
            .as_u64()
            .unwrap_or(0)
            >= 48,
        "{made}"
    );
    let handle = made["added"][0].as_u64().unwrap();
    call(&mut s, EXECUTE, json!({"op":"run","args":{"command":{"type":"new_layout","name":"Issued","paper":"ISO A3"}}})).unwrap();
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sheet| sheet.name == "Issued")
        .unwrap()
        .block;
    s.set_sheet(&block).unwrap();
    let edited = call(&mut s, EXECUTE, json!({"op":"run","args":{"command":{"type":"edit_text","target":{"handle":handle},"value":"Changed room","sheet":"Model"}}})).unwrap();
    assert_eq!(s.sheet, block);
    assert_eq!(edited["check"]["sheets"], json!(["*Model_Space"]));
    assert!(
        edited["check"]["counts"]["text_overlaps_text"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "{edited}"
    );
    assert_ne!(
        edited["ui"]["action"], "reveal",
        "An edit elsewhere must not reveal on the visible sheet"
    );
}

#[test]
fn plot_writes_the_sheet_as_a_pdf_beside_the_drawing() {
    let (dir, mut s) = sample();
    let out = call(&mut s, EXECUTE, json!({"op":"plot","args":{}})).unwrap();
    let written = std::path::PathBuf::from(out["written"].as_str().unwrap());
    assert_eq!(
        written.parent().unwrap().canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );
    assert_eq!(written.extension().unwrap(), "pdf");
    let bytes = std::fs::read(&written).unwrap();
    assert!(bytes.starts_with(b"%PDF"), "{written:?}");
    assert_eq!(out["sheet"], "Model");
    let named = call(
        &mut s,
        EXECUTE,
        json!({"op":"plot","args":{"sheet":"Model","paper":"ISO A3","scale":"1:100","path":"plans/model.pdf"}}),
    )
    .unwrap();
    assert!(dir.path().join("plans/model.pdf").exists(), "{named}");
    let error = call(
        &mut s,
        EXECUTE,
        json!({"op":"plot","args":{"sheet":"A-999"}}),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("No sheet A-999") && error.contains("Model"),
        "{error}"
    );
    let error = call(&mut s, EXECUTE, json!({"op":"plot","args":{"scale":"big"}}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("1:50"), "{error}");
}
