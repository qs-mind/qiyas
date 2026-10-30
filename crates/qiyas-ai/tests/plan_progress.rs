//! Plan structure and factual progress, with local scripted providers only.
use qiyas_ai::surface::{self, EXECUTE};
use qiyas_ai::{Agent, AgentEvent, AgentSettings};
use qiyas_core::{batch_progress::Step, Session, Source};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn frame() -> Value {
    json!({"expert_checks":"مسارات الهروب", "raw_data":"الجدران والفتحات", "methods":"قياس المساحات", "outside_check":"لا أبواب خارج حدود الموقع"})
}
fn plan() -> Value {
    json!({"frame":frame(), "phases":[{"id":"doors","title":"مراجعة الأبواب", "note":"مراجعة 60 بابًا ولوحتي A1", "status":"active"}, {"id":"check","title":"الفحص المستقل","status":"pending"}]})
}

#[test]
fn plan_frame_notes_and_limits_are_validated_without_changing_old_plans() {
    let value = plan();
    assert_eq!(
        serde_json::to_value(surface::read_plan(&value).unwrap()).unwrap(),
        value
    );
    let mut old = value.clone();
    old.as_object_mut().unwrap().remove("frame");
    old["phases"][0].as_object_mut().unwrap().remove("note");
    assert_eq!(
        serde_json::to_value(surface::read_plan(&old).unwrap()).unwrap(),
        old
    );
    for key in ["expert_checks", "raw_data", "methods", "outside_check"] {
        for bad in [json!(""), json!("  "), json!("ع".repeat(601)), json!(42)] {
            let mut input = value.clone();
            input["frame"][key] = bad;
            assert!(surface::read_plan(&input).is_err(), "{key}: {input}");
        }
        let mut missing = value.clone();
        missing["frame"].as_object_mut().unwrap().remove(key);
        assert!(surface::read_plan(&missing).is_err());
        let mut max = value.clone();
        max["frame"][key] = json!("ع".repeat(600));
        assert!(surface::read_plan(&max).is_ok());
    }
    let mut input = value;
    input["phases"][0]["note"] = json!("ع".repeat(600));
    input["phases"][0]["title"] = json!("ع".repeat(160));
    assert!(surface::read_plan(&input).is_ok());
    input["phases"][0]["note"] = json!("ع".repeat(601));
    assert!(surface::read_plan(&input).is_err());
    input["phases"][0]["note"] = json!("");
    input["phases"][0]["title"] = json!("ع".repeat(161));
    assert!(surface::read_plan(&input).is_err());
    input["phases"][0]["title"] = json!("Doors");
    input["phases"][1]["status"] = json!("active");
    let error = surface::read_plan(&input).unwrap_err().to_string();
    assert!(error.contains("doors, check") && error.contains("Example:"));
    input["phases"][0]["status"] = json!("done");
    assert!(surface::read_plan(&input).is_ok());
    input["phases"][1]["status"] = json!("pending");
    assert!(surface::read_plan(&input).is_ok());
}

#[test]
fn progress_throttle_orders_updates_at_200ms_and_keeps_the_final_total() {
    let mut throttle = qiyas_ai::progress::Throttle::default();
    let mut sent = Vec::new();
    for done in 1..=100 {
        let step = Step {
            done,
            total: 100,
            label: "draw.line".into(),
        };
        if throttle.accept(&step, Duration::from_millis((done * 10) as u64)) {
            sent.push(done);
        }
    }
    assert_eq!(sent, [1, 21, 41, 61, 81, 100]);
    let mut fast = qiyas_ai::progress::Throttle::default();
    assert!(fast.accept(
        &Step {
            done: 1,
            total: 10,
            label: "draw.line".into()
        },
        Duration::ZERO
    ));
    assert!(!fast.accept(
        &Step {
            done: 2,
            total: 10,
            label: "draw.line".into()
        },
        Duration::ZERO
    ));
    assert!(fast.accept(
        &Step {
            done: 10,
            total: 10,
            label: "draw.line".into()
        },
        Duration::ZERO
    ));
}

fn line(x: usize) -> Value {
    json!({"type":"draw","kind":"line","a":[x,0],"b":[x,1]})
}
fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

#[test]
fn progress_tracks_completed_steps_in_surface_raw_and_native_batches() {
    for op in ["batch", "run", "native"] {
        let (_dir, mut session) = session();
        let commands = json!([line(0), line(1), line(2)]);
        let args = if op == "batch" {
            json!({"commands":commands})
        } else {
            json!({"command":{"type":"batch","commands":commands}})
        };
        let steps = Arc::new(Mutex::new(Vec::new()));
        let observed = steps.clone();
        qiyas_core::batch_progress::observe(
            move |step| observed.lock().unwrap().push(step),
            || {
                if op == "native" {
                    session
                        .run_as(
                            serde_json::from_value(args["command"].clone()).unwrap(),
                            Source::Ai,
                        )
                        .unwrap();
                } else {
                    surface::run_session(&mut session, EXECUTE, op, &args, Source::Ai).unwrap();
                }
            },
        );
        let steps = steps.lock().unwrap();
        assert_eq!(
            steps
                .iter()
                .map(|s| (s.done, s.total, s.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (1, 3, "draw.line"),
                (2, 3, "draw.line"),
                (3, 3, "draw.line")
            ]
        );
        assert_eq!(session.history_steps().0.len(), 1);
        assert!(session.undo());
    }
}

#[test]
fn failed_steps_do_not_report_completion_and_the_observer_is_scoped() {
    let (_dir, mut session) = session();
    let steps = Arc::new(Mutex::new(Vec::new()));
    let observed = steps.clone();
    let before = session.document_revision();
    let result = qiyas_core::batch_progress::observe(
        move |step| observed.lock().unwrap().push(step),
        || {
            surface::run_session(
                &mut session,
                EXECUTE,
                "batch",
                &json!({"commands":[line(0),{"type":"delete_entities","targets":[{"handle":99999999}]}]}),
                Source::Ai,
            )
        },
    );
    assert!(result.is_err());
    assert_eq!(session.document_revision(), before);
    assert!(!session.can_undo());
    assert_eq!(
        steps
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.done)
            .collect::<Vec<_>>(),
        [1]
    );
    surface::run_session(
        &mut session,
        EXECUTE,
        "batch",
        &json!({"commands":[line(1),line(2)]}),
        Source::Ai,
    )
    .unwrap();
    assert_eq!(steps.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn plan_and_progress_events_reach_the_transcript_in_execution_order() {
    let (_dir, session) = session();
    let input = plan();
    let provider = crate::scripted::Script::new(
        vec![
            (EXECUTE, json!({"op":"plan", "args":input})),
            (
                EXECUTE,
                json!({"op":"batch", "args":{"commands":(0..10).map(line).collect::<Vec<_>>()}}),
            ),
        ],
        Arc::default(),
    );
    let mut agent = Agent::new(
        Box::new(provider),
        Arc::new(Mutex::new(Some(session))),
        AgentSettings::default(),
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(1000);
    tokio::time::timeout(
        Duration::from_secs(60),
        agent.run_turn("Draw lines".into(), tx, CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    let plan = events
        .iter()
        .find(|event| matches!(event, AgentEvent::Plan { .. }))
        .unwrap();
    let wire = serde_json::to_value(plan).unwrap();
    assert_eq!(wire["frame"], input["frame"]);
    assert_eq!(wire["phases"], input["phases"]);
    let start = events
        .iter()
        .position(|event| matches!(event, AgentEvent::ToolCall { id, .. } if id == "call_1"))
        .unwrap();
    let end = events.iter().position(|event| matches!(event, AgentEvent::ToolResult { id, is_error:false, .. } if id == "call_1")).unwrap();
    let steps: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(i, event)| match event {
            AgentEvent::ToolProgress {
                id,
                done,
                total,
                label,
            } => {
                assert_eq!(id, "call_1");
                assert!(i > start && i < end);
                assert_eq!(*total, 10);
                assert_eq!(label, "draw.line");
                Some(*done)
            }
            _ => None,
        })
        .collect();
    assert_eq!(steps.first(), Some(&1));
    assert_eq!(steps.last(), Some(&10));
    assert!(steps.windows(2).all(|pair| pair[0] < pair[1]));
    let snapshot = agent.snapshot();
    let restored = Agent::resume(
        snapshot,
        Box::new(crate::scripted::Script::new(vec![], Arc::default())),
        Arc::new(Mutex::new(None)),
        AgentSettings::default(),
    );
    let restored_wire = serde_json::to_value(restored.snapshot()).unwrap();
    assert!(restored_wire["transcript"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["event"]["frame"] == input["frame"]));
    // The serialized plan event is also backward compatible, independently of provider state.
    let _: AgentEvent =
        serde_json::from_value(json!({"type":"plan","phases":input["phases"]})).unwrap();
}

#[test]
fn prompts_separate_the_frame_and_require_self_repairs_without_a_finish_gate() {
    for (prompt, repair) in [
        (
            include_str!("../src/prompt_en.txt"),
            "yours to fix and re-check before finishing",
        ),
        (
            include_str!("../src/prompt_ar.txt"),
            "تصلحه وتعيد فحصه قبل الانتهاء",
        ),
    ] {
        for field in [
            "frame",
            "expert_checks",
            "raw_data",
            "methods",
            "outside_check",
            "note",
        ] {
            assert!(prompt.contains(field));
        }
        assert!(prompt.contains(repair));
    }
}

#[test]
fn private_mcp_progress_waits_for_its_call_and_never_attaches_to_a_finished_card() {
    let mut bridge = qiyas_ai::progress::Bridge::default();
    let args = json!({"commands":[line(0),line(1)]});
    let key = json!({"name":EXECUTE,"op":"batch","args":args}).to_string();
    let step = Step {
        done: 1,
        total: 2,
        label: "draw.line".into(),
    };
    assert!(bridge.step(key.clone(), step.clone()).is_none());
    let call = AgentEvent::ToolCall {
        id: "cli-call".into(),
        name: EXECUTE.into(),
        input: json!({"op":"batch","args":args.to_string()}),
        label: None,
        activity: None,
        mutating: true,
    };
    let buffered = bridge.event(&call);
    assert!(
        matches!(buffered.as_slice(), [AgentEvent::ToolProgress { id,done:1,total:2,label }] if id == "cli-call" && label == "draw.line")
    );
    assert!(matches!(
        bridge.step(
            key.clone(),
            Step {
                done: 2,
                ..step.clone()
            }
        ),
        Some(AgentEvent::ToolProgress { done: 2, .. })
    ));
    bridge.event(&AgentEvent::ToolResult {
        id: "cli-call".into(),
        activity: None,
        summary: String::new(),
        count: None,
        handles: Vec::new(),
        ui: None,
        is_error: false,
    });
    assert!(bridge.step(key, step).is_none());
    assert!(bridge.event(&call).is_empty());
}

#[tokio::test]
async fn continue_tells_mizan_to_repair_and_recheck_its_own_faults_in_both_languages() {
    for (message, expected) in [
        ("Continue", "yours to fix and re-check before finishing"),
        ("أكمل", "تصلحه وتعيد فحصه قبل الانتهاء"),
    ] {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let provider = crate::scripted::Script::new(vec![], requests.clone());
        let mut agent = Agent::new(
            Box::new(provider),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(1000);
        tokio::time::timeout(
            Duration::from_secs(60),
            agent.run_turn(message.into(), tx, CancellationToken::new()),
        )
        .await
        .unwrap()
        .unwrap();
        let requests = requests.lock().unwrap();
        let text = serde_json::to_string(&requests[0].messages).unwrap();
        assert!(
            text.contains("[continue]") && text.contains(expected),
            "{text}"
        );
    }
}

#[test]
fn grouped_openings_report_every_completed_input_and_keep_one_undo_step() {
    for op in ["batch", "run"] {
        let (_dir, mut session) = session();
        session.run_input(&json!({"type":"walls","runs":[{"points":[[30000,0],[36000,0]]},{"points":[[36000,0],[36000,6000]]}],"thickness":200}), Source::Ai).unwrap();
        let group = session.wall_groups(None).unwrap().pop().unwrap();
        let commands: Vec<_> = group.runs.iter().map(|run| json!({"type":"add_wall_openings","centreline":run.centreline,"openings":[{"at":2000,"width":900,"kind":"door"}]})).collect();
        let args = if op == "batch" {
            json!({"commands":commands})
        } else {
            json!({"command":{"type":"batch","commands":commands}})
        };
        let steps = Arc::new(Mutex::new(Vec::new()));
        let observed = steps.clone();
        qiyas_core::batch_progress::observe(
            move |step| observed.lock().unwrap().push(step),
            || {
                surface::run_session(&mut session, EXECUTE, op, &args, Source::Ai).unwrap();
            },
        );
        assert_eq!(
            steps
                .lock()
                .unwrap()
                .iter()
                .map(|s| (s.done, s.total, s.label.clone()))
                .collect::<Vec<_>>(),
            [
                (1, 2, "add_wall_openings".into()),
                (2, 2, "add_wall_openings".into())
            ]
        );
        assert_eq!(session.history_steps().0.len(), 2);
        assert!(session.undo());
        assert!(session
            .wall_groups(None)
            .unwrap()
            .iter()
            .flat_map(|g| &g.runs)
            .all(|run| run.openings.is_empty()));
    }
}

#[test]
fn rejected_nested_batches_never_restart_or_complete_the_outer_counter() {
    let (_dir, mut session) = session();
    let steps = Arc::new(Mutex::new(Vec::new()));
    let observed = steps.clone();
    let result = qiyas_core::batch_progress::observe(
        move |step| observed.lock().unwrap().push(step),
        || {
            surface::run_session(
                &mut session,
                EXECUTE,
                "batch",
                &json!({"commands":[line(0),{"type":"batch","commands":[line(1),line(2)]}]}),
                Source::Ai,
            )
        },
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("inside another batch"));
    assert_eq!(
        steps
            .lock()
            .unwrap()
            .iter()
            .map(|s| (s.done, s.total))
            .collect::<Vec<_>>(),
        [(1, 2)]
    );
    assert!(!session.can_undo());
}

#[tokio::test]
async fn mcp_delivers_real_progress_notifications_over_an_in_memory_transport() {
    use rmcp::model::{CallToolRequestParams, ClientRequest, ProgressNotificationParam, Request};
    use rmcp::{ClientHandler, ServiceExt};
    struct Client(tokio::sync::mpsc::UnboundedSender<ProgressNotificationParam>);
    impl ClientHandler for Client {
        async fn on_progress(
            &self,
            params: ProgressNotificationParam,
            _: rmcp::service::NotificationContext<rmcp::RoleClient>,
        ) {
            self.0.send(params).unwrap();
        }
    }
    tokio::time::timeout(Duration::from_secs(60), async {
        let (_dir, session) = session();
        let server = qiyas_ai::mcp::Qiyas::new(
            Arc::new(Mutex::new(Some(session))),
            None,
            Arc::new(|| {}),
            Arc::new(|_| {}),
        );
        let (server_pipe, client_pipe) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            server
                .serve(server_pipe)
                .await
                .unwrap()
                .waiting()
                .await
                .unwrap();
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let client = Client(tx).serve(client_pipe).await.unwrap();
        let args = json!({"op":"batch", "args":{"commands":(0..10).map(line).collect::<Vec<_>>()}});
        let handle = client
            .send_cancellable_request(
                ClientRequest::CallToolRequest(Request::new(
                    CallToolRequestParams::new(EXECUTE)
                        .with_arguments(args.as_object().unwrap().clone()),
                )),
                rmcp::service::PeerRequestOptions::no_options(),
            )
            .await
            .unwrap();
        let token = handle.progress_token.clone();
        let mut completed = Vec::new();
        loop {
            let progress = rx.recv().await.unwrap();
            assert_eq!(progress.progress_token, token);
            assert_eq!(progress.total, Some(10.0));
            assert_eq!(progress.message.as_deref(), Some("draw.line"));
            completed.push(progress.progress);
            if progress.progress == 10.0 {
                break;
            }
        }
        handle.await_response().await.unwrap();
        assert_eq!(completed[0], 1.0);
        assert!(completed.windows(2).all(|pair| pair[0] < pair[1]));
        client.cancel().await.unwrap();
        server_task.await.unwrap();
    })
    .await
    .unwrap();
}
