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
        assert_eq!(
            answer["index"],
            qiyas_core::registry::registry().index_text()
        );
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
    let index = registry.index_text();
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
async fn reads_stop_at_40_calls_and_results_warn_with_ten_left() {
    let (_dir, s) = sample();
    let calls = vec![(READ, json!({"op": "state", "args": "{}"})); 50];
    let (agent, events, requests) = scripted(Arc::new(Mutex::new(Some(s))), calls).await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 40);
    assert_eq!(requests[0].tools.len(), 3);
    for (offered, expected) in requests[0].tools.iter().zip(surface::tools()) {
        assert_eq!(offered.name, expected.name);
        assert_eq!(offered.input_schema, expected.input_schema);
    }
    let results = observed_results(&agent, &requests);
    assert_eq!(results.len(), 40);
    for (i, (error, value)) in results.iter().enumerate() {
        assert!(!error, "{value}");
        if i < 29 {
            assert!(value.get("budget").is_none(), "{value}");
        } else {
            assert_eq!(value["budget"], format!("{} calls left", 39 - i));
        }
    }
    assert_eq!(
        events.last(),
        Some(&AgentEvent::Stopped {
            reason: "stopped after 40 steps".into()
        })
    );
}

#[tokio::test]
async fn a_drawing_change_raises_the_turn_budget_to_120() {
    let (_dir, s) = sample();
    let mut calls = vec![(
        EXECUTE,
        json!({"op": "run", "args": {"command": {
            "type": "draw", "kind": "line", "a": [30, 0], "b": [35, 0]
        }}}),
    )];
    calls.extend(vec![(READ, json!({"op": "state", "args": "{}"})); 130]);
    let (agent, events, requests) = scripted(Arc::new(Mutex::new(Some(s))), calls).await;
    assert_eq!(requests.lock().unwrap().len(), 120);
    let results = observed_results(&agent, &requests.lock().unwrap());
    assert_eq!(results.len(), 120);
    for (i, (error, value)) in results.iter().enumerate() {
        assert!(!error, "{value}");
        if i < 109 {
            assert!(value.get("budget").is_none(), "{value}");
        } else {
            assert_eq!(value["budget"], format!("{} calls left", 119 - i));
        }
    }
    assert!(events.iter().any(|event| matches!(
        event, AgentEvent::ToolCall { name, label: Some(label), mutating: true, .. }
            if name == EXECUTE && label == "draw.line"
    )));
    assert_eq!(
        events.last(),
        Some(&AgentEvent::Stopped {
            reason: "stopped after 120 steps".into()
        })
    );
}

#[tokio::test]
async fn destructive_commands_in_run_and_batch_share_one_cap() {
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
    let remove = |id: &str| json!({"type": "remove_measurement", "id": id});
    let mut calls: Vec<_> = ids[..6]
        .iter()
        .map(|id| {
            (
                EXECUTE,
                json!({"op": "run", "args": {"command": remove(id)}}),
            )
        })
        .collect();
    let over = json!({"op": "batch", "args": {
        "commands": ids[6..11].iter().map(|id| remove(id)).collect::<Vec<_>>()
    }});
    assert_eq!(surface::destructive_count(EXECUTE, &over), 5);
    calls.push((EXECUTE, over));
    calls.push((
        EXECUTE,
        json!({"op": "batch", "args": {
            "commands": ids[6..10].iter().map(|id| remove(id)).collect::<Vec<_>>()
        }}),
    ));
    calls.push((
        EXECUTE,
        json!({"op": "run", "args": {"command": remove(&ids[10])}}),
    ));
    let session = Arc::new(Mutex::new(Some(s)));
    let (agent, _, _) = scripted(session.clone(), calls).await;
    let results = results(&agent);
    assert_eq!(results.len(), 9);
    assert!(results[..6].iter().all(|(error, _)| !error));
    for i in [6, 8] {
        assert!(results[i].0, "{:?}", results[i]);
        assert!(
            results[i]
                .1
                .to_string()
                .contains("Too many destructive commands"),
            "{:?}",
            results[i]
        );
    }
    assert!(!results[7].0, "{:?}", results[7]);
    let guard = session.lock().unwrap();
    let remaining = &guard.as_ref().unwrap().takeoff.items;
    assert_eq!(remaining.len(), 2);
    assert_eq!(
        remaining.iter().map(|row| &row.id).collect::<Vec<_>>(),
        ids[10..].iter().collect::<Vec<_>>()
    );
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
                        json!({"phases":[{"id":"sizes","title":"Check sizes","status":"active"}]})
                    }
                    "record_schema" => json!({"name":"line"}),
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
    assert_eq!(out, json!({"plan":{"phases":phases}}));
    assert_eq!(count(&session), before);
    assert_eq!(session.history_revision(), revision);
    assert!(!session.can_undo());
    let info = call(
        &mut session,
        READ,
        json!({"op":"commands","args":{"name":"plan"}}),
    )
    .unwrap();
    assert_eq!(info["op"], "plan");
    assert_eq!(info["undo"], false);
    let caps = call(&mut session, READ, json!({"op":"capabilities"})).unwrap();
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
    let pending = json!({"phases":[{"id":"sizes","title":"Check sizes","status":"pending"}]});
    let active = json!({"phases":[{"id":"sizes","title":"Check sizes","status":"active"}]});
    let done = json!({"phases":[{"id":"sizes","title":"Check sizes","status":"done"}]});
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
    let (tx, mut rx) = mpsc::channel(100);
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
            assert_eq!(requests.lock().unwrap().len(), 3);
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
                assert_eq!(requests.lock().unwrap().len(), 3);
            }
        }
    }
}

#[tokio::test]
async fn selected_modes_pause_and_autonomous_reports_unfixed_errors_without_blocking() {
    use qiyas_ai::Autonomy;
    let line = json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[100,100],"b":[110,100]}}});
    for mode in [Autonomy::PerJob, Autonomy::PerPhase, Autonomy::Autonomous] {
        for repair in [false, true] {
            let (_dir, session) = sample();
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
            assert_eq!(
                matches!(events.last(), Some(AgentEvent::AwaitingApproval { .. })),
                !repair && mode != Autonomy::Autonomous,
                "{mode:?}, repair={repair}: {events:?}"
            );
            if mode == Autonomy::Autonomous && !repair {
                assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
                assert!(events.iter().any(|e|matches!(e,AgentEvent::Text{text} if text.contains("Unresolved drawing checks"))));
            }
        }
        let (_dir, mut session) = sample();
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
            json!({"name":"office-method","rules_loaded":loaded})
        );
    }
}

#[tokio::test]
async fn unfinished_phase_error_attribution_survives_restart_until_repaired() {
    let (_dir, session) = sample();
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
    assert!(matches!(
        events_for(&mut agent, "Draw").await.last(),
        Some(AgentEvent::AwaitingApproval { .. })
    ));
    let saved = serde_json::to_vec(&agent.snapshot()).unwrap();
    let conversation = serde_json::from_slice(&saved).unwrap();
    let mut resumed = Agent::resume(
        conversation,
        Box::new(Script::new(vec![], Default::default())),
        shared.clone(),
        settings.clone(),
    );
    assert!(
        matches!(
            events_for(&mut resumed, "Approved, continue.").await.last(),
            Some(AgentEvent::AwaitingApproval { .. })
        ),
        "approval alone does not erase errors from the unfinished phase"
    );
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
        out["warnings"]
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
    assert_eq!(error["warnings"][0]["code"], "unknown_field");
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
fn successful_inputs_expose_ignored_fields_and_defaults_in_single_and_batch_results() {
    let (_dir, mut session) = sample();
    let command =
        json!({"type":"draw","kind":"polyline","points":[[20000,0],[21000,0]],"typo":true});
    let single = call(
        &mut session,
        EXECUTE,
        json!({"op":"run","args":{"command":command}}),
    )
    .unwrap();
    assert!(single["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["pointer"] == "/typo"));
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
            "type":"draw","kind":"polyline","points":[[22000,0],[23000,0]],"typo":true
        }]}}),
    )
    .unwrap();
    assert!(batch["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["pointer"] == "/commands/0/typo"));
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
fn unknown_reference_fields_are_reported_as_warnings_after_binding() {
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
    assert_eq!(validation["valid"], true, "{validation}");
    let out = call(
        &mut session,
        EXECUTE,
        json!({"op":"batch","args":{"commands":commands}}),
    )
    .unwrap();
    assert!(
        out["warnings"]
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
fn runtime_refusals_keep_input_warnings() {
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
        error["message"].as_str().unwrap().contains("radius"),
        "{error}"
    );
    assert!(
        error["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["pointer"] == "/typo" && warning["code"] == "unknown_field"),
        "{error}"
    );
}

#[test]
fn failed_batches_keep_warnings_and_the_bound_fields_error_path() {
    let (_dir, mut session) = sample();
    let before = count(&session);
    let error = call(&mut session, EXECUTE, json!({"op":"batch","args":{"commands":[
        {"type":"draw","kind":"polyline","points":[[30000,0],[31000,0]],"typo":true},
        {"type":"draw","kind":"line","a":{"$step":0,"pointer":"/added/0"},"b":[32000,0],"extra":true}
    ],"batch_typo":true}})).unwrap_err().json();
    assert_eq!(error["code"], "invalid_value", "{error}");
    assert!(
        error["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["pointer"] == "/commands/1/a"),
        "{error}"
    );
    let warnings = error["warnings"].as_array().unwrap();
    for pointer in ["/batch_typo", "/commands/0/typo", "/commands/1/extra"] {
        assert!(
            warnings.iter().any(|issue| issue["pointer"] == pointer),
            "{error}"
        );
    }
    assert!(error["schema"].is_object(), "{error}");
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
    let mut carry = kit["revision"][1].clone();
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
