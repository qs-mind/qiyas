use std::sync::{Arc, Mutex};

use super::*;
use crate::provider::{BoxFuture, Provider};
use crate::{Agent, AgentEvent, AgentLog, AgentSettings};
use qiyas_core::{Session, Source};
use tokio_util::sync::CancellationToken;

fn completed(output: Value) -> Value {
    json!({"type":"response.completed","response":{"output":output,"usage":{"input_tokens":100,"output_tokens":23,"input_tokens_details":{"cached_tokens":20},"output_tokens_details":{"reasoning_tokens":7}}}})
}
fn call(id: &str, name: &str, input: Value) -> Vec<Value> {
    let args = input.to_string();
    let split = args
        .char_indices()
        .nth(args.chars().count() / 2)
        .map_or(0, |(index, _)| index);
    vec![
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":format!("rs-{id}"),"summary":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":format!("rs-{id}"),"encrypted_content":"fake-reasoning-signature","summary":[]}}),
        json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":format!("fc-{id}"),"call_id":id,"name":name,"arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":format!("fc-{id}"),"delta":&args[..split]}),
        json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":format!("fc-{id}"),"delta":&args[split..]}),
        json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":format!("fc-{id}"),"arguments":args}),
        json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":format!("fc-{id}"),"call_id":id,"name":name,"arguments":input.to_string()}}),
        // Deliberately partial: only reasoning is repeated, no function call.
        completed(json!([{"type":"reasoning","id":format!("rs-{id}"),"summary":[]}])),
    ]
}
fn text_reply(text: &str) -> Vec<Value> {
    let characters: Vec<_> = text.chars().collect();
    let mut events: Vec<_> = characters.chunks(5).map(|chunk| json!({"type":"response.output_text.delta","item_id":"msg","output_index":0,"content_index":0,"delta":chunk.iter().collect::<String>()})).collect();
    events.push(completed(json!([])));
    events
}
fn fake_tokens() -> Tokens {
    Tokens {
        access_token: "fake-access-secret".into(),
        refresh_token: Some("fake-refresh-secret".into()),
        id_token: "fake-id-secret".into(),
        scopes: vec![super::super::PLAN_SCOPE.into()],
        expires_at: super::super::oauth::now() + 3600,
        client_id: "fake-client".into(),
        subject: "fake-person".into(),
        display_name: "Test".into(),
    }
}

#[test]
fn snapshots_and_deltas_merge_without_duplicate_text_calls_or_reasoning() {
    let mut events = Events::default();
    let mut seen = Vec::new();
    let mut values = call("one", "qiyas_read", json!({"op":"state","args":"{}"}));
    values.pop();
    values.extend([
        json!({"type":"response.output_text.delta","item_id":"msg","output_index":2,"content_index":0,"delta":"First "}),
        json!({"type":"response.output_text.done","item_id":"msg","output_index":2,"content_index":0,"text":"First answer."}),
        json!({"type":"response.content_part.done","item_id":"msg","output_index":2,"content_index":1,"part":{"type":"output_text","text":"Second paragraph."}}),
        completed(json!([
            {"type":"message","id":"msg","content":[{"type":"output_text","text":"First answer."},{"type":"output_text","text":"Second paragraph."}]},
            {"type":"function_call","id":"fc-one","call_id":"one","name":"qiyas_read","arguments":""}
        ]))
    ]);
    for value in values {
        events.read(value, &mut |e| seen.push(e)).unwrap();
    }
    let result = events.result.unwrap();
    assert_eq!(result.content.len(), 4);
    assert_eq!(
        result
            .content
            .iter()
            .filter(|c| matches!(c, ContentBlock::ToolUse { .. }))
            .count(),
        1
    );
    assert_eq!(
        result
            .content
            .iter()
            .filter(|c| matches!(c, ContentBlock::Raw(_)))
            .count(),
        1
    );
    let text: String = seen
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "First answer.Second paragraph.");
    assert_eq!(
        seen.iter()
            .filter(|e| matches!(e, StreamEvent::ToolUseStart { .. }))
            .count(),
        1
    );
    assert_eq!(
        seen.iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .count(),
        1
    );
}

#[test]
fn done_only_items_and_arguments_only_items_are_retained() {
    for values in [
        vec![
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc","call_id":"c","name":"qiyas_read","arguments":"{\"op\":\"state\",\"args\":\"{}\"}"}}),
        ],
        vec![
            json!({"type":"response.function_call_arguments.delta","item_id":"fc","output_index":0,"delta":"{\"op\":\"state\",\"args\":\"{}\"}"}),
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc","call_id":"c","name":"qiyas_read"}}),
        ],
    ] {
        let mut events = Events::default();
        for value in values {
            events.read(value, &mut |_| {}).unwrap();
        }
        events.read(completed(json!([])), &mut |_| {}).unwrap();
        assert!(
            matches!(&events.result.unwrap().content[0],ContentBlock::ToolUse{input,..} if input["op"]=="state")
        );
    }
}

#[test]
fn reasoning_only_and_unknown_output_fail_with_item_types_and_usage_in_log_events() {
    for output in [
        json!([]),
        json!([{"type":"reasoning","id":"rs","encrypted_content":"private-reasoning"}]),
        json!([{"type":"unexpected_item"}]),
    ] {
        let mut events = Events::default();
        let mut seen = Vec::new();
        let error = events
            .read(completed(output), &mut |e| seen.push(e))
            .unwrap_err();
        assert_eq!(error.code(), "chatgpt_empty");
        assert!(events.result.is_none());
        assert!(!seen.contains(&StreamEvent::Done));
        assert!(seen.iter().any(|e| matches!(e,StreamEvent::Diagnostic{kind,text} if kind=="EMPTY ANSWER" && text.contains("23 output tokens"))));
        assert!(!format!("{seen:?}").contains("private-reasoning"));
    }
}

#[test]
fn interrupted_streams_never_commit_calls() {
    let mut values = call("one", "qiyas_read", json!({"op":"state","args":"{}"}));
    values.pop();
    let mut events = Events::default();
    for value in values {
        events.read(value, &mut |_| {}).unwrap();
    }
    assert!(events.result.is_none());
    assert_eq!(
        events
            .read(json!({"type":"response.incomplete"}), &mut |_| {})
            .unwrap_err()
            .code(),
        "chatgpt_incomplete"
    );
    assert!(events.result.is_none());
}

struct Replay {
    values: Vec<Vec<Value>>,
    requests: Arc<Mutex<Vec<Value>>>,
    endpoint: Option<String>,
}
impl Provider for Replay {
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let body = request(&req)?;
            let n = {
                let mut requests = self.requests.lock().unwrap();
                let n = requests.len();
                requests.push(body);
                n
            };
            let tokens = fake_tokens();
            if let Some(endpoint) = &self.endpoint {
                return stream(endpoint, &tokens, req, &Mutex::default(), sink).await;
            }
            sink(StreamEvent::RequestSent(format!(
                "scripted Responses; {} input items",
                self.requests.lock().unwrap()[n]["input"]
                    .as_array()
                    .unwrap()
                    .len()
            )));
            let mut events = Events::with_tokens(&tokens);
            for value in &self.values[n.min(self.values.len() - 1)] {
                events.read(
                    serde_json::from_str(&tokens.scrub(&value.to_string())).unwrap(),
                    sink,
                )?;
            }
            events
                .result
                .ok_or(AiError::Subscription("chatgpt_incomplete"))
        })
    }
}

async fn exercise(endpoint: Option<String>, units: i16, factor: f64) {
    let dir = tempfile::tempdir().unwrap();
    let evidence = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE")
        .map(std::path::PathBuf::from)
        .map(|p| {
            p.join(format!(
                "chatgpt-{}-{units}",
                if endpoint.is_some() { "http" } else { "stream" }
            ))
        })
        .unwrap_or_else(|| dir.path().to_path_buf());
    std::fs::create_dir_all(&evidence).unwrap();
    let path = evidence.join("drawing.dxf");
    let session = Arc::new(Mutex::new(Some(Session::new_blank(&path, units))));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let values = vec![
        call(
            "edit",
            "qiyas_execute",
            json!({"op":"run","args":json!({"layer":"Provider check","command":{"type":"draw","kind":"line","a":[0,0],"b":[4.0/factor,0]}}).to_string()}),
        ),
        call(
            "read-back",
            "qiyas_read",
            json!({"op":"records","args":"{\"collection\":\"entities\",\"fields\":[\"length\"]}"}),
        ),
        text_reply("The line is ready. fake-access-secret fake-refresh-secret fake-id-secret"),
        text_reply("The earlier line is still here."),
    ];
    let log = AgentLog::evaluation(evidence.join("agent.log"));
    let mut agent = Agent::new(
        Box::new(Replay {
            values: values.clone(),
            requests: requests.clone(),
            endpoint,
        }),
        session.clone(),
        AgentSettings {
            model: "mock-model".into(),
            effort: "none".into(),
            ..AgentSettings::default()
        },
    )
    .with_log(log.clone());
    let (tx, mut rx) = tokio::sync::mpsc::channel(1000);
    let result = agent
        .run_turn(
            "Draw a line four metres long and read it back.".into(),
            tx,
            CancellationToken::new(),
        )
        .await;
    let mut seen = Vec::new();
    while let Ok(e) = rx.try_recv() {
        seen.push(e);
    }
    std::fs::write(evidence.join("transcript.json"),serde_json::to_vec_pretty(&json!({"requests":*requests.lock().unwrap(),"events":seen,"error":result.as_ref().err().map(ToString::to_string)})).unwrap()).unwrap();
    // This also records a useful before transcript when the old provider drops the call.
    result.unwrap();
    assert!(seen
        .iter()
        .any(|e| matches!(e,AgentEvent::ToolResult{id,is_error:false,..} if id=="edit")));
    assert!(seen
        .iter()
        .any(|e| matches!(e,AgentEvent::ToolResult{id,is_error:false,..} if id=="read-back")));
    {
        let mut guard = session.lock().unwrap();
        let session = guard.as_mut().unwrap();
        assert_eq!(
            session
                .drawing
                .doc
                .entities_in_block("*Model_Space")
                .count(),
            1
        );
        let read = crate::surface::run_session(
            session,
            "qiyas_read",
            "records",
            &json!({"collection":"entities","fields":["length"]}),
            Source::User,
        )
        .unwrap();
        let crate::ToolOutput::Json(read) = read else {
            panic!("Expected JSON readback")
        };
        std::fs::write(
            evidence.join("readback.json"),
            serde_json::to_vec_pretty(&read).unwrap(),
        )
        .unwrap();
        let rows = read["records"].as_array().unwrap();
        assert!(
            (rows[0]["fields"]["length"].as_f64().unwrap() * factor - 4.0).abs() < 1e-8,
            "{read}"
        );
        session.save_as_headless(&path).unwrap();
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(1000);
    agent
        .run_turn(
            "What did you just draw?".into(),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    while let Ok(e) = rx.try_recv() {
        seen.push(e);
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let last = requests.last().unwrap()["input"].as_array().unwrap();
    assert!(last
        .iter()
        .any(|v| v["role"] == "assistant" && v.to_string().contains("The line is ready.")));
    assert!(last
        .iter()
        .any(|v| v["type"] == "function_call_output" && v["call_id"] == "read-back"));
    assert!(last
        .iter()
        .any(|v| v["type"] == "reasoning" && v["encrypted_content"] == "fake-reasoning-signature"));
    let log = std::fs::read_to_string(log.path()).unwrap();
    for expected in [
        "text: The line is ready.",
        "call qiyas_execute (edit)",
        "23 out (7 thinking)",
        "3 messages",
        "7 messages",
    ] {
        assert!(log.contains(expected), "Missing {expected}: {log}");
    }
    for secret in [
        "fake-access-secret",
        "fake-refresh-secret",
        "fake-id-secret",
        "fake-reasoning-signature",
    ] {
        assert!(!log.contains(secret));
    }
    std::fs::write(evidence.join("transcript.json"),serde_json::to_vec_pretty(&json!({"requests":*requests,"events":seen,"transcript":agent.snapshot().transcript,"model_streams":values.iter().map(|events|events.iter().map(|v|serde_json::from_str::<Value>(&fake_tokens().scrub(&v.to_string())).unwrap()).collect::<Vec<_>>()).collect::<Vec<_>>()})).unwrap()).unwrap();
}

#[tokio::test]
async fn streamed_responses_real_agent_edits_reads_back_retains_history_and_scrubs_log() {
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        exercise(None, units, factor).await;
    }
}

#[tokio::test]
async fn mock_http_responses_real_agent_edits_and_reads_back() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Mock Responses acceptance requires a loopback socket");
    let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
    let values = vec![
        call(
            "edit",
            "qiyas_execute",
            json!({"op":"run","args":"{\"layer\":\"Provider check\",\"command\":{\"type\":\"draw\",\"kind\":\"line\",\"a\":[0,0],\"b\":[4,0]}}"}),
        ),
        call(
            "read-back",
            "qiyas_read",
            json!({"op":"records","args":"{\"collection\":\"entities\",\"fields\":[\"length\"]}"}),
        ),
        text_reply("The line is ready. fake-access-secret fake-refresh-secret fake-id-secret"),
        text_reply("The earlier line is still here."),
    ];
    let server = tokio::spawn(async move {
        for events in values {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            loop {
                let mut buf = [0; 8192];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                received.extend_from_slice(&buf[..n]);
                if let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&received[..end]).to_lowercase();
                    let length: usize = header
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if received.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body: String = events.iter().map(|v| format!("data: {v}\n\n")).collect();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }
    });
    exercise(Some(endpoint), 6, 1.0).await;
    server.await.unwrap();
}

#[tokio::test]
async fn delta_only_answers_are_sent_in_the_next_turn_history() {
    let dir = tempfile::tempdir().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let values = vec![text_reply("First answer."), text_reply("Second answer.")];
    let mut agent = Agent::new(
        Box::new(Replay {
            values,
            requests: requests.clone(),
            endpoint: None,
        }),
        Arc::new(Mutex::new(None)),
        AgentSettings {
            model: "mock-model".into(),
            effort: "none".into(),
            ..AgentSettings::default()
        },
    )
    .with_log(AgentLog::new(dir.path().join("agent.log")));
    for message in ["Hello", "What did I ask?"] {
        let (tx, _rx) = tokio::sync::mpsc::channel(1000);
        agent
            .run_turn(message.into(), tx, CancellationToken::new())
            .await
            .unwrap();
    }
    let requests = requests.lock().unwrap();
    if let Some(root) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("chatgpt-history.json"),
            serde_json::to_vec_pretty(&*requests).unwrap(),
        )
        .unwrap();
        std::fs::copy(
            dir.path().join("agent.log"),
            root.join("chatgpt-history.log"),
        )
        .unwrap();
    }
    assert_eq!(requests.len(), 2);
    assert!(requests[1]["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["role"] == "assistant" && item.to_string().contains("First answer.")));
}

#[test]
fn finished_reasoning_replaces_opaque_initial_value_and_is_not_logged() {
    let mut events = Events::default();
    let mut seen = Vec::new();
    for event in [
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs","status":"in_progress","encrypted_content":"initial-opaque"}}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs","status":"completed","encrypted_content":"final-opaque"}}),
        json!({"type":"response.output_text.delta","output_index":1,"item_id":"msg","delta":"Ready."}),
        completed(json!([{"type":"reasoning","id":"rs","summary":[]}])),
    ] {
        events.read(event, &mut |e| seen.push(e)).unwrap();
    }
    assert!(
        matches!(&events.result.unwrap().content[0],ContentBlock::Raw(v) if v["encrypted_content"]=="final-opaque" && v["status"]=="completed")
    );
    let text = format!("{seen:?}");
    assert!(!text.contains("initial-opaque"));
    assert!(!text.contains("final-opaque"));
}

#[test]
fn invalid_stream_items_return_an_error_without_panicking() {
    for item in [Value::Null, json!({"type":"message","content":{}})] {
        assert_eq!(
            Events::default()
                .read(
                    json!({"type":"response.output_item.added","item":item}),
                    &mut |_| {}
                )
                .unwrap_err()
                .code(),
            "chatgpt_incomplete"
        );
    }
}

#[test]
fn refusal_credentials_split_between_deltas_are_redacted() {
    let tokens = fake_tokens();
    let mut events = Events::with_tokens(&tokens);
    for delta in [
        "Cannot repeat fake-access-",
        "secret or fake-refresh-",
        "secret.",
    ] {
        events
            .read(
                json!({"type":"response.refusal.delta","output_index":0,"item_id":"refusal","delta":delta}),
                &mut |_| {},
            )
            .unwrap();
    }
    events.read(completed(json!([])), &mut |_| {}).unwrap();
    let result = events.result.unwrap();
    let StopReason::Refusal { text, .. } = result.stop_reason else {
        panic!("Expected the refusal to be retained");
    };
    assert!(!text.contains(&tokens.access_token));
    assert!(!text.contains(tokens.refresh_token.as_ref().unwrap()));
    assert!(text.starts_with("Cannot repeat "));
}
