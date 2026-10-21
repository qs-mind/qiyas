//! Any OpenAI-compatible chat service (OpenAI, OpenRouter, a local server, ...): Chat Completions
//! with streaming and function calling.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::provider::{
    http_client, read_events, send_with_retry, AiError, BoxFuture, ContentBlock, Message, Provider,
    ResultPart, Role, Sink, StopReason, StreamEvent, TurnRequest, TurnResult, Usage,
};

/// An OpenAI-compatible service at `base_url` (e.g. `https://api.openai.com/v1`). The key is only
/// ever put in the `Authorization` header; this type has no `Debug`.
pub struct OpenAiCompatible {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
    /// Each streamed call's `extra_content` by call id, sent back on that call. Gemini 3 puts its
    /// thought signature there (`{"google": {"thought_signature": ...}}`) and refuses a request
    /// whose calls of the current turn come back without it.
    extras: Mutex<HashMap<String, Value>>,
    /// The optional request fields this service refused once (`OPTIONAL_FIELDS`): left out of
    /// every request after.
    refused: Mutex<HashSet<&'static str>>,
    strict_budget: bool,
}

/// The request fields some services refuse with a 400: `reasoning_effort` (a model that doesn't
/// reason), `max_completion_tokens` (an older server: `max_tokens` is sent instead),
/// `stream_options` (a server without usage in streams). Asked for first, dropped when refused.
pub const OPTIONAL_FIELDS: [&str; 4] = [
    "reasoning_effort",
    "max_completion_tokens",
    "max_tokens",
    "stream_options",
];

impl OpenAiCompatible {
    /// `api_key` may be `None` for local servers that take none.
    pub fn new(base_url: &str, api_key: Option<String>) -> OpenAiCompatible {
        OpenAiCompatible {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key,
            client: http_client(),
            extras: Mutex::new(HashMap::new()),
            refused: Mutex::new(HashSet::new()),
            strict_budget: false,
        }
    }

    /// Keep output caps and streamed usage compulsory, with no unreserved HTTP retries.
    pub fn with_strict_budget(mut self) -> Self {
        self.strict_budget = true;
        self
    }
}

impl Provider for OpenAiCompatible {
    fn save_state(&self) -> Value {
        json!({
            "extras": self.extras.lock().map(|v| v.clone()).unwrap_or_default(),
            "refused": self.refused.lock().map(|v| v.clone()).unwrap_or_default(),
        })
    }

    fn restore_state(&self, state: Value) {
        if let Ok(mut extras) = self.extras.lock() {
            *extras = serde_json::from_value(state["extras"].clone()).unwrap_or_default();
        }
        if let Ok(mut refused) = self.refused.lock() {
            *refused = OPTIONAL_FIELDS
                .into_iter()
                .filter(|field| {
                    state["refused"]
                        .as_array()
                        .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(*field)))
                })
                .collect();
        }
    }

    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let extras = self.extras.lock().map(|m| m.clone()).unwrap_or_default();
            let url = format!("{}/chat/completions", self.base_url);
            let secret = self.api_key.as_deref().unwrap_or("");
            // A 400 that names one of the optional fields drops it, and the request goes again
            // once per field.
            let response = loop {
                let refused = self.refused.lock().map(|r| r.clone()).unwrap_or_default();
                let body = request_body(&req, &extras, &refused);
                sink(StreamEvent::RequestSent(crate::provider::sent_fields(
                    &body,
                )));
                let sent = send_with_retry(
                    || {
                        let r = self.client.post(&url).json(&body);
                        match &self.api_key {
                            Some(key) => r.bearer_auth(key),
                            None => r,
                        }
                    },
                    secret,
                    &body,
                    !self.strict_budget,
                )
                .await;
                if self.strict_budget {
                    break sent
                        .map_err(|e| e.for_model(&service_name(&self.base_url), &req.model))?;
                }
                match sent {
                    Err(AiError::Api {
                        status: 400,
                        message,
                        body: answer,
                        request,
                    }) => match refused_field(&format!("{message} {answer}"), &refused) {
                        Some(field) => {
                            if let Ok(mut r) = self.refused.lock() {
                                r.insert(field);
                            }
                        }
                        None => {
                            return Err(AiError::Api {
                                status: 400,
                                message,
                                body: answer,
                                request,
                            }
                            .for_model(&service_name(&self.base_url), &req.model))
                        }
                    },
                    other => {
                        break other
                            .map_err(|e| e.for_model(&service_name(&self.base_url), &req.model))?
                    }
                }
            };
            let mut turn = Turn::default();
            read_events(response, |e| turn.on_event(&e.data, sink))
                .await
                .map_err(|e| {
                    e.scrubbed(self.api_key.as_deref().unwrap_or(""))
                        .for_model(&service_name(&self.base_url), &req.model)
                })?;
            if let Ok(mut kept) = self.extras.lock() {
                kept.extend(turn.extras());
            }
            Ok(turn.finish(sink))
        })
    }
}

fn service_name(base_url: &str) -> String {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "OpenAI-compatible service".into())
}

/// The optional field a 400's text names, among those not refused yet.
fn refused_field(text: &str, refused: &HashSet<&'static str>) -> Option<&'static str> {
    OPTIONAL_FIELDS
        .into_iter()
        .filter(|f| !refused.contains(f))
        .find(|f| text.contains(f))
}

/// `effort` as OpenAI's `reasoning_effort`: low, medium or high (the deeper levels are high,
/// the most every service takes).
fn reasoning_effort(effort: &str) -> &'static str {
    match effort {
        "low" | "minimal" => "low",
        "medium" => "medium",
        _ => "high",
    }
}

/// The JSON body. Asks for the effort as `reasoning_effort`, the answer's length as
/// `max_completion_tokens` (OpenAI's reasoning models refuse `max_tokens`; `max_tokens` when a
/// service refused that) and the usage at the end of the stream, leaving out what the service
/// refused before (`refused`, see `OPTIONAL_FIELDS`). `extras` are the calls' `extra_content` as
/// streamed, by call id.
pub fn request_body(
    req: &TurnRequest,
    extras: &HashMap<String, Value>,
    refused: &HashSet<&'static str>,
) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": req.system })];
    for m in &req.messages {
        push_messages(&mut messages, m, extras);
    }
    let tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })
        })
        .collect();
    let mut body = json!({
        "model": req.model,
        "stream": true,
        "messages": messages,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!("auto");
    }
    if !refused.contains("reasoning_effort") && !req.effort.is_empty() {
        body["reasoning_effort"] = json!(reasoning_effort(&req.effort));
    }
    if req.max_tokens > 0 {
        if !refused.contains("max_completion_tokens") {
            body["max_completion_tokens"] = json!(req.max_tokens);
        } else if !refused.contains("max_tokens") {
            body["max_tokens"] = json!(req.max_tokens);
        }
    }
    if !refused.contains("stream_options") {
        body["stream_options"] = json!({ "include_usage": true });
    }
    body
}

/// One of our messages as Chat Completions messages. Tool results become `tool` messages; their
/// pictures follow in one user message, as a tool message can't carry images. A tool message
/// holds `role`, `tool_call_id` and a string `content`, nothing else.
fn push_messages(out: &mut Vec<Value>, m: &Message, extras: &HashMap<String, Value>) {
    match m.role {
        Role::Assistant => {
            let text: String = m
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let calls: Vec<Value> = m
                .tool_uses()
                .map(|(id, name, input)| {
                    // An input that was not valid JSON goes back as an empty object (its tool
                    // result says so): services refuse the whole request over bad arguments.
                    let arguments = if input.is_object() {
                        input.to_string()
                    } else {
                        "{}".to_string()
                    };
                    let mut call = json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": arguments }
                    });
                    if let Some(extra) = extras.get(id) {
                        call["extra_content"] = extra.clone();
                    }
                    call
                })
                .collect();
            // The content stays a string, empty beside calls with no text: OpenAI takes `null`
            // there, but Gemini's OpenAI-compatible endpoint (and Runware in front of it) refuses
            // it, and some services refuse a missing content; every one of them takes "".
            let mut msg = json!({ "role": "assistant", "content": text });
            if !calls.is_empty() {
                msg["tool_calls"] = Value::Array(calls);
            }
            out.push(msg);
        }
        Role::User => {
            let mut parts: Vec<Value> = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        let text: Vec<&str> = content
                            .iter()
                            .filter_map(|p| match p {
                                ResultPart::Text(t) => Some(t.as_str()),
                                ResultPart::Png(_) => None,
                            })
                            .collect();
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": text.join("\n"),
                        }));
                        for p in content {
                            if let ResultPart::Png(data) = p {
                                parts.push(json!({
                                    "type": "image_url",
                                    "image_url": { "url": format!("data:image/png;base64,{data}") }
                                }));
                            }
                        }
                    }
                    ContentBlock::Text { text } => {
                        parts.push(json!({ "type": "text", "text": text }));
                    }
                    ContentBlock::Image { media_type, data } => {
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{media_type};base64,{data}") }
                        }));
                    }
                    _ => {}
                }
            }
            if !parts.is_empty() {
                out.push(json!({ "role": "user", "content": parts }));
            }
        }
    }
}

/// A function call while it streams.
#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
    /// The service's `extra_content` (Gemini's thought signature), sent back with the call.
    extra: Option<Value>,
}

#[derive(Default)]
struct Turn {
    text: String,
    /// In the order they began.
    calls: Vec<Call>,
    /// The stream's `index` of each call to the newest call in `calls` with it.
    by_index: HashMap<u64, usize>,
    finish_reason: Option<String>,
    usage: Usage,
}

impl Turn {
    fn on_event(&mut self, data: &str, sink: Sink<'_>) -> Result<bool, AiError> {
        if data.trim() == "[DONE]" {
            return Ok(true);
        }
        let Ok(e) = serde_json::from_str::<Value>(data) else {
            return Err(AiError::Stream("an event was not valid JSON".into()));
        };
        if let Some(err) = e.get("error") {
            let message = format!(
                "{}: {}",
                err["type"]
                    .as_str()
                    .or_else(|| err["code"].as_str())
                    .unwrap_or("error"),
                err["message"].as_str().unwrap_or("error")
            );
            return Err(AiError::Stream(message));
        }
        if let Some(u) = e.get("usage").filter(|u| u.is_object()) {
            let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
            let cached = u["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
            self.usage.input_tokens = prompt.saturating_sub(cached);
            self.usage.cache_read_input_tokens = cached;
            self.usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0);
            if let Some(reasoning) = [
                u["completion_tokens_details"]["reasoning_tokens"].as_u64(),
                u["reasoning_tokens"].as_u64(),
            ]
            .into_iter()
            .flatten()
            .max()
            {
                // Some bridges keep an empty details object beside their own counter.
                self.usage.reasoning_tokens = reasoning;
            }
        }
        let choice = &e["choices"][0];
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            self.text.push_str(text);
            sink(StreamEvent::TextDelta(text.to_string()));
        }
        for c in delta["tool_calls"].as_array().into_iter().flatten() {
            let id = c["id"].as_str().filter(|s| !s.is_empty());
            let name = c["function"]["name"].as_str().filter(|s| !s.is_empty());
            let at = self.slot(c["index"].as_u64(), id, name);
            let call = &mut self.calls[at];
            if let Some(id) = id {
                call.id = id.to_string();
            }
            if let Some(extra) = c.get("extra_content").filter(|x| !x.is_null()) {
                call.extra = Some(extra.clone());
            }
            if let Some(name) = name {
                call.name.push_str(name);
                if call.id.is_empty() {
                    call.id = format!("call_{at}");
                }
                sink(StreamEvent::ToolUseStart {
                    id: call.id.clone(),
                    name: call.name.clone(),
                });
            }
            if let Some(args) = c["function"]["arguments"].as_str() {
                call.arguments.push_str(args);
                sink(StreamEvent::ToolInputDelta {
                    id: call.id.clone(),
                    json_fragment: args.to_string(),
                });
            }
        }
        if let Some(r) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(r.to_string());
        }
        Ok(false)
    }

    /// Where a streamed call fragment goes in `calls`. OpenAI gives each call its own `index` and
    /// sends its id and name once, in its first fragment. Gemini sends parallel calls whole, all
    /// at index 0 or with none, so another id, or a name for a call that has one, starts a call.
    fn slot(&mut self, index: Option<u64>, id: Option<&str>, name: Option<&str>) -> usize {
        let current = match index {
            Some(i) => self.by_index.get(&i).copied(),
            None => self.calls.len().checked_sub(1),
        };
        let at = match current {
            Some(at) => {
                let call = &self.calls[at];
                let other_id = id.is_some_and(|id| !call.id.is_empty() && call.id != id);
                let second_name = id.is_none() && name.is_some() && !call.name.is_empty();
                if other_id || second_name {
                    None
                } else {
                    Some(at)
                }
            }
            None => None,
        };
        at.unwrap_or_else(|| {
            self.calls.push(Call::default());
            let at = self.calls.len() - 1;
            if let Some(i) = index {
                self.by_index.insert(i, at);
            }
            at
        })
    }

    /// Each call's `extra_content`, by the id `finish` gives it.
    fn extras(&self) -> Vec<(String, Value)> {
        self.calls
            .iter()
            .enumerate()
            .filter_map(|(at, call)| {
                let id = if call.id.is_empty() {
                    format!("call_{at}")
                } else {
                    call.id.clone()
                };
                Some((id, call.extra.clone()?))
            })
            .collect()
    }

    fn finish(self, sink: Sink<'_>) -> TurnResult {
        let mut content = Vec::new();
        if !self.text.is_empty() {
            content.push(ContentBlock::Text {
                text: self.text.clone(),
            });
        }
        let has_calls = !self.calls.is_empty();
        for (at, call) in self.calls.into_iter().enumerate() {
            let id = if call.id.is_empty() {
                format!("call_{at}")
            } else {
                call.id
            };
            let parsed = if call.arguments.trim().is_empty() {
                Ok(json!({}))
            } else {
                serde_json::from_str::<Value>(&call.arguments)
            };
            let input = match parsed {
                Ok(v) if v.is_object() => {
                    sink(StreamEvent::ToolUse {
                        id: id.clone(),
                        name: call.name.clone(),
                        input: v.clone(),
                    });
                    v
                }
                _ => Value::String(call.arguments),
            };
            content.push(ContentBlock::ToolUse {
                id,
                name: call.name,
                input,
            });
        }
        sink(StreamEvent::Done);
        let stop_reason = match self.finish_reason.as_deref() {
            // Some servers say "stop" even when they called tools.
            _ if has_calls => StopReason::ToolUse,
            Some("stop") | None => StopReason::EndTurn,
            Some("length") => StopReason::MaxTokens,
            Some("content_filter") => StopReason::Refusal {
                category: Some("content_filter".into()),
                text: self.text,
            },
            Some(other) => StopReason::Other(other.to_string()),
        };
        TurnResult {
            content,
            stop_reason,
            usage: self.usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumed_calls_keep_gemini_extras_and_refused_fields() {
        let original = OpenAiCompatible::new("http://localhost/v1", None);
        let signature = json!({"google":{"thought_signature":"opaque+/=\r\n"}});
        original
            .extras
            .lock()
            .unwrap()
            .insert("c1".into(), signature.clone());
        original.refused.lock().unwrap().insert("reasoning_effort");
        let saved = serde_json::to_vec(&original.save_state()).unwrap();
        let resumed = OpenAiCompatible::new("http://localhost/v1", None);
        resumed.restore_state(serde_json::from_slice(&saved).unwrap());
        let request = TurnRequest {
            system: "Unchanged".into(),
            messages: vec![Message::assistant(vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "qiyas_read".into(),
                input: json!({"op":"state","args":"{}"}),
            }])],
            tools: vec![],
            model: "gemini".into(),
            effort: "high".into(),
            max_tokens: 1000,
        };
        let body = request_body(
            &request,
            &resumed.extras.lock().unwrap(),
            &resumed.refused.lock().unwrap(),
        );
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["extra_content"],
            signature
        );
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(original.save_state(), resumed.save_state());
    }

    fn request(effort: &str, max_tokens: u32) -> TurnRequest {
        TurnRequest {
            system: "s".into(),
            messages: vec![Message::user(vec![ContentBlock::Text {
                text: "hi".into(),
            }])],
            tools: vec![],
            model: "google:gemini@3.8-flash".into(),
            effort: effort.into(),
            max_tokens,
        }
    }

    /// The effort the user chose reaches the service (it was never sent), with the answer's
    /// length and the usage at the end of the stream.
    #[test]
    fn effort_tokens_and_usage_are_asked_for() {
        let body = request_body(&request("xhigh", 16_000), &HashMap::new(), &HashSet::new());
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["max_completion_tokens"], 16_000);
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["stream_options"]["include_usage"], true);
        for (effort, sent) in [
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("max", "high"),
        ] {
            let body = request_body(&request(effort, 1), &HashMap::new(), &HashSet::new());
            assert_eq!(body["reasoning_effort"], sent, "{effort}");
        }
    }

    #[test]
    fn a_refused_parameter_is_dropped_and_remembered() {
        let none = HashSet::new();
        assert_eq!(
            refused_field(
                "Unrecognized request argument supplied: reasoning_effort",
                &none
            ),
            Some("reasoning_effort")
        );
        assert_eq!(refused_field("model not found", &none), None);
        let mut refused: HashSet<&'static str> = HashSet::new();
        refused.insert("reasoning_effort");
        refused.insert("max_completion_tokens");
        let body = request_body(&request("high", 4_000), &HashMap::new(), &refused);
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("max_completion_tokens").is_none());
        assert_eq!(body["max_tokens"], 4_000, "the older name instead");
        // OpenAI's own answer to max_tokens names both: the one not refused yet is dropped.
        assert_eq!(
            refused_field(
                "'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.",
                &refused
            ),
            Some("max_tokens")
        );
    }

    #[test]
    fn reasoning_usage_accepts_both_confirmed_compatible_shapes() {
        for usage in [
            json!({"completion_tokens_details":{"reasoning_tokens":71}}),
            json!({"reasoning_tokens":71}),
            json!({"completion_tokens_details":{"reasoning_tokens":0},"reasoning_tokens":71}),
        ] {
            let mut turn = Turn::default();
            let mut sink = |_: StreamEvent| {};
            turn.on_event(&json!({"choices":[],"usage":usage}).to_string(), &mut sink)
                .unwrap();
            turn.on_event(
                &json!({"choices":[],"usage":{"completion_tokens":90}}).to_string(),
                &mut sink,
            )
            .unwrap();
            assert_eq!(turn.finish(&mut sink).usage.reasoning_tokens, 71);
        }
    }

    #[test]
    fn sent_fields_match_the_request_after_optional_fields_are_refused() {
        let first = request_body(&request("high", 16_000), &HashMap::new(), &HashSet::new());
        let sent = crate::provider::sent_fields(&first);
        assert!(sent.contains("reasoning_effort=high"));
        assert!(sent.contains("max_completion_tokens=16000"));
        let refused = HashSet::from(["reasoning_effort", "max_completion_tokens"]);
        let retry = request_body(&request("high", 16_000), &HashMap::new(), &refused);
        let sent = crate::provider::sent_fields(&retry);
        assert!(!sent.contains("reasoning_effort"));
        assert!(!sent.contains("max_completion_tokens"));
        assert!(sent.contains("max_tokens=16000"));
    }

    #[test]
    fn reasoning_and_cached_tokens_are_read() {
        let mut turn = Turn::default();
        let mut sink = |_: StreamEvent| {};
        let end = json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 300,
                "prompt_tokens_details": { "cached_tokens": 800 },
                "completion_tokens_details": { "reasoning_tokens": 250 }
            }
        });
        turn.on_event(&end.to_string(), &mut sink).unwrap();
        let usage = turn.finish(&mut sink).usage;
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.cache_read_input_tokens, 800);
        assert_eq!(usage.output_tokens, 300);
        assert_eq!(usage.reasoning_tokens, 250);
    }

    /// The calls a stream of `data:` payloads ends with, as (id, name, input).
    fn calls_of(events: &[Value]) -> Vec<(String, String, Value)> {
        let mut turn = Turn::default();
        let mut sink = |_: StreamEvent| {};
        for e in events {
            turn.on_event(&e.to_string(), &mut sink).unwrap();
        }
        turn.finish(&mut sink)
            .content
            .into_iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
                _ => None,
            })
            .collect()
    }

    fn call(id: Option<&str>, index: Option<u64>, name: &str, args: &str) -> Value {
        let mut c = json!({"type":"function","function":{"name":name,"arguments":args}});
        if let Some(id) = id {
            c["id"] = json!(id);
        }
        if let Some(i) = index {
            c["index"] = json!(i);
        }
        json!({"choices":[{"index":0,"delta":{"tool_calls":[c]}}]})
    }

    #[test]
    fn parallel_calls_without_their_own_index_stay_apart() {
        // Gemini (also behind Runware) streams each parallel call whole, all at index 0 or
        // with no index at all.
        for index in [Some(0), None] {
            let calls = calls_of(&[
                call(Some("f1"), index, "block_summary", "{}"),
                call(Some("f2"), index, "list_layers", "{}"),
                call(Some("f3"), index, "read_text", r#"{"layer":"A-TEXT"}"#),
            ]);
            let names: Vec<&str> = calls.iter().map(|c| c.1.as_str()).collect();
            assert_eq!(
                names,
                ["block_summary", "list_layers", "read_text"],
                "{index:?}"
            );
            assert_eq!(calls[1].0, "f2");
            assert_eq!(calls[2].2, json!({"layer": "A-TEXT"}));
        }
    }

    #[test]
    fn parallel_calls_with_no_id_are_told_apart_by_their_name() {
        let calls = calls_of(&[
            call(None, None, "list_layers", "{}"),
            call(None, None, "list_takeoff", "{}"),
        ]);
        let names: Vec<&str> = calls.iter().map(|c| c.1.as_str()).collect();
        assert_eq!(names, ["list_layers", "list_takeoff"]);
        assert_ne!(calls[0].0, calls[1].0, "each call gets its own id");
    }

    #[test]
    fn input_that_was_not_json_goes_back_as_an_empty_object() {
        let mut out = Vec::new();
        push_messages(
            &mut out,
            &Message::assistant(vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "list_layers".into(),
                input: Value::String("{}{}".into()),
            }]),
            &HashMap::new(),
        );
        assert_eq!(out[0]["tool_calls"][0]["function"]["arguments"], "{}");
    }

    #[test]
    fn tool_results_become_tool_messages_and_images_follow_as_user() {
        let mut out = Vec::new();
        push_messages(
            &mut out,
            &Message::assistant(vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "view_image".into(),
                input: json!({}),
            }]),
            &HashMap::new(),
        );
        push_messages(
            &mut out,
            &Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![
                    ResultPart::Text("cap".into()),
                    ResultPart::Png("QUJD".into()),
                ],
                is_error: false,
            }]),
            &HashMap::new(),
        );
        // Gemini's OpenAI-compatible endpoint refuses a null content beside the calls.
        assert_eq!(out[0]["content"], "");
        assert_eq!(out[0]["tool_calls"][0]["function"]["arguments"], "{}");
        assert!(out[0]["tool_calls"][0].get("extra_content").is_none());
        assert_eq!(
            out[1],
            json!({"role":"tool","tool_call_id":"c1","content":"cap"})
        );
        assert_eq!(out[2]["role"], "user");
        assert_eq!(
            out[2]["content"][0]["image_url"]["url"],
            "data:image/png;base64,QUJD"
        );
    }

    #[test]
    fn a_users_picture_goes_in_their_message_as_a_data_url() {
        let mut out = Vec::new();
        push_messages(
            &mut out,
            &Message::user(vec![
                ContentBlock::Image {
                    media_type: "image/webp".into(),
                    data: "QUJD".into(),
                },
                ContentBlock::Text {
                    text: "what is this?".into(),
                },
            ]),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(
            out[0]["content"][0]["image_url"]["url"],
            "data:image/webp;base64,QUJD"
        );
        assert_eq!(out[0]["content"][1]["text"], "what is this?");
    }

    #[test]
    fn a_tool_message_is_role_id_and_string_content_only() {
        let mut out = Vec::new();
        push_messages(
            &mut out,
            &Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: "call_446341".into(),
                content: vec![
                    ResultPart::Text("{\"rows\":[]}".into()),
                    ResultPart::Text("second".into()),
                ],
                is_error: true,
            }]),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 1, "no user message without pictures or text");
        let tool = out[0].as_object().unwrap();
        let mut keys: Vec<&str> = tool.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["content", "role", "tool_call_id"]);
        assert_eq!(tool["role"], "tool");
        assert_eq!(tool["tool_call_id"], "call_446341");
        assert_eq!(tool["content"], "{\"rows\":[]}\nsecond");
    }

    #[test]
    fn arguments_are_a_json_string_and_text_beside_calls_is_kept() {
        let mut out = Vec::new();
        push_messages(
            &mut out,
            &Message::assistant(vec![
                ContentBlock::Text {
                    text: "Looking.".into(),
                },
                ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "find_entities".into(),
                    input: json!({"layer": "A-WALL"}),
                },
            ]),
            &HashMap::new(),
        );
        assert_eq!(out[0]["content"], "Looking.");
        let args = &out[0]["tool_calls"][0]["function"]["arguments"];
        assert!(args.is_string());
        assert_eq!(
            serde_json::from_str::<Value>(args.as_str().unwrap()).unwrap(),
            json!({"layer": "A-WALL"})
        );
    }

    #[test]
    fn a_streamed_thought_signature_goes_back_on_its_call() {
        let signature = json!({"google": {"thought_signature": "c2ln"}});
        let mut turn = Turn::default();
        let mut sink = |_: StreamEvent| {};
        let mut first = call(
            Some("f1"),
            Some(0),
            "find_entities",
            r#"{"layer":"A-WALL"}"#,
        );
        first["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = signature.clone();
        turn.on_event(&first.to_string(), &mut sink).unwrap();
        turn.on_event(
            &call(Some("f2"), Some(0), "list_layers", "{}").to_string(),
            &mut sink,
        )
        .unwrap();
        let extras: HashMap<String, Value> = turn.extras().into_iter().collect();
        assert_eq!(extras.len(), 1);
        let result = turn.finish(&mut sink);
        let mut out = Vec::new();
        push_messages(&mut out, &Message::assistant(result.content), &extras);
        assert_eq!(out[0]["tool_calls"][0]["extra_content"], signature);
        assert!(out[0]["tool_calls"][1].get("extra_content").is_none());
    }
}
