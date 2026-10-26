//! Claude over raw HTTPS: the Messages API with streaming (SSE) and client tools. There is no
//! official Anthropic SDK for Rust, so the request and the events are built and read here.

use std::collections::BTreeMap;

use reqwest::header::HeaderValue;
use serde_json::{json, Value};

use crate::provider::{
    http_client, read_events, send_with_retry, AiError, BoxFuture, ContentBlock, Message, Provider,
    ResultPart, Role, Sink, StopReason, StreamEvent, TurnRequest, TurnResult, Usage,
};
use crate::ToolSpec;

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2023-06-01";
/// The beta for `fallbacks: "default"`: on a policy decline the API reruns the turn on its
/// recommended fallback model inside the same call.
const BETA: &str = "server-side-fallback-2026-07-01";

/// The Claude API. The key is only ever put in the `x-api-key` header; this type has no `Debug`
/// so it can't be printed by accident.
pub struct Anthropic {
    api_key: String,
    url: String,
    client: reqwest::Client,
    strict_budget: bool,
}

impl Anthropic {
    pub fn new(api_key: String) -> Anthropic {
        Anthropic::with_url(api_key, API_URL.to_string())
    }

    /// Another endpoint for the same API (tests use a local server).
    pub fn with_url(api_key: String, url: String) -> Anthropic {
        Anthropic {
            api_key,
            url,
            client: http_client(),
            strict_budget: false,
        }
    }

    /// Budgeted evaluations must not switch to an unpriced server-side fallback model.
    pub fn with_strict_budget(mut self) -> Self {
        self.strict_budget = true;
        self
    }
}

impl Provider for Anthropic {
    fn image_input(&self, model: &str) -> Option<bool> {
        crate::models::known_image_input(API_URL, model).or_else(|| {
            crate::models::CLAUDE_MODELS
                .iter()
                .any(|(id, _)| *id == model)
                .then_some(true)
        })
    }
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let mut body = request_body(&req);
            if self.strict_budget {
                body.as_object_mut().unwrap().remove("fallbacks");
            }
            // Marked sensitive so the HTTP stack never prints it; a key that is not a valid
            // header value can only be refused.
            let mut key = HeaderValue::from_str(&self.api_key).map_err(|_| AiError::Auth)?;
            key.set_sensitive(true);
            let started = tokio::time::Instant::now();
            let mut backoff = crate::provider::RetryBackoff::default();
            loop {
                let response = send_with_retry(
                    || {
                        let request = self
                            .client
                            .post(&self.url)
                            .header("x-api-key", key.clone())
                            .header("anthropic-version", VERSION)
                            .json(&body);
                        if self.strict_budget {
                            request
                        } else {
                            request.header("anthropic-beta", BETA)
                        }
                    },
                    &self.api_key,
                    &body,
                    !self.strict_budget,
                    sink,
                )
                .await
                .map_err(|e| e.for_model("Anthropic", &req.model))?;
                let mut turn = Turn::default();
                if let Err(error) = read_events(response, |e| turn.on_event(&e.data, sink)).await {
                    let error = error
                        .scrubbed(&self.api_key)
                        .for_model("Anthropic", &req.model);
                    // A retry clears any text/thinking shown from this unfinished response.
                    if crate::provider::retry_stream(
                        &error,
                        !self.strict_budget,
                        &mut backoff,
                        started,
                        sink,
                        true,
                    )
                    .await
                    {
                        continue;
                    }
                    return Err(error);
                }
                return Ok(turn.finish());
            }
        })
    }
}

/// The JSON body. Cache breakpoints on the last tool and on the system block, plus automatic
/// caching of the whole conversation prefix at the top level.
pub fn request_body(req: &TurnRequest) -> Value {
    let mut tools: Vec<Value> = req.tools.iter().map(tool_json).collect();
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral", "ttl": "1h" });
    }
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "system": [{
            "type": "text",
            "text": req.system,
            "cache_control": { "type": "ephemeral", "ttl": "1h" }
        }],
        "messages": req.messages.iter().map(message_json).collect::<Vec<_>>(),
        "output_config": { "effort": req.effort },
        "fallbacks": "default",
        // Automatic caching: the API caches the longest prefix it has seen, so each turn of a
        // conversation reads the one before it.
        "cache_control": { "type": "ephemeral" },
    });
    if req.effort == "none" {
        body.as_object_mut().unwrap().remove("output_config");
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    // Three explicit long-lived boundaries, then automatic caching on the newest block
    // with the default five-minute TTL. Never mark signed thinking as a breakpoint.
    if let Some(messages) = body["messages"].as_array_mut() {
        let stable_end = messages.len().saturating_sub(1);
        if let Some(block) = messages[..stable_end]
            .iter_mut()
            .rev()
            .filter_map(|message| message["content"].as_array_mut())
            .flat_map(|content| content.iter_mut().rev())
            .find(|block| {
                matches!(
                    block["type"].as_str(),
                    Some("text" | "tool_result" | "tool_use" | "image")
                )
            })
        {
            block["cache_control"] = json!({"type":"ephemeral","ttl":"1h"});
        }
    }
    body
}

fn tool_json(t: &ToolSpec) -> Value {
    json!({
        "name": t.name,
        "description": t.description,
        "input_schema": t.input_schema,
        "strict": true,
    })
}

/// One message as the API wants it. In an assistant message, the blocks before the last
/// `fallback` block are from a declined attempt: only its text goes back (and the markers).
fn message_json(m: &Message) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let boundary = m.fallback_boundary();
    let content: Vec<Value> = m
        .content
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            *i >= boundary
                || matches!(b, ContentBlock::Text { .. })
                || matches!(b, ContentBlock::Raw(v) if v["type"] == "fallback")
        })
        .map(|(_, b)| block_json(b))
        .collect();
    json!({ "role": role, "content": content })
}

fn block_json(b: &ContentBlock) -> Value {
    match b {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::ToolUse { id, name, input } => {
            // An input that was not valid JSON goes back as an empty object; its tool_result
            // says it was refused.
            let input = if input.is_object() {
                crate::artifact::arguments(input)
            } else {
                json!({})
            };
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let parts: Vec<Value> = content
                .iter()
                .map(|p| match p {
                    ResultPart::Text(text) => json!({ "type": "text", "text": text }),
                    ResultPart::Png(data) => json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": "image/png", "data": data }
                    }),
                })
                .collect();
            let mut v =
                json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": parts });
            if *is_error {
                v["is_error"] = json!(true);
            }
            v
        }
        ContentBlock::Image { media_type, data } => json!({
            "type": "image",
            "source": { "type": "base64", "media_type": media_type, "data": data }
        }),
        ContentBlock::Thinking {
            thinking,
            signature,
        } => json!({ "type": "thinking", "thinking": thinking, "signature": signature }),
        ContentBlock::Raw(v) => v.clone(),
    }
}

/// A content block while it streams.
enum Building {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        json: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    Raw(Value),
}

/// One streamed answer, built up event by event.
#[derive(Default)]
struct Turn {
    /// Blocks by their stream index; finished blocks are replaced by `done`.
    building: BTreeMap<usize, Building>,
    done: BTreeMap<usize, ContentBlock>,
    stop_reason: Option<String>,
    stop_details: Value,
    usage: Usage,
}

impl Turn {
    /// Handles one event's data; `Ok(true)` once the message has stopped.
    fn on_event(&mut self, data: &str, sink: Sink<'_>) -> Result<bool, AiError> {
        let Ok(e) = serde_json::from_str::<Value>(data) else {
            return Err(AiError::Stream("an event was not valid JSON".into()));
        };
        let index = e["index"].as_u64().unwrap_or(0) as usize;
        match e["type"].as_str().unwrap_or("") {
            "message_start" => self.read_usage(&e["message"]["usage"]),
            "content_block_start" => {
                let b = &e["content_block"];
                if b["type"] == "fallback" {
                    sink(StreamEvent::Diagnostic {
                        kind: "CACHE REBUILD".into(),
                        text: "Expected rebuild: the service selected its fallback model.".into(),
                    });
                }
                let building = match b["type"].as_str().unwrap_or("") {
                    "text" => Building::Text(b["text"].as_str().unwrap_or("").to_string()),
                    "tool_use" => {
                        let id = b["id"].as_str().unwrap_or("").to_string();
                        let name = b["name"].as_str().unwrap_or("").to_string();
                        sink(StreamEvent::ToolUseStart {
                            id: id.clone(),
                            name: name.clone(),
                        });
                        Building::ToolUse {
                            id,
                            name,
                            json: String::new(),
                        }
                    }
                    "thinking" => Building::Thinking {
                        thinking: b["thinking"].as_str().unwrap_or("").to_string(),
                        signature: b["signature"].as_str().unwrap_or("").to_string(),
                    },
                    _ => Building::Raw(b.clone()),
                };
                self.building.insert(index, building);
            }
            "content_block_delta" => {
                let d = &e["delta"];
                let text = |key: &str| d[key].as_str().unwrap_or("").to_string();
                match (self.building.get_mut(&index), d["type"].as_str()) {
                    (Some(Building::Text(t)), Some("text_delta")) => {
                        let piece = text("text");
                        t.push_str(&piece);
                        sink(StreamEvent::TextDelta(piece));
                    }
                    (Some(Building::ToolUse { id, json, .. }), Some("input_json_delta")) => {
                        let piece = text("partial_json");
                        json.push_str(&piece);
                        sink(StreamEvent::ToolInputDelta {
                            id: id.clone(),
                            json_fragment: piece,
                        });
                    }
                    (Some(Building::Thinking { thinking, .. }), Some("thinking_delta")) => {
                        let piece = text("thinking");
                        thinking.push_str(&piece);
                        sink(StreamEvent::Thinking(piece));
                    }
                    (Some(Building::Thinking { signature, .. }), Some("signature_delta")) => {
                        signature.push_str(&text("signature"));
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(b) = self.building.remove(&index) {
                    let block = finish_block(b, sink);
                    self.done.insert(index, block);
                }
            }
            "message_delta" => {
                if let Some(r) = e["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(r.to_string());
                }
                let details = &e["delta"]["stop_details"];
                self.stop_details = if details.is_null() {
                    e["stop_details"].clone()
                } else {
                    details.clone()
                };
                self.read_usage(&e["usage"]);
            }
            "message_stop" => {
                sink(StreamEvent::Done);
                return Ok(true);
            }
            "error" => {
                let err = &e["error"];
                return Err(AiError::Stream(format!(
                    "{}: {}",
                    err["type"].as_str().unwrap_or("error"),
                    err["message"].as_str().unwrap_or("")
                )));
            }
            _ => {} // ping and anything new
        }
        Ok(false)
    }

    /// Usage fields present in `u` replace the ones seen so far (the API's counts are totals).
    fn read_usage(&mut self, u: &Value) {
        let set = |field: &mut u64, key: &str| {
            if let Some(n) = u[key].as_u64() {
                *field = n;
            }
        };
        set(&mut self.usage.input_tokens, "input_tokens");
        set(&mut self.usage.output_tokens, "output_tokens");
        if let Some(n) = u["output_tokens_details"]["reasoning_tokens"].as_u64() {
            self.usage.reasoning_tokens = n;
        }
        set(
            &mut self.usage.cache_creation_input_tokens,
            "cache_creation_input_tokens",
        );
        set(
            &mut self.usage.cache_read_input_tokens,
            "cache_read_input_tokens",
        );
    }

    fn finish(mut self) -> TurnResult {
        // Blocks that never got their stop event are kept as they are.
        let open: Vec<usize> = self.building.keys().copied().collect();
        for index in open {
            if let Some(b) = self.building.remove(&index) {
                self.done.insert(index, finish_block(b, &mut |_| {}));
            }
        }
        let content: Vec<ContentBlock> = self.done.into_values().collect();
        let stop_reason = match self.stop_reason.as_deref() {
            Some("end_turn") | Some("stop_sequence") | None => StopReason::EndTurn,
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("refusal") => {
                let explanation = self.stop_details["explanation"].as_str().unwrap_or("");
                let text = if explanation.is_empty() {
                    content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("")
                } else {
                    explanation.to_string()
                };
                StopReason::Refusal {
                    category: self.stop_details["category"].as_str().map(str::to_string),
                    text,
                }
            }
            Some(other) => StopReason::Other(other.to_string()),
        };
        TurnResult {
            content,
            stop_reason,
            usage: self.usage,
        }
    }
}

/// A finished block. A tool input is parsed once, here, as a whole; one that is not valid JSON
/// is kept as its raw text (a JSON string) so the agent answers it with `INVALID_JSON`.
fn finish_block(b: Building, sink: Sink<'_>) -> ContentBlock {
    match b {
        Building::Text(text) => ContentBlock::Text { text },
        Building::ToolUse { id, name, json } => {
            let parsed = if json.trim().is_empty() {
                Ok(json!({}))
            } else {
                serde_json::from_str::<Value>(&json)
            };
            let input = match parsed {
                Ok(v) if v.is_object() => {
                    sink(StreamEvent::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: v.clone(),
                    });
                    v
                }
                _ => Value::String(json),
            };
            ContentBlock::ToolUse { id, name, input }
        }
        Building::Thinking {
            thinking,
            signature,
        } => ContentBlock::Thinking {
            thinking,
            signature,
        },
        Building::Raw(v) => ContentBlock::Raw(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lived_breakpoints_precede_the_newest_default_ttl() {
        let request = TurnRequest {
            cache_key: "conversation-1".into(),
            system: "Stable rules".into(),
            tools: crate::catalogue(),
            model: "mock".into(),
            effort: "high".into(),
            max_tokens: 100,
            messages: vec![
                Message::user(vec![ContentBlock::Text {
                    text: "First brief".into(),
                }]),
                Message::assistant(vec![
                    ContentBlock::Thinking {
                        thinking: "reason".into(),
                        signature: "opaque".into(),
                    },
                    ContentBlock::Text {
                        text: "Answer".into(),
                    },
                ]),
                Message::user(vec![ContentBlock::Text {
                    text: "Continue".into(),
                }]),
            ],
        };
        let before = request_body(&request);
        assert_eq!(before["tools"][2]["cache_control"]["ttl"], "1h");
        assert_eq!(before["system"][0]["cache_control"]["ttl"], "1h");
        assert_eq!(
            before["messages"][1]["content"][1]["cache_control"]["ttl"],
            "1h"
        );
        assert!(before["messages"][1]["content"][0]
            .get("cache_control")
            .is_none());
        assert_eq!(before["cache_control"], json!({"type":"ephemeral"}));
        assert_eq!(before.to_string().matches("cache_control").count(), 4);
        let mut next = request.clone();
        next.messages
            .push(Message::assistant(vec![ContentBlock::Text {
                text: "Next".into(),
            }]));
        next.messages.push(Message::user(vec![ContentBlock::Text {
            text: "Again".into(),
        }]));
        let after = request_body(&next);
        assert_eq!(before["system"], after["system"]);
        assert_eq!(before["tools"], after["tools"]);
        // Cache markers are transport hints, not prompt tokens. All content bytes stay exact.
        fn unmark(value: &mut Value) {
            match value {
                Value::Object(o) => {
                    o.remove("cache_control");
                    o.values_mut().for_each(unmark);
                }
                Value::Array(a) => a.iter_mut().for_each(unmark),
                _ => {}
            }
        }
        let mut before = before["messages"].clone();
        let mut after = after["messages"].clone();
        unmark(&mut before);
        unmark(&mut after);
        assert_eq!(before.as_array().unwrap(), &after.as_array().unwrap()[..3]);
    }

    fn run(events: &[Value]) -> (TurnResult, Vec<StreamEvent>) {
        let mut turn = Turn::default();
        let mut seen = Vec::new();
        let mut sink = |e| seen.push(e);
        for e in events {
            if turn.on_event(&e.to_string(), &mut sink).unwrap() {
                break;
            }
        }
        (turn.finish(), seen)
    }

    #[test]
    fn invalid_tool_json_is_kept_raw_and_not_announced() {
        let (result, seen) = run(&[
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"list_sheets","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"a\": "}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ]);
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert_eq!(
            result.content,
            vec![ContentBlock::ToolUse {
                id: "t1".into(),
                name: "list_sheets".into(),
                input: Value::String("{\"a\": ".into()),
            }]
        );
        assert!(!seen
            .iter()
            .any(|e| matches!(e, StreamEvent::ToolUse { .. })));
        // Sent back as an empty object.
        assert_eq!(block_json(&result.content[0])["input"], json!({}));
    }

    #[test]
    fn a_users_picture_is_an_image_block() {
        let block = ContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "QUJD".into(),
        };
        assert_eq!(
            block_json(&block),
            json!({"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"QUJD"}})
        );
    }

    #[test]
    fn thinking_signature_and_unknown_blocks_are_kept() {
        let (result, _) = run(&[
            json!({"type":"content_block_start","index":0,"content_block":{"type":"fallback","from":{"model":"a"},"to":{"model":"b"}}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"hm"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_stop"}),
        ]);
        assert_eq!(
            result.content,
            vec![
                ContentBlock::Raw(
                    json!({"type":"fallback","from":{"model":"a"},"to":{"model":"b"}})
                ),
                ContentBlock::Thinking {
                    thinking: "hm".into(),
                    signature: "sig".into()
                },
            ]
        );
        assert_eq!(
            block_json(&result.content[1]),
            json!({"type":"thinking","thinking":"hm","signature":"sig"})
        );
    }

    #[test]
    fn blocks_before_a_mid_stream_fallback_are_not_run_or_sent_back() {
        let tool = |id: &str| ContentBlock::ToolUse {
            id: id.into(),
            name: "list_layers".into(),
            input: json!({}),
        };
        let fallback = json!({"type":"fallback","from":{"model":"a"},"to":{"model":"b"}});
        let m = Message::assistant(vec![
            ContentBlock::Thinking {
                thinking: "t".into(),
                signature: "s".into(),
            },
            ContentBlock::Text {
                text: "partial".into(),
            },
            tool("old"),
            ContentBlock::Raw(fallback.clone()),
            tool("new"),
        ]);
        let ids: Vec<&str> = m.tool_uses().map(|(id, _, _)| id).collect();
        assert_eq!(ids, ["new"]);
        let sent = message_json(&m);
        let types: Vec<&str> = sent["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["text", "fallback", "tool_use"]);
        assert_eq!(sent["content"][2]["id"], "new");
    }
}
