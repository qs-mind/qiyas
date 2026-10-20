//! Offline evaluation of the real Agent loop. The scripted provider chooses tools;
//! expected quantities come from samples/test-plan.md, never from tool output.
use std::sync::{Arc, Mutex};

use qiyas_ai::{
    provider::{BoxFuture, Sink},
    Agent, AgentEvent, AgentSettings, AiError, ContentBlock, Provider, StopReason, TurnRequest,
    TurnResult,
};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct ScriptedModel(Mutex<Option<Vec<(String, Value)>>>);
impl Provider for ScriptedModel {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        let calls = self.0.lock().unwrap().take();
        Box::pin(async move {
            let (content, stop_reason) = match calls {
                Some(calls) => (
                    calls
                        .into_iter()
                        .enumerate()
                        .map(|(i, (name, input))| ContentBlock::ToolUse {
                            id: format!("eval_{i}"),
                            name,
                            input,
                        })
                        .collect(),
                    StopReason::ToolUse,
                ),
                None => {
                    assert!(request.messages.last().unwrap().has_tool_results());
                    (
                        vec![ContentBlock::Text {
                            text: "Tools complete.".into(),
                        }],
                        StopReason::EndTurn,
                    )
                }
            };
            Ok(TurnResult {
                content,
                stop_reason,
                usage: Default::default(),
            })
        })
    }
}

fn query(session: &mut Session, input: Value) -> Value {
    match qiyas_ai::surface::run_session(
        session,
        qiyas_ai::surface::READ,
        "query",
        &input,
        Source::Ai,
    )
    .unwrap()
    {
        qiyas_ai::ToolOutput::Json(value) => value,
        _ => panic!("expected JSON"),
    }
}

fn locate(session: &mut Session, layer: &str, kind: &str, x: f64, y: f64) -> Value {
    let found = query(session, json!({"kind":"window","layer":layer,"type":kind}));
    let row = found["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            let p = &r["bounds"][0];
            (p[0].as_f64().unwrap() - x).abs() < 1e-6 && (p[1].as_f64().unwrap() - y).abs() < 1e-6
        })
        .unwrap();
    json!({"handle":row["handle"]})
}

fn measurement_call(kind: &str, input: Value) -> (String, Value) {
    let command = if kind == "count_blocks" {
        json!({"type":"count_block","block_name":input["block_name"]})
    } else {
        json!({
            "type":"add_measurement",
            "kind":if kind == "measure_area" { "area" } else { "length" },
            "handles":[input["handle"]], "points":[], "description":"", "note":""
        })
    };
    (
        "qiyas_execute".into(),
        json!({"op":"run","args":{"command":command}}),
    )
}

async fn ask(session: Session, prompt: &str, calls: Vec<(String, Value)>) -> Session {
    let shared = Arc::new(Mutex::new(Some(session)));
    let model = ScriptedModel(Mutex::new(Some(calls)));
    let mut agent = Agent::new(Box::new(model), shared.clone(), AgentSettings::default());
    let (tx, mut rx) = mpsc::channel(1000);
    agent
        .run_turn(prompt.into(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut done = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::ToolResult {
                is_error, summary, ..
            } => assert!(!is_error, "{summary}"),
            AgentEvent::Done { .. } => done = true,
            _ => {}
        }
    }
    assert!(done);
    drop(agent);
    Arc::try_unwrap(shared)
        .ok()
        .unwrap()
        .into_inner()
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn ten_takeoff_questions_with_independent_answers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    // Copies deliberately exclude .qiyas and embedded prior measurements are cleared.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test-plan.dwg");
    std::fs::copy(root.join("test-plan.dwg"), &path).unwrap();
    std::fs::copy(root.join("test-boq.xlsx"), dir.path().join("test-boq.xlsx")).unwrap();
    let mut passed = 0;
    for (question, tool, layer, kind, x, y, expected, unit) in [
        (
            "مساحة غرفة المعيشة؟",
            "measure_area",
            "A-ROOM",
            "LwPolyline",
            0.15,
            0.15,
            58.29,
            "m2",
        ),
        (
            "مساحة غرفة النوم؟",
            "measure_area",
            "A-ROOM",
            "LwPolyline",
            7.15,
            0.15,
            22.09,
            "m2",
        ),
        (
            "مساحة المطبخ؟",
            "measure_area",
            "A-ROOM",
            "LwPolyline",
            7.15,
            5.15,
            17.39,
            "m2",
        ),
        (
            "محيط الحائط الخارجي؟",
            "measure_length",
            "A-WALL",
            "LwPolyline",
            0.,
            0.,
            42.,
            "m",
        ),
        (
            "طول القاطع الرأسي؟",
            "measure_length",
            "A-WALL",
            "Line",
            7.,
            0.,
            9.,
            "m",
        ),
        (
            "طول القاطع الأفقي؟",
            "measure_length",
            "A-WALL",
            "Line",
            7.,
            5.,
            5.,
            "m",
        ),
        ("عدد الأبواب؟", "count_blocks", "DOOR", "", 0., 0., 3., "pcs"),
        (
            "عدد الشبابيك؟",
            "count_blocks",
            "WINDOW",
            "",
            0.,
            0.,
            5.,
            "pcs",
        ),
        (
            "عدد الأعمدة؟",
            "count_blocks",
            "COLUMN",
            "",
            0.,
            0.,
            6.,
            "pcs",
        ),
    ] {
        let mut session = Session::open(&path).unwrap();
        session.takeoff.items.clear();
        let input = if tool == "count_blocks" {
            json!({"block_name":layer})
        } else {
            locate(&mut session, layer, kind, x, y)
        };
        let session = ask(session, question, vec![measurement_call(tool, input)]).await;
        assert_eq!(session.takeoff.items.len(), 1, "{question}");
        let m = &session.takeoff.items[0];
        assert!(
            (m.shown - expected).abs() <= 0.001,
            "{question}: {} != {expected}",
            m.shown
        );
        assert_eq!(m.unit, unit);
        assert_eq!(m.source, "ai");
        passed += 1;
        println!("PASS {passed}/10 {question} = {expected} {unit}");
    }
    let mut session = Session::open(&path).unwrap();
    session.takeoff.items.clear();
    let mut calls = Vec::new();
    for (tool, layer, kind, x, y) in [
        ("measure_length", "A-WALL", "LwPolyline", 0., 0.),
        ("measure_length", "A-WALL", "Line", 7., 0.),
        ("measure_length", "A-WALL", "Line", 7., 5.),
        ("measure_area", "A-ROOM", "LwPolyline", 0.15, 0.15),
        ("measure_area", "A-ROOM", "LwPolyline", 7.15, 0.15),
        ("count_blocks", "DOOR", "", 0., 0.),
        ("count_blocks", "WINDOW", "", 0., 0.),
    ] {
        let input = if tool == "count_blocks" {
            json!({"block_name":layer})
        } else {
            locate(&mut session, layer, kind, x, y)
        };
        // Actual measurements run through the Agent. Links are explicit in this case.
        calls.push(measurement_call(tool, input));
    }
    let mut session = ask(session, "احصر الرسم وقارنه بجدول الكميات", calls).await;
    let codes = ["A.1", "A.2", "A.2", "A.3", "A.4", "B.1", "B.2"];
    let mut calls = vec![(
        "qiyas_execute".into(),
        json!({"op":"import_boq","args":{"path":"test-boq.xlsx"}}),
    )];
    for (m, code) in session.takeoff.items.iter().zip(codes) {
        calls.push((
            "qiyas_execute".into(),
            json!({"op":"run","args":{"command":{"type":"link_boq","id":m.id,"code":code}}}),
        ));
    }
    calls.push(("qiyas_execute".into(), json!({"op":"compare","args":"{}"})));
    session = ask(session, "قارن الكميات المقاسة بالجدول", calls).await;
    let conflicts = serde_json::to_value(&session.conflicts).unwrap();
    assert_eq!(session.conflicts.len(), 2, "{conflicts}");
    for (code, expected, found, unit) in [("A.4", 30., 22.09, "m2"), ("B.2", 4., 5., "pcs")] {
        let c = session
            .conflicts
            .iter()
            .find(|c| c.boq_code.as_deref() == Some(code))
            .unwrap();
        assert_eq!(c.kind, qiyas_core::ConflictKind::QuantityMismatch);
        assert_eq!(c.expected, Some(expected));
        assert!((c.found.unwrap() - found).abs() <= 0.001);
        assert_eq!(c.unit, unit);
    }
    println!("PASS 10/10 BOQ: A.4 (30 vs 22.09 m2), B.2 (4 vs 5 pcs)");
    println!("Offline scripted-model evaluation: 10/10 passed; live model not evaluated.");
}

/// Private fixtures are supplied locally. Default CI never receives customer drawings.
#[tokio::test]
#[ignore = "requires QIYAS_REAL_EVAL_REFERENCES and private DWG files"]
async fn two_private_drawing_reference_answers() {
    let reference =
        std::env::var("QIYAS_REAL_EVAL_REFERENCES").expect("private reference JSON path");
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(reference).unwrap()).unwrap();
    assert_eq!(cases.len(), 2);
    let dir = tempfile::tempdir().unwrap();
    for case in cases {
        let path = dir
            .path()
            .join(format!("{}.dwg", case["id"].as_str().unwrap()));
        std::fs::copy(case["path"].as_str().unwrap(), &path).unwrap();
        let mut session = Session::open(&path).unwrap();
        session.takeoff.items.clear();
        let session = ask(
            session,
            case["question"].as_str().unwrap(),
            vec![measurement_call(
                "measure_length",
                json!({"handle":case["handle"]}),
            )],
        )
        .await;
        assert_eq!(session.takeoff.items.len(), 1);
        let measured = &session.takeoff.items[0];
        assert_eq!(measured.unit, case["unit"].as_str().unwrap());
        assert_eq!(measured.source, "ai");
        let expected = case["expected"].as_f64().unwrap();
        assert!(expected.is_finite() && expected > 0.0);
        assert_eq!(case["tolerance"].as_f64().unwrap(), 0.001);
        assert!((measured.shown - expected).abs() <= 0.001);
        println!(
            "PASS {} independent length = {expected} m (expert review: {})",
            case["id"], case["expert_review"]
        );
    }
}
