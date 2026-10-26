//! W1: Mizan's behaviour and cost, with a scripted provider (no model, no network).
//! Questions in one structured call, the finish gate, the reply language, the cost note given
//! once, "continue" resuming the plan, saving at phase ends, exports after a failed check,
//! and the request bodies a long turn sends (what the provider can reuse from cache).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use qiyas_ai::host::DrawingHost;
use qiyas_ai::provider::{BoxFuture, Sink};
use qiyas_ai::surface::{self, EXECUTE, READ};
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, AiError, ContentBlock, Provider, Role, StopReason,
    TurnRequest, TurnResult,
};
use qiyas_core::Session;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// One scripted model answer.
enum Reply {
    /// Ends the turn with this text.
    Text(&'static str),
    /// Calls tools, with optional words before them.
    Calls(&'static str, Vec<(&'static str, Value)>),
}

struct Queue {
    replies: Mutex<VecDeque<Reply>>,
    requests: Arc<Mutex<Vec<TurnRequest>>>,
}

impl Queue {
    fn new(replies: Vec<Reply>) -> (Self, Arc<Mutex<Vec<TurnRequest>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                replies: Mutex::new(replies.into()),
                requests: requests.clone(),
            },
            requests,
        )
    }
}

impl Provider for Queue {
    fn image_input(&self, _model: &str) -> Option<bool> {
        Some(false)
    }

    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        let id = {
            let mut requests = self.requests.lock().unwrap();
            requests.push(request);
            requests.len()
        };
        let next = self.replies.lock().unwrap().pop_front();
        Box::pin(async move {
            let (content, stop_reason) = match next {
                Some(Reply::Calls(words, calls)) => {
                    let mut content = Vec::new();
                    if !words.is_empty() {
                        content.push(ContentBlock::Text { text: words.into() });
                    }
                    for (n, (name, input)) in calls.into_iter().enumerate() {
                        content.push(ContentBlock::ToolUse {
                            id: format!("call_{id}_{n}"),
                            name: name.into(),
                            input,
                        });
                    }
                    (content, StopReason::ToolUse)
                }
                Some(Reply::Text(text)) => (
                    vec![ContentBlock::Text { text: text.into() }],
                    StopReason::EndTurn,
                ),
                None => (
                    vec![ContentBlock::Text {
                        text: "Everything is done.".into(),
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

/// The app's file tools, recording what was asked.
#[derive(Default)]
struct Host {
    saves: Mutex<usize>,
    untitled: bool,
}

impl DrawingHost for Host {
    fn list(&self) -> Result<Value, String> {
        Ok(json!({"open": []}))
    }
    fn new_drawing(&self, _units: &str) -> Result<Value, String> {
        Ok(json!({}))
    }
    fn open(&self, _path: &str, _named: bool) -> Result<Value, String> {
        Ok(json!({}))
    }
    fn switch(&self, _id: u32) -> Result<Value, String> {
        Ok(json!({}))
    }
    fn save(&self) -> Result<Value, String> {
        *self.saves.lock().unwrap() += 1;
        if self.untitled {
            Ok(json!({"asked_user": true, "message": "Save As is open"}))
        } else {
            Ok(json!({"message": "Saved"}))
        }
    }
    fn close(&self, _id: Option<u32>) -> Result<Value, String> {
        Ok(json!({}))
    }
}

fn sample() -> (tempfile::TempDir, Arc<Mutex<Option<Session>>>) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, Arc::new(Mutex::new(Some(session))))
}

fn agent(provider: Queue, session: Arc<Mutex<Option<Session>>>) -> Agent {
    Agent::new(Box::new(provider), session, AgentSettings::default())
}

async fn turn(agent: &mut Agent, text: &str) -> Vec<AgentEvent> {
    let (tx, mut rx) = mpsc::channel(10_000);
    agent
        .run_turn(text.to_string(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn plan(phases: &[(&str, &str)]) -> (&'static str, Value) {
    let phases: Vec<Value> = phases
        .iter()
        .map(|(title, status)| json!({"id": title.to_lowercase().replace(' ', "-"), "title": title, "status": status}))
        .collect();
    (EXECUTE, json!({"op":"plan","args":{"phases":phases}}))
}

/// Every text the user-role messages of a request carry.
fn user_texts(request: &TurnRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .flat_map(|m| &m.content)
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn results_text(request: &TurnRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|b| match b {
            ContentBlock::ToolResult { content, .. } => Some(format!("{content:?}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------------
// 1. Questions

fn ask(questions: Value) -> (&'static str, Value) {
    (EXECUTE, json!({"op":"ask","args":{"questions":questions}}))
}

#[tokio::test]
async fn several_questions_go_out_in_one_event_with_options_and_end_the_turn() {
    let (_dir, session) = sample();
    let questions = json!([
        {"question":"Which country's practice?","options":[
            {"label":"Egypt","recommended":true},{"label":"Saudi Arabia"},{"label":"UAE"}]},
        {"id":"finish","question":"Finishes level?","options":[
            {"label":"Standard","description":"Ceramic floors","recommended":true},{"label":"High-end"}]},
        {"question":"Roof?","options":[{"label":"Flat"},{"label":"Pitched"}]}
    ]);
    let (provider, requests) = Queue::new(vec![Reply::Calls("", vec![ask(questions)])]);
    let mut agent = agent(provider, session);
    let events = turn(&mut agent, "Design a villa package").await;

    let asked: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Question { questions } => Some(questions),
            _ => None,
        })
        .collect();
    assert_eq!(
        asked.len(),
        1,
        "one event for all the questions: {events:?}"
    );
    let q = asked[0];
    assert_eq!(q.len(), 3);
    assert_eq!(q[0].id, "q1");
    assert_eq!(q[1].id, "finish");
    assert!(
        q.iter().all(|q| q.allow_other),
        "free text is always allowed"
    );
    assert_eq!(q[0].options.len(), 3);
    assert!(q[0].options[0].recommended && !q[0].options[1].recommended);
    assert_eq!(
        q[1].options[0].description.as_deref(),
        Some("Ceramic floors")
    );
    // The turn ended on the question: one request, no Done, nothing run after it.
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(!events.iter().any(|e| matches!(e, AgentEvent::Done { .. })));
    // The event serialises with everything a window needs.
    let wire = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Question { .. }))
        .map(|e| serde_json::to_value(e).unwrap())
        .unwrap();
    assert_eq!(wire["type"], "question");
    assert_eq!(wire["questions"][0]["options"][0]["label"], "Egypt");
    assert_eq!(wire["questions"][0]["options"][0]["recommended"], true);
    assert_eq!(wire["questions"][0]["allow_other"], true);
    // The ask and its result are in the history, so the answer carries on from them.
    assert!(agent.history().last().unwrap().has_tool_results());
}

#[tokio::test]
async fn the_answer_to_a_question_continues_the_same_conversation() {
    let (_dir, session) = sample();
    let q = json!([{"question":"Which country?","options":[{"label":"Egypt"},{"label":"UAE"}]}]);
    let (provider, requests) = Queue::new(vec![
        Reply::Calls("", vec![ask(q)]),
        Reply::Text("Egypt, understood. I will use Egyptian practice."),
    ]);
    let mut agent = agent(provider, session);
    turn(&mut agent, "Draw a villa").await;
    let events = turn(&mut agent, "Egypt").await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(user_texts(&requests[1]).iter().any(|t| t == "Egypt"));
    assert!(results_text(&requests[1]).contains("asked"));
}

#[tokio::test]
async fn a_bad_question_is_an_error_the_model_can_fix_in_the_next_request() {
    let (_dir, session) = sample();
    let one_option = json!([{"question":"Which?","options":[{"label":"Only"}]}]);
    let two_recommended = json!([{"question":"Which?","options":[
        {"label":"A","recommended":true},{"label":"B","recommended":true}]}]);
    let (provider, requests) = Queue::new(vec![
        Reply::Calls("", vec![ask(one_option), ask(two_recommended)]),
        Reply::Text("Understood."),
    ]);
    let mut agent = agent(provider, session);
    let events = turn(&mut agent, "Draw").await;
    assert!(!events
        .iter()
        .any(|e| matches!(e, AgentEvent::Question { .. })));
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let text = results_text(&requests[1]);
    assert!(text.contains("minItems 2"), "{text}");
    assert!(text.contains("more than one option recommended"), "{text}");
}

#[test]
fn ask_is_an_operation_with_a_contract_and_never_a_drawing_edit() {
    assert!(surface::EXECUTE_OPS.contains(&"ask"));
    assert!(!surface::mutating(EXECUTE, &json!({"op":"ask","args":{}})));
    let info = surface::route(
        EXECUTE,
        &json!({"op":"ask","args":{"questions":[
        {"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]}}),
    );
    assert!(info.is_ok(), "{info:?}");
    // A question with five options is refused by the contract before anything runs.
    let five = json!({"op":"ask","args":{"questions":[{"question":"Which?","options":[
        {"label":"A"},{"label":"B"},{"label":"C"},{"label":"D"},{"label":"E"}]}]}});
    assert!(surface::route(EXECUTE, &five).is_err());
}

// ---------------------------------------------------------------------------------------------
// 4. The finish gate

#[tokio::test]
async fn ending_with_plan_phases_open_is_sent_back_naming_them() {
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls(
            "",
            vec![plan(&[
                ("Floor plans", "done"),
                ("Sections", "pending"),
                ("Roof plan", "active"),
            ])],
        ),
        Reply::Text("I stopped here to avoid more cost."),
        Reply::Calls(
            "",
            vec![plan(&[
                ("Floor plans", "done"),
                ("Sections", "done"),
                ("Roof plan", "done"),
            ])],
        ),
        Reply::Text("All three are done."),
    ]);
    let mut agent = agent(provider, session);
    let events = turn(&mut agent, "Build the full package").await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4, "the early ending was refused once");
    let refusal = user_texts(&requests[2]).join("\n");
    assert!(refusal.contains("[finish refused]"), "{refusal}");
    assert!(
        refusal.contains("Sections (pending)") && refusal.contains("Roof plan (active)"),
        "{refusal}"
    );
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Retrying { code, discard_partial: true, .. } if code == "ai.error.finish_gate")),
        "the window drops the early answer: {events:?}"
    );
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
}

#[tokio::test]
async fn the_finish_gate_lets_go_when_the_user_said_stop_and_after_two_refusals() {
    // The user said stop.
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls("", vec![plan(&[("Sections", "pending")])]),
        Reply::Text("Stopped as you asked. Not done: sections."),
    ]);
    let mut stopper = agent(provider, session);
    let events = turn(&mut stopper, "Stop, that's enough").await;
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));

    // Never stopped by the user: refused twice, then it may end, honestly.
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls("", vec![plan(&[("Sections", "pending")])]),
        Reply::Text("Done."),
        Reply::Text("Done."),
        Reply::Text("Not done: sections, because the soil data is missing."),
    ]);
    let mut trapped = agent(provider, session);
    let events = turn(&mut trapped, "Draw the sections").await;
    assert_eq!(requests.lock().unwrap().len(), 4);
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
}

#[tokio::test]
async fn a_closing_question_in_plain_text_or_a_skipped_phase_is_not_held_back() {
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls(
            "",
            vec![plan(&[("Sections", "skipped"), ("Plans", "done")])],
        ),
        Reply::Text("Sections skipped: no soil data. Anything else?"),
    ]);
    let mut a = agent(provider, session);
    turn(&mut a, "Draw").await;
    assert_eq!(requests.lock().unwrap().len(), 2);

    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls("", vec![plan(&[("Sections", "pending")])]),
        Reply::Text("Which soil report should the sections follow?"),
    ]);
    let mut a = agent(provider, session);
    turn(&mut a, "Draw the sections").await;
    assert_eq!(
        requests.lock().unwrap().len(),
        2,
        "a question for the user ends the turn"
    );
}

#[tokio::test]
async fn the_finish_operation_is_refused_while_phases_are_open() {
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls(
            "",
            vec![plan(&[("Plans", "done"), ("Sections", "pending")])],
        ),
        Reply::Calls(
            "",
            vec![(EXECUTE, json!({"op":"finish","args":{"answer":"All done"}}))],
        ),
        Reply::Text("Understood, continuing."),
    ]);
    let mut a = Agent::new(Box::new(provider), session, AgentSettings::default())
        .with_host(Arc::new(Host::default()));
    turn(&mut a, "Draw").await;
    let requests = requests.lock().unwrap();
    let text = results_text(&requests[2]);
    assert!(
        text.contains("open_phases") && text.contains("Sections (pending)"),
        "{text}"
    );
}

// ---------------------------------------------------------------------------------------------
// 3. Cost advice and "continue"

#[tokio::test]
async fn the_cost_note_is_given_once_and_never_says_to_stop() {
    let (_dir, session) = sample();
    let read = (READ, json!({"op":"state","args":{}}));
    let mut replies = Vec::new();
    for _ in 0..45 {
        replies.push(Reply::Calls("", vec![read.clone()]));
    }
    replies.push(Reply::Text("Done."));
    let (provider, requests) = Queue::new(replies);
    let mut a = agent(provider, session);
    turn(&mut a, "Read the state a lot").await;
    let requests = requests.lock().unwrap();
    let last = requests.last().unwrap();
    let notes = user_texts(last)
        .iter()
        .filter(|t| t.starts_with("[efficiency note]"))
        .count();
    let advice = results_text(last).matches("tool_cost").count();
    assert_eq!((notes, advice), (1, 1), "each is given once");
    let all = format!("{:?} {}", user_texts(last), results_text(last));
    assert!(all.contains("not a reason to stop"), "{all}");
    assert!(!all.contains("subscription cost"), "{all}");
}

#[tokio::test]
async fn continue_resumes_the_open_phases_instead_of_starting_over() {
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Calls(
            "",
            vec![plan(&[
                ("Plans", "done"),
                ("Structural sheets", "pending"),
                ("MEP sheets", "pending"),
            ])],
        ),
        Reply::Text("Stopped here. Anything else?"),
        Reply::Calls(
            "",
            vec![plan(&[
                ("Plans", "done"),
                ("Structural sheets", "done"),
                ("MEP sheets", "done"),
            ])],
        ),
        Reply::Text("Everything is drawn."),
    ]);
    let mut a = agent(provider, session);
    turn(&mut a, "Build the design package").await;
    let events = turn(&mut a, "Continue and complete everything !").await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    let requests = requests.lock().unwrap();
    let continuing = user_texts(&requests[2]).join("\n");
    assert!(continuing.contains("[continue]"), "{continuing}");
    assert!(
        continuing.contains("Structural sheets; MEP sheets"),
        "{continuing}"
    );
    assert!(continuing.contains("Do not stop for cost"), "{continuing}");
}

// ---------------------------------------------------------------------------------------------
// 9. Reply language

#[tokio::test]
async fn an_answer_in_the_wrong_language_is_asked_for_again_once() {
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Text("توقفت هنا لتجنب زيادة التكلفة. الموجود حاليا حزمة تطوير معماري مبدئية وليست حزمة تنفيذ كاملة أو معتمدة من المهندس."),
        Reply::Text("The package is a preliminary architectural set, not a complete or approved construction set."),
    ]);
    let mut a = agent(provider, session);
    let events = turn(&mut a, "Build a detailed full design package").await;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(user_texts(&requests[1])
        .join("\n")
        .contains("[reply language]"));
    assert!(events.iter().any(
        |e| matches!(e, AgentEvent::Retrying { code, .. } if code == "ai.error.language_retry")
    ));
    // An Arabic message gets an Arabic answer, and an English one is sent back.
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![
        Reply::Text("The package is a preliminary architectural set, not a complete or approved construction set by the engineer."),
        Reply::Text("الحزمة مبدئية وليست معتمدة."),
    ]);
    let mut b = agent(provider, session);
    turn(&mut b, "ارسم فيلا دورين وطلّع مقايستها").await;
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn names_and_files_in_the_other_language_do_not_trigger_the_language_retry() {
    let (_dir, session) = sample();
    let (provider, requests) = Queue::new(vec![Reply::Text(
        "The files are Cairo_Villa_Plans.pdf and Villa_Quantities.xlsx; rooms named مجلس and صالة are labelled.",
    )]);
    let mut a = agent(provider, session);
    turn(&mut a, "List the files").await;
    assert_eq!(requests.lock().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// 8. Saving

fn draw_line() -> (&'static str, Value) {
    (
        EXECUTE,
        json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[3,0]}}}),
    )
}

#[tokio::test]
async fn a_finished_phase_that_changed_a_titled_drawing_is_saved() {
    let (_dir, session) = sample();
    let host = Arc::new(Host::default());
    let (provider, _requests) = Queue::new(vec![
        Reply::Calls("", vec![plan(&[("Plans", "active")])]),
        Reply::Calls("", vec![draw_line()]),
        Reply::Calls("", vec![plan(&[("Plans", "done")])]),
        Reply::Text("Plans drawn."),
    ]);
    let mut a =
        Agent::new(Box::new(provider), session, AgentSettings::default()).with_host(host.clone());
    let events = turn(&mut a, "Draw the plans").await;
    // Once when the phase finished, once when the turn ended with the drawing still changed.
    assert_eq!(*host.saves.lock().unwrap(), 2, "{events:?}");
}

#[tokio::test]
async fn nothing_is_saved_when_nothing_changed() {
    let (_dir, session) = sample();
    let host = Arc::new(Host::default());
    let (provider, _requests) = Queue::new(vec![
        Reply::Calls("", vec![plan(&[("Look", "done")])]),
        Reply::Text("Looked."),
    ]);
    let mut a =
        Agent::new(Box::new(provider), session, AgentSettings::default()).with_host(host.clone());
    turn(&mut a, "Look at the drawing").await;
    assert_eq!(*host.saves.lock().unwrap(), 0);
}

#[tokio::test]
async fn an_untitled_drawing_is_not_asked_about_mid_work_but_once_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut blank = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 6);
    blank.mark_untitled();
    let session = Arc::new(Mutex::new(Some(blank)));
    let host = Arc::new(Host {
        untitled: true,
        ..Default::default()
    });
    let (provider, _requests) = Queue::new(vec![
        Reply::Calls("", vec![plan(&[("Plans", "active")])]),
        Reply::Calls("", vec![draw_line()]),
        Reply::Calls("", vec![plan(&[("Plans", "done")])]),
        Reply::Text("Plans drawn."),
    ]);
    let mut a =
        Agent::new(Box::new(provider), session, AgentSettings::default()).with_host(host.clone());
    let events = turn(&mut a, "Draw the plans").await;
    assert_eq!(*host.saves.lock().unwrap(), 1, "asked once, at the end");
    let said: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(said.contains("Save As"), "{said}");
}

// ---------------------------------------------------------------------------------------------
// 10. Exports after a failed quantity check

fn failing_check(session: &mut Session) {
    let mut kit = surface::run_session(
        session,
        READ,
        "takeoff_kit",
        &json!({}),
        qiyas_core::Source::Ai,
    )
    .map(|o| match o {
        qiyas_ai::ToolOutput::Json(v) => v,
        _ => panic!("json"),
    })
    .unwrap();
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
    rules[0] = rule;
    surface::run_session(
        session,
        EXECUTE,
        "batch",
        &kit["batch"],
        qiyas_core::Source::Ai,
    )
    .unwrap();
}

fn export(session: &mut Session, op: &str, args: Value) -> Result<Value, qiyas_ai::ToolError> {
    surface::run_session(session, EXECUTE, op, &args, qiyas_core::Source::Ai).map(|o| match o {
        qiyas_ai::ToolOutput::Json(v) => v,
        _ => panic!("json"),
    })
}

#[test]
fn quantities_are_not_exported_after_a_failed_check_unless_named_a_draft() {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&path).unwrap();
    // Before anything is wrong, the export goes ahead as it always did.
    let ok = export(&mut session, "export_takeoff", json!({"path":"fine.xlsx"})).unwrap();
    assert!(ok.get("draft").is_none(), "{ok}");
    failing_check(&mut session);

    let refused = export(&mut session, "export_takeoff", json!({"path":"villa.xlsx"})).unwrap_err();
    let refused = refused.json();
    assert_eq!(refused["code"], "quantity_check_failed", "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("DRAFT"));
    assert!(
        refused["measurement_errors"].as_u64().unwrap_or(0) >= 1
            || refused.to_string().contains("measurement_errors")
    );
    assert!(!dir.path().join("villa.xlsx").exists());

    let draft = export(
        &mut session,
        "export_takeoff",
        json!({"path":"villa_DRAFT.xlsx"}),
    )
    .unwrap();
    assert_eq!(draft["draft"], true);
    assert!(draft["advice"][0]["message"]
        .as_str()
        .unwrap()
        .contains("DRAFT"));
    assert!(dir.path().join("villa_DRAFT.xlsx").exists());

    // The deliverable command is held to the same rule.
    let command = |file: &str| json!({"command":{"type":"deliverable","kind":"export","report":"boq","format":"excel","path":file}});
    let refused = export(&mut session, "run", command("boq.xlsx"))
        .unwrap_err()
        .json();
    assert_eq!(refused["code"], "quantity_check_failed", "{refused}");
}
