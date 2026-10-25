//! Retire verbose tool evidence without changing call ids, signatures or the cached prefix.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::{ContentBlock, Message, ResultPart};

pub(crate) const KEEP_RESULTS: usize = 4;
pub(crate) const HIGH_TOKENS: usize = 100_000;
pub(crate) const LOW_TOKENS: usize = 55_000;
pub(crate) const CLEAR_AT_LEAST: usize = 30_000;
const SUMMARY_MARK: &str = "[tool result summary]";

#[derive(Debug, Clone, Copy)]
pub(crate) struct Trim {
    pub before: usize,
    pub after: usize,
    pub results: usize,
}

/// Conservative local estimate, not a provider tokenizer. Never count base64 as prose.
pub(crate) fn tokens(history: &[Message]) -> usize {
    history.iter().map(message_tokens).sum()
}

pub(crate) fn message_tokens(message: &Message) -> usize {
    4 + message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Image { .. } => 1600,
            ContentBlock::ToolResult { content, .. } => {
                8 + content
                    .iter()
                    .map(|part| match part {
                        ResultPart::Png(_) => 1600,
                        ResultPart::Text(text) => text.len().div_ceil(3),
                    })
                    .sum::<usize>()
            }
            _ => serde_json::to_vec(block).map_or(0, |v| v.len().div_ceil(3)),
        })
        .sum::<usize>()
}

/// Only a threshold crossing may rewrite results. Prepare a whole batch first, and do
/// nothing unless it clears enough context to justify rebuilding the provider cache.
pub(crate) fn trim(history: &mut [Message]) -> Option<Trim> {
    trim_at(history, HIGH_TOKENS, LOW_TOKENS, CLEAR_AT_LEAST)
}

pub(crate) fn trim_at(
    history: &mut [Message],
    high: usize,
    low: usize,
    minimum: usize,
) -> Option<Trim> {
    trim_with_keep(history, high, low, minimum, KEEP_RESULTS)
}

/// A provider has refused the context: one recovery attempt may retire even recent evidence.
/// The evidence stays reachable, and every call id, user request and signature stays intact.
pub(crate) fn context_refusal(history: &mut [Message]) -> Option<Trim> {
    trim_with_keep(history, 0, tokens(history) / 2, 1, 0)
}

fn trim_with_keep(
    history: &mut [Message],
    high: usize,
    low: usize,
    minimum: usize,
    keep: usize,
) -> Option<Trim> {
    let before = tokens(history);
    if before < high {
        return None;
    }
    let calls: BTreeMap<String, String> = history
        .iter()
        .flat_map(Message::tool_uses)
        .map(|(id, name, input)| (id.to_owned(), operation(name, input)))
        .collect();
    let digest = history
        .iter()
        .flat_map(Message::tool_uses)
        .find(|(_, name, input)| *name == "qiyas_read" && input["op"] == "digest")
        .map(|(id, _, _)| id.to_owned());
    let total = history
        .iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
        .count();
    let mut index = 0;
    let mut after = before;
    let mut changes = Vec::new();
    for (m, message) in history.iter().enumerate() {
        for (b, block) in message.content.iter().enumerate() {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = block
            else {
                continue;
            };
            index += 1;
            if index > total.saturating_sub(keep)
                || digest.as_ref() == Some(tool_use_id)
                || matches!(content.first(), Some(ResultPart::Text(t)) if t.starts_with(SUMMARY_MARK))
            {
                continue;
            }
            let mut refs = BTreeMap::<String, BTreeSet<String>>::new();
            let mut errors = Vec::new();
            let mut artifact = None;
            for part in content {
                if let ResultPart::Text(text) = part {
                    if let Ok(value) = serde_json::from_str::<Value>(text) {
                        references(&value, "", &mut refs);
                        artifact =
                            artifact.or_else(|| value["artifact"].as_str().map(str::to_owned));
                        for field in ["message", "error", "errors", "code"] {
                            if let Some(v) = value.get(field).filter(|v| !v.is_null()) {
                                errors.push(v.clone());
                            }
                        }
                    } else if *is_error {
                        errors.push(json!(text));
                    }
                }
            }
            for values in refs.values_mut() {
                *values = values.iter().take(5).cloned().collect();
            }
            let evidence: Vec<_> = content
                .iter()
                .map(|part| match part {
                    ResultPart::Png(_) => ResultPart::Text(
                        "[picture removed at trim; capture again if needed]".into(),
                    ),
                    _ => part.clone(),
                })
                .collect();
            let original = json!({"call_id":tool_use_id,"operation":calls.get(tool_use_id),"content":evidence,"is_error":is_error});
            let evidence_id = artifact.unwrap_or_else(|| {
                crate::artifact::hash(crate::artifact::canonical(&original).to_string().as_bytes())
            });
            let replacement = format!(
                "{SUMMARY_MARK} {}",
                json!({"call_id":tool_use_id,"operation":calls.get(tool_use_id),
                "is_error":is_error,"references":refs,"errors":errors,"artifact":evidence_id})
            );
            let old = message_tokens(&Message::user(vec![block.clone()]));
            let new = replacement.len().div_ceil(3) + 12;
            if old <= new {
                continue;
            }
            after = after.saturating_sub(old - new);
            changes.push((m, b, replacement, original));
            if after <= low {
                break;
            }
        }
        if after <= low {
            break;
        }
    }
    if before.saturating_sub(after) < minimum {
        return None;
    }
    let mut ready = Vec::new();
    let mut cleared = 0;
    for (m, b, text, original) in changes {
        // Keep the old result if its lossless evidence could not be saved.
        if crate::artifact::save(&original).is_err() {
            continue;
        }
        cleared += message_tokens(&Message::user(vec![history[m].content[b].clone()]))
            .saturating_sub(text.len().div_ceil(3) + 12);
        ready.push((m, b, text));
    }
    if cleared < minimum {
        return None;
    }
    let applied = ready.len();
    for (m, b, text) in ready {
        if let ContentBlock::ToolResult { content, .. } = &mut history[m].content[b] {
            *content = vec![ResultPart::Text(text)];
        }
    }
    (applied > 0).then(|| Trim {
        before,
        after: tokens(history),
        results: applied,
    })
}

fn operation(name: &str, input: &Value) -> String {
    let mut operation = format!("{name}.{}", input["op"].as_str().unwrap_or("picture"));
    if let Ok(args) = crate::surface::args_of(input) {
        let command = args["command"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or_else(|| args["command"].clone());
        if let Some(kind) = command["type"].as_str() {
            operation.push('/');
            operation.push_str(kind);
            if let Some(kind) = command["kind"].as_str() {
                operation.push('.');
                operation.push_str(kind);
            }
        }
    }
    operation
}

/// Latest successful discovery by operation and requested name; failed refreshes never replace it.
pub(crate) fn contract_ids(history: &[Message]) -> BTreeSet<String> {
    let calls: BTreeMap<_, _> = history
        .iter()
        .flat_map(Message::tool_uses)
        .filter_map(|(id, name, input)| {
            let op = input["op"].as_str()?;
            if name != "qiyas_read"
                || !matches!(
                    op,
                    "commands" | "record_schema" | "takeoff_kit" | "workflow"
                )
            {
                return None;
            }
            let args = crate::surface::args_of(input).ok()?;
            Some((id, format!("{op}:{args}")))
        })
        .collect();
    let mut latest = BTreeMap::new();
    for block in history.iter().flat_map(|m| &m.content) {
        if let crate::ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error: false,
        } = block
        {
            if !matches!(content.first(), Some(ResultPart::Text(t)) if t.starts_with(SUMMARY_MARK))
            {
                if let Some(key) = calls.get(tool_use_id.as_str()) {
                    latest.insert(key.clone(), tool_use_id.clone());
                }
            }
        }
    }
    latest.into_values().collect()
}

/// Carry discovered contracts over token-based compaction without inventing tool calls.
pub(crate) fn contracts(history: &[Message]) -> Vec<String> {
    let ids = contract_ids(history);
    history
        .iter()
        .flat_map(|m| &m.content)
        .flat_map(|b| match b {
            crate::ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } if ids.contains(tool_use_id) => content
                .iter()
                .filter_map(|p| match p {
                    ResultPart::Text(t) => Some(contract_reference(t)),
                    _ => None,
                })
                .collect(),
            crate::ContentBlock::Text { text } if text.starts_with("[known contracts]\n") => {
                serde_json::from_str::<Vec<String>>(&text[18..]).unwrap_or_default()
            }
            _ => Vec::new(),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Carry the addresses of retired evidence through a full compaction as well as a trim.
pub(crate) fn evidence(history: &[Message]) -> Result<Vec<Value>, crate::ToolError> {
    let calls: BTreeMap<_, _> = history
        .iter()
        .flat_map(Message::tool_uses)
        .map(|(id, name, input)| (id.to_owned(), operation(name, input)))
        .collect();
    let mut saved = BTreeMap::new();
    for block in history.iter().flat_map(|m| &m.content) {
        match block {
            ContentBlock::Text { text } if text.starts_with("[evidence artifacts]\n") => {
                if let Ok(entries) = serde_json::from_str::<Vec<Value>>(&text[21..]) {
                    for entry in entries {
                        saved.insert(entry["call_id"].to_string(), entry);
                    }
                }
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let text = content
                    .iter()
                    .filter_map(|part| match part {
                        ResultPart::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let value = serde_json::from_str::<Value>(
                    text.strip_prefix(SUMMARY_MARK).unwrap_or(&text).trim(),
                )
                .unwrap_or_else(|_| json!({"text":text}));
                let artifact = match value["artifact"].as_str() {
                    Some(id) => id.to_owned(),
                    None => crate::artifact::save(&value)?,
                };
                saved.insert(json!(tool_use_id).to_string(),json!({"call_id":tool_use_id,"operation":calls.get(tool_use_id),"is_error":is_error,"artifact":artifact}));
            }
            _ => {}
        }
    }
    Ok(saved.into_values().collect())
}

fn contract_reference(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.to_owned();
    };
    fn names(value: &Value, out: &mut Vec<Value>) {
        if value["name"].is_string() {
            out.push(json!({"name":value["name"],"version":value["version"],"artifact":value["artifact"]}));
        }
        for key in ["commands", "operations"] {
            for item in value[key].as_array().into_iter().flatten() {
                names(item, out);
            }
        }
    }
    let mut contracts = Vec::new();
    names(&value, &mut contracts);
    if contracts.is_empty() && text.len() < 2000 {
        return text.to_owned();
    }
    match crate::artifact::save(&value) {
        Ok(id) => json!({"contracts":contracts,"artifact":id,"detail":"Read the artifact for the discovered contract."}).to_string(),
        Err(_) => text.to_owned(),
    }
}

fn references(value: &Value, key: &str, refs: &mut BTreeMap<String, BTreeSet<String>>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                // Schemas and default metadata contain no identities of executed work.
                if !matches!(
                    key.as_str(),
                    "schema" | "examples" | "example" | "defaults_used"
                ) {
                    references(value, key, refs);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, key, refs);
            }
        }
        Value::String(_) | Value::Number(_)
            if matches!(
                key,
                "handle"
                    | "handles"
                    | "added"
                    | "changed"
                    | "removed"
                    | "centrelines"
                    | "outlines"
                    | "hatches"
                    | "inserts"
                    | "command"
                    | "object"
                    | "driver"
                    | "id"
                    | "key"
                    | "code"
                    | "name"
                    | "object_type"
                    | "rule"
                    | "state"
                    | "pointer"
            ) =>
        {
            refs.entry(key.into())
                .or_default()
                .insert(value.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    #[test]
    fn short_jobs_are_append_only_even_with_duplicate_reads() {
        let mut history = Vec::new();
        for n in 0..60 {
            history.push(Message::assistant(vec![ContentBlock::ToolUse {
                id: n.to_string(),
                name: "qiyas_read".into(),
                input: json!({"op":"state"}),
            }]));
            history.push(Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: n.to_string(),
                content: vec![ResultPart::Text(
                    json!({"revision":1,"state":"same"}).to_string(),
                )],
                is_error: false,
            }]));
            let before = history.clone();
            assert!(trim(&mut history).is_none());
            assert_eq!(history, before, "request {n} rewrote earlier evidence");
        }
    }

    #[test]
    fn watermark_trim_keeps_words_pairs_and_signatures_and_clears_a_large_batch() {
        let mut history = vec![Message::user(vec![ContentBlock::Text {
            text: "Keep the exact brief. راجع المقاسات.".into(),
        }])];
        for n in 0..16 {
            history.push(Message::assistant(vec![
                ContentBlock::Thinking {
                    thinking: format!("reason {n}"),
                    signature: format!("signed-{n}"),
                },
                ContentBlock::Raw(
                    json!({"type":"reasoning","encrypted_content":format!("opaque-{n}")}),
                ),
                ContentBlock::ToolUse {
                    id: n.to_string(),
                    name: "qiyas_read".into(),
                    input: json!({"op":if n == 0 {"digest"} else {"records"}}),
                },
            ]));
            history.push(Message::user(vec![ContentBlock::ToolResult { tool_use_id:n.to_string(), is_error:n == 2,
                content:vec![ResultPart::Text(json!({"handle":n,"quantity":12.34567890123456,"data":"x".repeat(30_000),"code":if n == 2 {"missing"} else {"ok"}}).to_string())] }]));
        }
        let before = history.clone();
        let report = trim(&mut history).unwrap();
        assert!(report.before - report.after >= CLEAR_AT_LEAST);
        assert!(report.after < HIGH_TOKENS);
        assert_eq!(history[0], before[0]);
        assert_eq!(history[2], before[2], "first digest");
        assert_eq!(&history[history.len() - 8..], &before[before.len() - 8..]);
        for (before, after) in before.iter().zip(&history) {
            if before.role == Role::Assistant {
                assert_eq!(before, after);
            }
            for (before, after) in before.content.iter().zip(&after.content) {
                if let (
                    ContentBlock::ToolResult {
                        tool_use_id: a,
                        is_error: x,
                        ..
                    },
                    ContentBlock::ToolResult {
                        tool_use_id: b,
                        is_error: y,
                        ..
                    },
                ) = (before, after)
                {
                    assert_eq!((a, x), (b, y));
                }
            }
        }
        let once = history.clone();
        assert!(trim(&mut history).is_none());
        assert_eq!(history, once);
    }

    #[test]
    fn keeps_recent_results_digest_ids_errors_and_protocol_pairs() {
        let mut history = vec![Message::assistant(vec![ContentBlock::ToolUse {
            id: "digest".into(),
            name: "qiyas_read".into(),
            input: json!({"op":"digest"}),
        }])];
        for id in ["digest".to_string(), "old".to_string()]
            .into_iter()
            .chain((0..KEEP_RESULTS).map(|i| format!("new{i}")))
        {
            history.push(Message::user(vec![ContentBlock::ToolResult {
                tool_use_id:id.clone(), is_error:id=="old", content:vec![ResultPart::Text(json!({
                    "handle":123,"id":"item-7","code":"invalid_value","schema":{"description":"large".repeat(2000)}
                }).to_string())]
            }]));
        }
        let before = history.clone();
        trim_at(&mut history, 1, 1, 1);
        assert_eq!(history[0], before[0]);
        assert_eq!(history[1], before[1]);
        assert_eq!(&history[3..], &before[3..]);
        assert_eq!(history[2].role, Role::User);
        let text = serde_json::to_string(&history[2]).unwrap();
        for identity in ["123", "item-7", "invalid_value", "old"] {
            assert!(text.contains(identity));
        }
        assert!(text.len() < 1000);
        let once = history.clone();
        trim_at(&mut history, 1, 1, 1);
        assert_eq!(history, once);
    }
}
