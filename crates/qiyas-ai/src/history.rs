//! Retire verbose tool evidence without changing call ids, signatures or the cached prefix.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::{ContentBlock, Message, ResultPart};

pub(crate) const KEEP_RESULTS: usize = 20;
const SUMMARY_MARK: &str = "[tool result summary]";

/// Keep the first digest byte for byte. Old results are summarized once, in place, so the
/// system, user request and digest remain a reusable cache prefix on every provider.
pub(crate) fn trim(history: &mut [Message]) {
    let pinned = contract_ids(history);
    let redundant = redundant_reads(history);
    let mut digest = None;
    let mut total = 0;
    for message in history.iter() {
        for (id, name, input) in message.tool_uses() {
            if digest.is_none() && name == "qiyas_read" && input["op"] == "digest" {
                digest = Some(id.to_owned());
            }
        }
        total += message
            .content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
            .count();
    }
    let mut index = 0;
    for message in history {
        for block in &mut message.content {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = block
            else {
                continue;
            };
            index += 1;
            if (!redundant.contains(tool_use_id) && index > total.saturating_sub(KEEP_RESULTS))
                || digest.as_ref() == Some(tool_use_id)
                || pinned.contains(tool_use_id)
            {
                continue;
            }
            if matches!(content.first(), Some(ResultPart::Text(t)) if t.starts_with(SUMMARY_MARK)) {
                continue;
            }
            let mut refs = BTreeMap::<String, BTreeSet<String>>::new();
            let mut notes = Vec::new();
            for part in content.iter() {
                if let ResultPart::Text(text) = part {
                    if let Ok(value) = serde_json::from_str::<Value>(text) {
                        references(&value, "", &mut refs);
                        for field in ["summary", "message", "error", "next_offset"] {
                            if let Some(value) = value.get(field).filter(|v| !v.is_null()) {
                                notes.push(format!("{field}: {}", short(&value.to_string(), 240)));
                            }
                        }
                    } else {
                        notes.push(short(text, 320));
                    }
                }
            }
            let pictures = content
                .iter()
                .filter(|p| matches!(p, ResultPart::Png(_)))
                .cloned()
                .collect::<Vec<_>>();
            *content = vec![ResultPart::Text(format!(
                "{SUMMARY_MARK} {}",
                json!({
                    "is_error":is_error,"references":refs,"notes":notes,
                    "detail":"Read records by handle/id or fetch command examples again if needed."
                })
            ))];
            content.extend(pictures);
        }
    }
}

/// Identical successful reads carry the same evidence. Keep its newest observation whole,
/// instead of paying for repeated copies; changed values and failures are never deduplicated.
fn redundant_reads(history: &[Message]) -> BTreeSet<String> {
    let calls: BTreeMap<_, _> = history
        .iter()
        .flat_map(Message::tool_uses)
        .filter(|(_, name, _)| *name == crate::surface::READ)
        .map(|(id, _, input)| {
            (
                id,
                format!(
                    "{}:{}",
                    input["op"],
                    crate::surface::args_of(input).unwrap_or_default()
                ),
            )
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut redundant = BTreeSet::new();
    for block in history.iter().rev().flat_map(|m| m.content.iter().rev()) {
        if let ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error: false,
        } = block
        {
            if let Some(call) = calls.get(tool_use_id.as_str()) {
                if content
                    .iter()
                    .any(|part| matches!(part, ResultPart::Png(_)))
                {
                    continue;
                }
                let evidence = serde_json::to_string(content).unwrap_or_default();
                if evidence.contains(SUMMARY_MARK) {
                    continue;
                }
                if !seen.insert(format!("{call}:{evidence}")) {
                    redundant.insert(tool_use_id.clone());
                }
            }
        }
    }
    redundant
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

/// Carry discovered contracts over message-count compaction without inventing tool calls.
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
                    ResultPart::Text(t) => Some(t.clone()),
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

fn short(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
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
        trim(&mut history);
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
        trim(&mut history);
        assert_eq!(history, once);
    }
}
