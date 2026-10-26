use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use super::{PlanModel, Tokens};
use crate::provider::{http_client, read_events, Sink};
use crate::{
    AiError, ContentBlock, ResultPart, RetryBackoff, Role, StopReason, StreamEvent, TurnRequest,
    TurnResult, Usage,
};

/// This adapter is private to Mizan; MCP never exposes it or its credentials.
pub(super) async fn stream(
    endpoint: &str,
    tokens: &Tokens,
    req: TurnRequest,
    refused_effort: &Mutex<HashSet<String>>,
    sink: Sink<'_>,
) -> Result<TurnResult, AiError> {
    tokens.require_plan()?;
    let mut body = request(&req)?;
    if refused_effort
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&req.model)
    {
        body.as_object_mut().unwrap().remove("reasoning");
    }
    let client = http_client();
    let mut retry = RetryBackoff::default();
    let started = Instant::now();
    let mut adaptations = 0;
    loop {
        sink(scrub_http_event(
            tokens,
            StreamEvent::RequestSent(format!(
                "ChatGPT Responses; {} input items; model {}; reasoning.effort={}",
                body["input"].as_array().map_or(0, Vec::len),
                req.model,
                body.pointer("/reasoning/effort")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
            )),
        ));
        let response = client
            .post(endpoint)
            .bearer_auth(&tokens.access_token)
            .json(&body)
            .send()
            .await;
        let mut retry_after = None;
        let mut unsupported = None;
        let result = match response {
            Err(_) => Err(AiError::Subscription("chatgpt_temporary")),
            Ok(response) if !response.status().is_success() => {
                let status = response.status().as_u16();
                retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok());
                // Provider detail and message fields are diagnostic only and never logged.
                let detail = response.json::<Value>().await.unwrap_or(Value::Null);
                unsupported = detail["error"]["param"].as_str().map(str::to_owned);
                Err(failure(status, &detail["error"]))
            }
            Ok(response) => {
                let mut events = Events::with_tokens(tokens);
                let read = read_events(response, |event| {
                    let value = serde_json::from_str(&tokens.scrub(&event.data))
                        .map_err(|_| AiError::Subscription("chatgpt_incomplete"))?;
                    events.read(value, sink)
                })
                .await;
                unsupported = events.unsupported;
                match read {
                    Ok(()) => events
                        .result
                        .ok_or(AiError::Subscription("chatgpt_incomplete")),
                    Err(e @ AiError::Subscription(_)) => Err(e),
                    Err(_) => Err(AiError::Subscription("chatgpt_incomplete")),
                }
            }
        };
        match result {
            Err(e) if e.code() == "chatgpt_capability" && adaptations < 3 => {
                if !unsupported
                    .as_deref()
                    .is_some_and(|param| drop_capability(&mut body, param))
                {
                    return Err(e);
                }
                if matches!(
                    unsupported.as_deref(),
                    Some("reasoning" | "reasoning.effort" | "reasoning[effort]")
                ) {
                    refused_effort
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(req.model.clone());
                }
                sink(StreamEvent::Diagnostic {
                    kind: "CACHE REBUILD".into(),
                    text: "Expected rebuild: the provider refused an optional request setting."
                        .into(),
                });
                adaptations += 1;
                let notice = AiError::Subscription("chatgpt_capability_removed").public_error();
                sink(StreamEvent::Retrying {
                    seconds: 0,
                    code: notice.code,
                    diagnostic: notice.message,
                    discard_partial: true,
                });
            }
            Err(e) if e.code() == "chatgpt_temporary" => {
                let Some(wait) = retry.next_delay(retry_after, started.elapsed()) else {
                    return Err(e);
                };
                sink(StreamEvent::Retrying {
                    seconds: wait.as_secs(),
                    code: e.public_error().code,
                    diagnostic: e.public_error().message,
                    discard_partial: true,
                });
                tokio::time::sleep(wait).await;
            }
            result => return result,
        }
    }
}

pub(super) fn failure(status: u16, detail: &Value) -> AiError {
    let code = match detail["code"]
        .as_str()
        .or_else(|| detail["type"].as_str())
        .unwrap_or("")
    {
        "subscription_sharing_user_not_eligible" => "chatgpt_ineligible",
        "subscription_sharing_usage_limit_exceeded" | "rate_limit_exceeded" => "chatgpt_limit",
        "subscription_sharing_usage_unavailable"
        | "subscription_sharing_user_unavailable"
        | "server_error"
        | "overloaded_error" => "chatgpt_temporary",
        "subscription_sharing_unsupported_capability" => "chatgpt_capability",
        "subscription_sharing_route_not_supported"
        | "chatpass_v2_scope_not_authorized"
        | "chatpass_v2_invalid_authorization_context" => "chatgpt_setup",
        "subscription_sharing_invalid_user" | "invalid_api_key" | "authentication_error" => {
            "chatgpt_access"
        }
        _ => match status {
            401 => "chatgpt_access",
            403 => "chatgpt_policy",
            429 => "chatgpt_limit",
            500..=599 => "chatgpt_temporary",
            _ => "chatgpt_failed",
        },
    };
    AiError::Subscription(code)
}

/// Remove only the specific optional feature identified by the provider. Required
/// protocol fields cannot be removed, and an unchanged body is never sent again.
fn drop_capability(body: &mut Value, param: &str) -> bool {
    let path = param.replace('[', ".").replace(']', "");
    let parts: Vec<_> = path.split('.').collect();
    if !matches!(
        parts.first().copied(),
        Some(
            "input"
                | "tools"
                | "reasoning"
                | "include"
                | "max_output_tokens"
                | "parallel_tool_calls"
                | "prompt_cache_key"
        )
    ) {
        return false;
    }
    if parts == ["input"] {
        return false;
    }
    let mut parent = &mut *body;
    for key in &parts[..parts.len() - 1] {
        parent = match parent {
            Value::Array(a) => match key.parse::<usize>().ok().and_then(|i| a.get_mut(i)) {
                Some(v) => v,
                None => return false,
            },
            Value::Object(o) => match o.get_mut(*key) {
                Some(v) => v,
                None => return false,
            },
            _ => return false,
        };
    }
    let last = parts[parts.len() - 1];
    match parent {
        Value::Object(o) => o.remove(last).is_some(),
        Value::Array(a) => {
            let Some(i) = last.parse::<usize>().ok().filter(|i| *i < a.len()) else {
                return false;
            };
            if parts[0] == "input" && parts.len() == 2 && a.len() == 1 {
                return false;
            }
            a.remove(i);
            true
        }
        _ => false,
    }
}

pub(super) async fn models(endpoint: &str, tokens: &Tokens) -> Result<Vec<PlanModel>, AiError> {
    tokens.require_plan()?;
    let mut retry = RetryBackoff::default();
    let started = Instant::now();
    loop {
        let response = http_client()
            .get(endpoint)
            .bearer_auth(&tokens.access_token)
            .send()
            .await;
        let result = match response {
            Ok(r) if r.status().is_success() => {
                let value: Value = r
                    .json()
                    .await
                    .map_err(|_| AiError::Subscription("chatgpt_failed"))?;
                let models = visible_models(value)?;
                crate::models::remember(
                    endpoint,
                    &models
                        .iter()
                        .map(|model| crate::models::ModelInfo {
                            id: model.id.clone(),
                            label: model.label.clone(),
                            image_input: model.image_input,
                        })
                        .collect::<Vec<_>>(),
                );
                return Ok(models);
            }
            Ok(r) => {
                let status = r.status().as_u16();
                let value = r.json::<Value>().await.unwrap_or(Value::Null);
                failure(status, &value["error"])
            }
            Err(_) => AiError::Subscription("chatgpt_temporary"),
        };
        if result.code() != "chatgpt_temporary" {
            return Err(result);
        }
        let Some(wait) = retry.next_delay(None, started.elapsed()) else {
            return Err(result);
        };
        tokio::time::sleep(wait).await;
    }
}
fn visible_models(value: Value) -> Result<Vec<PlanModel>, AiError> {
    let models = value["models"]
        .as_array()
        .ok_or(AiError::Subscription("chatgpt_failed"))?;
    Ok(models
        .iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| {
            let id = m["slug"].as_str()?.trim();
            let label = m["display_name"].as_str()?.trim();
            (!id.is_empty() && !label.is_empty()).then(|| PlanModel {
                id: id.into(),
                label: label.into(),
                image_input: crate::models::image_input(m),
                // Unknown metadata is not permission to send an optional field.
                reasoning_efforts: m["supported_reasoning_levels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|level| level.as_str().or_else(|| level["effort"].as_str()))
                    .filter(|level| matches!(*level, "low" | "medium" | "high" | "xhigh" | "max"))
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .collect())
}

fn scrub_http_event(tokens: &Tokens, event: StreamEvent) -> StreamEvent {
    match event {
        StreamEvent::RequestSent(text) => StreamEvent::RequestSent(tokens.scrub(&text)),
        StreamEvent::Retrying {
            seconds,
            code,
            diagnostic,
            discard_partial,
        } => StreamEvent::Retrying {
            seconds,
            code,
            diagnostic: tokens.scrub(&diagnostic),
            discard_partial,
        },
        event => event,
    }
}

fn request(req: &TurnRequest) -> Result<Value, AiError> {
    if req.model.trim().is_empty() {
        return Err(AiError::Subscription("chatgpt_model"));
    }
    let mut input = Vec::new();
    for message in &req.messages {
        for block in &message.content {
            match block {
                ContentBlock::Text { text } => {
                    let (role, kind) = if message.role == Role::Assistant {
                        ("assistant", "output_text")
                    } else { ("user", "input_text") };
                    input.push(json!({"role":role,"content":[{"type":kind,"text":text}]}));
                }
                ContentBlock::Image { media_type, data } => input.push(json!({
                    "role":"user","content":[{"type":"input_image","image_url":format!("data:{media_type};base64,{data}")}]
                })),
                ContentBlock::ToolUse { id, name, input: args } => input.push(json!({
                    "type":"function_call","call_id":id,"name":name,"arguments":crate::artifact::canonical(args).to_string()
                })),
                ContentBlock::ToolResult { tool_use_id, content, .. } => {
                    let output: Vec<_> = content.iter().map(|part| match part {
                        ResultPart::Text(text) => json!({"type":"input_text","text":text}),
                        ResultPart::Png(data) => json!({"type":"input_image","image_url":format!("data:image/png;base64,{data}")}),
                    }).collect();
                    input.push(json!({"type":"function_call_output","call_id":tool_use_id,"output":output}));
                }
                ContentBlock::Raw(item) if item["type"] == "reasoning" => input.push(item.clone()),
                ContentBlock::Raw(_) => {}
                ContentBlock::Thinking { .. } => {}
            }
        }
    }
    let tools: Vec<_> = req
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type":"function","name":tool.name,"description":tool.description,
                "parameters":tool.input_schema,"strict":true,
            })
        })
        .collect();
    let mut body = json!({
        "model":req.model, "instructions":req.system, "input":input, "tools":tools,
        "store":false, "stream":true, "max_output_tokens":req.max_tokens,
        "include":["reasoning.encrypted_content"],
        "prompt_cache_key":req.cache_key,
    });
    if !req.effort.is_empty() && req.effort != "none" {
        body["reasoning"] = json!({"effort":req.effort});
    }
    Ok(body)
}

mod streamed;
use streamed::Events;

#[cfg(test)]
mod tests {
    use super::super::PLAN_SCOPE;

    #[test]
    fn cache_key_and_reasoning_prefix_are_stable() {
        let mut req = req();
        req.messages.push(Message::assistant(vec![ContentBlock::Raw(
            json!({"type":"reasoning","encrypted_content":"opaque+/="}),
        )]));
        let first = request(&req).unwrap();
        req.messages.push(Message::user(vec![ContentBlock::Text {
            text: "Continue".into(),
        }]));
        let next = request(&req).unwrap();
        assert_eq!(first["prompt_cache_key"], next["prompt_cache_key"]);
        assert_eq!(
            first["input"].as_array().unwrap(),
            &next["input"].as_array().unwrap()[..first["input"].as_array().unwrap().len()]
        );
        assert_eq!(first["tools"], next["tools"]);
        assert_eq!(first["instructions"], next["instructions"]);
    }
    use super::*;
    use crate::Message;

    /// One model step of a long turn: its reasoning item, a call, and the call's big result.
    fn step(i: usize, size: usize) -> Vec<Message> {
        vec![
            Message::assistant(vec![
                ContentBlock::Raw(
                    json!({"type":"reasoning","encrypted_content":format!("opaque{i}")}),
                ),
                ContentBlock::ToolUse {
                    id: format!("call_{i}"),
                    name: "qiyas_read".into(),
                    input: json!({"op":"records","args":"{}"}),
                },
            ]),
            Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: format!("call_{i}"),
                content: vec![ResultPart::Text("x".repeat(size))],
                is_error: false,
            }]),
        ]
    }

    /// Evidence for the cache work (W1): the body a long turn sends only ever grows at its end,
    /// request after request, with the same instructions, tools and cache key; the one thing that
    /// moves an early part of it is a trim at the watermark, which happens once and then the
    /// requests append again. So whatever the provider fails to reuse between two trims is not
    /// caused by how the request is built.
    #[test]
    fn a_long_turn_only_appends_between_trims_and_a_trim_rewrites_the_prefix_once() {
        let body = |history: &[Message]| {
            let mut r = req();
            r.messages = history.to_vec();
            request(&r).unwrap()
        };
        let appended = |before: &Value, after: &Value| {
            let (a, b) = (
                before["input"].as_array().unwrap(),
                after["input"].as_array().unwrap(),
            );
            assert!(b.len() > a.len());
            assert_eq!(&b[..a.len()], &a[..], "an earlier item changed");
            for field in ["instructions", "tools", "prompt_cache_key", "model", "reasoning"] {
                assert_eq!(before[field], after[field], "{field} changed");
            }
        };
        let mut history = vec![Message::user(vec![ContentBlock::Text {
            text: "Build the package".into(),
        }])];
        let mut bodies = Vec::new();
        for i in 0..40 {
            history.extend(step(i, 9_000));
            bodies.push(body(&history));
        }
        for pair in bodies.windows(2) {
            appended(&pair[0], &pair[1]);
        }

        let before = bodies.last().unwrap().clone();
        let trim = crate::history::trim_at(&mut history, 50_000, 20_000, 10_000)
            .expect("the watermark was crossed");
        assert!(trim.after < trim.before);
        let after = body(&history);
        let (a, b) = (
            before["input"].as_array().unwrap(),
            after["input"].as_array().unwrap(),
        );
        let first = a.iter().zip(b).position(|(x, y)| x != y).unwrap();
        assert!(first <= 4, "the oldest result is the first to be retired: item {first}");
        assert_eq!(a.len(), b.len(), "retiring evidence keeps every call and its result");

        // After the trim the turn appends again.
        let mut previous = after;
        for i in 40..50 {
            history.extend(step(i, 9_000));
            let next = body(&history);
            appended(&previous, &next);
            previous = next;
        }
    }

    fn req() -> TurnRequest {
        TurnRequest {
            cache_key: "offline-test-conversation".into(),
            system: "You are Mizan.".into(),
            model: "mock-model".into(),
            effort: "none".into(),
            max_tokens: 100,
            messages: vec![Message::user(vec![ContentBlock::Text {
                text: "Measure".into(),
            }])],
            tools: crate::catalogue(),
        }
    }
    fn completed() -> Value {
        json!({"type":"response.completed","response":{"output":[
            {"type":"reasoning","id":"reasoning-1","encrypted_content":"opaque-test-reasoning","summary":[]},
            {"type":"message","content":[{"type":"output_text","text":"Measured."}]},
            {"type":"function_call","call_id":"call-1","name":"qiyas_read","arguments":"{\"op\":\"state\",\"args\":\"{}\"}"}
        ],"usage":{"input_tokens":100,"output_tokens":20,"input_tokens_details":{"cached_tokens":25},"output_tokens_details":{"reasoning_tokens":10}}}})
    }

    #[test]
    fn responses_stream_preserves_tools_reasoning_usage_and_stateless_requests() {
        let mut events = Events::default();
        let mut seen = Vec::new();
        events.read(json!({"type":"response.output_item.added","item":{"type":"function_call","id":"item-1","call_id":"call-1","name":"qiyas_read"}}), &mut |e| seen.push(e)).unwrap();
        events.read(json!({"type":"response.function_call_arguments.delta","item_id":"item-1","delta":"{\"op\":"}), &mut |e| seen.push(e)).unwrap();
        events
            .read(
                json!({"type":"response.output_text.delta","delta":"Measured."}),
                &mut |e| seen.push(e),
            )
            .unwrap();
        events
            .read(
                json!({"type":"response.reasoning_summary_text.delta","delta":"Checking."}),
                &mut |e| seen.push(e),
            )
            .unwrap();
        assert!(events.read(completed(), &mut |e| seen.push(e)).unwrap());
        assert!(seen
            .iter()
            .any(|e| matches!(e, StreamEvent::ToolInputDelta { id, .. } if id == "call-1")));
        let result = events.result.unwrap();
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert_eq!(result.usage.input_tokens, 75);
        assert_eq!(result.usage.cache_read_input_tokens, 25);
        assert_eq!(result.usage.reasoning_tokens, 10);
        let mut req = req();
        req.messages.push(Message::assistant(result.content));
        req.messages
            .push(Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: "call-1".into(),
                content: vec![
                    ResultPart::Text("Read back".into()),
                    ResultPart::Png("fake-image".into()),
                ],
                is_error: false,
            }]));
        let body = request(&req).unwrap();
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"].as_array().unwrap().len(), 3);
        assert!(body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["type"] == "function"));
        assert!(body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["encrypted_content"] == "opaque-test-reasoning"));
        assert!(body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "function_call_output" && v["call_id"] == "call-1"));
    }

    #[test]
    fn subscription_errors_have_their_own_retry_and_account_policy() {
        for (code, expected) in [
            (
                "subscription_sharing_user_not_eligible",
                "chatgpt_ineligible",
            ),
            ("subscription_sharing_usage_limit_exceeded", "chatgpt_limit"),
            (
                "subscription_sharing_usage_unavailable",
                "chatgpt_temporary",
            ),
            (
                "subscription_sharing_unsupported_capability",
                "chatgpt_capability",
            ),
            ("subscription_sharing_route_not_supported", "chatgpt_setup"),
            ("subscription_sharing_invalid_user", "chatgpt_access"),
            ("chatpass_v2_scope_not_authorized", "chatgpt_setup"),
            ("chatpass_v2_invalid_authorization_context", "chatgpt_setup"),
            ("subscription_sharing_user_unavailable", "chatgpt_temporary"),
        ] {
            let error = Events::default().read(json!({"type":"response.failed","response":{"error":{"code":code,"message":"fake-secret"}}}), &mut |_| {}).unwrap_err();
            assert_eq!(error.code(), expected);
            assert!(
                !error.retryable(),
                "The generic API retry policy must not handle SIWC"
            );
            assert!(!error.for_window().contains("fake-secret"));
        }
        for (status, expected) in [
            (401, "chatgpt_access"),
            (403, "chatgpt_policy"),
            (503, "chatgpt_temporary"),
        ] {
            assert_eq!(failure(status, &Value::Null).code(), expected);
        }
    }
    #[test]
    fn incomplete_is_never_success_and_unsupported_body_must_change() {
        let mut e = Events::default();
        assert_eq!(e.read(json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}), &mut |_| {}).unwrap_err().code(), "chatgpt_incomplete");
        assert!(e.result.is_none());
        let mut body = request(&req()).unwrap();
        let before = body.clone();
        assert!(drop_capability(&mut body, "tools[0]"));
        assert_ne!(before, body);
        assert!(!drop_capability(&mut body, "store"));
        assert!(!drop_capability(&mut body, "input"));
        assert!(!drop_capability(&mut body, "input[0]"));
        assert!(!drop_capability(&mut body, "tools[999]"));
        assert!(drop_capability(&mut body, "include"));
        assert!(!drop_capability(&mut body, "include"));
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
    }
    #[test]
    fn models_use_only_visible_slugs_and_display_names() {
        let models = visible_models(json!({"models":[
            {"slug":"visible-slug","display_name":"Visible model","visibility":"list"},
            {"slug":"hidden","display_name":"Hidden model","visibility":"hidden"},
            {"slug":"missing-label","visibility":"list"}
        ]}))
        .unwrap();
        assert_eq!(
            models,
            vec![PlanModel {
                image_input: None,
                id: "visible-slug".into(),
                reasoning_efforts: Vec::new(),
                label: "Visible model".into()
            }]
        );
    }

    #[tokio::test]
    #[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
    async fn mock_responses_server_gets_only_mizan_requests_and_no_stored_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock Responses needs a loopback port");
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(end) = buffer.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&buffer[..end]).to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if buffer.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let text = String::from_utf8(buffer).unwrap();
            let body = format!("data: {}\n\n", completed());
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            text
        });
        let tokens = Tokens {
            access_token: "fake-access-secret".into(),
            refresh_token: Some("fake-refresh-secret".into()),
            id_token: "fake-id-secret".into(),
            scopes: vec![PLAN_SCOPE.into()],
            expires_at: super::super::oauth::now() + 3600,
            client_id: "fake-client".into(),
            subject: "fake-person".into(),
            display_name: "Test Surveyor".into(),
        };
        let mut seen = Vec::new();
        let result = stream(&endpoint, &tokens, req(), &Mutex::default(), &mut |e| {
            seen.push(e)
        })
        .await
        .unwrap();
        assert_eq!(result.usage.output_tokens, 20);
        let received = server.await.unwrap();
        let (headers, body) = received.split_once("\r\n\r\n").unwrap();
        assert!(headers
            .to_lowercase()
            .contains("authorization: bearer fake-access-secret"));
        assert!(!received.contains("fake-refresh-secret"));
        assert!(!received.contains("fake-id-secret"));
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        for secret in [
            "fake-access-secret",
            "fake-refresh-secret",
            "fake-id-secret",
        ] {
            assert!(!format!("{seen:?}").contains(secret));
        }
    }

    #[test]
    fn http_retry_diagnostics_cannot_leak_any_of_the_three_tokens() {
        let tokens = Tokens {
            access_token: "fake-access-secret".into(),
            refresh_token: Some("fake-refresh-secret".into()),
            id_token: "fake-id-secret".into(),
            scopes: vec![PLAN_SCOPE.into()],
            expires_at: 0,
            client_id: "test".into(),
            subject: "test".into(),
            display_name: "Test".into(),
        };
        let event = scrub_http_event(
            &tokens,
            StreamEvent::Retrying {
                seconds: 1,
                code: "ai.error.service_error".into(),
                discard_partial: false,
                diagnostic: "Service echoed fake-access-secret fake-refresh-secret fake-id-secret"
                    .into(),
            },
        );
        for secret in [
            "fake-access-secret",
            "fake-refresh-secret",
            "fake-id-secret",
        ] {
            assert!(!format!("{event:?}").contains(secret));
        }
    }
}

#[cfg(test)]
mod streamed_regressions {
    use super::*;

    fn run(items: Vec<Value>, output: Value) -> TurnResult {
        let mut events = Events::default();
        for item in items {
            events.read(item, &mut |_| {}).unwrap();
        }
        events.read(json!({"type":"response.completed","response":{"output":output,"usage":{"output_tokens":23}}}), &mut |_| {}).unwrap();
        events.result.unwrap()
    }

    #[test]
    fn delta_only_answer_survives_empty_completed_output() {
        let result = run(
            vec![
                json!({"type":"response.output_text.delta","item_id":"msg-1","output_index":0,"content_index":0,"delta":"Hello "}),
                json!({"type":"response.output_text.delta","item_id":"msg-1","output_index":0,"content_index":0,"delta":"surveyor."}),
            ],
            json!([]),
        );
        assert_eq!(
            result.content,
            vec![ContentBlock::Text {
                text: "Hello surveyor.".into()
            }]
        );
    }

    #[test]
    fn streamed_call_survives_partial_completed_output() {
        let result = run(
            vec![
                json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"qiyas_read","arguments":""}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc-1","delta":"{\"op\":\"state\",\"args\":\"{}\"}"}),
                json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"qiyas_read","arguments":"{\"op\":\"state\",\"args\":\"{}\"}"}}),
            ],
            json!([{"type":"reasoning","id":"rs-1","summary":[]} ]),
        );
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(result.content.iter().any(|c| matches!(c,ContentBlock::ToolUse{id,input,..} if id=="call-1" && input["op"]=="state")));
    }
}

#[cfg(test)]
mod acceptance;

#[cfg(test)]
mod capability_regressions {
    use super::*;
    #[test]
    fn model_discovery_never_invents_effort_support() {
        let models=visible_models(json!({"models":[
            {"slug":"with-effort","display_name":"With effort","visibility":"list","supported_reasoning_levels":[{"effort":"low","display_name":"Low"},{"effort":"high"}]},
            {"slug":"without-effort","display_name":"Without effort","visibility":"list"},
            {"slug":"future","display_name":"Unknown","visibility":"list","supported_reasoning_levels":["medium","invented"]}
        ]})).unwrap();
        assert_eq!(models[0].reasoning_efforts, vec!["low", "high"]);
        assert!(models[1].reasoning_efforts.is_empty());
        assert_eq!(models[2].reasoning_efforts, vec!["medium"]);
    }

    #[tokio::test]
    async fn unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Capability adaptation test needs loopback sockets");
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut bodies = Vec::new();
            for index in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut received = Vec::new();
                loop {
                    let mut buf = [0; 8192];
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    received.extend_from_slice(&buf[..n]);
                    if let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&received[..end]).to_lowercase();
                        let len: usize = header
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        if received.len() >= end + 4 + len {
                            bodies.push(
                                serde_json::from_slice::<Value>(&received[end + 4..end + 4 + len])
                                    .unwrap(),
                            );
                            break;
                        }
                    }
                }
                let (status, kind, body) = if index == 0 {
                    (400,"application/json",json!({"error":{"code":"subscription_sharing_unsupported_capability","param":"reasoning.effort"}}).to_string())
                } else {
                    (
                        200,
                        "text/event-stream",
                        format!(
                            "data: {}\n\n",
                            json!({"type":"response.completed","response":{"output":[{"type":"message","content":[{"type":"output_text","text":"Ready."}]}],"usage":{"output_tokens":2}}})
                        ),
                    )
                };
                socket.write_all(format!("HTTP/1.1 {status} Reply\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
            bodies
        });
        let tokens = Tokens {
            access_token: "fake-access".into(),
            refresh_token: None,
            id_token: "fake-id".into(),
            scopes: vec![super::super::PLAN_SCOPE.into()],
            expires_at: 0,
            client_id: "fake".into(),
            subject: "fake".into(),
            display_name: "Fake".into(),
        };
        let req = TurnRequest {
            cache_key: "offline-test-conversation".into(),
            system: "Mizan".into(),
            messages: vec![crate::Message::user(vec![ContentBlock::Text {
                text: "Hi".into(),
            }])],
            tools: crate::catalogue(),
            model: "advertised".into(),
            effort: "high".into(),
            max_tokens: 100,
        };
        let refused = Mutex::default();
        stream(&endpoint, &tokens, req.clone(), &refused, &mut |_| {})
            .await
            .unwrap();
        stream(&endpoint, &tokens, req, &refused, &mut |_| {})
            .await
            .unwrap();
        let bodies = server.await.unwrap();
        assert_eq!(bodies[0]["reasoning"]["effort"], "high");
        assert!(bodies[1].pointer("/reasoning/effort").is_none());
        assert!(bodies[2].pointer("/reasoning/effort").is_none());
        assert!(refused.lock().unwrap().contains("advertised"));
    }
}
