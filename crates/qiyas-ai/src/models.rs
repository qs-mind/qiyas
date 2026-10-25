//! The models a provider offers, for the settings' model list: Anthropic's Models API
//! (`GET /v1/models`, paged) or an OpenAI-compatible service's `GET {base_url}/models`. When the
//! service can't be asked, Claude's current models are the built-in list.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::provider::{http_client, send_with_retry, AiError};

/// Anthropic's Models API.
pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
/// The API version the Models API is asked with.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Most pages read from Anthropic's Models API (a thousand models each).
const MAX_PAGES: usize = 5;
/// How long the list may take.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Claude's current models, the most capable first after the default.
pub const CLAUDE_MODELS: [(&str, &str); 4] = [
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-fable-5-1", "Claude Fable 5.1"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("claude-haiku-5-5", "Claude Haiku 5.5"),
];

/// One model: its id, as the API takes it, and a name to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    /// None means the service supplied no image capability.
    pub image_input: Option<bool>,
}

/// Where the list comes from.
#[derive(Debug, Clone, Copy)]
pub enum ModelSource<'a> {
    Anthropic,
    OpenAiCompatible { base_url: &'a str },
}

/// The models offered when the service can't be asked: Claude's for Anthropic, none for an
/// OpenAI-compatible service (whose ids only it knows).
pub fn builtin_models(source: ModelSource) -> Vec<ModelInfo> {
    match source {
        ModelSource::Anthropic => CLAUDE_MODELS
            .iter()
            .map(|(id, label)| ModelInfo {
                id: (*id).to_string(),
                label: (*label).to_string(),
                image_input: Some(true),
            })
            .collect(),
        ModelSource::OpenAiCompatible { .. } => Vec::new(),
    }
}

/// The models the service offers. The key is only put in the request's header and scrubbed
/// from any error.
pub async fn list_models(
    source: ModelSource<'_>,
    key: Option<&str>,
) -> Result<Vec<ModelInfo>, AiError> {
    let client = http_client();
    let secret = key.unwrap_or("");
    match source {
        ModelSource::Anthropic => {
            let mut models = Vec::new();
            let mut after: Option<String> = None;
            for _ in 0..MAX_PAGES {
                let mut url = format!("{MODELS_URL}?limit=1000");
                if let Some(id) = &after {
                    url.push_str(&format!("&after_id={id}"));
                }
                let response = send_with_retry(
                    || {
                        client
                            .get(&url)
                            .timeout(TIMEOUT)
                            .header("x-api-key", secret)
                            .header("anthropic-version", ANTHROPIC_VERSION)
                    },
                    secret,
                    &Value::Null,
                    false,
                    &mut |_| {},
                )
                .await?;
                let page = read_json(response, secret).await?;
                let (found, next) = parse_anthropic_page(&page);
                models.extend(found);
                match next {
                    Some(id) => after = Some(id),
                    None => break,
                }
            }
            Ok(models)
        }
        ModelSource::OpenAiCompatible { base_url } => {
            let base = crate::endpoint::normalize(base_url).map_err(AiError::Network)?;
            let url = format!("{base}/models");
            let response = send_with_retry(
                || {
                    let r = client.get(&url).timeout(TIMEOUT);
                    match key {
                        Some(key) => r.bearer_auth(key),
                        None => r,
                    }
                },
                secret,
                &Value::Null,
                false,
                &mut |_| {},
            )
            .await?;
            Ok(parse_openai_models(&read_json(response, secret).await?))
        }
    }
}

async fn read_json(response: reqwest::Response, secret: &str) -> Result<Value, AiError> {
    response
        .json::<Value>()
        .await
        .map_err(|e| AiError::Network(e.to_string()).scrubbed(secret))
}

/// One page of Anthropic's Models API: its models, and the id to read on from when there are
/// more (`has_more`, `last_id`).
pub fn parse_anthropic_page(page: &Value) -> (Vec<ModelInfo>, Option<String>) {
    let models = page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            if !supports_assistant(m) {
                return None;
            }
            let id = m["id"].as_str()?.to_string();
            let label = m["display_name"].as_str().unwrap_or(&id).to_string();
            Some(ModelInfo {
                id,
                label,
                image_input: image_input(m),
            })
        })
        .collect();
    let next = page["has_more"]
        .as_bool()
        .unwrap_or(false)
        .then(|| page["last_id"].as_str().map(str::to_string))
        .flatten();
    (models, next)
}

/// An OpenAI-compatible `/models` answer (`data[].id`, a `name` when the service gives one),
/// sorted by id.
pub fn parse_openai_models(answer: &Value) -> Vec<ModelInfo> {
    let mut models: Vec<ModelInfo> = answer["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            if !supports_assistant(m) {
                return None;
            }
            let id = m["id"].as_str()?.to_string();
            let label = m["name"].as_str().unwrap_or(&id).to_string();
            Some(ModelInfo {
                id,
                label,
                image_input: image_input(m),
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

/// A missing capability is unknown; an explicit refusal is never offered to Mizan.
/// OpenRouter and compatible servers describe both input modalities and tool parameters.
fn supports_assistant(model: &Value) -> bool {
    for key in ["tools", "tool_calling", "function_calling"] {
        let cap = &model["capabilities"][key];
        if cap.as_bool() == Some(false) || cap["supported"].as_bool() == Some(false) {
            return false;
        }
    }
    if let Some(parameters) = model["supported_parameters"].as_array() {
        if !parameters
            .iter()
            .any(|p| p == "tools" || p == "tool_choice")
        {
            return false;
        }
    }
    true
}

/// An explicit capability or complete input list is evidence; a model name is not.
pub fn image_input(model: &Value) -> Option<bool> {
    let cap = &model["capabilities"]["vision"];
    if let Some(value) = cap.as_bool().or_else(|| cap["supported"].as_bool()) {
        return Some(value);
    }
    for path in ["/architecture/input_modalities", "/input_modalities"] {
        if let Some(modalities) = model.pointer(path).and_then(Value::as_array) {
            return Some(modalities.iter().any(|m| m == "image"));
        }
    }
    None
}

pub(crate) fn image_state(supported: Option<bool>) -> Value {
    serde_json::json!({"supported":supported,"instruction":match supported {
        Some(true) => "Images are supported. Verify exact geometry first, then inspect pictures.",
        Some(false) => "This model cannot see images. Use exact queries and checks. Never claim to have inspected a picture.",
        None => "Image capability is unknown. Use exact queries and checks; do not claim visual inspection without confirmed image support.",
    }})
}

/// What the settings show: the service's list, or the built-in one with why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelList {
    pub models: Vec<ModelInfo>,
    /// The list came from the service (else it is the built-in one).
    pub from_service: bool,
    pub error: Option<String>,
}

/// `list_models`, or the built-in list with the error when the service can't be asked.
pub async fn model_list(source: ModelSource<'_>, key: Option<&str>) -> ModelList {
    match list_models(source, key).await {
        Ok(models) => ModelList {
            models,
            from_service: true,
            error: None,
        },
        Err(e) => ModelList {
            models: builtin_models(source),
            from_service: false,
            error: Some(e.for_window()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_anthropic_page_gives_models_and_where_to_read_on() {
        let page = json!({
            "data": [
                {"type": "model", "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5"},
                {"type": "model", "id": "claude-sonnet-5-5"}
            ],
            "has_more": true,
            "first_id": "claude-opus-5-5",
            "last_id": "claude-sonnet-5-5"
        });
        let (models, next) = parse_anthropic_page(&page);
        assert_eq!(models[0].label, "Claude Opus 5.5");
        assert_eq!(
            models[1].label, "claude-sonnet-5-5",
            "the id when there is no name"
        );
        assert_eq!(next.as_deref(), Some("claude-sonnet-5-5"));
        let last = json!({"data": [], "has_more": false, "last_id": null});
        assert_eq!(parse_anthropic_page(&last).1, None);
    }

    #[test]
    fn an_openai_list_is_sorted_and_named() {
        let answer = json!({"object": "list", "data": [
            {"id": "openai/gpt-x", "name": "GPT X"},
            {"id": "google:gemini@3.8-flash"},
            {"id": "openai/gpt-x"}
        ]});
        let models = parse_openai_models(&answer);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["google:gemini@3.8-flash", "openai/gpt-x"]
        );
        assert_eq!(models[1].label, "GPT X");
    }

    #[test]
    fn the_builtin_list_is_claudes_current_models() {
        let ids: Vec<String> = builtin_models(ModelSource::Anthropic)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids[0], "claude-opus-5-5");
        assert!(ids.contains(&"claude-haiku-5-5".to_string()));
        assert!(builtin_models(ModelSource::OpenAiCompatible { base_url: "x" }).is_empty());
    }

    #[test]
    fn missing_tools_are_excluded_but_blind_models_have_explicit_capabilities() {
        let models = parse_openai_models(&json!({"data":[
            {"id":"image-only","supported_parameters":[],"architecture":{"input_modalities":["text","image"]}},
            {"id":"text-only","supported_parameters":["tools"],"architecture":{"input_modalities":["text"]}},
            {"id":"no-vision","capabilities":{"vision":false}},
            {"id":"no-tools","capabilities":{"tool_calling":{"supported":false}}},
            {"id":"unknown"},
            {"id":"suitable","supported_parameters":["tools"],"architecture":{"input_modalities":["text","image"]}}
        ]}));
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["no-vision", "suitable", "text-only", "unknown"]
        );
        assert_eq!(
            models.iter().map(|m| m.image_input).collect::<Vec<_>>(),
            vec![Some(false), Some(true), Some(false), None]
        );
    }

    #[tokio::test]
    async fn an_unreachable_service_falls_back_to_the_builtin_list() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        let list = model_list(
            ModelSource::OpenAiCompatible { base_url: &url },
            Some("sk-SECRET-123"),
        )
        .await;
        assert!(!list.from_service);
        assert!(list.models.is_empty());
        let why = list.error.unwrap();
        assert!(!why.contains("SECRET"), "{why}");
    }
}
