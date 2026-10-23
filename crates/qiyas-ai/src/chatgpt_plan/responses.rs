use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use super::Tokens;
use crate::provider::{http_client, read_events, response_error, send_with_retry, Sink};
use crate::{
    AiError, ContentBlock, ResultPart, RetryBackoff, Role, StopReason, StreamEvent, TurnRequest,
    TurnResult, Usage,
};

/// This adapter is private to the Mizan provider; MCP never exposes it or its credentials.
pub(super) async fn stream(
    endpoint: &str,
    tokens: &Tokens,
    req: TurnRequest,
    sink: Sink<'_>,
) -> Result<TurnResult, AiError> {
    tokens.require_plan()?;
    let body = request(&req)?;
    let client = http_client();
    let mut retry = RetryBackoff::default();
    let started = Instant::now();
    loop {
        let mut http_sink = |event| sink(scrub_http_event(tokens, event));
        let response = send_with_retry(
            || {
                client
                    .post(endpoint)
                    .bearer_auth(&tokens.access_token)
                    .json(&body)
            },
            &tokens.access_token,
            &body,
            true,
            &mut http_sink,
        )
        .await
        .map_err(|e| {
            e.scrubbed(&tokens.id_token)
                .scrubbed(tokens.refresh_token.as_deref().unwrap_or(""))
        })?;
        let mut events = Events::default();
        let result = read_events(response, |event| {
            let text = tokens.scrub(&event.data);
            let value = serde_json::from_str(&text)
                .map_err(|_| AiError::Stream("ChatGPT returned an unreadable event".into()))?;
            events.read(value, sink)
        })
        .await;
        match result {
            Ok(()) => {
                return events
                    .result
                    .ok_or_else(|| AiError::Stream("ChatGPT did not finish its answer".into()))
            }
            Err(error) if error.retryable() => {
                let Some(wait) = retry.next_delay(None, started.elapsed()) else {
                    return Err(error);
                };
                sink(StreamEvent::Retrying {
                    seconds: wait.as_secs(),
                    code: error.public_error().code,
                    diagnostic: tokens.scrub(&error.to_string()),
                    discard_partial: true,
                });
                tokio::time::sleep(wait).await;
            }
            Err(error) => return Err(error),
        }
    }
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
                    "type":"function_call","call_id":id,"name":name,"arguments":args.to_string()
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
    });
    if req.effort != "none" {
        body["reasoning"] = json!({"effort":req.effort});
    }
    Ok(body)
}

#[derive(Default)]
struct Events {
    call_ids: HashMap<String, String>,
    result: Option<TurnResult>,
}

impl Events {
    fn read(&mut self, value: Value, sink: Sink<'_>) -> Result<bool, AiError> {
        match value["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => {
                if let Some(text) = value["delta"].as_str() {
                    sink(StreamEvent::TextDelta(text.into()));
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(text) = value["delta"].as_str() {
                    sink(StreamEvent::Thinking(text.into()));
                }
            }
            "response.output_item.added" if value["item"]["type"] == "function_call" => {
                let item = &value["item"];
                if let (Some(item_id), Some(id), Some(name)) = (
                    item["id"].as_str(),
                    item["call_id"].as_str(),
                    item["name"].as_str(),
                ) {
                    self.call_ids.insert(item_id.into(), id.into());
                    sink(StreamEvent::ToolUseStart {
                        id: id.into(),
                        name: name.into(),
                    });
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some(id), Some(text)) = (
                    value["item_id"]
                        .as_str()
                        .and_then(|id| self.call_ids.get(id)),
                    value["delta"].as_str(),
                ) {
                    sink(StreamEvent::ToolInputDelta {
                        id: id.clone(),
                        json_fragment: text.into(),
                    });
                }
            }
            "response.completed" | "response.incomplete" => {
                let response = &value["response"];
                let mut content = Vec::new();
                let mut refused = None;
                for item in response["output"].as_array().into_iter().flatten() {
                    match item["type"].as_str().unwrap_or("") {
                        "message" => {
                            for block in item["content"].as_array().into_iter().flatten() {
                                if block["type"] == "output_text" {
                                    content.push(ContentBlock::Text {
                                        text: block["text"].as_str().unwrap_or("").into(),
                                    });
                                }
                                if block["type"] == "refusal" {
                                    refused =
                                        Some(block["refusal"].as_str().unwrap_or("").to_owned());
                                }
                            }
                        }
                        "reasoning" => content.push(ContentBlock::Raw(item.clone())),
                        "function_call" => {
                            let id = item["call_id"].as_str().ok_or_else(|| {
                                AiError::Stream("ChatGPT omitted a tool call ID".into())
                            })?;
                            let name = item["name"].as_str().ok_or_else(|| {
                                AiError::Stream("ChatGPT omitted a tool name".into())
                            })?;
                            let raw = item["arguments"].as_str().unwrap_or("");
                            let input = serde_json::from_str(raw)
                                .unwrap_or_else(|_| Value::String(raw.into()));
                            sink(StreamEvent::ToolUse {
                                id: id.into(),
                                name: name.into(),
                                input: input.clone(),
                            });
                            content.push(ContentBlock::ToolUse {
                                id: id.into(),
                                name: name.into(),
                                input,
                            });
                        }
                        _ => {}
                    }
                }
                let stop_reason = if let Some(text) = refused {
                    StopReason::Refusal {
                        category: None,
                        text,
                    }
                } else if value["type"] == "response.incomplete" {
                    if response["incomplete_details"]["reason"] == "max_output_tokens" {
                        StopReason::MaxTokens
                    } else {
                        return Err(AiError::Stream(
                            "ChatGPT did not complete its answer".into(),
                        ));
                    }
                } else if content
                    .iter()
                    .any(|c| matches!(c, ContentBlock::ToolUse { .. }))
                {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                };
                let u = &response["usage"];
                let cached = u["input_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                self.result = Some(TurnResult {
                    content,
                    stop_reason,
                    usage: Usage {
                        input_tokens: u["input_tokens"]
                            .as_u64()
                            .unwrap_or(0)
                            .saturating_sub(cached),
                        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                        cache_read_input_tokens: cached,
                        reasoning_tokens: u["output_tokens_details"]["reasoning_tokens"]
                            .as_u64()
                            .unwrap_or(0),
                        ..Usage::default()
                    },
                });
                sink(StreamEvent::Done);
                return Ok(true);
            }
            "response.failed" | "error" => {
                let detail = value.pointer("/response/error").unwrap_or(&value);
                let code = detail["code"]
                    .as_str()
                    .or_else(|| detail["type"].as_str())
                    .unwrap_or("");
                let status = match code {
                    "rate_limit_exceeded" => 429,
                    "server_error" | "overloaded_error" => 503,
                    "invalid_api_key" | "authentication_error" => 401,
                    _ => 400,
                };
                return Err(response_error(
                    status,
                    &json!({"error":detail}).to_string(),
                    &Value::Null,
                    "",
                ));
            }
            _ => {}
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::super::PLAN_SCOPE;
    use super::*;
    use crate::Message;

    fn req() -> TurnRequest {
        TurnRequest {
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
    fn stream_billing_is_terminal_and_overload_is_retryable() {
        for (code, retryable) in [
            ("insufficient_quota", false),
            ("invalid_api_key", false),
            ("server_error", true),
            ("rate_limit_exceeded", true),
        ] {
            let error=Events::default().read(json!({"type":"response.failed","response":{"error":{"code":code,"message":code}}}), &mut |_|{}).unwrap_err();
            assert_eq!(error.retryable(), retryable, "{code}");
        }
    }

    #[tokio::test]
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
        let result = stream(&endpoint, &tokens, req(), &mut |e| seen.push(e))
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
