//! Compact contracts derived from the registry, with no drawing or trade interpretation.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::registry::{registry, CommandInfo};

pub const MAX_BYTES: usize = 6144;

/// Required object members and enum choices, grouped by schema path. Alternatives retain
/// their tag so callers can distinguish their requirements. No descriptions are duplicated.
pub fn fields(schema: &Value) -> Value {
    fn visit(
        schema: &Value,
        path: &str,
        required: &mut BTreeMap<String, BTreeSet<String>>,
        enums: &mut BTreeMap<String, BTreeSet<String>>,
    ) {
        if let Some(names) = schema["required"].as_array() {
            for name in names.iter().filter_map(Value::as_str) {
                required.entry(path.into()).or_default().insert(name.into());
            }
        }
        if let Some(values) = schema["enum"].as_array() {
            for value in values {
                enums.entry(path.into()).or_default().insert(
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned),
                );
            }
        }
        if let Some(props) = schema["properties"].as_object() {
            for (name, prop) in props {
                let next = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                visit(prop, &next, required, enums);
            }
        }
        if let Some(items) = schema.get("items") {
            visit(items, &format!("{path}[]"), required, enums);
        }
        if let Some(items) = schema.get("additionalProperties").filter(|v| v.is_object()) {
            visit(items, &format!("{path}.*"), required, enums);
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            if let Some(variants) = schema[key].as_array() {
                for variant in variants {
                    let tag = variant["properties"]["kind"]["const"].as_str();
                    if let Some(tag) = tag {
                        enums
                            .entry(format!("{path}.kind"))
                            .or_default()
                            .insert(tag.into());
                    }
                    let next = tag.map_or_else(|| path.to_owned(), |tag| format!("{path}({tag})"));
                    visit(variant, &next, required, enums);
                }
            }
        }
    }
    let (mut required, mut enums) = (BTreeMap::new(), BTreeMap::new());
    visit(schema, "", &mut required, &mut enums);
    json!({"required":required,"enums":enums})
}

pub fn example(info: &CommandInfo) -> Value {
    let index = info
        .takeoff_kit
        .as_ref()
        .and_then(|m| m["example"].as_u64())
        .unwrap_or(0) as usize;
    info.examples[index].clone()
}

pub fn contract(info: &CommandInfo) -> Value {
    let mut value = fields(&info.schema);
    value["example"] = example(info);
    value
}

/// Complete examples carry the required fields. Keep enum choices, without repeating
/// every example's field names; full required-field contracts remain in validation repairs.
pub fn example_contract(info: &CommandInfo) -> Value {
    json!({"enums":fields(&info.schema)["enums"],"example":example(info)})
}

pub fn kit() -> Value {
    let mut entries: Vec<_> = registry()
        .commands()
        .iter()
        .filter(|c| c.takeoff_kit.is_some())
        .collect();
    entries.sort_by_key(|c| c.takeoff_kit.as_ref().unwrap()["order"].as_u64());
    let commands: BTreeMap<_, _> = entries
        .iter()
        .map(|c| (c.name.clone(), example_contract(c)))
        .collect();
    let mut batch: Vec<_> = entries
        .iter()
        .filter_map(|c| {
            c.takeoff_kit.as_ref().unwrap()["batch"]
                .as_u64()
                .map(|order| (order, example(c)))
        })
        .collect();
    batch.sort_by_key(|(order, _)| *order);
    let revision: Vec<_> = ["compare_revisions", "carry_knowledge", "variation_report"]
        .iter()
        .map(|name| example(registry().get(name).expect("revision command")))
        .collect();
    let project: Vec<_> = [
        "create",
        "open",
        "add_drawing",
        "remove_drawing",
        "list",
        "run",
        "set_method",
        "set_checks",
        "set_register_rules",
    ]
    .iter()
    .map(|kind| {
        example(
            registry()
                .get(&format!("project.{kind}"))
                .expect("project command"),
        )
    })
    .collect();
    json!({"measurement_rules":"Declare kind: count/length/area/volume; basis: geometric or rule:<cited note name>. named_handles are in scope: measure them and apply their note's factors. Exclusions: Exclude <handle>: <reason> (note <handle>) in takeoff_check.end_report and final answer. Counts are whole numbers. Fix takeoff_check errors before finishing.","complete":true,"project":project,"revision":revision,"commands":commands,"batch":{"commands":batch.into_iter().map(|(_,v)|v).collect::<Vec<_>>()}})
}

/// Return exactly the kit entry for covered commands, including the auxiliary paths.
pub fn short_entry(info: &CommandInfo) -> Option<Value> {
    if info.takeoff_kit.is_some() {
        return Some(example_contract(info));
    }
    if info.name.starts_with("project.")
        || ["compare_revisions", "carry_knowledge", "variation_report"]
            .contains(&info.name.as_str())
    {
        return Some(json!({"example":example(info)}));
    }
    if !info.name.contains('.') {
        let prefix = format!("{}.", info.name);
        let entries: BTreeMap<_, _> = registry()
            .commands()
            .iter()
            .filter(|c| c.name.starts_with(&prefix))
            .filter_map(|c| short_entry(c).map(|entry| (c.name.clone(), entry)))
            .collect();
        if !entries.is_empty() {
            return Some(json!({"commands":entries}));
        }
    }
    None
}
