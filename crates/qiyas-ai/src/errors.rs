//! Stable error codes at the automation boundary. Human messages may change; codes do not.

use serde_json::{json, Value};

pub const CODES: &[&str] = &[
    "missing_field",
    "unknown_field",
    "invalid_value",
    "no_drawing",
    "needs_window",
    "needs_host",
    "refused",
    "not_found",
    "conflict",
    "stale",
    "stale_read",
    "repeated_failing_call",
    "budget",
    "cancelled",
    "request_id_reused",
    "failed",
];

/// Legacy runners return words; classify them once at the shared boundary.
pub fn code(message: &str) -> &'static str {
    let text = message.to_ascii_lowercase();
    if text.contains("no drawing is open") {
        "no_drawing"
    } else if message == crate::host::NO_WINDOW {
        "needs_window"
    } else if message == crate::host::NO_HOST {
        "needs_host"
    } else if text.contains("cancelled") {
        "cancelled"
    } else if text.contains("drawing changed") || text.contains("changed since") {
        "stale"
    } else if text.contains("budget") || text.contains("limit of") || text.contains("too many") {
        "budget"
    } else if text.contains("missing field") || text.contains("is required") {
        "missing_field"
    } else if text.contains("no command")
        || text.contains("no tool")
        || text.contains("no entity")
        || text.contains("no sheet")
        || text.contains("no record type")
        || text.contains("not found")
    {
        "not_found"
    } else if text.contains("already exists") || text.contains("conflict") {
        "conflict"
    } else if text.contains("refused")
        || text.contains("locked")
        || text.contains("only files beside")
        || text.contains("run by qiyas")
    {
        "refused"
    } else if text.contains("invalid")
        || text.contains("must be")
        || text.contains("bad input")
        || text.contains("has no op")
    {
        "invalid_value"
    } else {
        "failed"
    }
}

pub fn json_error(message: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(message) {
        if value["code"].is_string() && value["message"].is_string() {
            return value;
        }
    }
    json!({"code": code(message), "message": message})
}

pub fn message(text: &str) -> String {
    json_error(text)["message"]
        .as_str()
        .unwrap_or(text)
        .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn contradictory_values_are_not_reported_as_missing_fields() {
        let error = crate::execute::ToolError::BadInput(
            "An opening needs 0 <= sill < head <= wall height, in metres".into(),
        );
        assert_eq!(error.json()["code"], "invalid_value");
        assert_eq!(super::code("missing field `points`"), "missing_field");
        assert_eq!(super::code("No drawing is open"), "no_drawing");
    }
}
