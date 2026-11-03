//! Keep only the documented top-level written field in transcript summaries.
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::conversation::TranscriptEntry;
use crate::AgentEvent;

fn paths(value: &Value) -> Vec<&str> {
    match value.get("written") {
        Some(Value::String(path)) => vec![path],
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn summary(value: &Value) -> Option<String> {
    let paths = paths(value);
    (!paths.is_empty()).then(|| json!({ "written": paths }).to_string())
}

/// Only successful execute results grant access to an output, never reads or model text.
pub(crate) fn completed(transcript: &[TranscriptEntry], event: &AgentEvent) -> Vec<PathBuf> {
    let AgentEvent::ToolResult {
        id,
        summary,
        is_error: false,
        ..
    } = event
    else {
        return Vec::new();
    };
    let call = transcript.iter().rev().find_map(|entry| match entry {
        TranscriptEntry::Event {
            event: AgentEvent::ToolCall {
                id: call_id, name, ..
            },
        } if call_id == id => Some(name),
        _ => None,
    });
    if call.map(String::as_str) != Some(crate::surface::EXECUTE) {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<Value>(summary) else {
        return Vec::new();
    };
    paths(&value)
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_top_level_written_paths_survive_the_transcript() {
        for written in [
            json!("/tower/plan.pdf"),
            json!(["/tower/plan.pdf", "/tower/boq.xlsx"]),
        ] {
            let value = json!({"written": written, "data": {"written": "/tower/unrelated.pdf"}});
            let kept: Value = serde_json::from_str(&summary(&value).unwrap()).unwrap();
            assert_eq!(kept["written"], json!(paths(&value)));
        }
        for value in [
            json!({"outcome": {"steps": [{"written": "/tower/unrelated.pdf"}]}}),
            json!({"data": {"written": ["/tower/unrelated.pdf"]}}),
            json!([{ "written": "/tower/unrelated.pdf" }]),
            json!({"written": [["/tower/unrelated.pdf"], {"written": "/tower/other.pdf"}]}),
            json!({"path": "/tower/read.pdf"}),
        ] {
            assert!(summary(&value).is_none(), "{value}");
        }
    }

    #[test]
    fn output_access_requires_a_successful_execute_call() {
        let path = std::env::temp_dir().join("tower-plan.pdf");
        let event = |is_error| AgentEvent::ToolResult {
            id: "write".into(),
            activity: None,
            summary: json!({"written": [path, "relative.pdf"]}).to_string(),
            count: None,
            handles: vec![],
            ui: None,
            is_error,
        };
        for name in [crate::surface::READ, crate::surface::EXECUTE] {
            let transcript = vec![TranscriptEntry::Event {
                event: AgentEvent::ToolCall {
                    id: "write".into(),
                    name: name.into(),
                    input: json!({}),
                    label: None,
                    activity: None,
                    mutating: true,
                },
            }];
            assert_eq!(
                completed(&transcript, &event(false)).len(),
                usize::from(name == crate::surface::EXECUTE)
            );
            assert!(completed(&transcript, &event(true)).is_empty());
        }
        assert!(completed(&[], &event(false)).is_empty());
    }
}
