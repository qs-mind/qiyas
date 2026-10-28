use std::collections::{BTreeMap, HashSet};

use super::*;

/// Keep streamed items until completion. A completed response can omit any of them.
#[derive(Default)]
pub(in crate::chatgpt_plan) struct Events {
    pub(super) unsupported: Option<String>,
    pub(super) result: Option<TurnResult>,
    items: BTreeMap<usize, Value>,
    ids: HashMap<String, usize>,
    announced: HashSet<String>,
    emitted: HashMap<(usize, usize), String>,
    tokens: Option<Tokens>,
    pub(super) account_id: Option<String>,
    finalizing: bool,
    thinking: String,
    emitted_thinking: String,
}

impl Events {
    pub(in crate::chatgpt_plan) fn with_tokens(tokens: &Tokens) -> Self {
        Self {
            tokens: Some(tokens.clone()),
            ..Self::default()
        }
    }

    fn clean(&self, text: &str) -> String {
        self.tokens
            .as_ref()
            .map_or_else(|| text.to_owned(), |tokens| tokens.scrub(text))
    }
    fn index(&mut self, event: &Value, item: &Value, completed: bool) -> usize {
        let id = item["id"].as_str().or_else(|| event["item_id"].as_str());
        if let Some(index) = id.and_then(|id| self.ids.get(id)) {
            return *index;
        }
        if let Some(call) = item["call_id"].as_str() {
            if let Some((&index, _)) = self.items.iter().find(|(_, old)| old["call_id"] == call) {
                if let Some(id) = id {
                    self.ids.insert(id.into(), index);
                }
                return index;
            }
        }
        let index = event["output_index"].as_u64().map(|n| n as usize);
        let index = index
            .filter(|index| {
                !completed
                    || self.items.get(index).is_none_or(|old| {
                        (old["id"].is_null() || id.is_none())
                            && (old["type"].is_null() || old["type"] == item["type"])
                    })
            })
            .or_else(|| {
                // Some streams omit both indices and IDs. Match their one anonymous item.
                self.items
                    .iter()
                    .find(|(_, old)| {
                        old["type"] == item["type"]
                            && old["id"].is_null()
                            && (item["call_id"].is_null() || old["call_id"] == item["call_id"])
                    })
                    .map(|(&index, _)| index)
            })
            .unwrap_or_else(|| self.items.keys().next_back().map_or(0, |i| i + 1));
        if let Some(id) = id {
            self.ids.insert(id.into(), index);
        }
        index
    }

    fn merge_item(
        &mut self,
        event: &Value,
        item: Value,
        completed: bool,
        sink: Sink<'_>,
    ) -> Result<(), AiError> {
        if !item.is_object() || item.get("content").is_some_and(|v| !v.is_array()) {
            return Err(AiError::Subscription("chatgpt_incomplete"));
        }
        let index = self.index(event, &item, completed);
        let stored = self.items.entry(index).or_insert(Value::Null);
        fill(stored, &item);
        if event["type"] == "response.output_item.done" {
            // Opaque reasoning is not append-only text. Keep its final streamed value.
            for key in ["status", "encrypted_content"] {
                if let Some(value) = item
                    .get(key)
                    .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                {
                    stored[key] = value.clone();
                }
            }
        }
        self.announce(index, sink);
        self.text(index, sink);
        Ok(())
    }

    fn announce(&mut self, index: usize, sink: Sink<'_>) {
        let item = &self.items[&index];
        if item["type"] == "function_call" {
            if let (Some(id), Some(name)) = (item["call_id"].as_str(), item["name"].as_str()) {
                if self.announced.insert(id.into()) {
                    sink(StreamEvent::ToolUseStart {
                        id: id.into(),
                        name: name.into(),
                    });
                }
            }
        }
    }

    fn text(&mut self, index: usize, sink: Sink<'_>) {
        for (part, block) in self.items[&index]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if block["type"] != "output_text" {
                continue;
            }
            let text = safe_text(
                block["text"].as_str().unwrap_or(""),
                self.tokens.as_ref(),
                self.finalizing,
            );
            let emitted = self.emitted.entry((index, part)).or_default();
            if let Some(suffix) = text
                .strip_prefix(emitted.as_str())
                .filter(|s| !s.is_empty())
            {
                sink(StreamEvent::TextDelta(suffix.into()));
                *emitted = text;
            }
        }
    }

    fn emit_thinking(&mut self, sink: Sink<'_>) {
        let text = safe_text(&self.thinking, self.tokens.as_ref(), self.finalizing);
        if let Some(suffix) = text
            .strip_prefix(self.emitted_thinking.as_str())
            .filter(|s| !s.is_empty())
        {
            sink(StreamEvent::Thinking(suffix.into()));
            self.emitted_thinking = text;
        }
    }

    pub(in crate::chatgpt_plan) fn read(
        &mut self,
        value: Value,
        sink: Sink<'_>,
    ) -> Result<bool, AiError> {
        match value["type"].as_str().unwrap_or("") {
            "response.output_item.added" | "response.output_item.done" => {
                self.merge_item(&value, value["item"].clone(), false, sink)?;
            }
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "response.content_part.added"
            | "response.content_part.done" => {
                let mut item = json!({"type":"message","content":[]});
                if let Some(id) = value.get("item_id") {
                    item["id"] = id.clone();
                }
                let index = self.index(&value, &item, false);
                let entry = self.items.entry(index).or_insert(item);
                let content = entry
                    .as_object_mut()
                    .ok_or(AiError::Subscription("chatgpt_incomplete"))?
                    .entry("content")
                    .or_insert(json!([]));
                let parts = content
                    .as_array_mut()
                    .ok_or(AiError::Subscription("chatgpt_incomplete"))?;
                let part = value["content_index"].as_u64().unwrap_or(0) as usize;
                if part > 4096 {
                    return Err(AiError::Subscription("chatgpt_incomplete"));
                }
                parts.resize(parts.len().max(part + 1), Value::Null);
                if value.get("part").is_some() {
                    fill(&mut parts[part], &value["part"]);
                } else {
                    let refusal = value["type"].as_str().unwrap_or("").contains("refusal");
                    let field = if refusal { "refusal" } else { "text" };
                    if parts[part].is_null() {
                        parts[part] =
                            json!({"type": if refusal {"refusal"} else {"output_text"}, field:""});
                    }
                    if let Some(delta) = value["delta"].as_str() {
                        let mut text = parts[part][field].as_str().unwrap_or("").to_owned();
                        text.push_str(delta);
                        parts[part][field] = json!(text);
                    } else if let Some(text) = value.get(field) {
                        fill(&mut parts[part][field], text);
                    }
                }
                self.text(index, sink);
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(text) = value["delta"].as_str() {
                    self.thinking.push_str(text);
                    self.emit_thinking(sink);
                }
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                let mut item = json!({"type":"function_call","arguments":""});
                if let Some(id) = value.get("item_id") {
                    item["id"] = id.clone();
                }
                let index = self.index(&value, &item, false);
                let entry = self.items.entry(index).or_insert(item);
                if let Some(delta) = value["delta"].as_str() {
                    let mut args = entry["arguments"].as_str().unwrap_or("").to_owned();
                    args.push_str(delta);
                    entry["arguments"] = json!(args);
                    if let Some(id) = entry["call_id"].as_str() {
                        sink(StreamEvent::ToolInputDelta {
                            id: id.into(),
                            json_fragment: delta.into(),
                        });
                    }
                } else if let Some(args) = value.get("arguments") {
                    fill(&mut entry["arguments"], args);
                }
            }
            "response.incomplete" => return Err(AiError::Subscription("chatgpt_incomplete")),
            "response.completed" => {
                self.finalizing = true;
                self.emit_thinking(sink);
                let response = &value["response"];
                for (index, item) in response["output"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    self.merge_item(&json!({"output_index":index}), item.clone(), true, sink)?;
                }
                for index in self.items.keys().copied().collect::<Vec<_>>() {
                    self.text(index, sink);
                }
                let types: Vec<_> = self
                    .items
                    .values()
                    .map(|i| i["type"].as_str().unwrap_or("unknown"))
                    .collect();
                sink(StreamEvent::Diagnostic {
                    kind: "RESPONSE ITEMS".into(),
                    text: format!("ChatGPT: {}", types.join(", ")),
                });
                let mut content = Vec::new();
                let mut refused = None;
                for item in self.items.values() {
                    match item["type"].as_str().unwrap_or("") {
                        "message" => {
                            for block in item["content"].as_array().into_iter().flatten() {
                                if block["type"] == "output_text" {
                                    if let Some(text) =
                                        block["text"].as_str().filter(|s| !s.is_empty())
                                    {
                                        content.push(ContentBlock::Text {
                                            text: self.clean(text),
                                        });
                                    }
                                }
                                if block["type"] == "refusal" {
                                    refused =
                                        Some(self.clean(block["refusal"].as_str().unwrap_or("")));
                                }
                            }
                        }
                        "reasoning" => content.push(ContentBlock::Raw(item.clone())),
                        "function_call" => {
                            let id = item["call_id"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .ok_or(AiError::Subscription("chatgpt_incomplete"))?;
                            let name = item["name"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .ok_or(AiError::Subscription("chatgpt_incomplete"))?;
                            let raw = self.clean(item["arguments"].as_str().unwrap_or(""));
                            let input = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
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
                let u = &response["usage"];
                let cached = u["input_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                let usage = Usage {
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
                };
                if refused.is_none()
                    && !content.iter().any(|c| {
                        matches!(c, ContentBlock::Text{text} if !text.trim().is_empty())
                            || matches!(c, ContentBlock::ToolUse { .. })
                    })
                {
                    sink(StreamEvent::Diagnostic {
                        kind: "EMPTY ANSWER".into(),
                        text: format!(
                            "ChatGPT: {} output tokens, {} reasoning tokens",
                            usage.output_tokens, usage.reasoning_tokens
                        ),
                    });
                    return Err(AiError::Subscription("chatgpt_empty"));
                }
                let stop_reason = if let Some(text) = refused {
                    StopReason::Refusal {
                        category: None,
                        text,
                    }
                } else if content
                    .iter()
                    .any(|c| matches!(c, ContentBlock::ToolUse { .. }))
                {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                };
                self.result = Some(TurnResult {
                    content,
                    stop_reason,
                    usage,
                });
                sink(StreamEvent::Done);
                return Ok(true);
            }
            "response.failed" | "error" => {
                let detail = value
                    .pointer("/response/error")
                    .or_else(|| value.get("error"))
                    .unwrap_or(&value);
                self.unsupported = detail["param"].as_str().map(str::to_owned);
                sink(error_diagnostic(
                    "stream",
                    detail,
                    self.tokens.as_ref(),
                    self.account_id.as_deref(),
                ));
                return Err(classify_failure(None, detail));
            }
            _ => {}
        }
        Ok(false)
    }
}

/// Snapshots fill missing fields and extend prefixes, never discard a streamed item.
fn fill(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                fill(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (Value::Array(target), Value::Array(source)) => {
            target.resize(target.len().max(source.len()), Value::Null);
            for (index, value) in source.iter().enumerate() {
                fill(&mut target[index], value);
            }
        }
        (Value::String(target), Value::String(source)) if source.starts_with(target.as_str()) => {
            target.clone_from(source)
        }
        (target, source) if target.is_null() => *target = source.clone(),
        _ => {}
    }
}

/// Hold only a possible credential prefix between deltas, so joining fragments cannot leak it.
fn safe_text(text: &str, tokens: Option<&Tokens>, finalizing: bool) -> String {
    let Some(tokens) = tokens else {
        return text.into();
    };
    let clean = tokens.scrub(text);
    if finalizing {
        return clean;
    }
    let secrets = [
        tokens.access_token.as_str(),
        tokens.id_token.as_str(),
        tokens.refresh_token.as_deref().unwrap_or(""),
    ];
    let max = secrets.iter().map(|s| s.len()).max().unwrap_or(0);
    let boundary = clean
        .char_indices()
        .find_map(|(index, _)| {
            let suffix = &clean[index..];
            (suffix.len() < max
                && secrets
                    .iter()
                    .any(|secret| !secret.is_empty() && secret.starts_with(suffix)))
            .then_some(index)
        })
        .unwrap_or(clean.len());
    clean[..boundary].into()
}
