//! The agent loop and both providers against a local mock HTTP server that serves canned
//! Server-Sent Events. No real API is called.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use qiyas_ai::anthropic::Anthropic;
use qiyas_ai::openai::OpenAiCompatible;
use qiyas_ai::{Agent, AgentEvent, AgentSettings, AiError, Provider, StreamEvent, TurnRequest};
use qiyas_core::Session;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const KEY: &str = "sk-ant-test-SECRET-0123456789";

// ---------------------------------------------------------------------------------------------
// The mock server

/// What the mock answers one request with.
enum Reply {
    /// 200 with these SSE events, each written separately with a short pause.
    Sse(Vec<Value>),
    /// 200 with these raw `data:` payloads (OpenAI style, ends with `[DONE]`).
    Data(Vec<String>),
    /// A status, extra headers and a body.
    Status(u16, Vec<(&'static str, &'static str)>, String),
    /// 200, a text block that gets one `text_delta` every 200 ms until the client hangs up.
    Drip,
}

#[derive(Debug, Clone)]
struct Request {
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

struct Mock {
    base: String,
    requests: Arc<Mutex<Vec<Request>>>,
    /// Set when a dripping reply finds the client gone.
    hung_up: Arc<AtomicBool>,
}

impl Mock {
    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn anthropic(&self) -> Anthropic {
        Anthropic::with_url(KEY.to_string(), format!("{}/v1/messages", self.base))
    }
}

/// Starts a server on a free local port; `reply(n)` answers the n-th request (0-based).
async fn serve(reply: impl Fn(usize) -> Reply + Send + Sync + 'static) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hung_up = Arc::new(AtomicBool::new(false));
    let reply = Arc::new(reply);
    let (reqs, hung) = (requests.clone(), hung_up.clone());
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (reqs, hung, reply) = (reqs.clone(), hung.clone(), reply.clone());
            tokio::spawn(async move {
                let Some(request) = read_request(&stream).await else {
                    return;
                };
                let n = {
                    let mut r = reqs.lock().unwrap();
                    r.push(request);
                    r.len() - 1
                };
                if write_reply(stream, reply(n)).await.is_err() {
                    hung.store(true, Ordering::SeqCst);
                }
            });
        }
    });
    Mock {
        base,
        requests,
        hung_up,
    }
}

async fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        stream.readable().await.ok()?;
        match stream.try_read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => return None,
        }
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let path = lines.next()?.split(' ').nth(1)?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        stream.readable().await.ok()?;
        match stream.try_read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => return None,
        }
    }
    Some(Request {
        path,
        headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    })
}

const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                        cache-control: no-cache\r\nconnection: close\r\n\r\n";

async fn write_reply(mut stream: TcpStream, reply: Reply) -> std::io::Result<()> {
    match reply {
        Reply::Sse(events) => {
            stream.write_all(SSE_HEAD.as_bytes()).await?;
            for e in events {
                let name = e["type"].as_str().unwrap_or("").to_string();
                let frame = format!("event: {name}\ndata: {e}\n\n");
                stream.write_all(frame.as_bytes()).await?;
                stream.flush().await?;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        Reply::Data(lines) => {
            stream.write_all(SSE_HEAD.as_bytes()).await?;
            for d in lines {
                stream
                    .write_all(format!("data: {d}\n\n").as_bytes())
                    .await?;
                stream.flush().await?;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        Reply::Status(status, headers, body) => {
            let mut head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n",
                body.len()
            );
            for (n, v) in headers {
                head.push_str(&format!("{n}: {v}\r\n"));
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes()).await?;
            stream.write_all(body.as_bytes()).await?;
        }
        Reply::Drip => {
            stream.write_all(SSE_HEAD.as_bytes()).await?;
            for e in [
                message_start(),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            ] {
                stream
                    .write_all(format!("data: {e}\n\n").as_bytes())
                    .await?;
            }
            loop {
                let e = json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"la "}});
                stream
                    .write_all(format!("event: content_block_delta\ndata: {e}\n\n").as_bytes())
                    .await?;
                stream.flush().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    stream.shutdown().await
}

// ---------------------------------------------------------------------------------------------
// Canned Claude answers

fn message_start() -> Value {
    json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant",
        "model":"claude-opus-5-5","content":[],"stop_reason":null,
        "usage":{"input_tokens":10,"output_tokens":1}}})
}

fn stop(reason: &str) -> Vec<Value> {
    vec![
        json!({"type":"message_delta","delta":{"stop_reason":reason},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ]
}

/// A text answer streamed in `parts`, ending the turn.
fn text_turn(parts: &[&str]) -> Reply {
    let mut e = vec![
        message_start(),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"ping"}),
    ];
    for p in parts {
        e.push(
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":p}}),
        );
    }
    e.push(json!({"type":"content_block_stop","index":0}));
    e.extend(stop("end_turn"));
    Reply::Sse(e)
}

/// One `tool_use` block per call `(id, name, input fragments)`, stopping for tool use.
fn tool_turn(calls: &[(&str, &str, &[&str])]) -> Reply {
    let mut e = vec![message_start()];
    for (i, (id, name, fragments)) in calls.iter().enumerate() {
        e.push(json!({"type":"content_block_start","index":i,
            "content_block":{"type":"tool_use","id":id,"name":name,"input":{}}}));
        for f in *fragments {
            e.push(json!({"type":"content_block_delta","index":i,
                "delta":{"type":"input_json_delta","partial_json":f}}));
        }
        e.push(json!({"type":"content_block_stop","index":i}));
    }
    e.extend(stop("tool_use"));
    Reply::Sse(e)
}

// ---------------------------------------------------------------------------------------------
// Helpers

fn sample() -> (tempfile::TempDir, Arc<Mutex<Option<Session>>>) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, Arc::new(Mutex::new(Some(session))))
}

/// Runs one user turn and returns its result and every event it sent.
async fn turn(agent: &mut Agent, text: &str) -> (Result<(), AiError>, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(10_000);
    let result = agent
        .run_turn(text.to_string(), tx, CancellationToken::new())
        .await;
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (result, events)
}

fn texts(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Tests

#[tokio::test]
async fn text_stream_is_forwarded_in_order() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| text_turn(&["Hel", "lo", ", wor", "ld"])).await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );

        let (result, events) = turn(&mut agent, "Hi").await;
        result.unwrap();
        assert_eq!(texts(&events), ["Hel", "lo", ", wor", "ld"]);
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "{events:?}"
        );

        // The request is the one the API expects.
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 1);
        let r = &reqs[0];
        assert_eq!(r.path, "/v1/messages");
        assert_eq!(r.header("x-api-key"), Some(KEY));
        assert_eq!(r.header("anthropic-version"), Some("2023-06-01"));
        assert_eq!(
            r.header("anthropic-beta"),
            Some("server-side-fallback-2026-07-01")
        );
        let b = &r.body;
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["max_tokens"], 16000);
        assert_eq!(b["stream"], true);
        assert_eq!(b["output_config"], json!({"effort": "high"}));
        assert_eq!(b["fallbacks"], "default");
        assert!(b.get("thinking").is_none());
        assert!(b.get("tool_choice").is_none());
        assert_eq!(b["system"][0]["type"], "text");
        assert_eq!(
            b["system"][0]["cache_control"],
            json!({"type": "ephemeral", "ttl": "1h"})
        );
        assert!(b["system"][0]["text"].as_str().unwrap().contains("Qiyas"));
        let tools = b["tools"].as_array().unwrap();
        assert!(tools.iter().all(|t| t["strict"] == true));
        assert_eq!(
            tools.last().unwrap()["cache_control"],
            json!({"type": "ephemeral", "ttl": "1h"})
        );
        assert!(tools[..tools.len() - 1]
            .iter()
            .all(|t| t.get("cache_control").is_none()));
        assert_eq!(b["cache_control"], json!({"type": "ephemeral"}));
        // One user message: the [state] block, then what the user typed.
        let messages = b["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        let content = messages[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert!(content[0]["text"].as_str().unwrap().starts_with("[state]"));
        assert_eq!(content[1], json!({"type": "text", "text": "Hi"}));

        // The answer is kept for the next turn.
        let h = agent.history();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].content.len(), 2);
        assert_eq!(
            h[1].content,
            vec![qiyas_ai::ContentBlock::Text {
                text: "Hello, world".into()
            }]
        );
    })
    .await
    .expect("test text_stream_is_forwarded_in_order timed out after 60 seconds");
}

#[tokio::test]
async fn tool_use_round_trip_executes_against_the_session() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            // Two calls in one answer: the input of the first arrives in pieces.
            0 => tool_turn(&[
                (
                    "toolu_1",
                    "qiyas_execute",
                    &[r#"{"op":"run","args":{"command":{"type":"measure_area","points": [[0, 0], [10"#, r#", 0], [10, 10], [0, 10]]}}}"#],
                ),
                ("toolu_2", "qiyas_read", &[r#"{"op":"query","args":"{\"kind\":\"layers\"}"}"#]),
            ]),
            _ => text_turn(&["Measured 100 m2."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session.clone(),
            AgentSettings::default(),
        );

        let (result, events) = turn(&mut agent, "Measure the slab").await;
        result.unwrap();

        // The session gained one measurement, made by the AI.
        {
            let guard = session.lock().unwrap();
            let items = &guard.as_ref().unwrap().takeoff.items;
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].source, "ai");
        }

        // Events: the call, then its result, for each tool, then the closing text.
        let calls: Vec<(&str, bool)> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolCall { name, mutating, .. } => Some((name.as_str(), *mutating)),
                _ => None,
            })
            .collect();
        assert_eq!(calls, [("qiyas_execute", true), ("qiyas_read", false)]);
        let first_call = events
            .iter()
            .position(|e| matches!(e, AgentEvent::ToolCall { id, .. } if id == "toolu_1"))
            .unwrap();
        match &events[first_call + 1] {
            AgentEvent::ToolResult {
                id,
                is_error,
                summary,
                ..
            } => {
                assert_eq!(id, "toolu_1");
                assert!(!is_error, "{summary}");
            }
            other => panic!("expected the tool result, got {other:?}"),
        }
        assert_eq!(texts(&events), ["Measured 100 m2."]);

        // The second request sends both results back in ONE user message, after the assistant's
        // tool calls kept unchanged.
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 2);
        let messages = reqs[1].body["messages"].as_array().unwrap().clone();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(
            messages[1]["content"][0]["input"],
            json!({"op":"run","args":{"command":{"type":"measure_area","points": [[0, 0], [10, 0], [10, 10], [0, 10]]}}})
        );
        assert_eq!(messages[2]["role"], "user");
        // Goal D: a take-off turn also receives its job guidance once, after the results.
        let results: Vec<_> = messages[2]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "tool_result")
            .cloned()
            .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["type"], "tool_result");
        assert_eq!(results[0]["tool_use_id"], "toolu_1");
        assert_eq!(results[1]["tool_use_id"], "toolu_2");
        assert!(results.iter().all(|r| r.get("is_error").is_none()));
    })
    .await
    .expect("test tool_use_round_trip_executes_against_the_session timed out after 60 seconds");
}

#[tokio::test]
async fn invalid_tool_json_is_answered_with_an_error_and_not_run() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => tool_turn(&[("toolu_1", "qiyas_execute", &[r#"{"points": [[0, "#])]),
            _ => text_turn(&["Sorry."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session.clone(),
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "Measure").await;
        result.unwrap();
        assert!(session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .takeoff
            .items
            .is_empty());
        assert!(
            events.iter().any(|e| matches!(e,
            AgentEvent::ToolResult { is_error: true, summary, .. } if summary == "invalid JSON input"))
        );
        let body = &mock.requests()[1].body;
        let result = &body["messages"][2]["content"][0];
        assert_eq!(result["is_error"], true);
        let error: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(error["message"], "invalid JSON input");
        assert_eq!(error["code"], "invalid_value");
        // The broken input goes back as an empty object, so the history stays valid.
        assert_eq!(body["messages"][1]["content"][0]["input"], json!({}));
    })
    .await
    .expect("test invalid_tool_json_is_answered_with_an_error_and_not_run timed out after 60 seconds");
}

#[tokio::test]
async fn refusal_stop_reason_surfaces_category() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| {
            Reply::Sse(vec![
                message_start(),
                json!({"type":"message_delta",
                    "delta":{"stop_reason":"refusal","stop_details":{"type":"refusal",
                        "category":"cyber","explanation":"This looks like malware work."}},
                    "usage":{"output_tokens":0}}),
                json!({"type":"message_stop"}),
            ])
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "Hi").await;
        result.unwrap();
        assert!(
            events.iter().any(|e| matches!(e,
                AgentEvent::Refused { category: Some(c), text }
                    if c == "cyber" && text == "This looks like malware work.")),
            "{events:?}"
        );
        // Nothing of the declined answer is kept; the user's message stays, last.
        assert_eq!(agent.history().len(), 1);
    })
    .await
    .expect("test refusal_stop_reason_surfaces_category timed out after 60 seconds");
}

#[tokio::test]
async fn successful_reads_continue_beyond_40_calls() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| {
            if n >= 45 {
                return text_turn(&["Done."]);
            }
            let id = format!("toolu_{n}");
            let id: &'static str = Box::leak(id.into_boxed_str());
            tool_turn(&[(
                id,
                "qiyas_read",
                &[r#"{"op":"query","args":{"kind":"layers"}}"#],
            )])
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "Loop forever").await;
        result.unwrap();

        let run = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    AgentEvent::ToolResult {
                        is_error: false,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(run, 45);
        assert_eq!(mock.requests().len(), 46);
        assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
        assert!(serde_json::to_string(agent.history())
            .unwrap()
            .contains("tool_cost"));
    })
    .await
    .expect("test successful_reads_continue_beyond_40_calls timed out after 60 seconds");
}

#[tokio::test]
async fn identical_malformed_input_stops_on_sixth_attempt() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| {
            let id: &'static str = Box::leak(format!("toolu_{n}").into_boxed_str());
            tool_turn(&[(id, "qiyas_execute", &[r#"{"points": [[0, "#])])
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "Loop").await;
        result.unwrap();
        let answered = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. }))
            .count();
        assert_eq!(answered, 6);
        assert_eq!(mock.requests().len(), 6);
        assert_eq!(
            events.last(),
            Some(&AgentEvent::Stopped {
                reason: "repeated_ineffective_edit".into()
            })
        );
    })
    .await
    .expect("test identical_malformed_input_stops_on_sixth_attempt timed out after 60 seconds");
}

/// Cache-control markers are transport metadata; every content byte, including old guidance,
/// must stay unchanged between trims.
fn serialised(body: &Value) -> Vec<String> {
    fn without_markers(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("cache_control");
                for value in map.values_mut() {
                    without_markers(value);
                }
            }
            Value::Array(values) => values.iter_mut().for_each(without_markers),
            _ => {}
        }
    }
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| {
            let mut message = message.clone();
            without_markers(&mut message);
            message.to_string()
        })
        .collect()
}

#[tokio::test]
async fn history_is_append_only_and_system_prompt_is_stable() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => tool_turn(&[(
                "toolu_1",
                "qiyas_execute",
                &[r#"{"op":"run","args":{"command":{"type":"measure_area","points": [[0, 0], [10, 0], [10, 10], [0, 10]]}}}"#],
            )]),
            _ => text_turn(&["Done."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        turn(&mut agent, "Measure the slab").await.0.unwrap();
        turn(&mut agent, "And now?").await.0.unwrap();

        let reqs = mock.requests();
        assert_eq!(reqs.len(), 3);
        // The take-off changed between the turns, yet the system prompt and tools did not.
        let system = |r: &Request| r.body["system"].to_string();
        assert_eq!(system(&reqs[0]), system(&reqs[2]));
        assert_eq!(system(&reqs[1]), system(&reqs[2]));
        assert_eq!(reqs[0].body["tools"], reqs[2].body["tools"]);
        assert!(
            !system(&reqs[2]).contains("m2"),
            "no live take-off in the system prompt"
        );

        // Evidence, user messages and old job guidance only append.
        for pair in reqs.windows(2) {
            let (before, after) = (serialised(&pair[0].body), serialised(&pair[1].body));
            assert!(after.len() > before.len());
            assert_eq!(
                after[..before.len()],
                before[..],
                "an earlier message changed"
            );
        }

        // The second user message starts with the state block, which now shows the measurement.
        let messages = reqs[2].body["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last["role"], "user");
        let state = last["content"][0]["text"].as_str().unwrap();
        assert!(state.starts_with("[state]"), "{state}");
        assert!(
            state.contains("- area: 1 measurement, total 100.000 m2"),
            "{state}"
        );
        assert_eq!(last["content"][1]["text"], "And now?");
        let first_state = messages[0]["content"][0]["text"].as_str().unwrap();
        assert!(first_state.contains("no measurements yet"), "{first_state}");
    })
    .await
    .expect("test history_is_append_only_and_system_prompt_is_stable timed out after 60 seconds");
}

#[tokio::test]
async fn a_question_ends_the_turn_cleanly_and_the_answer_carries_on() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let question = "Which one: the wall on A-WALL, or the arc on A-WALL-CURVE?";
        let mock = serve(move |n| match n {
            0 => text_turn(&[question]),
            1 => tool_turn(&[(
                "toolu_1",
                "qiyas_read",
                &[r#"{"op":"query","args":{"kind":"layers"}}"#],
            )]),
            _ => text_turn(&["The arc is 6.28 m."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "How long is the curved wall?").await;
        result.unwrap();
        // The turn is over after the question: one request, no tool run, a Done at the end.
        assert_eq!(mock.requests().len(), 1);
        assert_eq!(texts(&events).join(""), question);
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "{events:?}"
        );
        assert!(!events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolCall { .. })));

        // The engineer's answer is the next message; the question stays in the history before it.
        let (result, _events) = turn(&mut agent, "The arc.").await;
        result.unwrap();
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 3);
        let messages = reqs[1].body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3, "question, then the answer");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["text"], question);
        assert_eq!(
            messages[2]["content"].as_array().unwrap().last().unwrap()["text"],
            "The arc."
        );
    })
    .await
    .expect("test a_question_ends_the_turn_cleanly_and_the_answer_carries_on timed out after 60 seconds");
}

#[tokio::test]
async fn an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room() {
    tokio::time::timeout(Duration::from_secs(60), async {
        // Gemini through Runware once thought for 94 s, wrote nothing and hit the limit: the turn
        // must not stop there.
        let mock = serve(|n| match n {
            0 => {
                let mut e = vec![message_start()];
                e.extend(stop("max_tokens"));
                Reply::Sse(e)
            }
            _ => text_turn(&["Done."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "Draw the villa.").await;
        result.unwrap();
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 2, "asked again");
        assert_eq!(reqs[0].body["max_tokens"], 16_000);
        assert_eq!(reqs[1].body["max_tokens"], 32_000, "with twice the room");
        assert_eq!(
            reqs[1].body["messages"].as_array().unwrap().len(),
            1,
            "the empty answer is dropped"
        );
        assert_eq!(texts(&events).join(""), "Done.");
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "{events:?}"
        );
    })
    .await
    .expect("test an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room timed out after 60 seconds");
}

#[tokio::test]
async fn an_answer_that_keeps_running_out_stops_at_the_cap() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| {
            let mut e = vec![message_start()];
            e.extend(stop("max_tokens"));
            Reply::Sse(e)
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (_result, events) = turn(&mut agent, "Draw the villa.").await;
        let sizes: Vec<_> = mock
            .requests()
            .iter()
            .map(|r| r.body["max_tokens"].clone())
            .collect();
        assert_eq!(sizes, [16_000, 32_000, 64_000]);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::Stopped { reason } if reason == "max_tokens")),
            "{events:?}"
        );
    })
    .await
    .expect("test an_answer_that_keeps_running_out_stops_at_the_cap timed out after 60 seconds");
}

#[tokio::test]
async fn cancel_stops_within_one_second() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| Reply::Drip).await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (tx, mut rx) = mpsc::channel(10_000);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let cancelled_at = Arc::new(Mutex::new(None));
        let at = cancelled_at.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(700)).await;
            *at.lock().unwrap() = Some(Instant::now());
            trigger.cancel();
        });

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            agent.run_turn("Talk".into(), tx, cancel),
        )
        .await
        .expect("the turn did not stop");
        let returned = Instant::now();
        result.unwrap();
        let cancelled = cancelled_at.lock().unwrap().unwrap();
        assert!(returned - cancelled < Duration::from_secs(1));

        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert!(
            !texts(&events).is_empty(),
            "the drip should have arrived first"
        );
        assert_eq!(
            events.last(),
            Some(&AgentEvent::Stopped {
                reason: "cancelled".into()
            })
        );

        // The HTTP stream was dropped: the server sees the client gone.
        let start = Instant::now();
        while !mock.hung_up.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(mock.hung_up.load(Ordering::SeqCst));
    })
    .await
    .expect("test cancel_stops_within_one_second timed out after 60 seconds");
}

#[tokio::test]
async fn api_key_absent_from_logs_and_errors() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let echo = format!(
            r#"{{"type":"error","error":{{"type":"invalid_request_error","message":"bad key {KEY}"}}}}"#
        );
        let mock = serve(move |n| match n {
            0 => Reply::Status(401, vec![], r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#.into()),
            1 => Reply::Status(400, vec![], echo.clone()),
            // Scrubbing is independent of retry policy; retry behaviour is tested below.
            2 => Reply::Status(429, vec![("retry-after", "0")], "{}".into()),
            3 => Reply::Status(500, vec![], format!("internal {KEY}")),
            _ => Reply::Sse(vec![
                message_start(),
                json!({"type":"error","error":{"type":"overloaded_error","message":format!("echo {KEY}")}}),
            ]),
        })
        .await;
        let provider = mock.anthropic().with_strict_budget();
        let req = TurnRequest {
            cache_key: "offline-test-conversation".into(),
            system: "s".into(),
            messages: vec![qiyas_ai::Message::user(vec![
                qiyas_ai::ContentBlock::Text { text: "hi".into() },
            ])],
            tools: vec![],
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            max_tokens: 100,
        };
        let mut errors = Vec::new();
        for _ in 0..5 {
            let mut sink = |_: StreamEvent| {};
            errors.push(
                provider
                    .stream_turn(req.clone(), &mut sink)
                    .await
                    .unwrap_err(),
            );
        }
        assert_eq!(errors[0], AiError::Auth);
        assert!(
            matches!(&errors[1], AiError::Api { status: 400, message, .. } if message.contains("bad key"))
        );
        assert!(
            matches!(&errors[3], AiError::Api { status: 500, message, .. } if !message.contains(KEY)),
            "{:?}",
            errors[2]
        );
        assert_eq!(mock.requests().len(), 5, "metered requests never retry without a reservation");
        assert!(matches!(&errors[2], AiError::RateLimited { .. }));
        assert!(matches!(&errors[4], AiError::Stream(m) if m.contains("overloaded_error")));

        // Nothing reachable: a network error.
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = format!("http://{}/v1/messages", closed.local_addr().unwrap());
        drop(closed);
        let mut sink = |_: StreamEvent| {};
        let e = Anthropic::with_url(KEY.into(), dead).with_strict_budget()
            .stream_turn(req.clone(), &mut sink)
            .await
            .unwrap_err();
        assert!(matches!(e, AiError::Network(_)), "{e:?}");
        errors.push(e);

        // The OpenAI-compatible client too.
        let mut sink = |_: StreamEvent| {};
        let e = OpenAiCompatible::new(&mock.base, Some(KEY.into())).with_strict_budget()
            .stream_turn(req.clone(), &mut sink)
            .await
            .unwrap_err();
        errors.push(e);

        // And through the agent.
        let (_dir, session) = sample();
        let mut agent = Agent::new(Box::new(provider), session, AgentSettings::default());
        let (result, events) = turn(&mut agent, "hi").await;
        errors.push(result.unwrap_err());

        for e in &errors {
            for text in [e.to_string(), format!("{e:?}")] {
                assert!(!text.contains(KEY), "the key leaked: {text}");
                assert!(!text.contains("SECRET"), "part of the key leaked: {text}");
            }
        }
        for e in &events {
            assert!(!format!("{e:?}").contains(KEY));
        }
    })
    .await
    .expect("test api_key_absent_from_logs_and_errors timed out after 60 seconds");
}

#[tokio::test]
async fn openai_compatible_streams_text_and_tool_calls() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => Reply::Data(vec![
                json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"Let me look."}}]}).to_string(),
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function",
                    "function":{"name":"qiyas_read","arguments":""}}]}}]}).to_string(),
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"op\":\"query\",\"args\":{\"kind\":\"layers\",\"off"}}]}}]}).to_string(),
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"set\": 0}}"}}]}}]}).to_string(),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}).to_string(),
                "[DONE]".into(),
            ]),
            _ => Reply::Data(vec![
                json!({"choices":[{"index":0,"delta":{"content":"Three layers."}}]}).to_string(),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}).to_string(),
                "[DONE]".into(),
            ]),
        })
        .await;
        let (_dir, session) = sample();
        let provider = OpenAiCompatible::new(&format!("{}/v1/", mock.base), Some(KEY.into()));
        let settings = AgentSettings {
            model: "gpt-test".into(),
            ..AgentSettings::default()
        };
        let mut agent = Agent::new(Box::new(provider), session, settings);
        let (result, events) = turn(&mut agent, "Which layers?").await;
        result.unwrap();
        assert_eq!(texts(&events), ["Let me look.", "Three layers."]);
        assert!(events.iter().any(|e| matches!(e,
            AgentEvent::ToolCall { id, name, input, mutating, label, .. }
                if id == "call_1" && name == "qiyas_read" && *input == json!({"op":"query","args":{"kind":"layers","offset":0}})
                    && !mutating && label.as_deref() == Some("query"))));

        let reqs = mock.requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].path, "/v1/chat/completions");
        assert_eq!(
            reqs[0].header("authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
        let b = &reqs[0].body;
        assert_eq!(b["model"], "gpt-test");
        assert_eq!(b["stream"], true);
        assert_eq!(b["tool_choice"], "auto");
        assert_eq!(b["tools"][0]["type"], "function");
        assert!(b["tools"][0]["function"]["parameters"].is_object());
        assert_eq!(b["messages"][0]["role"], "system");
        let second = reqs[1].body["messages"].as_array().unwrap();
        assert_eq!(second[2]["role"], "assistant");
        assert_eq!(second[2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(second[3]["role"], "tool");
        assert_eq!(second[3]["tool_call_id"], "call_1");
    })
    .await
    .expect("test openai_compatible_streams_text_and_tool_calls timed out after 60 seconds");
}

#[tokio::test]
async fn gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn() {
    tokio::time::timeout(Duration::from_secs(60), async {
        // Gemini behind an OpenAI-compatible gateway: each parallel call whole, all at index 0.
        let call = |id: &str, input: Value| {
            json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":id,
                "type":"function","function":{"name":"qiyas_read","arguments":input.to_string()}}]}}]})
            .to_string()
        };
        let first = vec![
            call("f1", json!({"op":"query","args":{"kind":"layers"}})),
            call("f2", json!({"op":"state","args":"{}"})),
            call(
                "f3",
                json!({"op":"records","args":{"collection":"takeoff"}}),
            ),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}).to_string(),
            "[DONE]".into(),
        ];
        let mock = serve(move |n| match n {
            0 => Reply::Data(first.clone()),
            1 => Reply::Data(vec![
                json!({"choices":[{"index":0,"delta":{"content":"Done."},"finish_reason":"stop"}]})
                    .to_string(),
                "[DONE]".into(),
            ]),
            _ => Reply::Status(
                400,
                vec![],
                r#"[{"error":{"code":400,"message":"The request was rejected","status":"INVALID_ARGUMENT"}}]"#.into(),
            ),
        })
        .await;
        let (dir, session) = sample();
        let log = qiyas_ai::AgentLog::new(dir.path().join("logs").join("agent.log"));
        let provider = OpenAiCompatible::new(&mock.base, Some(KEY.into()));
        let mut agent = Agent::new(Box::new(provider), session, AgentSettings::default()).with_log(log);
        let (result, events) = turn(&mut agent, "What is in the drawing?").await;
        result.unwrap();
        let ran: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolCall { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ran, ["qiyas_read", "qiyas_read", "qiyas_read"]);
        assert!(events
            .iter()
            .all(|e| !matches!(e, AgentEvent::ToolResult { is_error: true, .. })));
        let second = &mock.requests()[1].body["messages"];
        let calls = second[2]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[2]["function"]["name"], "qiyas_read");
        assert_eq!(
            second[2]["content"], "",
            "never null beside calls (Gemini refuses it)"
        );
        assert_eq!(second[5]["tool_call_id"], "f3");

        let (result, _) = turn(&mut agent, "Again").await;
        assert!(
            matches!(result, Err(AiError::Api { status: 400, ref message, .. })
            if message == "The request was rejected")
        );

        let text = std::fs::read_to_string(dir.path().join("logs").join("agent.log")).unwrap();
        for part in [
            "USER\n    What is in the drawing?",
            "REQUEST 1; sent: ",
            "ANSWER 1 (",
            r#"call qiyas_read (f2): {"args":"{}","op":"state"}"#,
            "TOOL qiyas_read (",
            "text: Done.",
            "DONE\n",
            "ERROR\n    The AI service answered 400: The request was rejected",
            "INVALID_ARGUMENT",
            // A 4xx logs what was sent (its last 4,000 characters after "…" when longer),
            // so the next refusal can be read in the log.
            "\n    The request: ",
            r#""content":"","role":"assistant","tool_calls":["#,
            r#""role":"tool","tool_call_id":"f3""#,
            " tools: ",
        ] {
            assert!(text.contains(part), "the log lacks {part:?}:\n{text}");
        }
        // The system prompt's stand-in sits at the front of the request, which a long request's
        // 4,000-character tail leaves out.
        if text.contains("\n    The request: {") {
            assert!(text.contains("<system prompt, "), "the log lacks the system prompt's length");
        }
        assert_eq!(text.matches("TOOL qiyas_read (").count(), 3);
        assert!(!text.contains(KEY));
    })
    .await
    .expect("test gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn timed out after 60 seconds");
}

const SLAB: &str = r#"{"op":"run","args":{"command":{"type":"measure_area","points": [[0, 0], [10, 0], [10, 10], [0, 10]]}}}"#;

#[tokio::test]
async fn cancel_between_tool_calls_runs_no_more_of_them() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => tool_turn(&[
                ("toolu_1", "qiyas_execute", &[SLAB]),
                ("toolu_2", "qiyas_execute", &[SLAB]),
                ("toolu_3", "qiyas_execute", &[SLAB]),
            ]),
            _ => text_turn(&["Should not be asked."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session.clone(),
            AgentSettings::default(),
        );
        // One event at a time, on this one thread: the cancel lands right after the first result.
        let (tx, mut rx) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let listen = async {
            let mut events = Vec::new();
            while let Some(e) = rx.recv().await {
                if matches!(&e, AgentEvent::ToolResult { id, .. } if id == "toolu_1") {
                    trigger.cancel();
                }
                events.push(e);
            }
            events
        };
        let (result, events) = tokio::join!(agent.run_turn("Measure".into(), tx, cancel), listen);
        result.unwrap();

        let items = session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .takeoff
            .items
            .len();
        assert_eq!(items, 1, "only the first call ran");
        let skipped: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolResult {
                    id,
                    summary,
                    is_error: true,
                    ..
                } if summary == "Not run: cancelled" => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(skipped, ["toolu_2", "toolu_3"]);
        assert_eq!(
            events.last(),
            Some(&AgentEvent::Stopped {
                reason: "cancelled".into()
            })
        );
        assert_eq!(mock.requests().len(), 1, "nothing more is asked");
        // Every call is answered, so the conversation can go on.
        let last = agent.history().last().unwrap();
        assert!(last.has_tool_results());
        assert_eq!(last.content.len(), 3);
    })
    .await
    .expect("test cancel_between_tool_calls_runs_no_more_of_them timed out after 60 seconds");
}

/// `count` calls of `name`, each with `input(i)`, in one answer.
fn many(name: &'static str, count: usize, input: impl Fn(usize) -> String) -> Reply {
    let calls: Vec<(&'static str, &'static str, Vec<&'static str>)> = (0..count)
        .map(|i| {
            let id: &'static str = Box::leak(format!("toolu_{i}").into_boxed_str());
            let json: &'static str = Box::leak(input(i).into_boxed_str());
            (id, name, vec![json])
        })
        .collect();
    let refs: Vec<(&str, &str, &[&str])> = calls
        .iter()
        .map(|(id, name, f)| (*id, *name, f.as_slice()))
        .collect();
    tool_turn(&refs)
}

#[tokio::test]
async fn removals_and_resolutions_report_counts_without_blocking() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let (_dir, session) = sample();
        let ids: Vec<String> = {
            let mut guard = session.lock().unwrap();
            let s = guard.as_mut().unwrap();
            (0..12)
                .map(|_| {
                    let cmd = qiyas_core::Command::MeasureDistance {
                        points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
                    };
                    match s.run(cmd).unwrap() {
                        qiyas_core::Outcome::Measured { measurement } => measurement.id,
                        other => panic!("{other:?}"),
                    }
                })
                .collect()
        };
        let removals = ids.clone();
        let mock = serve(move |n| match n {
            0 => many("qiyas_execute", 11, |i| {
                json!({"op":"run","args":{"command":{"type":"remove_measurement", "id": removals[i] }}}).to_string()
            }),
            1 => many("qiyas_execute", 11, |i| {
                json!({"op":"run","args":{"command":{"type":"resolve_conflict", "id": format!("c{i}"), "status": "resolved", "note": "" }}}).to_string()
            }),
            _ => text_turn(&["Done."]),
        })
        .await;
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session.clone(),
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "Remove them all and resolve everything").await;
        result.unwrap();

        let left = session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .takeoff
            .items
            .len();
        assert_eq!(left,1,"all eleven requested rows removed");
        assert!(events.iter().filter(|e|matches!(e,AgentEvent::ToolResult{is_error:false,..})).count()>=11);
        let history=serde_json::to_string(agent.history()).unwrap();
        assert!(history.contains("many_removals"));
        assert!(!history.contains("Too many destructive commands"));
        assert!(events.iter().filter(|e|matches!(e,AgentEvent::ToolResult{is_error:true,..})).count()>=11,"missing conflicts still fail");

    })
    .await
    .expect("test removals_and_resolutions_report_counts_without_blocking timed out after 60 seconds");
}

#[tokio::test]
async fn view_image_stops_when_the_image_budget_is_used() {
    tokio::time::timeout(Duration::from_secs(60), async {
        // The size of one 64-pixel picture of the sample, as base64.
        let (_d, probe) = sample();
        let one = {
            let mut guard = probe.lock().unwrap();
            let s = guard.as_mut().unwrap();
            match qiyas_ai::execute(
                s,
                "view_image",
                &json!({ "width": 64 }),
                qiyas_core::Source::Ai,
            )
            .unwrap()
            {
                qiyas_ai::ToolOutput::Image { png, .. } => png.len().div_ceil(3) * 4,
                _ => panic!("expected an image"),
            }
        };
        let mock = serve(|n| match n {
            0 | 2 => tool_turn(&[
                (
                    "toolu_1",
                    "qiyas_capture",
                    &[r#"{"source":"picture","args":{"width":64}}"#],
                ),
                (
                    "toolu_2",
                    "qiyas_capture",
                    &[r#"{"source":"picture","args":{"width":64}}"#],
                ),
                (
                    "toolu_3",
                    "qiyas_capture",
                    &[r#"{"source":"picture","args":{"width":65}}"#],
                ),
            ]),
            _ => text_turn(&["Looked."]),
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        )
        .with_image_budget(one + one / 2);
        turn(&mut agent, "Look twice").await.0.unwrap();

        let body = &mock.requests()[1].body;
        let results = body["messages"][2]["content"].as_array().unwrap();
        assert!(results[0].get("is_error").is_none(), "{}", results[0]);
        assert_eq!(results[0]["content"][1]["type"], "image");
        let unchanged: Value =
            serde_json::from_str(results[1]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(unchanged["status"], "unchanged");
        assert_eq!(results[2]["is_error"], true);
        let error: Value =
            serde_json::from_str(results[2]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(error["code"], "budget");
        assert_eq!(
            error["message"],
            "Image budget for this conversation is used up; start a new conversation to look again"
        );

        // A new conversation has the whole budget again.
        agent.clear();
        turn(&mut agent, "Look twice").await.0.unwrap();
        let body = &mock.requests()[3].body;
        assert!(body["messages"][2]["content"][0].get("is_error").is_none());
    })
    .await
    .expect("test view_image_stops_when_the_image_budget_is_used timed out after 60 seconds");
}

/// Answers its first request with one mutating call, after taking the session's lock on a
/// thread of its own: the call then waits for the lock, as one dispatched just before a tab
/// switch. The thread puts the drawing it is handed in the session, then lets go. Later requests
/// get a short text.
struct HoldsTheLock {
    session: Arc<Mutex<Option<Session>>>,
    swap: Mutex<Option<std::sync::mpsc::Receiver<Session>>>,
}

impl Provider for HoldsTheLock {
    fn stream_turn<'a>(
        &'a self,
        _: TurnRequest,
        _: qiyas_ai::provider::Sink<'a>,
    ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, AiError>> {
        let swap = self.swap.lock().unwrap().take();
        let session = self.session.clone();
        Box::pin(async move {
            let Some(swap) = swap else {
                return Ok(qiyas_ai::TurnResult {
                    content: vec![qiyas_ai::ContentBlock::Text { text: "ok".into() }],
                    stop_reason: qiyas_ai::StopReason::EndTurn,
                    usage: Default::default(),
                });
            };
            let (held, is_held) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let mut guard = session.lock().unwrap();
                held.send(()).unwrap();
                if let Ok(next) = swap.recv() {
                    *guard = Some(next);
                }
            });
            is_held.await.unwrap();
            Ok(qiyas_ai::TurnResult {
                content: vec![qiyas_ai::ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "qiyas_execute".into(),
                    input: serde_json::from_str(SLAB).unwrap(),
                }],
                stop_reason: qiyas_ai::StopReason::ToolUse,
                usage: Default::default(),
            })
        })
    }
}

/// Swaps another drawing in while a call waits for the session (cancelling the turn first when
/// `cancel_too`, as a tab switch does) and returns the call's result and the measurements on the
/// drawing now in the session.
async fn swap_while_a_call_waits(cancel_too: bool) -> (String, usize) {
    let (_dir, session) = sample();
    let original_path = session
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .drawing
        .path
        .clone();
    let other_dir = tempfile::tempdir().unwrap();
    let other =
        Session::open(&qiyas_core::fixtures::write_sample(other_dir.path(), "dwg")).unwrap();
    let (swap_tx, swap_rx) = std::sync::mpsc::channel();
    let provider = HoldsTheLock {
        session: session.clone(),
        swap: Mutex::new(Some(swap_rx)),
    };
    let checkpoint = Arc::new(Mutex::new(None));
    let mut agent = Agent::new(
        Box::new(provider),
        session.clone(),
        AgentSettings::default(),
    )
    .with_checkpoint(checkpoint.clone());
    let (tx, mut rx) = mpsc::channel(100);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let listen = async move {
        let mut other = Some(other);
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            if matches!(&e, AgentEvent::ToolCall { .. }) {
                // The streamed checkpoint must not wait for the held session lock.
                {
                    let saved = checkpoint.lock().unwrap();
                    let saved = saved.as_ref().unwrap();
                    assert_eq!(saved.drawing.as_ref(), Some(&original_path));
                    assert!(saved.transcript.iter().any(|entry| matches!(
                        entry,
                        qiyas_ai::TranscriptEntry::Event {
                            event: AgentEvent::ToolCall { .. }
                        }
                    )));
                }
                // The call is past the cancel check and waiting for the lock by now.
                tokio::time::sleep(Duration::from_millis(200)).await;
                if cancel_too {
                    trigger.cancel();
                }
                swap_tx.send(other.take().unwrap()).unwrap();
            }
            events.push(e);
        }
        events
    };
    // Keep the timeout driver separate from the turn: if a synchronous session lock is
    // accidentally reintroduced, timing out drops `listen` and releases the holder too.
    let run = tokio::spawn(async move { agent.run_turn("Measure".into(), tx, cancel).await });
    let (result, events) = tokio::join!(run, listen);
    result.unwrap().unwrap();
    let summary = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolResult {
                summary, is_error, ..
            } => {
                assert!(is_error, "{summary}");
                Some(summary.clone())
            }
            _ => None,
        })
        .unwrap();
    let items = session
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .takeoff
        .items
        .len();
    (summary, items)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_waiting_through_a_tab_switch_is_not_run_on_the_new_drawing() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let (summary, items) = swap_while_a_call_waits(true).await;
        assert_eq!(summary, "Not run: cancelled");
        assert_eq!(items, 0, "the new drawing has no AI measurement");

        // Even without the cancel, the call sees the drawing is another one.
        let (summary, items) = swap_while_a_call_waits(false).await;
        assert!(
            summary.starts_with("Not run: the drawing changed"),
            "{summary}"
        );
        assert_eq!(items, 0);
    })
    .await
    .expect("test a_call_waiting_through_a_tab_switch_is_not_run_on_the_new_drawing timed out after 60 seconds");
}

/// Reproduce the recorded failure boundary: find_entities succeeds, then request 2 fails.
/// This verifies the current request and diagnostic log, not a live provider fix.
#[tokio::test]
async fn rejected_request_after_find_entities_is_logged_without_retry() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => Reply::Data(vec![
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_446341","type":"function","function":{"name":"qiyas_read","arguments":"{\"op\":\"query\",\"args\":{\"kind\":\"window\",\"layer\":\"A-WALL\"}}"}}]},"finish_reason":"tool_calls"}]}).to_string(),
                "[DONE]".into(),
            ]),
            _ => Reply::Status(400, vec![], json!({"error":{"message":"The request was rejected","type":"invalid_request_error","param":null,"code":"invalid_value"}}).to_string()),
        }).await;
        let (dir, session) = sample();
        let log_path = dir.path().join("agent.log");
        let mut agent = Agent::new(
            Box::new(OpenAiCompatible::new(&mock.base, Some(KEY.into()))),
            session,
            AgentSettings::default(),
        )
        .with_log(qiyas_ai::AgentLog::new(log_path.clone()));
        let (result, _) = turn(
            &mut agent,
            "Measure every wall on A-WALL and add it as length",
        )
        .await;
        assert!(matches!(result, Err(AiError::Api { status: 400, .. })));
        let requests = mock.requests();
        assert_eq!(requests.len(), 2, "a rejected request must not be retried");
        let messages = requests[1].body["messages"].as_array().unwrap();
        let assistant = messages.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant["content"], "");
        assert_eq!(assistant["tool_calls"][0]["id"], "call_446341");
        assert_eq!(
            assistant["tool_calls"][0]["function"]["arguments"],
            r#"{"args":{"kind":"window","layer":"A-WALL"},"op":"query"}"#
        );
        let tool = messages.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(tool["tool_call_id"], "call_446341");
        assert!(tool["content"].is_string());
        let log = std::fs::read_to_string(log_path).unwrap();
        assert!(log.contains("The request: {"));
        let requests: Vec<_> = log
            .lines()
            .filter(|line| line.contains("  REQUEST "))
            .collect();
        assert_eq!(requests.len(), 2, "{log}");
        assert!(
            requests
                .iter()
                .all(|line| line.contains("sent: ") && line.contains("max_completion_tokens=")),
            "{log}"
        );
        assert!(log.contains("invalid_value"));
        assert!(log.contains("call_446341"));
        assert!(!log.contains(KEY));
    })
    .await
    .expect("test rejected_request_after_find_entities_is_logged_without_retry timed out after 60 seconds");
}

/// The app's drawings, faked: `new_drawing` puts a new blank session in the shared slot (a new
/// epoch, as a new tab does), `save_drawing` asks the user, and every call is recorded.
struct FakeHost {
    session: Arc<Mutex<Option<Session>>>,
    dir: std::path::PathBuf,
    calls: Mutex<Vec<String>>,
}

impl qiyas_ai::DrawingHost for FakeHost {
    fn list(&self) -> Result<Value, String> {
        self.calls.lock().unwrap().push("list".into());
        Ok(json!({ "open": [], "recent": [] }))
    }
    fn new_drawing(&self, units: &str) -> Result<Value, String> {
        self.calls.lock().unwrap().push(format!("new {units}"));
        let blank = Session::new_blank(&self.dir.join("Untitled-1.dwg"), 6);
        *self.session.lock().unwrap() = Some(blank);
        Ok(json!({ "message": "New drawing Untitled-1.dwg on screen" }))
    }
    fn open(&self, path: &str, user_named: bool) -> Result<Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("open {path} {user_named}"));
        Ok(json!({}))
    }
    fn switch(&self, id: u32) -> Result<Value, String> {
        self.calls.lock().unwrap().push(format!("switch {id}"));
        Ok(json!({}))
    }
    fn save(&self) -> Result<Value, String> {
        self.calls.lock().unwrap().push("save".into());
        Ok(qiyas_ai::host::asked(
            "The Save dialog is open for the user",
        ))
    }
    fn close(&self, id: Option<u32>) -> Result<Value, String> {
        self.calls.lock().unwrap().push(format!("close {id:?}"));
        Ok(json!({}))
    }
}

#[tokio::test]
async fn a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => tool_turn(&[("toolu_1", "qiyas_execute", &[r#"{"op":"new_drawing","args":{"units":"m"}}"#])]),
            1 => tool_turn(&[(
                "toolu_2",
                "qiyas_execute",
                &[r#"{"op":"run","args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[5,0]}}}"#],
            )]),
            2 => tool_turn(&[
                ("toolu_3", "qiyas_execute", &[r#"{"op":"save_drawing","args":"{}"}"#]),
                (
                    "toolu_4",
                    "qiyas_execute",
                    &[r#"{"op":"open_drawing","args":{"path":"/jobs/villa.dwg"}}"#],
                ),
            ]),
            _ => text_turn(&["Drawn; choose where to save it."]),
        })
        .await;
        // A drawing is open when the turn starts: the new one replaces it on screen.
        let (dir, session) = sample();
        let host = Arc::new(FakeHost {
            session: session.clone(),
            dir: dir.path().to_path_buf(),
            calls: Mutex::new(Vec::new()),
        });
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session.clone(),
            AgentSettings::default(),
        )
        .with_host(host.clone());

        let (result, events) = turn(&mut agent, "new drawing in metres, draw a line, save it").await;
        result.unwrap();

        let results: Vec<(String, bool, String)> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolResult {
                    id,
                    is_error,
                    summary,
                    ..
                } => Some((id.clone(), *is_error, summary.clone())),
                _ => None,
            })
            .collect();
        assert!(results.iter().all(|(_, err, _)| !err), "{results:?}");
        assert_eq!(results[0].2, "New drawing Untitled-1.dwg on screen");
        assert_eq!(results[2].2, "The Save dialog is open for the user");

        // The line is in the new drawing, which is still untitled: nothing saved it.
        let guard = session.lock().unwrap();
        let s = guard.as_ref().unwrap();
        assert!(s.is_untitled());
        assert_eq!(s.drawing.doc.entities_in_block("*Model_Space").count(), 1);
        // The path was not in the user's message, so it is not counted as named by the user.
        assert_eq!(
            *host.calls.lock().unwrap(),
            ["new m", "save", "open /jobs/villa.dwg false"]
        );
    })
    .await
    .expect("test a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user timed out after 60 seconds");
}

#[tokio::test]
async fn file_tools_without_a_host_say_to_use_the_app() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|n| match n {
            0 => tool_turn(&[(
                "toolu_1",
                "qiyas_execute",
                &[r#"{"op":"new_drawing","args":{"units":"m"}}"#],
            )]),
            _ => text_turn(&["Use New in Qiyas."]),
        })
        .await;
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "make a new drawing").await;
        result.unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolResult { is_error: true, summary, .. } if summary == qiyas_ai::host::NO_HOST
        )));
    })
    .await
    .expect("test file_tools_without_a_host_say_to_use_the_app timed out after 60 seconds");
}

// ---------------------------------------------------------------------------------------------
// What the user points at

/// Runs one user turn with what the user points at and the pictures they attached.
async fn turn_with(agent: &mut Agent, message: qiyas_ai::UserMessage) -> Result<(), AiError> {
    let (tx, _rx) = mpsc::channel(10_000);
    agent
        .run_turn_with(message, tx, CancellationToken::new())
        .await
}

fn rectangle_of(session: &Arc<Mutex<Option<Session>>>) -> u64 {
    let guard = session.lock().unwrap();
    guard
        .as_ref()
        .unwrap()
        .find_entities(&qiyas_core::EntityFilter {
            kind: Some("LwPolyline".into()),
            ..Default::default()
        })
        .rows[0]
        .handle
}

/// The selection reaches the `[state]` block as rows Mizan can act on, with a picture of it
/// marked first; the view on screen is there too.
#[tokio::test]
async fn a_selection_is_listed_and_shown() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| text_turn(&["Seen."])).await;
        let (_dir, session) = sample();
        let rect = rectangle_of(&session);
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let message = qiyas_ai::UserMessage {
            text: "What is this?".into(),
            pointed: vec![],
            context: qiyas_ai::TurnContext {
                selection: vec![qiyas_core::Target {
                    handle: rect,
                    chain: vec![],
                }],
                view: Some([[0.0, 0.0], [20.0, 10.0]]),
                picture: true,
                ..Default::default()
            },
            images: vec![],
        };
        turn_with(&mut agent, message).await.unwrap();
        let body = &mock.requests()[0].body;
        let content = body["messages"][0]["content"].as_array().unwrap();
        let state = content[0]["text"].as_str().unwrap();
        assert!(
            state.contains("View on screen: [[0.000, 0.000], [20.000, 10.000]]"),
            "{state}"
        );
        assert!(state.contains("Selected by the user (1;"), "{state}");
        assert!(
            state.contains(&format!("- handle {rect} LwPolyline on A-WALL")),
            "{state}"
        );
        assert!(state.contains("area 12.000 m²"), "{state}");
        let caption = content[1]["text"].as_str().unwrap();
        assert!(
            caption.starts_with("[picture of what the user points at]"),
            "{caption}"
        );
        assert!(
            caption.contains(&format!("#1 handle {rect} LwPolyline")),
            "{caption}"
        );
        assert_eq!(content[2]["type"], "image");
        assert_eq!(content[3]["text"], "What is this?");

        // The same selection again, unchanged: listed, but no second picture.
        let again = qiyas_ai::UserMessage {
            text: "And its perimeter?".into(),
            pointed: vec![],
            context: qiyas_ai::TurnContext {
                selection: vec![qiyas_core::Target {
                    handle: rect,
                    chain: vec![],
                }],
                ..Default::default()
            },
            images: vec![],
        };
        turn_with(&mut agent, again).await.unwrap();
        let body = &mock.requests()[1].body;
        let last = body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_array()
            .unwrap()
            .clone();
        assert!(last[0]["text"]
            .as_str()
            .unwrap()
            .contains("Selected by the user"));
        assert!(!last.iter().any(|b| b["type"] == "image"));
    })
    .await
    .expect("test a_selection_is_listed_and_shown timed out after 60 seconds");
}

/// A region the user marked lists what is in it or crosses it, with its area.
#[tokio::test]
async fn a_marked_region_lists_what_is_in_it() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| text_turn(&["Seen."])).await;
        let (_dir, session) = sample();
        let rect = rectangle_of(&session);
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let message = qiyas_ai::UserMessage {
            text: "Count what is here".into(),
            pointed: vec![],
            context: qiyas_ai::TurnContext {
                region: Some(qiyas_ai::Region {
                    kind: qiyas_ai::RegionKind::Box,
                    points: vec![[-1.0, -1.0], [4.5, 3.5]],
                }),
                picture: true,
                ..Default::default()
            },
            images: vec![],
        };
        turn_with(&mut agent, message).await.unwrap();
        let body = &mock.requests()[0].body;
        let content = body["messages"][0]["content"].as_array().unwrap();
        let state = content[0]["text"].as_str().unwrap();
        assert!(state.contains("Region marked by the user (box;"), "{state}");
        assert!(state.contains("24.750 m²"), "{state}");
        assert!(
            state.contains(&format!("- handle {rect} LwPolyline")),
            "{state}"
        );
        assert!(content[1]["text"].as_str().unwrap().contains("dashed blue"));
        assert_eq!(content[2]["type"], "image");
    })
    .await
    .expect("test a_marked_region_lists_what_is_in_it timed out after 60 seconds");
}

/// Pictures the user attaches go before their words; a kind the model can't read is refused.
#[tokio::test]
async fn attached_pictures_go_before_the_words() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mock = serve(|_| text_turn(&["A site photo."])).await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let photo = qiyas_ai::UserImage {
            media_type: "image/jpeg".into(),
            data: "QUJD".into(),
        };
        let message = qiyas_ai::UserMessage {
            text: "What is in this photo?".into(),
            images: vec![photo.clone()],
            ..Default::default()
        };
        turn_with(&mut agent, message).await.unwrap();
        let body = &mock.requests()[0].body;
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 3, "{content:?}");
        assert_eq!(content[1]["source"]["media_type"], "image/jpeg");
        assert_eq!(content[1]["source"]["data"], "QUJD");
        assert_eq!(content[2]["text"], "What is in this photo?");

        let (tx, mut rx) = mpsc::channel(100);
        let pdf = qiyas_ai::UserMessage {
            text: "And this?".into(),
            images: vec![qiyas_ai::UserImage {
                media_type: "application/pdf".into(),
                ..photo
            }],
            ..Default::default()
        };
        agent
            .run_turn_with(pdf, tx, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(mock.requests().len(), 1, "nothing sent");
        let stopped = std::iter::from_fn(|| rx.try_recv().ok()).any(
            |e| matches!(e, AgentEvent::Stopped { reason } if reason.contains("can't be sent")),
        );
        assert!(stopped);
    })
    .await
    .expect("test attached_pictures_go_before_the_words timed out after 60 seconds");
}

/// Identical captures keep their first picture and append unchanged replies.
#[tokio::test]
async fn identical_pictures_keep_the_prefix_and_do_not_resend_images() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let calls: Vec<String> = (0..15).map(|k| format!("toolu_{k}")).collect();
        let mock = serve(move |n| {
            if n < 15 {
                let id: &'static str = Box::leak(calls[n].clone().into_boxed_str());
                tool_turn(&[(
                    id,
                    "qiyas_capture",
                    &[r#"{"source":"picture","args":{"width":64}}"#],
                )])
            } else {
                text_turn(&["Looked."])
            }
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        turn(&mut agent, "Look round").await.0.unwrap();
        let requests = mock.requests();
        let body = &requests.last().unwrap().body;
        let text = body.to_string();
        let images = text.matches("\"type\":\"image\"").count();
        assert_eq!(images, 1, "the same PNG is sent once");
        let messages = body["messages"].as_array().unwrap();
        let newest = &messages[messages.len() - 1]["content"][0]["content"];
        let value: Value = serde_json::from_str(newest[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(value["status"], "unchanged");
        assert_eq!(newest.as_array().unwrap().len(),1);
        for pair in requests.windows(2) {
            let before=serialised(&pair[0].body);
            let after=serialised(&pair[1].body);
            assert_eq!(&after[..before.len()],&before);
        }
    })
    .await
    .expect("test identical_pictures_keep_the_prefix_and_do_not_resend_images timed out after 60 seconds");
}

/// The effort reaches an OpenAI-compatible service; one that refuses `reasoning_effort` gets the
/// request again without it, and never again in this conversation.
#[tokio::test]
async fn openai_compatible_drops_a_refused_field_and_asks_again() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let refusal =
            r#"{"error":{"message":"Unrecognized request argument supplied: reasoning_effort"}}"#;
        let mock = serve(move |n| match n {
            0 => Reply::Status(400, vec![], refusal.to_string()),
            _ => Reply::Data(vec![
                json!({"choices":[{"index":0,"delta":{"content":"Hello."}}]}).to_string(),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}).to_string(),
                json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":4,
                    "completion_tokens_details":{"reasoning_tokens":3}}})
                .to_string(),
                "[DONE]".into(),
            ]),
        })
        .await;
        let (_dir, session) = sample();
        let provider = OpenAiCompatible::new(&format!("{}/v1", mock.base), Some(KEY.into()));
        let settings = AgentSettings {
            model: "google:gemini@3.8-flash".into(),
            effort: "high".into(),
            ..AgentSettings::default()
        };
        let mut agent = Agent::new(Box::new(provider), session, settings);
        let (result, events) = turn(&mut agent, "hi").await;
        result.unwrap();
        assert_eq!(texts(&events), ["Hello."]);
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].body["reasoning_effort"], "high");
        assert!(reqs[0].body["max_completion_tokens"].is_u64());
        assert!(reqs[1].body.get("reasoning_effort").is_none());
        assert!(
            reqs[1].body["max_completion_tokens"].is_u64(),
            "only the refused field goes"
        );
        assert!(events.iter().any(|e| matches!(e,
            AgentEvent::Done { usage } if usage.reasoning_tokens == 3)));

        turn(&mut agent, "again").await.0.unwrap();
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 3, "no second refusal");
        assert!(reqs[2].body.get("reasoning_effort").is_none());
    })
    .await
    .expect(
        "test openai_compatible_drops_a_refused_field_and_asks_again timed out after 60 seconds",
    );
}

/// Exercise presentation events through the real agent/session loop without a local HTTP port.
#[tokio::test]
async fn activity_metadata_and_counts_follow_the_real_edit_without_network() {
    struct Scripted(AtomicBool);
    impl Provider for Scripted {
        fn stream_turn<'a>(
            &'a self,
            _: TurnRequest,
            _: qiyas_ai::provider::Sink<'a>,
        ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, AiError>> {
            let first = !self.0.swap(true, Ordering::SeqCst);
            Box::pin(async move {
                Ok(qiyas_ai::TurnResult {
                    content: if first {
                        vec![qiyas_ai::ContentBlock::ToolUse {
                            id: "activity-edit".into(),
                            name: "qiyas_execute".into(),
                            input: json!({"op":"run", "args":{"command":{
                                "type":"draw", "kind":"line", "a":[0,0], "b":[1,0]
                            }}}),
                        }]
                    } else {
                        vec![qiyas_ai::ContentBlock::Text {
                            text: "تم رسم الخط.".into(),
                        }]
                    },
                    stop_reason: if first {
                        qiyas_ai::StopReason::ToolUse
                    } else {
                        qiyas_ai::StopReason::EndTurn
                    },
                    usage: Default::default(),
                })
            })
        }
    }
    let (_dir, session) = sample();
    let entities = || {
        session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .count()
    };
    let before = entities();
    let mut agent = Agent::new(
        Box::new(Scripted(AtomicBool::new(false))),
        session.clone(),
        AgentSettings::default(),
    );
    let (result, events) = turn(&mut agent, "ارسم خطًا").await;
    result.unwrap();
    assert_eq!(
        entities(),
        before + 1,
        "read back the actual drawing change"
    );
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::ToolCall { id, activity: Some(activity), .. }
        if id == "activity-edit" && activity.category == "draw" && activity.effect == "edit"
    )));
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::ToolResult { id, count: Some(1), is_error: false, .. }
        if id == "activity-edit"
    )));
}

// Goal E uses real HTTP/SSE adapters and the production agent, with no paid calls.
#[tokio::test]
async fn billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report() {
    for status in [400, 402, 403, 429] {
        let mock = serve(move |_| {
            Reply::Status(
                status,
                vec![],
                json!({"error":{"code":"insufficient_quota","message":"Insufficient credits"}})
                    .to_string(),
            )
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(OpenAiCompatible::new(&mock.base, None)),
            session,
            AgentSettings::default(),
        );
        let (result, events) =
            tokio::time::timeout(Duration::from_secs(2), turn(&mut agent, "ارسم خطًا"))
                .await
                .unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code(), "billing");
        assert_eq!(mock.requests().len(), 1);
        assert!(
            texts(&events).is_empty(),
            "A provider failure must not invent an end report"
        );
        assert!(!events
            .iter()
            .any(|e| matches!(e, AgentEvent::Retrying { .. })));
        let payload: Value = serde_json::from_str(&error.for_window()).unwrap();
        assert_eq!(payload["code"], "ai.error.billing");
        assert!(payload["message_ar"].as_str().unwrap().contains("رصيد"));
    }
}

#[tokio::test]
async fn all_server_failures_retry_with_a_visible_wait_and_identical_request() {
    for status in [500, 502, 503, 504, 529] {
        let mock = serve(move |n| {
            if n == 0 {
                Reply::Status(status, vec![], "temporary service problem".into())
            } else {
                text_turn(&["Ready."])
            }
        })
        .await;
        let (_dir, session) = sample();
        let mut agent = Agent::new(
            Box::new(mock.anthropic()),
            session,
            AgentSettings::default(),
        );
        let (result, events) = turn(&mut agent, "What is on the screen?").await;
        result.unwrap();
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Retrying { seconds: 5, code, .. } if code == "ai.error.service_error")));
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body, requests[1].body);
        assert_eq!(texts(&events), ["Ready."]);
    }
}

#[tokio::test]
async fn mid_stream_overload_discards_partial_text_and_keeps_a_retry_log() {
    for anthropic in [true, false] {
        let mock = serve(move |n| {
            if anthropic {
                if n == 0 { Reply::Sse(vec![
                    message_start(),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Unfinished"}}),
                    json!({"type":"error","error":{"type":"overloaded_error","message":"try later"}}),
                ]) } else { text_turn(&["Complete."]) }
            } else if n == 0 { Reply::Data(vec![
                json!({"choices":[{"delta":{"content":"Unfinished"}}]}).to_string(),
                json!({"error":{"type":"server_error","message":"try later"}}).to_string(),
            ]) } else { Reply::Data(vec![
                json!({"choices":[{"delta":{"content":"Complete."},"finish_reason":"stop"}]}).to_string(),
                "[DONE]".into(),
            ]) }
        }).await;
        let (dir, session) = sample();
        let provider: Box<dyn Provider> = if anthropic {
            Box::new(mock.anthropic())
        } else {
            Box::new(OpenAiCompatible::new(&mock.base, None))
        };
        let log = qiyas_ai::AgentLog::new(dir.path().join("retry.log"));
        let mut agent =
            Agent::new(provider, session, AgentSettings::default()).with_log(log.clone());
        let (result, events) = turn(&mut agent, "Continue the drawing.").await;
        result.unwrap();
        assert_eq!(mock.requests().len(), 2);
        assert!(events.iter().any(|e| matches!(
            e,
            AgentEvent::Retrying {
                discard_partial: true,
                ..
            }
        )));
        let saved = serde_json::to_string(&agent.snapshot().transcript).unwrap();
        assert!(!saved.contains("Unfinished"));
        assert!(saved.contains("Complete."));
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("RETRY") && text.contains("Unfinished"));
    }
}

#[tokio::test]
async fn invalid_output_value_keeps_the_cap_and_does_not_retry() {
    let mock = serve(|_| Reply::Status(400, vec![], json!({"error":{"message":"Invalid max_completion_tokens value, the valid range is [1, 8192]"}}).to_string())).await;
    let provider = OpenAiCompatible::new(&mock.base, None);
    let req = TurnRequest {
        cache_key: "offline-test-conversation".into(),
        system: "s".into(),
        messages: vec![],
        tools: vec![],
        model: "local".into(),
        effort: "none".into(),
        max_tokens: 16000,
    };
    for _ in 0..2 {
        let error = provider
            .stream_turn(req.clone(), &mut |_| {})
            .await
            .unwrap_err();
        assert_eq!(error.code(), "output_limit");
    }
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|r| r.body["max_completion_tokens"] == 16000));
    assert!(requests
        .iter()
        .all(|r| r.body.get("reasoning_effort").is_none()));
    assert!(!provider
        .save_state()
        .to_string()
        .contains("max_completion_tokens"));
}

#[tokio::test]
async fn partial_usage_chunks_retain_prior_counts_through_the_http_adapter() {
    let mock = serve(|_| Reply::Data(vec![
        json!({"choices":[{"delta":{"content":"Done"}}],"usage":{"prompt_tokens":1000,"prompt_tokens_details":{"cached_tokens":800},"completion_tokens":60}}).to_string(),
        json!({"choices":[],"usage":{"completion_tokens":90,"reasoning_tokens":71}}).to_string(),
        json!({"choices":[],"usage":{"prompt_tokens_details":{"cached_tokens":900}}}).to_string(),
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]}).to_string(),
        "[DONE]".into(),
    ])).await;
    let req = TurnRequest {
        cache_key: "offline-test-conversation".into(),
        system: "s".into(),
        messages: vec![],
        tools: vec![],
        model: "local".into(),
        effort: "low".into(),
        max_tokens: 1000,
    };
    let result = OpenAiCompatible::new(&mock.base, None)
        .stream_turn(req, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(result.usage.input_tokens, 100);
    assert_eq!(result.usage.cache_read_input_tokens, 900);
    assert_eq!(result.usage.output_tokens, 90);
    assert_eq!(result.usage.reasoning_tokens, 71);
}

#[tokio::test]
async fn provider_failure_after_a_real_drawing_edit_preserves_work_without_a_takeoff_report() {
    struct EditThenFail(AtomicBool);
    impl Provider for EditThenFail {
        fn stream_turn<'a>(
            &'a self,
            _: TurnRequest,
            _: qiyas_ai::provider::Sink<'a>,
        ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, AiError>> {
            let first = !self.0.swap(true, Ordering::SeqCst);
            Box::pin(async move {
                if first {
                    Ok(qiyas_ai::TurnResult {
                        content: vec![qiyas_ai::ContentBlock::ToolUse {
                            id: "retained-edit".into(),
                            name: "qiyas_execute".into(),
                            input: json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[20,20],"b":[23,20]}}}),
                        }],
                        stop_reason: qiyas_ai::StopReason::ToolUse,
                        usage: Default::default(),
                    })
                } else {
                    Err(AiError::Api {
                        status: 429,
                        message: "insufficient_quota".into(),
                        body: String::new(),
                        request: None,
                    })
                }
            })
        }
    }
    let (_dir, session) = sample();
    let count = || {
        session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .count()
    };
    let before = count();
    let mut agent = Agent::new(
        Box::new(EditThenFail(AtomicBool::new(false))),
        session.clone(),
        AgentSettings::default(),
    );
    let (result, events) = turn(&mut agent, "ارسم خطًا بطول ثلاثة أمتار").await;
    let error = result.unwrap_err();
    assert_eq!(error.code(), "billing");
    assert_eq!(count(), before + 1);
    assert!(texts(&events).is_empty());
    assert!(events.iter().any(
        |e| matches!(e, AgentEvent::ToolResult { id, is_error: false, .. } if id == "retained-edit")
    ));
    if let Ok(dir) = std::env::var("QIYAS_GOAL_E_EVIDENCE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("provider-error-after-edit.json"), serde_json::to_vec_pretty(&json!({
            "events":events,"error":error.public_error(),"entities_before":before,"entities_after":count(),"transcript":agent.snapshot().transcript,
        })).unwrap()).unwrap();
    }
}

#[tokio::test]
async fn mock_openai_keeps_cache_identity_prefix_and_exact_deepseek_reasoning() {
    use qiyas_ai::{ContentBlock, Message, ResultPart};
    let mock = serve(|n| Reply::Data(if n == 0 { vec![
        json!({"choices":[{"delta":{"reasoning_content":"Exact thinking\nأفكار"}}]}).to_string(),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"read-1","function":{"name":"qiyas_read","arguments":"{\"op\":\"state\",\"args\":\"{}\"}"}}]},"finish_reason":"tool_calls"}]}).to_string(),
        "[DONE]".into(),
    ] } else { vec![json!({"choices":[{"delta":{"content":"Done"},"finish_reason":"stop"}]}).to_string(),"[DONE]".into()] })).await;
    let provider = OpenAiCompatible::new(&mock.base, None);
    let mut request = TurnRequest {
        cache_key: "mock-conversation".into(),
        system: "Stable rules".into(),
        tools: qiyas_ai::catalogue(),
        messages: vec![Message::user(vec![ContentBlock::Text {
            text: "Measure accurately".into(),
        }])],
        model: "mock-model".into(),
        effort: "high".into(),
        max_tokens: 1000,
    };
    let result = provider
        .stream_turn(request.clone(), &mut |_| {})
        .await
        .unwrap();
    request.messages.push(Message::assistant(result.content));
    request
        .messages
        .push(Message::user(vec![ContentBlock::ToolResult {
            tool_use_id: "read-1".into(),
            content: vec![ResultPart::Text("{}".into())],
            is_error: false,
        }]));
    provider.stream_turn(request, &mut |_| {}).await.unwrap();
    let requests = mock.requests();
    let (first, next) = (&requests[0].body, &requests[1].body);
    assert_eq!(first["prompt_cache_key"], "mock-conversation");
    assert_eq!(first["prompt_cache_key"], next["prompt_cache_key"]);
    assert_eq!(first["tools"], next["tools"]);
    for (a, b) in first["messages"]
        .as_array()
        .unwrap()
        .iter()
        .zip(next["messages"].as_array().unwrap())
    {
        assert_eq!(a.to_string(), b.to_string());
    }
    let assistant = next["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["reasoning_content"], "Exact thinking\nأفكار");
}
