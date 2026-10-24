//! Offline live-session replays. No credentials or model calls.
use qiyas_ai::surface::{CAPTURE, EXECUTE, READ};
use qiyas_ai::{Agent, AgentEvent, AgentSettings, ContentBlock, ResultPart, TurnRequest};
use qiyas_core::Session;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
#[path = "support/scripted.rs"]
mod scripted;
use scripted::{Script, ScriptCall};

fn latest_rows(req: &TurnRequest) -> Vec<Value> {
    req.messages
        .iter()
        .rev()
        .flat_map(|m| m.content.iter().rev())
        .find_map(|b| {
            if let ContentBlock::ToolResult { content, .. } = b {
                for p in content {
                    if let ResultPart::Text(t) = p {
                        if let Ok(v) = serde_json::from_str::<Value>(t) {
                            if let Some(rows) = v["records"].as_array() {
                                return Some(rows.clone());
                            }
                        }
                    }
                }
            }
            None
        })
        .unwrap_or_default()
}
fn replace_texts(req: &TurnRequest) -> (&'static str, Value) {
    let handles: Vec<_> = latest_rows(req)
        .iter()
        .filter(|r| r["type"] == "Text")
        .map(|r| r["key"].clone())
        .collect();
    (
        EXECUTE,
        json!({"op":"batch","args":{"layer":"Names","commands":[
            {"type":"delete_entities","targets":handles},
            {"type":"text","at":[17,3],"height":0.2,"value":"Bedroom 1"},
            {"type":"text","at":[21,3],"height":0.2,"value":"Bedroom 2"}
        ]}}),
    )
}
fn replace_dimensions(req: &TurnRequest) -> (&'static str, Value) {
    let handles: Vec<_> = latest_rows(req)
        .iter()
        .filter(|r| r["type"] == "Dimension")
        .map(|r| r["key"].clone())
        .collect();
    (
        EXECUTE,
        json!({"op":"batch","args":{"layer":"Dimensions","commands":[
            {"type":"delete_entities","targets":handles},
            {"type":"dimension","kind":"linear","p1":[16,0],"p2":[25,0],"line_point":[20,-1.2]}
        ]}}),
    )
}
fn edit_opening(req: &TurnRequest) -> (&'static str, Value) {
    let insert = latest_rows(req)
        .iter()
        .find(|r| r["type"] == "Insert")
        .map(|r| r["key"].clone())
        .expect("opening");
    (
        EXECUTE,
        json!({"op":"run","args":{"command":{"type":"edit_wall_opening","insert":insert,"swing":"right"}}}),
    )
}
async fn run(agent: &mut Agent, text: &str) -> Vec<AgentEvent> {
    let (tx, mut rx) = mpsc::channel(512);
    agent
        .run_turn(text.into(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut events = vec![];
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    events
}
#[tokio::test]
async fn draw_log_replay_applies_rebuilds_and_reports_cheaper_edits() {
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("blank.dxf"),
        6,
    ))));
    let recorded: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/goal_j_draw_calls.json")).unwrap();
    let mut calls: Vec<ScriptCall> = vec![];
    let read = || {
        ScriptCall::Fixed(
            READ,
            json!({"op":"records","args":{"collection":"entities","limit":200,"fields":["key","type"]}}),
        )
    };
    for (i, c) in recorded.iter().enumerate() {
        if i == 4 {
            calls.push(ScriptCall::FromHistory(edit_opening));
            calls.push(read());
        }
        calls.push(ScriptCall::Fixed(EXECUTE, c["input"].clone()));
        calls.push(read());
    }
    calls.push(ScriptCall::Fixed(
        CAPTURE,
        json!({"source":"picture","args":{"width":1000}}),
    ));
    calls.push(ScriptCall::Fixed(EXECUTE,json!({"op":"batch","args":{"layer":"Names","commands":[{"type":"text","at":[17,3],"height":0.2,"value":"Bedroom one"},{"type":"text","at":[21,3],"height":0.2,"value":"Bedroom two"}]}})));
    calls.push(read());
    for _ in 0..2 {
        calls.push(ScriptCall::FromHistory(replace_texts));
        calls.push(read());
    }
    calls.push(ScriptCall::Fixed(EXECUTE,json!({"op":"batch","args":{"layer":"Dimensions","commands":[{"type":"dimension","kind":"linear","p1":[16,0],"p2":[25,0],"line_point":[20,-1]}]}})));
    calls.push(read());
    for _ in 0..2 {
        calls.push(ScriptCall::FromHistory(replace_dimensions));
        calls.push(read());
    }
    calls.push(ScriptCall::Fixed(
        CAPTURE,
        json!({"source":"picture","args":{"width":1000}}),
    ));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut agent = Agent::new(
        Box::new(Script::steps(calls, requests.clone())),
        session.clone(),
        AgentSettings::default(),
    );
    let events = run(&mut agent, "Draw a small 2-bedroom").await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    let reqs = requests.lock().unwrap();
    if let Some(path) = std::env::var_os("QIYAS_GOAL_J_EVIDENCE") {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("scripted-events.json"),
            serde_json::to_vec_pretty(&events).unwrap(),
        )
        .unwrap();
        std::fs::write(
            path.join("scripted-requests.json"),
            serde_json::to_vec_pretty(
                &reqs
                    .iter()
                    .map(|r| json!({"system":r.system,"messages":r.messages}))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        )
        .unwrap();
        for (i, m) in scripted::observed_messages(&reqs).iter().enumerate() {
            for b in &m.content {
                if let ContentBlock::ToolResult { content, .. } = b {
                    for p in content {
                        if let ResultPart::Png(data) = p {
                            std::fs::write(
                                path.join(format!("scripted-{i}.png")),
                                qiyas_ai::from_base64(data).unwrap(),
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    let mut errors = Vec::new();
    let mut advice = Vec::new();
    let mut previous = std::collections::BTreeSet::new();
    let mut churn = Vec::new();
    for m in scripted::observed_messages(&reqs) {
        for block in m.content {
            if let ContentBlock::ToolResult {
                content, is_error, ..
            } = block
            {
                for part in content {
                    if let ResultPart::Text(text) = part {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            if is_error {
                                errors.push(value.clone());
                            }
                            advice
                                .extend(value["advice"].as_array().into_iter().flatten().cloned());
                            if let Some(rows) = value["records"].as_array() {
                                let now: std::collections::BTreeSet<_> = rows
                                    .iter()
                                    .filter_map(|r| qiyas_core::registry::handle_number(&r["key"]))
                                    .collect();
                                let added = now.difference(&previous).count();
                                let removed = previous.difference(&now).count();
                                churn.push(
                                    json!({"added":added,"removed":removed,"total":added+removed}),
                                );
                                previous = now;
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("QIYAS_GOAL_J_EVIDENCE") {
        std::fs::write(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path).join("scripted-churn.json"),serde_json::to_vec_pretty(&json!({"turn_total":churn.iter().filter_map(|v|v["total"].as_u64()).sum::<u64>(),"calls":churn})).unwrap()).unwrap();
    }
    assert!(
        errors.iter().all(|e| e["code"] != "repeated_rebuild"),
        "{errors:?}"
    );
    let guard = session.lock().unwrap();
    let drawing = guard.as_ref().unwrap();
    assert!(!drawing.wall_groups(None).unwrap().is_empty());
    assert_eq!(
        drawing
            .drawing
            .doc
            .entities()
            .filter(
                |e| matches!(e, opencadcodec::EntityType::Text(t) if t.value.starts_with("Bedroom"))
            )
            .count(),
        2
    );
    assert_eq!(
        drawing
            .drawing
            .doc
            .entities()
            .filter(|e| matches!(e, opencadcodec::EntityType::Dimension(_)))
            .count(),
        1
    );
    for command in ["edit_text", "dim_edit", "edit_wall_opening"] {
        assert!(
            advice.iter().any(|e| e["code"] == "repeated_rebuild"
                && e["message"].as_str().is_some_and(|s| s.contains(command))),
            "second rebuild must advise {command}: {advice:?}"
        );
    }
}
#[tokio::test]
async fn new_design_on_content_gives_context_and_leaves_the_question_to_mizan() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("existing.dxf"), 6);
    s.run_input(
        &json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
        qiyas_core::Source::User,
    )
    .unwrap();
    let revision = s.document_revision();
    let session = Arc::new(Mutex::new(Some(s)));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut agent = Agent::new(
        Box::new(Script::new(
            vec![(READ, json!({"op":"state"}))],
            requests.clone(),
        )),
        session.clone(),
        AgentSettings::default(),
    );
    let events = run(&mut agent, "Draw a small 2-bedroom").await;
    let reqs = requests.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    assert!(serde_json::to_string(&reqs[0].messages)
        .unwrap()
        .contains("Mizan decides"));
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    assert_eq!(
        session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .document_revision(),
        revision
    );
}

struct LanguageScript(Arc<Mutex<Vec<TurnRequest>>>);
impl qiyas_ai::Provider for LanguageScript {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        sink: qiyas_ai::provider::Sink<'a>,
    ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, qiyas_ai::AiError>> {
        let arabic = request
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
                    text.contains("[reply language]").then_some(text)
                } else {
                    None
                }
            })
            .next_back()
            .is_some_and(|s| s.contains("الفصحى"));
        self.0.lock().unwrap().push(request);
        sink(qiyas_ai::StreamEvent::TextDelta(
            if arabic {
                "سأراجع الرسم."
            } else {
                "I will check the drawing."
            }
            .into(),
        ));
        Box::pin(async move {
            Ok(qiyas_ai::TurnResult {
                content: vec![ContentBlock::Text {
                    text: if arabic {
                        "سأراجع الرسم."
                    } else {
                        "I will check the drawing."
                    }
                    .into(),
                }],
                stop_reason: qiyas_ai::StopReason::EndTurn,
                usage: Default::default(),
            })
        })
    }
}
#[tokio::test]
async fn latest_message_controls_language_after_arabic_greeting_and_english_followup() {
    let dir = tempfile::tempdir().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut agent = Agent::new(
        Box::new(LanguageScript(requests.clone())),
        Arc::new(Mutex::new(Some(Session::new_blank(
            &dir.path().join("lang.dxf"),
            6,
        )))),
        AgentSettings::default(),
    );
    for (message, arabic) in [
        ("هاي", true),
        ("Please read the drawing", false),
        ("أضف الأبعاد إلى المخطط", true),
        ("What are the room areas?", false),
    ] {
        let events = run(&mut agent, message).await;
        assert!(events.iter().any(|e| matches!(e,AgentEvent::Text{text} if text == if arabic {"سأراجع الرسم."}else{"I will check the drawing."})));
        let reqs = requests.lock().unwrap();
        let req = reqs.last().unwrap();
        let guidance: Vec<_> = req
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
                    text.contains("[reply language]").then_some(text)
                } else {
                    None
                }
            })
            .collect();
        assert!(!guidance.is_empty());
        assert_eq!(guidance.last().unwrap().contains("الفصحى"), arabic);
        assert_eq!(
            guidance
                .last()
                .unwrap()
                .contains("Answer this turn in English"),
            !arabic
        );
    }
    if let Some(path) = std::env::var_os("QIYAS_GOAL_J_EVIDENCE") {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("language-requests.json"),
            serde_json::to_vec_pretty(
                &requests
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|r| json!({"messages":r.messages}))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        )
        .unwrap();
    }
}
fn surface_call(s: &mut Session, tool: &str, input: Value) -> Result<Value, qiyas_ai::ToolError> {
    let qiyas_ai::surface::Route::Session { tool, op, args } =
        qiyas_ai::surface::route(tool, &input).map_err(qiyas_ai::ToolError::BadInput)?
    else {
        panic!()
    };
    match qiyas_ai::surface::run_session(s, &tool, &op, &args, qiyas_core::Source::Ai)? {
        qiyas_ai::ToolOutput::Json(v) => Ok(v),
        _ => panic!(),
    }
}
#[test]
fn surface_normalisations_are_reported_and_ambiguous_inputs_stay_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("input.dxf"), 6);
    for value in ["First", "Second"] {
        surface_call(
            &mut s,
            EXECUTE,
            json!({"op":"run","args":{"command":{"type":"text","at":[0,0],"value":value}}}),
        )
        .unwrap();
    }
    for args in [
        json!({"kind":"texts","limit":1}),
        json!({"kind":"layers","limit":1,"offset":0}),
        json!({"kind":"window","corner1":[-1,-1],"corner2":[5,5]}),
    ] {
        let out = surface_call(&mut s, READ, json!({"op":"query","args":args})).unwrap();
        assert!(!out["warnings"].as_array().unwrap().is_empty(), "{out}");
        if args["kind"] == "texts" {
            assert_eq!(out["rows"].as_array().unwrap().len(), 1);
            assert_eq!(out["total"], 2);
            assert_eq!(out["next_offset"], 1);
        }
    }
    let out=surface_call(&mut s,READ,json!({"op":"records","args":{"collection":"entities","where":{"path":"type","op":"eq","value":"Text"}}})).unwrap();
    assert_eq!(out["total"], 2);
    assert!(out["warnings"][0]["message"]
        .as_str()
        .unwrap()
        .contains("where"));
    assert!(qiyas_ai::surface::route(
        EXECUTE,
        &json!({"command":{"type":"new_layer","name":"Normalised"}})
    )
    .is_ok());
    for input in [
        json!({"op":"query","args":{"kind":"window","corner1":[0,0]}}),
        json!({"op":"query","args":{"kind":"window","bounds":[[0,0],[5,5]],"corner1":[0,0],"corner2":[5,5]}}),
        json!({"op":"measure","args":{"handles":[1,2]}}),
        json!({"op":"measure","args":{"handle":1,"handles":[1]}}),
    ] {
        assert!(qiyas_ai::surface::route(READ, &input).is_err());
    }
    let plan = surface_call(&mut s, EXECUTE, json!({"op":"plan","args":{"phases":[{"id":"all","title":"Whole design","status":"active"}]}})).unwrap();
    assert_eq!(plan["plan"]["phases"].as_array().unwrap().len(), 1);
    assert_eq!(plan["advice"][0]["code"], "small_phases");
}
#[test]
fn latest_language_recognises_uppercase_prose_and_keeps_numeric_replies_neutral() {
    use qiyas_ai::{judgement::language, prompt::Lang};
    assert_eq!(language("HELLO", Lang::Ar), Lang::En);
    assert_eq!(language("هاي", Lang::En), Lang::Ar);
    assert_eq!(language("افحص A-WALL", Lang::En), Lang::Ar);
    assert_eq!(language("٢", Lang::En), Lang::En);
}

#[test]
fn a_design_description_does_not_imply_a_destination() {
    assert!(qiyas_ai::judgement::new_design(
        "Draw a flat where both rooms have windows"
    ));
    assert!(qiyas_ai::judgement::new_design("ارسم شقة صغيرة بغرفتين"));
    assert!(!qiyas_ai::judgement::new_design("Draw a flat here."));
    assert!(!qiyas_ai::judgement::new_design("Draw a circle"));
    assert!(!qiyas_ai::judgement::new_design("Draw a circle."));
    assert!(!qiyas_ai::judgement::new_design("ارسم خطًا بطول ثلاثة أمتار"));
}
