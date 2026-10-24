//! Advice describes an observed cost or result; the model decides what to do next.
use crate::{ToolError, ToolOutput};
use serde_json::{json, Value};

pub(crate) fn append(value: &mut Value, advice: Value) {
    if !value.is_object() {
        *value = json!({"value":value});
    }
    if !value["advice"].is_array() {
        value["advice"] = json!([]);
    }
    value["advice"].as_array_mut().unwrap().push(advice);
}

pub(crate) fn result(
    result: Result<ToolOutput, ToolError>,
    advice: Value,
) -> Result<ToolOutput, ToolError> {
    match result {
        Ok(ToolOutput::Json(mut value)) => {
            append(&mut value, advice);
            Ok(ToolOutput::Json(value))
        }
        Err(error) => {
            let mut details = error.json();
            append(&mut details, advice);
            Err(ToolError::Structured {
                code: details["code"].as_str().unwrap_or("failed").into(),
                message: details["message"].as_str().unwrap_or_default().into(),
                details,
            })
        }
        other => other,
    }
}

pub(crate) fn repeated(attempts: usize) -> Value {
    json!({"code":"repeated_ineffective_edit","attempts":attempts,
        "message":format!("This identical input has failed or made no change {attempts} times. Read its result and consider a different input. Mizan decides how to proceed.")})
}

pub(crate) fn stopped() -> ToolError {
    ToolError::Structured { code:"repeated_failing_call".into(),
        message:"This identical input has failed five times in this turn. A sixth attempt was stopped to avoid spending more of your subscription on the same failure.".into(),
        details:json!({"failed_attempts":5}) }
}
