//! Outputs and errors shared by the generic tools.

use qiyas_core::CoreError;
use serde_json::{json, Value};

/// What a tool returns: JSON for the model, or a picture of the drawing.
#[derive(Debug, Clone)]
pub enum ToolOutput {
    Json(Value),
    Image { png: Vec<u8>, caption: String },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("{message}")]
    Structured {
        code: String,
        message: String,
        details: Value,
    },
    #[error("No tool named {0}")]
    UnknownTool(String),
    /// The input is wrong: bad JSON shape, a missing entity, sheet, block or measurement.
    #[error("{0}")]
    BadInput(String),
    /// The tool is in the catalogue but its backing is not built yet.
    #[error("{0}")]
    NotAvailable(String),
    #[error("{0}")]
    Failed(String),
}

impl ToolError {
    pub fn json(&self) -> Value {
        match self {
            Self::Structured {
                code,
                message,
                details,
            } => {
                let mut out = details.clone();
                if !out.is_object() {
                    out = json!({});
                }
                out["code"] = json!(code);
                out["message"] = json!(message);
                out
            }
            other => {
                let mut out = crate::errors::json_error(&self.to_string());
                if out["code"] == "failed" {
                    out["code"] = json!(match other {
                        Self::UnknownTool(_) => "not_found",
                        Self::BadInput(_) => "invalid_value",
                        Self::NotAvailable(_) => "refused",
                        _ => "failed",
                    });
                }
                out
            }
        }
    }
}

impl From<CoreError> for ToolError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::StaleRead { details } => ToolError::Structured { code: "stale_read".into(), message: "The drawing changed since this read. No edit was applied. Read the changed objects and use their current revision.".into(), details },
            CoreError::InputWarnings { error, warnings } => {
                let underlying = ToolError::from(*error);
                let mut details = underlying.json();
                details["warnings"] = json!(warnings);
                ToolError::Structured {
                    code: details["code"].as_str().unwrap_or("failed").into(),
                    message: details["message"].as_str().unwrap_or_default().into(),
                    details,
                }
            }
            CoreError::Step {
                index,
                label,
                error,
            } => {
                let mut details = ToolError::from(*error).json();
                for field in ["errors", "warnings"] {
                    if let Some(issues) = details.get_mut(field).and_then(Value::as_array_mut) {
                        for issue in issues {
                            if let Some(pointer) = issue["pointer"].as_str() {
                                issue["pointer"] = json!(format!("/commands/{index}{pointer}"));
                            }
                        }
                    }
                }
                let message = format!(
                    "Batch step {index} ({label}): {}",
                    details["message"].as_str().unwrap_or_default()
                );
                details["step"] = json!(index);
                details["command"] = json!(label);
                ToolError::Structured {
                    code: details["code"].as_str().unwrap_or("failed").into(),
                    message,
                    details,
                }
            }
            // Handles are integers in the tools' JSON, not hex as in the core's message.
            CoreError::NoSuchEntity(h) => ToolError::BadInput(format!("No entity with handle {h}")),
            CoreError::NoSuchSheet(_) | CoreError::InvalidCommand(_) => {
                ToolError::BadInput(e.to_string())
            }
            other => ToolError::Failed(other.to_string()),
        }
    }
}
