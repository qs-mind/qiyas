//! Natural input forms resolved before schema checks and serde.
use crate::validation::DefaultUsed;
use serde_json::{json, Value};

fn alias(value: &mut Value, from: &str, to: &str) {
    if value.get(to).is_none() {
        if let Some(v) = value.as_object_mut().and_then(|m| m.remove(from)) {
            value[to] = v;
        }
    }
}

pub(crate) fn records(query: &mut Value) {
    if query["where"].is_object() {
        query["where"] = json!([query["where"]]);
    }
    if let Some(conditions) = query.get_mut("where").and_then(Value::as_array_mut) {
        for condition in conditions {
            alias(condition, "field", "path");
            if condition.get("path").is_none() {
                if let Some(map) = condition.as_object_mut() {
                    let keys: Vec<_> = map
                        .keys()
                        .filter(|k| !matches!(k.as_str(), "op" | "value"))
                        .cloned()
                        .collect();
                    if keys.len() == 1 && !map.contains_key("value") {
                        let key = &keys[0];
                        let value = map.remove(key).unwrap();
                        map.insert("path".into(), json!(key));
                        map.insert("value".into(), value);
                    }
                }
            }
            if condition["path"] == "block" {
                condition["path"] = json!("block_name");
            }
        }
    }
    if let Some(fields) = query.get_mut("fields").and_then(Value::as_array_mut) {
        for field in fields {
            if field == "block" {
                *field = json!("block_name");
            }
        }
    }
}

pub(crate) fn normalize(value: &mut Value) {
    if matches!(value["type"].as_str(), Some("set" | "list")) {
        value["kind"] = value["type"].clone();
        value["type"] = json!("decision");
    }
    let name = value["type"].as_str().unwrap_or_default().to_owned();
    if name == "decision" || name.starts_with("decision.") {
        alias(value, "action", "kind");
    }
    match name.as_str() {
        "records" => {
            if let Some(query) = value.get_mut("query") {
                records(query);
            }
        }
        "define_block" | "create_entities" => {
            if let Some(entities) = value.get_mut("entities").and_then(Value::as_array_mut) {
                for entity in entities {
                    if entity["type"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("LwPolyline"))
                    {
                        if let Some(vertices) = entity
                            .get_mut("data")
                            .and_then(|data| data.get_mut("vertices"))
                            .and_then(Value::as_array_mut)
                        {
                            for vertex in vertices {
                                if let Some(map) = vertex.as_object_mut() {
                                    for field in ["bulge", "start_width", "end_width", "vertex_id"]
                                    {
                                        map.entry(field).or_insert_with(|| {
                                            if field == "vertex_id" {
                                                json!(0)
                                            } else {
                                                json!(0.0)
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "measurement" if value["kind"] == "set_method" => {
            if value.get("scope").is_none() {
                value["scope"] = json!("project");
            }
            if value["scope"] == "drawing" {
                value["scope"] = json!("project");
            }
            if let Some(method) = value.get_mut("method").and_then(Value::as_object_mut) {
                let name = method.get("name").cloned().unwrap_or(Value::Null);
                method.entry("title").or_insert(name);
                method.entry("based_on").or_insert(json!("none"));
                method.entry("source_note").or_insert(json!(""));
                fn labels(v: &mut Value) {
                    if let Some(map) = v.as_object_mut() {
                        if let Some(factors) = map.get("factors").and_then(Value::as_array) {
                            let expression = factors
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" * ");
                            map.entry("label").or_insert(json!(expression));
                        }
                        for child in map.values_mut() {
                            labels(child);
                        }
                    } else if let Some(items) = v.as_array_mut() {
                        for item in items {
                            labels(item);
                        }
                    }
                }
                for child in method.values_mut() {
                    labels(child);
                }
            }
        }
        "table" => {
            if value["rows"].is_array() && value.get("data").is_none() {
                let rows = value.as_object_mut().unwrap().remove("rows").unwrap();
                value["data"] = rows;
            }
            if value["columns"].is_array() {
                let columns = value.as_object_mut().unwrap().remove("columns").unwrap();
                if value.get("data").is_none() {
                    value["data"] = json!([]);
                }
                if let Some(data) = value["data"].as_array_mut() {
                    data.insert(0, columns);
                }
            }
            if let Some(data) = value["data"].as_array() {
                let rows = data.len();
                let cols = data
                    .iter()
                    .filter_map(Value::as_array)
                    .map(Vec::len)
                    .max()
                    .unwrap_or(0);
                if value.get("rows").is_none() {
                    value["rows"] = json!(rows);
                }
                if value.get("cols").is_none() {
                    value["cols"] = json!(cols);
                }
            }
        }
        "dim_edit" if value.get("op").is_none() => {
            let choices: Vec<_> = ["line_point", "move_text", "text_override", "flip"]
                .into_iter()
                .filter(|key| value.get(*key).is_some())
                .collect();
            if choices.len() == 1 {
                let key = choices[0];
                let v = value[key].clone();
                if key != "flip" || v == true {
                    value.as_object_mut().unwrap().remove(key);
                    value["op"] = if key == "flip" {
                        json!("flip")
                    } else {
                        json!({key:v})
                    };
                }
            }
        }
        _ => {}
    }
}

/// Only inferred values, not renamed fields, are defaults.
pub(crate) fn defaults(raw: &Value, canonical: &Value, prefix: &str) -> Vec<DefaultUsed> {
    fn visit(a: &Value, b: &Value, path: &str, out: &mut Vec<DefaultUsed>) {
        if let Some(map) = b.as_object() {
            for (key, value) in map {
                let next = format!("{path}/{key}");
                if a.get(key).is_none()
                    && matches!(
                        key.as_str(),
                        "bulge"
                            | "start_width"
                            | "end_width"
                            | "vertex_id"
                            | "scope"
                            | "title"
                            | "based_on"
                            | "source_note"
                            | "label"
                            | "rows"
                            | "cols"
                            | "op"
                    )
                    || (key == "rows" && a[key].is_array() && value.is_number())
                {
                    out.push(DefaultUsed {
                        field: next,
                        value: value.clone(),
                        unit: None,
                    });
                } else {
                    visit(&a[key], value, &next, out);
                }
            }
        } else if let Some(items) = b.as_array() {
            for (i, item) in items.iter().enumerate() {
                visit(&a[i], item, &format!("{path}/{i}"), out);
            }
        }
    }
    let mut out = Vec::new();
    visit(raw, canonical, prefix, &mut out);
    out
}

pub(crate) fn schema(name: &str, schema: &mut Value) {
    fn optional(schema: &mut Value, fields: &[&str]) {
        if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
            required.retain(|key| !fields.iter().any(|field| key.as_str() == Some(*field)));
        }
    }
    match name {
        "decision.set" | "decision.list" => {
            optional(schema, &["kind"]);
            schema["properties"]["action"] = schema["properties"]["kind"].clone();
            schema["description"] = json!(format!(
                "{} Accepts action as kind; a bare type of set/list also names decision.set/list.",
                schema["description"].as_str().unwrap_or_default()
            ));
        }
        "table" => {
            optional(schema, &["rows", "cols"]);
            schema["properties"]["rows"] = json!({"anyOf":[{"type":"integer","minimum":0},{"type":"array","items":{"type":"array","items":{"type":"string"}}}],"description":"Row count or row data. Data infers rows and cols; columns supplies the first header row."});
            schema["properties"]["columns"] = json!({"type":"array","items":{"type":"string"},"description":"Header labels prepended to row data"});
        }
        "dim_edit" => {
            optional(schema, &["op"]);
            for field in ["line_point", "move_text"] {
                schema["properties"][field] =
                    json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2});
            }
            schema["properties"]["text_override"] = json!({"type":"string"});
            schema["properties"]["flip"] = json!({"type":"boolean"});
        }
        "records" => {
            let condition = &mut schema["properties"]["query"]["properties"]["where"]["items"];
            optional(condition, &["path"]);
            condition["properties"]["field"] =
                json!({"type":"string","description":"Alias for path"});
            for field in [
                "block",
                "block_name",
                "layer",
                "type",
                "name",
                "text",
                "layout",
                "area",
                "length",
            ] {
                condition["properties"][field] =
                    json!({"description":"A single summary field can replace path/value"});
            }
        }
        "measurement.set_method" => {
            fn dynamic_defaults(schema: &mut Value) {
                if let Some(properties) =
                    schema.get_mut("properties").and_then(Value::as_object_mut)
                {
                    for (field, source) in [("title", "method name"), ("label", "term expression")]
                    {
                        if let Some(property) =
                            properties.get_mut(field).and_then(Value::as_object_mut)
                        {
                            property.remove("default");
                            property.insert(
                                "description".into(),
                                json!(format!("Omitted: use the {source}")),
                            );
                        }
                    }
                }
                match schema {
                    Value::Object(map) => {
                        for child in map.values_mut() {
                            dynamic_defaults(child);
                        }
                    }
                    Value::Array(items) => {
                        for child in items {
                            dynamic_defaults(child);
                        }
                    }
                    _ => {}
                }
            }
            dynamic_defaults(schema);
            schema["properties"]["scope"]["enum"] = json!(["settings", "project", "drawing"]);
            schema["properties"]["method"]["description"] = json!("Omitted title uses name; based_on defaults to none, source_note to empty, and term labels to their expressions. Every rule and term still requires citation {source, clause}: every quantity has a source.");
        }
        _ => {}
    }
}
