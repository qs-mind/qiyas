//! Automatic picture capability, shared by discovery and real requests.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::{BoxFuture, Sink};
use crate::{
    AiError, ContentBlock, Message, Provider, ResultPart, StreamEvent, TurnRequest, TurnResult,
};

pub const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC";
const NOTICE: &str = "Pictures are unavailable for this model. Use exact drawing queries and checks. No picture was sent.";

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    pub provider: String,
    pub scope: String,
    pub model: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    key: Key,
    image_input: bool,
    checked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_model: Option<String>,
}
#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    attempts: HashMap<Key, Instant>,
}
pub struct Cache {
    path: PathBuf,
    state: Mutex<State>,
}
impl Cache {
    pub fn new(path: PathBuf) -> Self {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            state: Mutex::new(State {
                entries,
                ..State::default()
            }),
        }
    }
    pub fn get(&self, key: &Key) -> Option<bool> {
        self.state
            .lock()
            .ok()?
            .entries
            .iter()
            .find(|entry| {
                &entry.key == key
                    && DateTime::parse_from_rfc3339(&entry.checked_at).is_ok_and(|checked| {
                        checked <= Utc::now()
                            && Utc::now().signed_duration_since(checked)
                                < chrono::Duration::days(30)
                    })
            })
            .map(|entry| entry.image_input)
    }
    pub fn remember(&self, key: &Key, image_input: bool) {
        self.remember_model(key, image_input, None);
    }
    fn remember_model(&self, key: &Key, image_input: bool, resolved_model: Option<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.entries.retain(|entry| &entry.key != key);
        state.entries.push(Entry {
            key: key.clone(),
            image_input,
            checked_at: Utc::now().to_rfc3339(),
            resolved_model,
        });
        let saved = (|| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temporary = self.path.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_vec(&state.entries)?)?;
            std::fs::rename(temporary, &self.path)?;
            Ok(())
        })();
        if saved.is_err() {
            eprintln!("Picture support could not be saved.");
        }
    }
    pub fn due(&self, key: &Key) -> bool {
        self.get(key).is_none()
            && self.state.lock().is_ok_and(|state| {
                state
                    .attempts
                    .get(key)
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(300))
            })
    }
    fn claim(&self, key: &Key) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .attempts
            .get(key)
            .is_some_and(|last| last.elapsed() < Duration::from_secs(300))
        {
            return false;
        }
        state.attempts.insert(key.clone(), Instant::now());
        true
    }
}

pub async fn detect(cache: &Cache, key: &Key, provider: &dyn Provider) -> Option<bool> {
    if let Some(answer) = cache.get(key) {
        return Some(answer);
    }
    if !cache.claim(key) {
        return None;
    }
    let answer = match tokio::time::timeout(Duration::from_secs(30), async {
        if let Some(answer) = provider.image_input(&key.model) {
            return Ok(answer);
        }
        provider.probe_image_input(&key.model).await
    })
    .await
    {
        Ok(Ok(answer)) => Some(answer),
        Ok(Err(error)) if image_refusal(&error) => Some(false),
        _ => None,
    };
    // A real refusal received during the probe takes precedence.
    if let Some(current) = cache.get(key) {
        return Some(current);
    }
    if let Some(answer) = answer {
        cache.remember_model(key, answer, provider.resolved_model());
    }
    answer
}

pub fn probe_request(model: &str) -> TurnRequest {
    TurnRequest {
        cache_key: "picture-support".into(),
        system: String::new(),
        model: model.into(),
        effort: "none".into(),
        max_tokens: 1,
        tools: vec![],
        messages: vec![Message::user(vec![
            ContentBlock::Image {
                media_type: "image/png".into(),
                data: PNG.into(),
            },
            ContentBlock::Text {
                text: "Describe".into(),
            },
        ])],
    }
}

pub fn unsupported_image_message(message: &str) -> bool {
    let text = message.to_ascii_lowercase().replace(['_', '-'], " ");
    if [
        "format",
        "size",
        "dimension",
        "decode",
        "base64",
        "quota",
        "rate limit",
        "authentication",
        "api key",
        "billing",
        "credit",
    ]
    .iter()
    .any(|word| text.contains(word))
    {
        return false;
    }
    if text.contains("access denied") || text.contains("not authorized") {
        return false;
    }
    let normalized: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let text = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if [
        "image url is only supported by certain models",
        "unsupported content type image url",
        "unsupported message content type image url",
    ]
    .iter()
    .any(|word| text.contains(word))
    {
        return true;
    }
    [
        "image",
        "images",
        "image input",
        "image inputs",
        "image url",
        "vision",
        "multimodal",
        "picture",
        "pictures",
    ]
    .iter()
    .any(|subject| {
        [
            "not supported",
            "is not supported",
            "are not supported",
            "unsupported",
            "is unsupported",
            "are unsupported",
            "not allowed",
        ]
        .iter()
        .any(|suffix| text.contains(&format!("{subject} {suffix}")))
            || [
                "does not support",
                "doesn't support",
                "cannot accept",
                "does not accept",
                "unsupported",
            ]
            .iter()
            .any(|prefix| text.contains(&format!("{prefix} {subject}")))
    }) || ((text.contains("only supports text") || text.contains("text only model"))
        && ["image", "vision", "multimodal", "picture"]
            .iter()
            .any(|word| text.contains(word)))
}
pub fn image_refusal(error: &AiError) -> bool {
    match error {
        AiError::Api {
            status: 400 | 415 | 422,
            message,
            body,
            ..
        } => {
            !matches!(
                error.code(),
                "auth"
                    | "billing"
                    | "rate_limited"
                    | "context_too_long"
                    | "output_limit"
                    | "model_not_found"
            ) && unsupported_image_message(&format!("{message} {body}"))
        }
        AiError::Subscription("image_unsupported") => true,
        _ => false,
    }
}
fn has_images(req: &TurnRequest) -> bool {
    req.messages
        .iter()
        .flat_map(|m| &m.content)
        .any(|block| match block {
            ContentBlock::Image { .. } => true,
            ContentBlock::ToolResult { content, .. } => content
                .iter()
                .any(|part| matches!(part, ResultPart::Png(_))),
            _ => false,
        })
}
fn without_images(req: &mut TurnRequest) {
    for block in req.messages.iter_mut().flat_map(|m| &mut m.content) {
        match block {
            ContentBlock::Image { .. } => {
                *block = ContentBlock::Text {
                    text: NOTICE.into(),
                }
            }
            ContentBlock::ToolResult { content, .. } => {
                for part in content {
                    if matches!(part, ResultPart::Png(_)) {
                        *part = ResultPart::Text(NOTICE.into());
                    }
                }
            }
            _ => {}
        }
    }
    req.system.push('\n');
    req.system.push_str(NOTICE);
}

pub struct Detected {
    inner: Box<dyn Provider>,
    cache: Arc<Cache>,
    key: Key,
}
impl Detected {
    pub fn new(inner: Box<dyn Provider>, cache: Arc<Cache>, key: Key) -> Self {
        Self { inner, cache, key }
    }
}
impl Provider for Detected {
    fn image_input(&self, model: &str) -> Option<bool> {
        let mut key = self.key.clone();
        key.model = model.into();
        self.cache
            .get(&key)
            .or_else(|| self.inner.image_input(model))
    }
    fn owns_turn(&self) -> bool {
        self.inner.owns_turn()
    }
    fn set_host(&mut self, host: Arc<dyn crate::DrawingHost>) {
        self.inner.set_host(host);
    }
    fn begin_turn(&self, text: &str) {
        self.inner.begin_turn(text);
    }
    fn budget_notice(&self) -> Option<String> {
        self.inner.budget_notice()
    }
    fn save_state(&self) -> Value {
        self.inner.save_state()
    }
    fn restore_state(&self, state: Value) {
        self.inner.restore_state(state);
    }
    fn stream_turn<'a>(
        &'a self,
        mut req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let mut key = self.key.clone();
            key.model = req.model.clone();
            if self.cache.get(&key) == Some(false) && has_images(&req) {
                without_images(&mut req);
            }
            self.inner.set_image_input(self.image_input(&req.model));
            let result = self.inner.stream_turn(req.clone(), sink).await;
            if (has_images(&req) || self.inner.owns_turn())
                && result.as_ref().err().is_some_and(image_refusal)
            {
                self.cache
                    .remember_model(&key, false, self.inner.resolved_model());
                without_images(&mut req);
                if self.inner.owns_turn() {
                    req.system.push_str("\nWork from this turn may already be in the drawing. Read the current drawing and history before continuing. Do not repeat completed changes.");
                }
                self.inner.set_image_input(Some(false));
                sink(StreamEvent::Retrying {
                    seconds: 0,
                    code: "ai.visionUnavailable".into(),
                    diagnostic: NOTICE.into(),
                    discard_partial: true,
                });
                sink(StreamEvent::Activity(Box::new(
                    crate::AgentEvent::ProviderStatus {
                        code: "ai.visionUnavailable".into(),
                    },
                )));
                return self.inner.stream_turn(req, sink).await;
            }
            result
        })
    }
}

#[cfg(test)]
mod tests;
