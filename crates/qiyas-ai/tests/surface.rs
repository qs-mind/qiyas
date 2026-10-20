//! The model-facing surface, with scripted providers and no network.

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use qiyas_ai::prompt::{system_prompt, DrawingFacts, Lang};
use qiyas_ai::surface::{self, CAPTURE, EXECUTE, READ};
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, AiError, ContentBlock, Provider, ResultPart, StopReason,
    ToolError, ToolOutput, TurnRequest, TurnResult,
};
use qiyas_core::{Command, EntityFilter, Session, Source};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn sample() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
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
        let (_, names) = line.split_once(": ").unwrap();
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

/// Sends one tool call per answer, then ends the turn. Requests are kept to check the budget.
struct Script {
    calls: Mutex<VecDeque<(&'static str, Value)>>,
    requests: Arc<Mutex<Vec<TurnRequest>>>,
}

impl Provider for Script {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        _: qiyas_ai::provider::Sink<'a>,
    ) -> qiyas_ai::provider::BoxFuture<'a, Result<TurnResult, AiError>> {
        let mut requests = self.requests.lock().unwrap();
        let id = requests.len();
        requests.push(request);
        let next = self.calls.lock().unwrap().pop_front();
        Box::pin(async move {
            let (content, stop_reason) = match next {
                Some((name, input)) => (
                    vec![ContentBlock::ToolUse {
                        id: format!("call_{id}"),
                        name: name.into(),
                        input,
                    }],
                    StopReason::ToolUse,
                ),
                None => (
                    vec![ContentBlock::Text {
                        text: "Done".into(),
                    }],
                    StopReason::EndTurn,
                ),
            };
            Ok(TurnResult {
                content,
                stop_reason,
                usage: Default::default(),
            })
        })
    }
}

async fn scripted(
    session: Arc<Mutex<Option<Session>>>,
    calls: Vec<(&'static str, Value)>,
) -> (Agent, Vec<AgentEvent>, Arc<Mutex<Vec<TurnRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = Script {
        calls: Mutex::new(calls.into()),
        requests: requests.clone(),
    };
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
    agent
        .history()
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
    let results = results(&agent);
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
    let results = results(&agent);
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
                    points: vec![[0.0, 0.0], [1.0, 0.0]],
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
