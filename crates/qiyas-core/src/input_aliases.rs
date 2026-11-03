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
    // Entities have no `name`; the one an insert carries is its block's.
    let entities = matches!(query.get("collection"), None | Some(Value::Null))
        || query["collection"] == "entities";
    let block_word = |path: &Value| path == "block" || (entities && path == "name");
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
            if block_word(&condition["path"]) {
                condition["path"] = json!("block_name");
            }
        }
    }
    if let Some(fields) = query.get_mut("fields").and_then(Value::as_array_mut) {
        for field in fields {
            if block_word(field) {
                *field = json!("block_name");
            }
        }
    }
}

/// Other words for a dimension edit's operations, at the top or inside `op`
/// (the office run sent `op: {text_position: [x, y]}`).
fn dim_edit_words(value: &mut Value) {
    const WORDS: [(&str, &str); 6] = [
        ("text_position", "move_text"),
        ("text_point", "move_text"),
        ("text_at", "move_text"),
        ("dimension_line_point", "line_point"),
        ("line_position", "line_point"),
        ("override", "text_override"),
    ];
    for (from, to) in WORDS {
        alias(value, from, to);
        if value["op"].is_object() {
            alias(&mut value["op"], from, to);
        }
    }
}

/// A decision written as one entry with its fields beside `kind` (no `entries`): those
/// fields are the entry. A nonnumeric value with no unit has the empty unit.
fn decision_entry_fields(value: &mut Value) {
    wrap_decision_entry(value);
    let mut unit = |entry: &mut Value| {
        if entry.is_object()
            && entry.get("unit").is_none()
            && entry
                .get("value")
                .is_some_and(|v| !v.is_number() && !v.is_null())
        {
            entry["unit"] = json!("");
        }
    };
    match value.get_mut("entries") {
        Some(Value::Array(entries)) => entries.iter_mut().for_each(&mut unit),
        Some(entry) => unit(entry),
        None => {}
    }
}

/// Moves a lone entry's fields into `entries`: a rename, never reported as defaults.
fn wrap_decision_entry(value: &mut Value) {
    const ENTRY: [&str; 10] = [
        "name",
        "value",
        "unit",
        "reason",
        "source",
        "citation",
        "set_by",
        "status",
        "question",
        "takeoff_exclusion",
    ];
    if value.get("entries").is_none() && value.get("name").is_some() {
        if let Some(map) = value.as_object_mut() {
            let mut entry = serde_json::Map::new();
            for field in ENTRY {
                if let Some(v) = map.remove(field) {
                    entry.insert(field.into(), v);
                }
            }
            map.insert("entries".into(), Value::Array(vec![Value::Object(entry)]));
        }
    }
}

/// Paper corners together with a model window: the corners are the frame, the window's
/// middle its centre and, with no scale given, the largest scale that shows the whole window.
fn mview_mixed(value: &mut Value) {
    let point = |v: &Value| Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?]);
    let (Some(c1), Some(c2), Some(b1), Some(b2)) = (
        point(&value["corner1"]),
        point(&value["corner2"]),
        point(&value["model_bounds"][0]),
        point(&value["model_bounds"][1]),
    ) else {
        return;
    };
    if !value["paper_centre"].is_null() && value.get("paper_centre").is_some() {
        return;
    }
    if value.get("centre").is_none_or(Value::is_null) {
        value["centre"] = json!([(b1[0] + b2[0]) / 2.0, (b1[1] + b2[1]) / 2.0]);
    }
    if value.get("scale").is_none_or(Value::is_null) {
        let (mw, mh) = ((b2[0] - b1[0]).abs(), (b2[1] - b1[1]).abs());
        let (pw, ph) = ((c2[0] - c1[0]).abs(), (c2[1] - c1[1]).abs());
        let fit = match (mw > 0.0, mh > 0.0) {
            (true, true) => (pw / mw).min(ph / mh),
            (true, false) => pw / mw,
            (false, true) => ph / mh,
            (false, false) => return,
        };
        value["scale"] = json!(fit);
    }
    if let Some(map) = value.as_object_mut() {
        map.remove("model_bounds");
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
    if name == "dim_edit" {
        dim_edit_words(value);
    }
    if (name == "decision" && value["kind"] == "set") || name == "decision.set" {
        decision_entry_fields(value);
    }
    match name.as_str() {
        "set_layer"
            if value.get("prop").is_none()
                && value.get("value").is_none()
                && ![
                    "color",
                    "linetype",
                    "lineweight",
                    "on",
                    "frozen",
                    "locked",
                    "plot",
                ]
                .iter()
                .any(|key| value.get(*key).is_some()) =>
        {
            if let Some(layer) = value.as_object_mut().and_then(|map| map.remove("name")) {
                value["type"] = json!("set_current");
                value["current"] = json!({"layer":layer, "color":"ByLayer"});
            }
        }
        "opening_schedule" => alias(value, "height", "text_height"),
        "mview" => mview_mixed(value),
        "decision" | "decision.set" if value["kind"] == "set" || name == "decision.set" => {
            let description = value
                .as_object_mut()
                .and_then(|map| map.remove("description"));
            fn reason(entry: &mut Value, description: Option<&Value>) {
                // A decision that does not say who set it was set by the one writing it in
                // a command, Mizan; the window names the engineer itself. Reported as a default.
                if entry.is_object() && entry.get("set_by").is_none() {
                    entry["set_by"] = json!("mizan");
                }
                let own_description = entry
                    .as_object_mut()
                    .and_then(|map| map.remove("description"));
                if entry.get("reason").is_none()
                    && entry["source"] == "mizan_design"
                    && entry["set_by"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("mizan"))
                    && matches!(entry["status"].as_str(), Some("answered" | "provisional"))
                    && crate::decisions::has_value(&entry["value"])
                {
                    if let Some(text) = own_description
                        .as_ref()
                        .or(description)
                        .and_then(Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                    {
                        entry["reason"] = json!(text);
                    }
                }
            }
            if let Some(entries) = value.get_mut("entries") {
                if let Some(entries) = entries.as_array_mut() {
                    for entry in entries {
                        reason(entry, description.as_ref());
                    }
                } else {
                    reason(entries, description.as_ref());
                }
            }
        }
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
    let mut wrapped;
    let set = raw["kind"] == "set" || raw["action"] == "set";
    let raw = if (raw["type"] == "decision" && set) || raw["type"] == "set" {
        wrapped = raw.clone();
        wrap_decision_entry(&mut wrapped);
        &wrapped
    } else {
        raw
    };
    fn visit(a: &Value, b: &Value, path: &str, out: &mut Vec<DefaultUsed>) {
        if a["type"] == "mview"
            && a.get("model_bounds").is_some()
            && b.get("model_bounds").is_none()
        {
            for field in ["centre", "scale"] {
                if a.get(field).is_none_or(Value::is_null) && b.get(field).is_some() {
                    out.push(DefaultUsed {
                        field: format!("{path}/{field}"),
                        value: b[field].clone(),
                        unit: None,
                    });
                }
            }
        }
        if a["type"] == "set_layer" && b["type"] == "set_current" {
            out.push(DefaultUsed {
                field: format!("{path}/current/color"),
                value: json!("ByLayer"),
                unit: None,
            });
        }
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
                            | "reason"
                            | "set_by"
                            | "unit"
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
        "opening_schedule" => {
            // The alias has no default of its own: an omitted height reads as text_height's.
            let mut alias = schema["properties"]["text_height"].clone();
            if let Some(map) = alias.as_object_mut() {
                map.remove("default");
            }
            schema["properties"]["height"] = alias;
        }
        "decision.set" | "decision.list" => {
            if name == "decision.set" {
                schema["properties"]["description"] = json!({"type":"string", "description":"Used as an omitted reason only for a nonempty Mizan design value (source mizan_design, set_by mizan)."});
                if let Some(variants) = schema["properties"]["entries"]["anyOf"].as_array_mut() {
                    for variant in variants {
                        let entry = if variant["type"] == "array" {
                            &mut variant["items"]
                        } else {
                            variant
                        };
                        entry["properties"]["description"] = json!({"type":"string"});
                        optional(entry, &["set_by"]);
                        entry["properties"]["set_by"]["description"] =
                            json!("Who set it; omitted, mizan (reported as a default)");
                    }
                }
            }
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
