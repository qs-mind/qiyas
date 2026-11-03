//! TypeScript wire inputs from the registry's self-contained JSON schemas.

use serde_json::{Map, Value};

use crate::registry::registry;

/// Generate the committed window contract without timestamps or machine-specific paths.
/// Unsupported structural schema keywords fail instead of silently weakening the types.
pub fn typescript() -> Result<String, String> {
    let mut commands: Vec<_> = registry().commands().iter().collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = String::from(
        "// Generated from qiyas-core's command registry. Do not edit.\n\
         // Refresh: cargo run -p qiyas-cli -- gen-ts\n\n\
         export type CommandMap = {\n",
    );
    for command in &commands {
        let ty = if command.kinds.is_empty() {
            schema_type(&command.schema).map_err(|e| format!("{}: {e}", command.name))?
        } else {
            let mut kinds = command.kinds.clone();
            kinds.sort();
            kinds
                .iter()
                .map(|kind| format!("CommandMap[{}]", quote(&format!("{}.{kind}", command.name))))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        out.push_str(&format!("  {}: {ty};\n", quote(&command.name)));
    }
    out.push_str("};\n\nexport type CommandName = keyof CommandMap;\n");
    let top = commands
        .iter()
        .filter(|command| !command.name.contains('.'))
        .map(|command| quote(&command.name))
        .collect::<Vec<_>>()
        .join(" | ");
    out.push_str(&format!("export type Command = CommandMap[{top}];\n\n"));
    out.push_str(
        "export type CommandWord = { word: string; command: string; summary: string };\n\
         export const COMMAND_WORDS: readonly CommandWord[] = [\n",
    );
    for word in registry().words() {
        out.push_str(&format!(
            "  {{ word: {}, command: {}, summary: {} }},\n",
            quote(&word.word),
            quote(&word.command),
            quote(&word.summary)
        ));
    }
    out.push_str("];\n");
    Ok(out)
}

fn quote(value: &str) -> String {
    Value::String(value.into()).to_string()
}

fn joined(parts: Vec<String>, separator: &str, empty: &str) -> String {
    let mut unique = Vec::new();
    for part in parts {
        if !unique.contains(&part) {
            unique.push(part);
        }
    }
    match unique.len() {
        0 => empty.into(),
        1 => unique.remove(0),
        _ => format!("({})", unique.join(separator)),
    }
}

fn literal(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(values) => {
            let mut fields: Vec<_> = values.iter().collect();
            fields.sort_by_key(|(name, _)| *name);
            format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|(name, value)| format!("{}: {};", quote(name), literal(value)))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        _ => value.to_string(),
    }
}

fn schema_type(schema: &Value) -> Result<String, String> {
    if let Some(value) = schema.as_bool() {
        return Ok(if value { "unknown" } else { "never" }.into());
    }
    let map = schema
        .as_object()
        .ok_or("expected a schema object or boolean")?;
    for keyword in [
        "$ref",
        "$dynamicRef",
        "not",
        "if",
        "then",
        "else",
        "patternProperties",
        "dependentSchemas",
        "dependentRequired",
        "unevaluatedProperties",
    ] {
        if map.contains_key(keyword) {
            return Err(format!("unsupported schema keyword {keyword}"));
        }
    }
    if let Some(value) = map.get("const") {
        return Ok(literal(value));
    }
    if let Some(values) = map.get("enum").and_then(Value::as_array) {
        return Ok(joined(values.iter().map(literal).collect(), " | ", "never"));
    }
    // Required-only branches inherit their property types from the containing object.
    for keyword in ["oneOf", "anyOf"] {
        if let Some(branches) = map.get(keyword).and_then(Value::as_array) {
            let mut base = map.clone();
            base.remove(keyword);
            let mut parts = Vec::new();
            for branch in branches {
                let required_only = branch
                    .as_object()
                    .is_some_and(|b| b.keys().all(|k| k == "required"));
                if required_only {
                    let mut merged = base.clone();
                    let mut required = merged
                        .get("required")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    required.extend(branch["required"].as_array().into_iter().flatten().cloned());
                    merged.insert("required".into(), Value::Array(required));
                    parts.push(schema_type(&Value::Object(merged))?);
                } else {
                    let base_type = schema_type(&Value::Object(base.clone()))?;
                    let branch_type = schema_type(branch)?;
                    parts.push(if base_type == "unknown" {
                        branch_type
                    } else {
                        joined(vec![base_type, branch_type], " & ", "unknown")
                    });
                }
            }
            return Ok(joined(parts, " | ", "never"));
        }
    }
    if let Some(branches) = map.get("allOf").and_then(Value::as_array) {
        let mut base = map.clone();
        base.remove("allOf");
        let mut parts = vec![schema_type(&Value::Object(base))?];
        parts.extend(
            branches
                .iter()
                .map(schema_type)
                .collect::<Result<Vec<_>, _>>()?,
        );
        parts.retain(|part| part != "unknown");
        return Ok(joined(parts, " & ", "unknown"));
    }
    if let Some(types) = map.get("type").and_then(Value::as_array) {
        let parts = types
            .iter()
            .map(|ty| {
                let mut branch = map.clone();
                branch.insert("type".into(), ty.clone());
                schema_type(&Value::Object(branch))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(joined(parts, " | ", "never"));
    }
    match map.get("type").and_then(Value::as_str) {
        Some("null") => Ok("null".into()),
        Some("boolean") => Ok("boolean".into()),
        Some("integer" | "number") => Ok("number".into()),
        Some("string") => Ok("string".into()),
        Some("array") => array_type(map),
        Some("object") => object_type(map),
        None if map.contains_key("properties") || map.contains_key("required") => object_type(map),
        None => Ok("unknown".into()),
        Some(other) => Err(format!("unsupported schema type {other}")),
    }
}

fn object_type(map: &Map<String, Value>) -> Result<String, String> {
    let mut properties: Vec<_> = map
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .collect();
    properties.sort_by_key(|(name, _)| *name);
    let required = map.get("required").and_then(Value::as_array);
    let mut fields = Vec::new();
    for (name, schema) in properties {
        let optional =
            if required.is_some_and(|items| items.iter().any(|v| v.as_str() == Some(name))) {
                ""
            } else {
                "?"
            };
        fields.push(format!(
            "{}{optional}: {};",
            quote(name),
            schema_type(schema)?
        ));
    }
    if let Some(required) = required {
        for name in required.iter().filter_map(Value::as_str) {
            if map.get("properties").and_then(|p| p.get(name)).is_none() {
                fields.push(format!("{}: unknown;", quote(name)));
            }
        }
    }
    if let Some(additional) = map
        .get("additionalProperties")
        .filter(|v| **v != Value::Bool(false))
    {
        fields.push(format!("[key: string]: {};", schema_type(additional)?));
    }
    if fields.is_empty() {
        return Ok(
            if map.get("additionalProperties") == Some(&Value::Bool(false)) {
                "Record<string, never>"
            } else {
                "Record<string, unknown>"
            }
            .into(),
        );
    }
    Ok(format!("{{ {} }}", fields.join(" ")))
}

fn array_type(map: &Map<String, Value>) -> Result<String, String> {
    if let Some(items) = map
        .get("prefixItems")
        .or_else(|| map.get("items").filter(|v| v.is_array()))
        .and_then(Value::as_array)
    {
        let min = map.get("minItems").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut parts = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let ty = schema_type(item)?;
                Ok(if i < min { ty } else { format!("({ty})?") })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let rest = if map.contains_key("prefixItems") {
            map.get("items")
        } else {
            map.get("additionalItems")
        };
        let closed = rest == Some(&Value::Bool(false))
            || map
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|max| max as usize == items.len());
        if !closed {
            parts.push(format!(
                "...Array<{}>",
                schema_type(rest.unwrap_or(&Value::Bool(true)))?
            ));
        }
        return Ok(format!("[{}]", parts.join(", ")));
    }
    let item = schema_type(map.get("items").unwrap_or(&Value::Bool(true)))?;
    let min = map.get("minItems").and_then(Value::as_u64).unwrap_or(0);
    if map.get("maxItems").and_then(Value::as_u64) == Some(min) {
        // Schemars uses homogeneous items for Rust arrays, prefixItems for tuples.
        let count = usize::try_from(min).map_err(|_| "array length exceeds usize")?;
        return Ok(format!("[{}]", vec![item; count].join(", ")));
    }
    Ok(format!("Array<{item}>"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn committed_types_match_the_registry() {
        let generated = typescript().expect("registry schemas must generate TypeScript");
        assert_eq!(
            generated,
            typescript().unwrap(),
            "generation must be deterministic"
        );
        let committed = include_str!("../../../src/generated/commands.ts");
        let line = generated
            .lines()
            .zip(committed.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| generated.lines().count().min(committed.lines().count()))
            + 1;
        assert!(
            generated == committed,
            "Command types differ at line {line}. Refresh with: cargo run -p qiyas-cli -- gen-ts"
        );
    }

    #[test]
    fn schema_shapes_keep_required_nullable_tuples_maps_and_unions() {
        let schema = json!({"type":"object","required":["type","point","data"],"properties":{
            "type":{"const":"draw"},
            "kind":{"enum":["line","point"]},
            "point":{"type":"array","prefixItems":[{"type":"number"},{"type":"number"}],"minItems":2,"maxItems":2},
            "data":{"type":"object","additionalProperties":{"type":["string","null"]}},
            "value":{"anyOf":[{"type":"boolean"},{"type":"integer"}]}
        }});
        assert_eq!(schema_type(&schema).unwrap(), "{ \"data\": { [key: string]: (string | null); }; \"kind\"?: (\"line\" | \"point\"); \"point\": [number, number]; \"type\": \"draw\"; \"value\"?: (boolean | number); }");
        assert_eq!(schema_type(&json!({"allOf":[{"type":"object","properties":{"type":{"const":"draw"}},"required":["type"]},{"type":"object","properties":{"kind":{"const":"line"}},"required":["kind"]}]})).unwrap(), "({ \"type\": \"draw\"; } & { \"kind\": \"line\"; })");
        assert_eq!(schema_type(&json!(true)).unwrap(), "unknown");
        assert_eq!(schema_type(&json!(false)).unwrap(), "never");
        assert!(schema_type(&json!({"$ref":"#/$defs/Future"})).is_err());
    }

    #[test]
    fn required_branches_preserve_property_types() {
        let schema = json!({"type":"object","properties":{"a":{"type":"string"},"b":{"type":"number"}},"anyOf":[{"required":["a"]},{"required":["b"]}]});
        assert_eq!(
            schema_type(&schema).unwrap(),
            "({ \"a\": string; \"b\"?: number; } | { \"a\"?: string; \"b\": number; })"
        );
    }

    #[test]
    fn rust_coordinate_arrays_and_tuples_keep_their_length() {
        let array = schemars::schema_for!([[f64; 2]; 2]).to_value();
        assert_eq!(
            schema_type(&array).unwrap(),
            "[[number, number], [number, number]]"
        );
        let tuple = schemars::schema_for!((String, bool)).to_value();
        assert_eq!(schema_type(&tuple).unwrap(), "[string, boolean]");
        let empty = schemars::schema_for!([u8; 0]).to_value();
        assert_eq!(schema_type(&empty).unwrap(), "[]");
    }
}
