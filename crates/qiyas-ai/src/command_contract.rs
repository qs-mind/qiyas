//! Compact field signatures generated from registry schemas.
use serde_json::{json, Value};

/// Contracts have their own inline budget; ordinary evidence keeps its smaller summary budget.
pub(crate) const INLINE_BYTES: usize = 32_000;

pub(crate) fn is_compact(value: &Value) -> bool {
    value["signature"].is_string()
        || ["commands", "operations"].iter().any(|key| {
            value[*key]
                .as_array()
                .is_some_and(|items| !items.is_empty() && items.iter().all(is_compact))
        })
}

pub(crate) fn compact(info: &qiyas_core::registry::CommandInfo) -> Value {
    json!({
        "name": info.name,
        "summary": info.summary.split_whitespace().collect::<Vec<_>>().join(" "),
        "signature": signature(&info.name, &info.schema),
        "example": info.examples.first(),
        "words": info.words,
        "effect": info.effect,
        "undo": info.undo,
        "destructive": info.destructive,
        "batchable": info.batchable,
        "needs": info.needs,
    })
}

pub(crate) fn signature(name: &str, schema: &Value) -> String {
    format!("{name} {}", shape(schema, schema, "", &mut Vec::new()))
}

fn shape(schema: &Value, root: &Value, path: &str, refs: &mut Vec<String>) -> String {
    if let Some(reference) = schema["$ref"].as_str() {
        if !refs.iter().any(|r| r == reference) {
            if let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p)) {
                refs.push(reference.to_owned());
                let result = shape(target, root, path, refs);
                refs.pop();
                return result;
            }
        }
        return reference.to_owned();
    }
    if let Some(value) = schema.get("const") {
        return value.to_string();
    }
    if let Some(values) = schema["enum"].as_array() {
        return values
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
    }
    let mut parts = Vec::new();
    if let Some(properties) = schema["properties"].as_object() {
        let fields = properties
            .iter()
            .map(|(name, field)| {
                let required = schema["required"].as_array().is_some_and(|required| {
                    required.iter().any(|v| v.as_str() == Some(name.as_str()))
                });
                let field_path = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                let default = field
                    .get("default")
                    .or_else(|| {
                        field["$ref"]
                            .as_str()
                            .and_then(|r| r.strip_prefix('#'))
                            .and_then(|p| root.pointer(p))
                            .and_then(|s| s.get("default"))
                    })
                    .map(|v| format!(" = {v}"))
                    .unwrap_or_default();
                format!(
                    "{field_path}{}: {}{default}",
                    if required { "" } else { "?" },
                    shape(field, root, &field_path, refs)
                )
            })
            .collect::<Vec<_>>();
        parts.push(format!("{{{}}}", fields.join("; ")));
    }
    for (key, separator) in [("oneOf", " | "), ("anyOf", " | "), ("allOf", " & ")] {
        if let Some(variants) = schema[key].as_array() {
            parts.push(
                variants
                    .iter()
                    .map(|v| shape(v, root, path, refs))
                    .collect::<Vec<_>>()
                    .join(separator),
            );
        }
    }
    if !parts.is_empty() {
        return parts.join(" & ");
    }
    if let Some(types) = schema["type"].as_array() {
        return types
            .iter()
            .map(|kind| {
                let mut branch = schema.clone();
                branch["type"] = kind.clone();
                shape(&branch, root, path, refs)
            })
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match schema["type"].as_str() {
        Some("array") => {
            let item_path = format!("{path}[]");
            if let Some(tuple) = schema["prefixItems"]
                .as_array()
                .or_else(|| schema["items"].as_array())
            {
                return format!(
                    "[{}]",
                    tuple
                        .iter()
                        .map(|v| shape(v, root, &item_path, refs))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let item = shape(&schema["items"], root, &item_path, refs);
            let item = if item.contains(" | ") && !item.starts_with('{') {
                format!("({item})")
            } else {
                item
            };
            match (schema["minItems"].as_u64(), schema["maxItems"].as_u64()) {
                (Some(min), Some(max)) if min == max => format!("{item}[{min}]"),
                _ => format!("{item}[]"),
            }
        }
        Some("object") if schema["additionalProperties"].is_object() => {
            format!(
                "{{{path}.*: {}}}",
                shape(&schema["additionalProperties"], root, path, refs)
            )
        }
        Some(kind) => kind.to_owned(),
        None if schema == &Value::Bool(false) => "never".into(),
        None => "any".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(args: &Value) -> crate::ToolOutput {
        crate::surface::run_without_drawing(crate::surface::READ, "commands", args, None)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn eight_complete_contracts_fit_inline_and_are_never_replaced_by_artifacts() {
        let args = json!({"names":["draw","batch","table","make_sheet","text",
            "dimension","label_spaces","title_block"]});
        let mut known = std::collections::BTreeMap::new();
        for _ in 0..2 {
            let output =
                crate::result::format(crate::surface::READ, "commands", &args, read(&args))
                    .unwrap();
            let crate::ToolOutput::Json(mut value) = output else {
                panic!("JSON contracts")
            };
            crate::result::deduplicate(&mut value, &mut known);
            assert!(
                value.to_string().len() <= INLINE_BYTES,
                "{} bytes",
                value.to_string().len()
            );
            assert!(value.get("artifact").is_none());
            let contracts = value["commands"].as_array().unwrap();
            assert_eq!(contracts.len(), 8);
            for contract in contracts {
                assert!(contract["signature"].is_string(), "{contract}");
                assert!(contract["example"].is_object(), "{contract}");
                assert!(contract.get("examples").is_none());
                assert!(contract.get("schema").is_none());
                assert!(contract.get("artifact").is_none());
                assert!(contract.get("effect").is_some());
                assert!(contract.get("undo").is_some());
            }
        }
    }

    #[test]
    fn make_sheet_required_and_optional_fields_and_full_schema_are_discoverable() {
        let crate::ToolOutput::Json(compact) = read(&json!({"name":"make_sheet"})) else {
            panic!()
        };
        let signature = compact["signature"].as_str().unwrap();
        assert!(signature.contains("name: string"), "{signature}");
        assert!(!signature.contains("name?:"), "{signature}");
        assert!(signature.contains("paper?:"), "{signature}");
        let crate::ToolOutput::Json(full) = read(&json!({"name":"make_sheet","full":true})) else {
            panic!()
        };
        assert_eq!(
            full["schema"],
            qiyas_core::registry::registry()
                .get("make_sheet")
                .unwrap()
                .schema
        );
        let crate::ToolOutput::Json(full_many) = read(&json!({"names":["make_sheet"],"full":true}))
        else {
            panic!()
        };
        assert_eq!(full_many["commands"][0]["schema"], full["schema"]);
        let args = json!({"name":"make_sheet","full":true});
        let crate::ToolOutput::Json(wire) =
            crate::result::format(crate::surface::READ, "commands", &args, read(&args)).unwrap()
        else {
            panic!()
        };
        assert_eq!(wire["schema"], full["schema"]);
    }

    #[test]
    fn unknown_command_still_offers_close_names() {
        let error = crate::surface::run_without_drawing(
            crate::surface::READ,
            "commands",
            &json!({"name":"make_shee"}),
            None,
        )
        .unwrap()
        .unwrap_err()
        .to_string();
        assert!(error.contains("close ones:"), "{error}");
        assert!(error.contains("make_sheet"), "{error}");
    }

    #[test]
    fn signatures_resolve_definitions_and_keep_unions_defaults_and_nested_paths() {
        let schema = json!({"type":"object","required":["items"],"properties":{
            "items":{"type":"array","items":{"$ref":"#/$defs/entry"}},
            "mode":{"anyOf":[{"enum":["a","b"]},{"type":"null"}],"default":"a"}
        },"$defs":{"entry":{"type":"object","required":["value"],"properties":{
            "value":{"oneOf":[{"type":"string"},{"type":"number"}]},
            "size":{"$ref":"#/definitions/size"}
        }}},"definitions":{"size":{"type":"integer","default":3}}});
        let text = signature("sample", &schema);
        assert!(text.contains("items[].size?: integer = 3"), "{text}");
        assert!(text.contains("items[].value: string | number"), "{text}");
        assert!(
            text.contains("mode?: \"a\" | \"b\" | null = \"a\""),
            "{text}"
        );
        assert!(!text.contains("$ref"));
    }
}
