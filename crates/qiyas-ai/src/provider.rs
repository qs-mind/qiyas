//! What every AI provider shares: the conversation types, one streamed turn, the errors, and a
//! small Server-Sent Events reader. The API key never appears in any of these.

use std::future::Future;
use std::pin::Pin;
use std::sync::Once;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ToolSpec;

/// A boxed future that can move between threads, so `Provider` can be used as `dyn Provider`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where streamed events go while a turn is running.
pub type Sink<'a> = &'a mut (dyn FnMut(StreamEvent) + Send);

/// One AI service that can stream a turn of the conversation.
pub trait Provider: Send + Sync {
    /// Current request/cost limits, supplied by a metered host without changing the system.
    fn budget_notice(&self) -> Option<String> {
        None
    }

    /// Conversation-local memory only; credentials and URLs never belong here.
    fn save_state(&self) -> Value {
        Value::Null
    }

    fn restore_state(&self, _state: Value) {}

    /// Sends `req`, calls `sink` with each event as it arrives, and returns the whole turn.
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>>;
}

#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    /// Used exactly as the user typed it.
    pub model: String,
    /// "low", "medium", "high", "xhigh" or "max".
    pub effort: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(content: Vec<ContentBlock>) -> Message {
        Message {
            role: Role::User,
            content,
        }
    }

    pub fn assistant(content: Vec<ContentBlock>) -> Message {
        Message {
            role: Role::Assistant,
            content,
        }
    }

    /// The `tool_use` blocks to answer, in order. Calls made before a `fallback` block belong to
    /// a declined attempt the API has thrown away, so they are left out (and not sent back).
    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.content[self.fallback_boundary()..]
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.as_str(), name.as_str(), input))
                }
                _ => None,
            })
    }

    /// The index just after the last `fallback` block (0 when there is none). Before it, only
    /// text blocks are sent back to the API.
    pub fn fallback_boundary(&self) -> usize {
        self.content
            .iter()
            .rposition(|b| matches!(b, ContentBlock::Raw(v) if v["type"] == "fallback"))
            .map_or(0, |i| i + 1)
    }

    pub fn has_tool_results(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// `input` is the parsed JSON object. When the model's input was not valid JSON it is kept
    /// as `Value::String` holding the raw text, and the tool is not run.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Vec<ResultPart>,
        is_error: bool,
    },
    /// A picture in a user message (one the user attached, or of what they selected): its
    /// media type ("image/png", ...) and base64 data.
    Image {
        media_type: String,
        data: String,
    },
    /// Kept with its signature exactly as streamed; the API checks it when it is sent back.
    Thinking {
        thinking: String,
        signature: String,
    },
    /// Any other block the API returned (`redacted_thinking`, `fallback`, ...), sent back as is.
    Raw(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ResultPart {
    Text(String),
    /// A PNG, base64-encoded.
    Png(String),
}

/// What arrives while a turn streams.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// The optional fields in an actual HTTP request, including retries after a refusal.
    RequestSent(String),
    TextDelta(String),
    ToolUseStart {
        id: String,
        name: String,
    },
    ToolInputDelta {
        id: String,
        json_fragment: String,
    },
    /// A tool call whose input parsed as JSON.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    Thinking(String),
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    /// The model or a safety classifier declined. `category` is `stop_details.category` (e.g.
    /// "cyber"), which may be absent; `text` is its explanation, or what was written before.
    Refusal {
        category: Option<String>,
        text: String,
    },
    /// Any other stop reason, by name.
    Other(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    /// Output tokens the model spent thinking, when the service says (OpenAI-compatible
    /// `completion_tokens_details.reasoning_tokens`); part of `output_tokens`.
    pub reasoning_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnResult {
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

/// Errors from a provider. None of them ever holds the API key.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AiError {
    #[error("{service} has no model {model}: choose one in Settings")]
    ModelNotFound { service: String, model: String },
    #[error("The API key was refused. Check it in Settings.")]
    Auth,
    #[error("The AI service is busy or the rate limit was reached; try again shortly")]
    RateLimited { retry_after: Option<u64> },
    /// `body` is the service's whole answer (scrubbed, clipped), for the log. `request` is, for
    /// a 4xx, the JSON that was sent as `request_for_log` writes it, so the log shows what the
    /// service refused.
    #[error("The AI service answered {status}: {message}")]
    Api {
        status: u16,
        message: String,
        body: String,
        request: Option<String>,
    },
    #[error("Could not reach the AI service: {0}")]
    Network(String),
    /// The stream broke off or carried an error event.
    #[error("The AI service's answer broke off: {0}")]
    Stream(String),
    /// The MCP server could not start (the port is taken, say).
    #[error("The MCP server could not start on {0}")]
    Server(String),
}

impl AiError {
    /// Give both providers the same actionable model selection error.
    pub(crate) fn for_model(self, service: &str, model: &str) -> AiError {
        let missing = match &self {
            AiError::Api { status, body, .. } => {
                let value: Value = serde_json::from_str(body).unwrap_or_default();
                let error = if value.is_array() {
                    &value[0]["error"]
                } else {
                    &value["error"]
                };
                *status == 404
                    || ["type", "code"].iter().any(|key| {
                        matches!(
                            error[*key].as_str(),
                            Some("model_not_found" | "not_found_error" | "model_not_found_error")
                        )
                    })
            }
            AiError::Stream(message) => {
                message.contains("model_not_found") || message.contains("not_found_error")
            }
            _ => false,
        };
        if missing {
            AiError::ModelNotFound {
                service: service.into(),
                model: model.into(),
            }
        } else {
            self
        }
    }

    /// The same error with every copy of `secret` blanked out of its text.
    pub(crate) fn scrubbed(self, secret: &str) -> AiError {
        match self {
            AiError::Api {
                status,
                message,
                body,
                request,
            } => AiError::Api {
                status,
                message: scrub(&message, secret),
                body: scrub(&body, secret),
                request: request.map(|r| scrub(&r, secret)),
            },
            AiError::Network(m) => AiError::Network(scrub(&m, secret)),
            AiError::Stream(m) => AiError::Stream(scrub(&m, secret)),
            other => other,
        }
    }
}

/// What a tool input that is not valid JSON is answered with.
pub const INVALID_JSON: &str = "invalid JSON input";

/// A PNG as the base64 text a `ResultPart::Png` holds.
pub fn png_base64(png: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(png)
}

/// The bytes of base64 `text` (a picture the window sent).
pub fn from_base64(text: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|e| format!("Not base64: {e}"))
}

/// Longest wait before the one retry after a 429 or 529.
const MAX_RETRY_WAIT: u64 = 30;
/// Wait when a 429 or 529 gives no `retry-after`.
const DEFAULT_RETRY_WAIT: u64 = 2;

/// The HTTP client both providers use: rustls with the `ring` crypto provider.
pub(crate) fn http_client() -> reqwest::Client {
    static PROVIDER: Once = Once::new();
    PROVIDER.call_once(|| {
        // Fails only when another provider was installed first, which is fine too.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // No byte for this long means the connection is dead (the API pings while it thinks).
        .read_timeout(Duration::from_secs(300))
        .build()
        .unwrap_or_default()
}

/// Sends the request built by `make`, retrying once after a 429 or 529, and maps failures to
/// `AiError`. `make` is called again for the retry. `body` is the JSON `make` sends, kept in a
/// 4xx error for the log (the key is a header, never in it).
pub(crate) async fn send(
    make: impl Fn() -> reqwest::RequestBuilder,
    secret: &str,
    body: &Value,
) -> Result<reqwest::Response, AiError> {
    send_with_retry(make, secret, body, true).await
}

/// Budgeted evaluations reserve one request, so they must disable the automatic retry.
pub(crate) async fn send_with_retry(
    make: impl Fn() -> reqwest::RequestBuilder,
    secret: &str,
    body: &Value,
    allow_retry: bool,
) -> Result<reqwest::Response, AiError> {
    let mut retried = false;
    loop {
        let response = make()
            .send()
            .await
            .map_err(|e| network(e).scrubbed(secret))?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return Ok(response);
        }
        if matches!(status, 429 | 503 | 529) {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| retry_after_seconds(v, chrono::Utc::now()));
            if retried || !allow_retry {
                return Err(AiError::RateLimited { retry_after });
            }
            retried = true;
            let wait = retry_after
                .unwrap_or(DEFAULT_RETRY_WAIT)
                .min(MAX_RETRY_WAIT);
            tokio::time::sleep(Duration::from_secs(wait)).await;
            continue;
        }
        if status == 401 {
            return Err(AiError::Auth);
        }
        // Scrubbed before `error_message` may cut the body short, so no part of the key is left.
        let answer = scrub(&response.text().await.unwrap_or_default(), secret);
        let request = (400..500)
            .contains(&status)
            .then(|| request_for_log(body, secret));
        return Err(AiError::Api {
            status,
            message: error_message(&answer),
            body: crate::log::clip(&answer),
            request,
        });
    }
}

fn retry_after_seconds(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<u64> {
    if let Ok(seconds) = value.trim().parse::<f64>() {
        return seconds.is_finite().then(|| seconds.max(0.0).ceil() as u64);
    }
    chrono::DateTime::parse_from_rfc2822(value.trim())
        .ok()
        .map(|date| {
            (date.signed_duration_since(now).num_milliseconds().max(0) as u64).div_ceil(1000)
        })
}

#[test]
fn retry_after_accepts_seconds_and_http_dates_without_shortening_the_delay() {
    let now = chrono::DateTime::parse_from_rfc2822("Thu, 22 Oct 2026 12:00:00 GMT")
        .unwrap()
        .to_utc();
    assert_eq!(retry_after_seconds("1.2", now), Some(2));
    assert_eq!(
        retry_after_seconds("Thu, 22 Oct 2026 12:01:30 GMT", now),
        Some(90)
    );
    assert_eq!(
        retry_after_seconds("Thu, 22 Oct 2026 11:59:59 GMT", now),
        Some(0)
    );
    assert_eq!(retry_after_seconds("invalid", now), None);
}

/// Only request controls, never messages, URLs or credentials.
pub(crate) fn sent_fields(body: &Value) -> String {
    [
        "reasoning_effort",
        "max_completion_tokens",
        "max_tokens",
        "stream_options",
        "output_config",
        "thinking",
        "fallbacks",
    ]
    .into_iter()
    .filter_map(|key| {
        body.get(key).map(|value| {
            format!(
                "{key}={}",
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_string)
            )
        })
    })
    .collect::<Vec<_>>()
    .join(", ")
}

/// A request body as the log keeps it: compact JSON, pictures as `<image>`, the tool definitions
/// as their names and the system prompt as its length (both the same in every request of a
/// conversation, and long enough to push the messages past the clip), `secret` blanked out, and
/// only then, when it is longer than `log::MAX_TEXT` characters, its last `MAX_TEXT` kept after a
/// leading `…` (the messages that fail are the last ones; keys sort `messages` first, so the head
/// would keep the oldest).
pub(crate) fn request_for_log(body: &Value, secret: &str) -> String {
    fn pictures(v: &mut Value) {
        match v {
            Value::String(s) if s.starts_with("data:image/") => *s = "<image>".into(),
            Value::Array(items) => items.iter_mut().for_each(pictures),
            Value::Object(map) => {
                // Claude's `{"type": "base64", "media_type": ..., "data": ...}`.
                if map.get("type").and_then(Value::as_str) == Some("base64") {
                    if let Some(data) = map.get_mut("data") {
                        *data = Value::String("<image>".into());
                    }
                }
                map.values_mut().for_each(pictures);
            }
            _ => {}
        }
    }
    let mut v = body.clone();
    pictures(&mut v);
    if let Some(tools) = v.get_mut("tools").filter(|t| t.is_array()) {
        let names: Vec<&str> = tools
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["function"]["name"].as_str().or(t["name"].as_str()))
            .collect();
        *tools = Value::String(format!("<{} tools: {}>", names.len(), names.join(", ")));
    }
    let system = |text: &Value| {
        let n = text
            .as_str()
            .map_or_else(|| text.to_string().chars().count(), |s| s.chars().count());
        format!("<system prompt, {n} characters>")
    };
    if let Some(s) = v.get_mut("system") {
        *s = Value::String(system(s));
    }
    for m in v["messages"].as_array_mut().into_iter().flatten() {
        if m["role"] == "system" {
            m["content"] = Value::String(system(&m["content"]));
        }
    }
    crate::log::clip_tail(&scrub(&v.to_string(), secret))
}

/// `text` with every copy of `secret` blanked out, in case a service echoes the key back.
pub(crate) fn scrub(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "[key hidden]")
    }
}

pub(crate) fn network(e: reqwest::Error) -> AiError {
    AiError::Network(e.without_url().to_string())
}

/// `error.message` from an error body (Gemini sends `[{"error": ...}]`), or the start of the
/// body when it has none.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let error = if v.is_array() {
                &v[0]["error"]
            } else {
                &v["error"]
            };
            error["message"].as_str().map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// Reads Server-Sent Events from a byte stream: feed it chunks, take out complete events.
#[derive(Default)]
pub(crate) struct SseReader {
    buffer: Vec<u8>,
}

/// One event's `data:` lines joined by newlines. The `event:` line is not needed: the data
/// carries its own `type`.
pub(crate) struct SseEvent {
    pub data: String,
}

impl SseReader {
    pub fn push(&mut self, chunk: &[u8]) {
        // Carriage returns only ever end lines (JSON escapes them inside strings).
        self.buffer.extend(chunk.iter().filter(|&&b| b != b'\r'));
    }

    /// The next complete event, if one has fully arrived.
    pub fn next_event(&mut self) -> Option<SseEvent> {
        loop {
            let end = self.buffer.windows(2).position(|w| w == b"\n\n")?;
            let raw: Vec<u8> = self.buffer.drain(..end + 2).collect();
            let text = String::from_utf8_lossy(&raw[..end]);
            let data: Vec<&str> = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|v| v.strip_prefix(' ').unwrap_or(v))
                .collect();
            // Comments, and events with no data, are skipped.
            if !data.is_empty() {
                return Some(SseEvent {
                    data: data.join("\n"),
                });
            }
        }
    }
}

/// Reads `response` chunk by chunk, calling `on_event` for each SSE event until it returns
/// `Ok(true)` (finished). A stream that ends first is an error.
pub(crate) async fn read_events(
    mut response: reqwest::Response,
    mut on_event: impl FnMut(SseEvent) -> Result<bool, AiError>,
) -> Result<(), AiError> {
    let mut reader = SseReader::default();
    while let Some(chunk) = response.chunk().await.map_err(network)? {
        reader.push(&chunk);
        while let Some(event) = reader.next_event() {
            if on_event(event)? {
                return Ok(());
            }
        }
    }
    Err(AiError::Stream(
        "the connection closed before the answer finished".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_logged_request_hides_pictures_tools_and_the_system_prompt() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "You are Mizan."},
                {"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,QUJD"}},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QUJD"}},
                ]},
                {"role": "tool", "tool_call_id": "c1", "content": "done"},
            ],
            "tools": [{"type": "function", "function": {"name": "find_entities"}}, {"name": "list_layers"}],
            "system": [{"type": "text", "text": "long"}],
        });
        let text = request_for_log(&body, "");
        assert!(!text.contains("QUJD"), "{text}");
        assert_eq!(text.matches("<image>").count(), 2, "{text}");
        assert!(
            text.contains("<2 tools: find_entities, list_layers>"),
            "{text}"
        );
        assert!(!text.contains("You are Mizan"), "{text}");
        assert!(text.contains("<system prompt, 14 characters>"), "{text}");
        assert!(text.contains(r#""tool_call_id":"c1""#), "{text}");
    }

    #[test]
    fn a_long_logged_request_keeps_its_last_characters_after_a_marker() {
        let mut messages: Vec<Value> = (0..20)
            .map(|i| json!({"role": "user", "content": format!("early {i} {}", "a".repeat(990))}))
            .collect();
        messages.push(
            json!({"role": "tool", "tool_call_id": "last", "content": "the one that failed"}),
        );
        let long = json!({"messages": messages});
        let text = request_for_log(&long, "");
        assert_eq!(
            text.chars().count(),
            crate::log::MAX_TEXT + 1,
            "{}",
            text.len()
        );
        assert!(text.starts_with('…'), "{text}");
        assert!(
            text.ends_with(
                r#""content":"the one that failed","role":"tool","tool_call_id":"last"}]}"#
            ),
            "{text}"
        );
        assert!(!text.contains("early 0 "), "the head is what is cut");
        // A short one is whole, without the marker.
        let short = request_for_log(
            &json!({"messages": [{"role": "user", "content": "hi"}]}),
            "",
        );
        assert!(!short.starts_with('…'), "{short}");
    }

    #[test]
    fn the_logged_request_is_scrubbed_before_it_is_cut() {
        // The key straddles the start of the kept tail: cut first and its end would be left.
        let key = "sk-secret-key-0123456789";
        let body = json!({"messages": [{"role": "user", "content": format!("{}{key}{}", "a".repeat(100), "b".repeat(3968))}]});
        let text = request_for_log(&body, key);
        assert!(text.starts_with('…'), "{text}");
        assert!(text.contains("[key hidden]"), "{text}");
        for part in ["key-0123456789", "0123456789", "sk-secret"] {
            assert!(!text.contains(part), "{part} left in the log");
        }
    }

    #[test]
    fn sse_reader_joins_split_chunks_and_crlf() {
        let mut r = SseReader::default();
        r.push(b"event: a\r\ndata: {\"x\":");
        assert!(r.next_event().is_none());
        r.push(b"1}\r\n\r\n: comment\n\ndata: [DONE]\n\n");
        let e = r.next_event().unwrap();
        assert_eq!(e.data, "{\"x\":1}");
        let e = r.next_event().unwrap();
        assert_eq!(e.data, "[DONE]");
        assert!(r.next_event().is_none());
    }

    #[test]
    fn missing_models_name_the_service_and_settings_for_both_providers() {
        for service in ["Anthropic", "api.example.test"] {
            for (status, body) in [
                (404, "{}"),
                (400, r#"{"error":{"code":"model_not_found"}}"#),
                (400, r#"{"error":{"type":"not_found_error"}}"#),
            ] {
                let error = AiError::Api {
                    status,
                    body: body.into(),
                    message: "missing".into(),
                    request: None,
                }
                .for_model(service, "chosen-model");
                assert_eq!(
                    error.to_string(),
                    format!("{service} has no model chosen-model: choose one in Settings")
                );
            }
        }
        let refused = AiError::Api {
            status: 400,
            body: "{}".into(),
            message: "bad input".into(),
            request: None,
        };
        assert_eq!(refused.clone().for_model("Anthropic", "model"), refused);
    }

    #[test]
    fn error_message_reads_the_api_shape() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"}}"#;
        assert_eq!(error_message(body), "bad");
        let gemini =
            r#"[{"error":{"code":400,"message":"Invalid tool call","status":"INVALID_ARGUMENT"}}]"#;
        assert_eq!(error_message(gemini), "Invalid tool call");
        assert_eq!(error_message("oops"), "oops");
    }
}
