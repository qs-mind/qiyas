//! Unambiguous wire shortcuts, with an explanation of every accepted change.
use crate::{
    surface::{EXECUTE, READ},
    ToolError, ToolOutput,
};
use qiyas_core::validation::InputIssue;
use serde_json::{json, Value};
fn warning(pointer: &str, message: &str) -> InputIssue {
    InputIssue {
        code: "normalised_input".into(),
        pointer: pointer.into(),
        message: message.into(),
    }
}
pub(crate) fn envelope(tool: &str, input: &Value) -> Result<(Value, Vec<InputIssue>), ToolError> {
    let mut v = input.clone();
    let mut warnings = vec![];
    if tool == EXECUTE && v.get("op").is_none() {
        let mut args = crate::surface::args_of(&v).map_err(ToolError::BadInput)?;
        if let Some(command) = v.get("command").cloned() {
            if args.get("command").is_some() || args.get("commands").is_some() {
                return Err(ToolError::BadInput(
                    "Supply one command, either at the top or inside args.".into(),
                ));
            }
            args["command"] = command;
            v.as_object_mut().unwrap().remove("command");
        }
        if args.get("command").is_some() && args.get("commands").is_none() {
            v["op"] = json!("run");
            v["args"] = args;
            warnings.push(warning(
                "/op",
                "Normalised omitted op to run because one command was supplied.",
            ));
        }
    }
    Ok((v, warnings))
}
pub(crate) fn args(
    tool: &str,
    op: &str,
    input: &Value,
) -> Result<(Value, Vec<InputIssue>), ToolError> {
    let mut v = input.clone();
    let mut warnings = vec![];
    if tool == READ && op == "records" && v["where"].is_object() {
        v["where"] = json!([v["where"]]);
        warnings.push(warning(
            "/args/where",
            "Normalised one where condition to an array.",
        ));
    }
    if tool == READ
        && ((op == "query" && v["kind"] == "entity") || op == "measure")
        && v.get("handles").is_some()
    {
        if v.get("handle").is_some() || v["handles"].as_array().is_none_or(|a| a.len() != 1) {
            return Err(ToolError::BadInput(
                "Choose one handle; do not supply both handle and handles.".into(),
            ));
        }
        v["handle"] = v["handles"][0].clone();
        v.as_object_mut().unwrap().remove("handles");
        warnings.push(warning(
            "/args/handle",
            "Normalised the single handles entry to handle.",
        ));
    }
    if tool == READ
        && op == "query"
        && v["kind"] == "window"
        && (v.get("corner1").is_some() || v.get("corner2").is_some())
    {
        if v.get("bounds").is_some() || v.get("corner1").is_none() || v.get("corner2").is_none() {
            return Err(ToolError::BadInput(
                "Supply bounds or both corner1 and corner2, not both forms.".into(),
            ));
        }
        v["bounds"] = json!([v["corner1"], v["corner2"]]);
        v.as_object_mut().unwrap().remove("corner1");
        v.as_object_mut().unwrap().remove("corner2");
        warnings.push(warning(
            "/args/bounds",
            "Normalised corner1 and corner2 to bounds.",
        ));
    }
    if matches!(op, "run" | "batch") {
        let raw = if op == "run" {
            match &v["command"] {
                Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
                c => c.clone(),
            }
        } else {
            let mut c = v.clone();
            c["type"] = json!("batch");
            c
        };
        warnings.extend(
            qiyas_core::registry::normalization_warnings(&raw)
                .into_iter()
                .map(|mut w| {
                    w.pointer = format!(
                        "/args{}{}",
                        if op == "run" { "/command" } else { "" },
                        w.pointer
                    );
                    w
                }),
        );
    }
    Ok((v, warnings))
}
pub(crate) fn append(value: &mut Value, warnings: &[InputIssue]) {
    if warnings.is_empty() {
        return;
    }
    if !value["warnings"].is_array() {
        value["warnings"] = json!([]);
    }
    let list = value["warnings"].as_array_mut().unwrap();
    for w in warnings {
        let w = json!(w);
        if !list.contains(&w) {
            list.push(w);
        }
    }
}
pub(crate) fn output(mut result: ToolOutput, warnings: &[InputIssue]) -> ToolOutput {
    if let ToolOutput::Json(v) = &mut result {
        append(v, warnings);
    }
    result
}
