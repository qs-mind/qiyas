//! Argument contracts for every operation, checked by the core's shared schema validator.
use serde_json::{json, Value};

use crate::execute::ToolError;
use crate::surface::{self, CAPTURE, EXECUTE, READ};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
}

fn nullable(schema: &Value) -> Value {
    json!({"anyOf":[schema,{"type":"null"}]})
}

fn command_args(name: &str) -> Value {
    let mut schema = qiyas_core::registry::registry()
        .get(name)
        .expect("registered command")
        .schema
        .clone();
    schema["properties"].as_object_mut().unwrap().remove("type");
    if let Some(required) = schema["required"].as_array_mut() {
        required.retain(|name| name != "type");
    }
    schema
}

/// Contracts also appear in capabilities, so clients can discover the exact accepted form.
pub(super) fn contract(tool: &str, op: &str) -> (Value, Value) {
    let (mut schema, example) = base_contract(tool, op);
    if tool == EXECUTE {
        schema["properties"]["expected_revision"] = qiyas_core::revision_schema();
    }
    fn options(schema: &mut Value, tool: &str) {
        for union in ["anyOf", "oneOf"] {
            if let Some(variants) = schema.get_mut(union).and_then(Value::as_array_mut) {
                for variant in variants {
                    options(variant, tool);
                }
            }
        }
        if schema["properties"].is_object() {
            schema["properties"]["detail"] =
                json!({"type":"string","enum":["summary","full"],"default":"summary"});
            if tool == READ {
                schema["properties"]["fields"] = json!({"type":"array","items":{"type":"string"}});
            }
        }
    }
    if tool == READ || tool == EXECUTE {
        options(&mut schema, tool);
    }
    (schema, example)
}

fn base_contract(tool: &str, op: &str) -> (Value, Value) {
    let string = json!({"type":"string"});
    let number = json!({"type":"number"});
    let boolean = json!({"type":"boolean"});
    let handle = json!({"type":"integer", "minimum":0});
    let handles = json!({"type":"array", "items":handle});
    let point =
        json!({"type":"array", "minItems":2, "maxItems":2, "items":number, "description":"[x, y]"});
    let bounds = json!({"type":"array", "minItems":2, "maxItems":2, "items":point, "description":"[[x1, y1], [x2, y2]]"});
    let points = json!({"type":"array", "minItems":2, "items":point});
    let command = json!({});
    let commands = json!({"type":"array"});
    match (tool, op) {
        (READ, "events") => (
            object(
                json!({"after":{"type":"integer","minimum":0},"session":{"type":"string"}}),
                &[],
            ),
            json!({"after":0}),
        ),
        (READ, "brief_check") => crate::brief_check::contract(),
        (READ, "artifact") => (
            object(
                json!({"id":string,"offset":{"type":"integer","minimum":0},"pointer":{"type":"string","description":"JSON pointer to one part of the evidence, e.g. /outcome/result/items/3; paged like the whole"}}),
                &["id"],
            ),
            json!({"id":"artifact id from a result","pointer":"/items/0"}),
        ),
        (READ, "explain") => (
            object(json!({"item":string,"project":string}), &["item"]),
            json!({"item":"generated:A"}),
        ),
        (READ, "workflow") => (
            object(
                json!({"language":{"type":"string","enum":["en","ar"]}}),
                &[],
            ),
            json!({"language":"en"}),
        ),
        (READ, "digest" | "takeoff_check" | "takeoff_kit") => {
            let mut props = json!({"offset":{"type":"integer","minimum":0}});
            if op == "takeoff_check" {
                props["end_report"] = json!({"type":"string","description":"Draft final answer; exclusions use: Exclude <handle>: <reason>; optional (note <handle>)"});
                props["expectations"] =
                    command_args("verify")["properties"]["expectations"].clone();
            }
            (object(props, &[]), json!({}))
        }
        (READ, "document") => (
            object(
                json!({"path":string,"pages":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"integer","minimum":1}}}),
                &["path"],
            ),
            json!({"path":"drawing.pdf","pages":[1]}),
        ),
        (CAPTURE, "document") => (
            object(
                json!({"path":string,"page":{"type":"integer","minimum":1},"width":{"type":"integer","minimum":64,"maximum":1600}}),
                &["path", "page"],
            ),
            json!({"path":"drawing.pdf","page":1}),
        ),
        (READ, "records") => (
            command_args("records")["properties"]["query"].clone(),
            json!({"collection":"entities","detail":"summary"}),
        ),
        (READ, "check") => (
            command_args("check_drawing")["properties"]["scope"].clone(),
            json!({"rules":["layer_zero"]}),
        ),
        (READ, "rooms") => (
            command_args("rooms")["properties"]["query"].clone(),
            json!({"seeds":[{"point":[1,1]}]}),
        ),
        (READ, "commands") => (
            object(
                json!({"name":nullable(&string),"names":{"type":"array","minItems":1,"maxItems":40,"items":string},"full":{"type":"boolean"},"search":nullable(&string),"category":nullable(&string),"offset":{"type":"integer","minimum":0}}),
                &[],
            ),
            json!({"name":"draw.line"}),
        ),
        (READ, "record_schema") => (
            object(json!({"name":string}), &["name"]),
            json!({"name":"layer"}),
        ),
        (EXECUTE, "run") => (
            object(
                json!({"command":command,"layer":nullable(&string),"color":nullable(&string)}),
                &["command"],
            ),
            json!({"command":{"type":"draw","kind":"line","a":[0,0],"b":[1000,0]},"layer":"A-WALL"}),
        ),
        (READ, "run") => (
            object(json!({"command":command}), &["command"]),
            json!({"command":{"type":"records","query":{"collection":"layers"}}}),
        ),
        (READ, "validate") => (
            json!({"anyOf":[object(json!({"command":command}), &["command"]),object(json!({"commands":commands}), &["commands"])]}),
            json!({"command":{"type":"title_block"}}),
        ),
        (EXECUTE, "batch") => {
            let mut schema = command_args("batch");
            schema["properties"]["commands"] = commands;
            (
                schema,
                json!({"commands":[{"type":"new_layer","name":"A-WALL"}]}),
            )
        }
        (EXECUTE, "set_records" | "create_entities") => {
            let info = qiyas_core::registry::registry()
                .get(op)
                .expect("registered command");
            let mut example = info.examples[0].clone();
            example.as_object_mut().unwrap().remove("type");
            (command_args(op), example)
        }
        (EXECUTE, "finish") => (
            object(json!({"answer":string}), &[]),
            json!({"answer":"Requested work completed; remaining issues are listed."}),
        ),
        (EXECUTE, "plan") => {
            let info = surface::plan_info();
            (info["schema"].clone(), info["examples"][0].clone())
        }
        (EXECUTE, "ask") => {
            let info = surface::ask_info();
            (info["schema"].clone(), info["examples"][0].clone())
        }
        (EXECUTE, "type") => (
            object(json!({"line":string}), &["line"]),
            json!({"line":"PLINE 0,0 10,0 C"}),
        ),
        (EXECUTE, "view") => (
            object(
                json!({"sheet":nullable(&string),"bounds":nullable(&bounds),"handles":nullable(&handles)}),
                &[],
            ),
            json!({"bounds":[[0,0],[10,10]]}),
        ),
        (EXECUTE, "select") => (
            object(json!({"handles":handles,"zoom":boolean}), &["handles"]),
            json!({"handles":[1],"zoom":true}),
        ),
        (EXECUTE, "new_drawing") => (
            object(
                json!({"name":{"type":"string","minLength":1},"units":{"type":"string","enum":["mm","cm","m"]}}),
                &["name", "units"],
            ),
            json!({"name":"Villa G+1 plans","units":"m"}),
        ),
        (EXECUTE, "open_drawing" | "import_boq") => (
            object(json!({"path":string}), &["path"]),
            json!({"path":"drawing.dwg"}),
        ),
        (EXECUTE, "switch_drawing" | "close_drawing") => (
            object(
                json!({"id":{"type":["integer","null"],"minimum":0,"maximum":u32::MAX}}),
                if op == "switch_drawing" { &["id"] } else { &[] },
            ),
            json!({"id":1}),
        ),
        (EXECUTE, "export_takeoff" | "export_conflicts") => (
            object(json!({"path":string}), &[]),
            json!({"path":"takeoff.xlsx"}),
        ),
        (EXECUTE, "export_evidence") => (
            object(
                json!({"path":{"type":["string","null"]},"language":{"type":["string","null"],"enum":["en","ar",null]}}),
                &[],
            ),
            json!({"path":"drawing evidence.pdf","language":"en"}),
        ),
        (EXECUTE, "plot") => (
            object(
                json!({"sheet":{"type":["string","null"],"description":"A sheet's name or block; the current sheet when omitted"},"paper":{"type":["string","null"],"description":"A catalog paper such as ISO A1; a layout's own page setup when omitted"},"orientation":{"type":["string","null"],"enum":["portrait","landscape",null]},"scale":{"type":["string","null"],"description":"fit, or num:den such as 1:50; a layout plots 1:1 and Model fits when omitted"},"path":{"type":["string","null"],"description":"The PDF's name in the drawing's folder; <drawing>-<sheet>.pdf when omitted"}}),
                &[],
            ),
            json!({"sheet":"A-101","scale":"1:1"}),
        ),
        (EXECUTE, "compare") => (
            object(json!({"tolerance":{"type":["number","null"]}}), &[]),
            json!({"tolerance":0.01}),
        ),
        (CAPTURE, "screen") => (object(json!({"marks":boolean}), &[]), json!({"marks":true})),
        (CAPTURE, "picture") => (
            object(
                json!({"sheet":nullable(&string),"bounds":nullable(&bounds),"width":{"type":["integer","null"],"minimum":0,"maximum":u32::MAX},"highlight":handles,"marks":boolean}),
                &[],
            ),
            json!({"bounds":[[0,0],[10,10]],"width":1200}),
        ),
        (READ, "measure") => (
            json!({"anyOf":[object(json!({"handle":handle,"chain":handles}), &["handle"]),object(json!({"points":points,"closed":boolean}), &["points"])]}),
            json!({"handle":1}),
        ),
        (READ, "query") => {
            let variants: Vec<_> = surface::QUERY_KINDS
                .iter()
                .map(|kind| {
                    let mut props = json!({"kind":{"const":kind},"within":nullable(&bounds),"bounds":nullable(&bounds)});
                    let mut required = vec!["kind"];
                    match *kind {
                        "near" => {
                            props["point"] = point.clone();
                            props["sheet"] = nullable(&string);
                            props["offset"] = nullable(&handle);
                            props["radius"] = nullable(&number);
                            props["layer"] = nullable(&string);
                            props["type"] = nullable(&string);
                            props["limit"] = nullable(&handle);
                            required.push("point");
                        }
                        "window" => {
                            props["bounds"] = nullable(&bounds);
                            props["sheet"] = nullable(&string);
                            props["layer"] = nullable(&string);
                            props["type"] = nullable(&string);
                            props["limit"] = nullable(&handle);
                            props["offset"] = nullable(&handle);
                        }
                        "entity" => {
                            props["handle"] = handle.clone();
                            props["chain"] = handles.clone();
                            required.push("handle");
                        }
                        "texts" => {
                            props["layer"] = nullable(&string);
                            props["bounds"] = nullable(&bounds);
                            props["sheet"] = nullable(&string);
                            props["within"] = nullable(&bounds);
                            props["offset"] = handle.clone();
                            props["limit"] = handle.clone();
                        }
                        "tables" | "dimensions" | "blocks" => {
                            props["sheet"] = nullable(&string);
                            props["offset"] = handle.clone();
                            props["limit"] = handle.clone();
                        }
                        "boundary" => {
                            props["point"] = point.clone();
                            required.push("point");
                        }
                        "layers" => {
                            props["offset"] = handle.clone();
                            props["limit"] = handle.clone();
                        }
                        _ => unreachable!("QUERY_KINDS lists every query"),
                    }
                    object(props, &required)
                })
                .collect();
            (
                json!({"oneOf":variants}),
                json!({"kind":"near","point":[0,0],"radius":1}),
            )
        }
        _ => (object(json!({}), &[]), json!({})),
    }
}

fn core_contract(tool: &str, op: &str) -> bool {
    matches!(
        (tool, op),
        (EXECUTE, "batch" | "set_records" | "create_entities")
            | (READ, "records" | "check" | "rooms")
    )
}

pub(super) fn validate(
    tool: &str,
    op: &str,
    args: &Value,
) -> Result<Vec<qiyas_core::validation::InputIssue>, ToolError> {
    let (schema, example) = contract(tool, op);
    let mut envelope = args.clone();
    // Command payloads have their own registry validation, including batch references.
    if matches!(op, "run" | "validate" | "batch") {
        if envelope["command"].is_object() {
            envelope["command"] = json!({});
        }
        if let Some(commands) = envelope.get_mut("commands").and_then(Value::as_array_mut) {
            for command in commands {
                *command = json!({});
            }
        }
    }
    let mut report = qiyas_core::validation::validate_schema(&schema, &envelope);
    // Private runners refuse unknown fields in serde. Preserve that refusal with the shared
    // error shape, while registry-backed operations keep the core's unknown-field warnings.
    if !core_contract(tool, op) {
        for mut issue in std::mem::take(&mut report.warnings) {
            issue.message = issue
                .message
                .replace("; it is ignored", "; use a documented property");
            report.errors.push(issue);
            report.valid = false;
        }
    }
    for issue in report.errors.iter_mut().chain(report.warnings.iter_mut()) {
        issue.pointer = format!("/args{}", issue.pointer);
    }
    if report.valid {
        return Ok(report.warnings);
    }
    let hint = if op == "run" && args.get("commands").is_some() {
        " For several commands use op batch."
    } else {
        ""
    };
    Err(ToolError::Structured {
        code: report.errors[0].code.clone(),
        message: format!(
            "{tool} {op}: {}.{hint} Expected form: {schema}. Example: {example}",
            report.message()
        ),
        details: json!({"errors":report.errors,"warnings":report.warnings,"schema":schema,"example":example}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_has_a_valid_example_and_uses_the_shared_validator() {
        for operation in surface::OPERATIONS {
            let (schema, example) = contract(operation.tool, operation.op);
            assert!(!schema.is_null(), "{}", operation.op);
            assert!(
                validate(operation.tool, operation.op, &example).is_ok(),
                "{}: {example}",
                operation.op
            );
            let error = validate(operation.tool, operation.op, &json!(true))
                .unwrap_err()
                .json();
            assert!(
                error["errors"].as_array().is_some(),
                "{}: {error}",
                operation.op
            );
            assert!(!error["schema"].is_null());
            assert!(!error["example"].is_null());
        }
    }

    #[test]
    fn closed_surface_contracts_report_unknown_arguments_with_the_expected_form() {
        for operation in surface::OPERATIONS {
            let (_, mut example) = contract(operation.tool, operation.op);
            example["misspelled"] = json!(true);
            if core_contract(operation.tool, operation.op) {
                let warnings = validate(operation.tool, operation.op, &example).unwrap();
                assert!(warnings
                    .iter()
                    .any(|issue| issue.code == "unknown_field"
                        && issue.pointer == "/args/misspelled"));
                continue;
            }
            let error = validate(operation.tool, operation.op, &example)
                .unwrap_err()
                .json();
            assert!(
                error["errors"].as_array().unwrap().iter().any(|issue| {
                    issue["code"] == "unknown_field" && issue["pointer"] == "/args/misspelled"
                }),
                "{}: {error}",
                operation.op
            );
            assert!(error["schema"].is_object() && error["example"].is_object());
        }
    }

    #[test]
    fn query_contracts_match_the_private_runners_fields_and_nullable_options() {
        for args in [
            json!({"kind":"near","point":[0,0],"layer":null,"limit":null}),
            json!({"kind":"window","sheet":null,"bounds":null,"offset":null}),
            json!({"kind":"entity","handle":1,"chain":[]}),
            json!({"kind":"texts","sheet":null,"within":null,"offset":0}),
            json!({"kind":"tables","sheet":null,"offset":0}),
            json!({"kind":"tables","within":[[0,0],[1,1]]}),
            json!({"kind":"dimensions","sheet":null,"offset":0}),
            json!({"kind":"blocks","sheet":null,"offset":0}),
            json!({"kind":"boundary","point":[0,0]}),
            json!({"kind":"layers","offset":0,"limit":20}),
            json!({"kind":"texts","limit":20}),
        ] {
            assert!(validate(READ, "query", &args).is_ok(), "{args}");
        }
        for (args, pointer) in [
            (json!({"kind":"texts","contains":"Room"}), "/args/contains"),
            (json!({"kind":"blocks","name":"DOOR"}), "/args/name"),
            (
                json!({"kind":"tables","within_typo":[[0,0],[1,1]]}),
                "/args/within_typo",
            ),
            (json!({"kind":"dimensions","layer":"A-DIMS"}), "/args/layer"),
            (
                json!({"kind":"entity","handle":1,"sheet":"Model"}),
                "/args/sheet",
            ),
        ] {
            let error = validate(READ, "query", &args).unwrap_err().json();
            assert!(
                error["errors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|issue| issue["pointer"] == pointer),
                "{error}"
            );
        }
        assert!(validate(
            CAPTURE,
            "picture",
            &json!({"sheet":null,"bounds":null,"width":null})
        )
        .is_ok());
    }
}
