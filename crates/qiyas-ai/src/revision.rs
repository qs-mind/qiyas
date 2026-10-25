//! Shared revision metadata for provider and MCP results.
use crate::{ToolError, ToolOutput};
use serde_json::{json, Value};

pub(crate) fn stamp(
    result: Result<ToolOutput, ToolError>,
    revision: Value,
) -> Result<ToolOutput, ToolError> {
    match result {
        Ok(ToolOutput::Json(mut value)) => {
            if !value.is_object() {
                value = json!({"value":value});
            }
            if let Some(original) = value.get("artifact_revision").cloned() {
                // A historical artifact must never grant authority over the current drawing.
                value["revision"] = original;
                value["current_revision"] = revision;
            } else {
                value["revision"] = revision;
            }
            Ok(ToolOutput::Json(value))
        }
        Ok(ToolOutput::Image { png, caption }) => Ok(ToolOutput::Image {
            png,
            caption: format!("{caption}\nrevision: {revision}"),
        }),
        Err(error) => {
            let mut details = error.json();
            details["revision"] = revision;
            Err(ToolError::Structured {
                code: details["code"].as_str().unwrap_or("failed").into(),
                message: details["message"].as_str().unwrap_or_default().into(),
                details,
            })
        }
    }
}

pub(crate) fn check(session: Option<&qiyas_core::Session>, args: &Value) -> Result<(), ToolError> {
    let Some(expected) = args.get("expected_revision").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    match session {
        Some(session) => session.check_revision(expected).map_err(Into::into),
        None => Err(ToolError::Structured {code:"stale_read".into(),
            message:"The drawing from this read is no longer open. Read the current drawing before editing.".into(),
            details:json!({"revision":null,"added":[],"changed":[],"removed":[],"handle_map":{},"handle_maps":[],"history_complete":false})}),
    }
}
