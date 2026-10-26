//! Compact wire replies. The saved artifact always contains the untouched result.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::{artifact, surface, ToolError, ToolOutput};

const SAMPLE: usize = 5;
const SMALL: usize = 2_000;

pub(crate) fn format(
    tool: &str,
    op: &str,
    args: &Value,
    output: ToolOutput,
) -> Result<ToolOutput, ToolError> {
    let ToolOutput::Json(value) = output else {
        return Ok(output);
    };
    if op == "artifact" {
        return Ok(ToolOutput::Json(value));
    }
    let full = args["detail"] == "full";
    let execute = tool == surface::EXECUTE;
    let discovery =
        tool == surface::READ && matches!(op, "commands" | "record_schema" | "takeoff_kit");
    let large = value.to_string().len() > SMALL;
    if !execute && !large && !discovery && args.get("fields").is_none() {
        return Ok(ToolOutput::Json(value));
    }
    // Failure to save evidence must not disguise a committed edit as a failed command.
    let id = match artifact::save(&value) {
        Ok(id) => id,
        Err(error) => {
            let mut value = value;
            crate::advice::append(
                &mut value,
                json!({"code":"evidence_storage","message":error.to_string()}),
            );
            return Ok(ToolOutput::Json(value));
        }
    };
    let mut out = if full || (tool == surface::READ && op == "records") {
        value.clone()
    } else if discovery {
        compact_contracts(&value)
    } else {
        summary(&value, 0)
    };
    if !out.is_object() {
        out = json!({"value":out});
    }
    if execute {
        if value.get("status").is_none() {
            out["status"] = json!(if value["ok"] == false { "error" } else { "ok" });
        }
        for field in ["added", "changed", "removed"] {
            if let Some(handles) = value[field].as_array() {
                out[format!("{field}_count")] = json!(handles.len());
                if !full {
                    out[field] = json!(handles.iter().take(SAMPLE).collect::<Vec<_>>());
                }
            } else {
                out[format!("{field}_count")] = json!(0);
                out[field] = json!([]);
            }
        }
    }
    if !full {
        if let Some(check) = value.get("check") {
            out["check"] = check_summary(check);
        }
        let mut advice = BTreeMap::new();
        collect_advice(&value, &mut advice);
        remove_advice(&mut out);
        if !advice.is_empty() {
            out["advice"] = json!(advice
                .into_values()
                .map(|a| summary(&a, 0))
                .collect::<Vec<_>>());
        }
        // UI payloads are consumed locally and must retain every changed handle.
        // Agent/MCP remove them from the model-facing JSON after dispatching the event.
        if let Some(ui) = value.get("ui") {
            out["ui"] = ui.clone();
        }
    }
    if tool == surface::READ && op != "records" {
        if let Some(fields) = args["fields"].as_array() {
            // A name, a JSON pointer, or a path written without its first slash. A field the
            // result does not have is named in `missing_fields` beside the ones it has, so one
            // call answers instead of a refusal and a second call.
            let mut selected = json!({});
            let mut missing = Vec::new();
            for field in fields.iter().filter_map(Value::as_str) {
                let found = value
                    .get(field)
                    .or_else(|| value.pointer(field))
                    .or_else(|| {
                        (!field.starts_with('/'))
                            .then(|| value.pointer(&format!("/{field}")))
                            .flatten()
                    });
                if let Some(found) = found {
                    selected[field] = if full {
                        found.clone()
                    } else {
                        summary(found, 0)
                    };
                } else {
                    missing.push(json!(field));
                }
            }
            if !missing.is_empty() {
                let top: Vec<_> = value
                    .as_object()
                    .map(|map| map.keys().take(30).cloned().collect())
                    .unwrap_or_default();
                if selected.as_object().is_some_and(|map| map.is_empty()) {
                    // Nothing asked for exists: keep the summary and say what does.
                    out["missing_fields"] = json!(missing);
                    out["available_fields"] = json!(top);
                } else {
                    selected["missing_fields"] = json!(missing);
                    selected["available_fields"] = json!(top);
                    out = selected;
                }
            } else {
                out = selected;
            }
        }
    }
    if tool == surface::READ && op == "records" && !full {
        bound_records(&mut out);
    }
    if let Some(revision) = value.get("revision") {
        out["revision"] = revision.clone();
    }
    out["artifact"] = json!(id);
    if discovery && !full {
        if op == "takeoff_kit" {
            if let Some(items) = out.get_mut("commands").and_then(Value::as_object_mut) {
                let total = items.len();
                let offset = args["offset"].as_u64().unwrap_or(0) as usize;
                let page = items
                    .iter()
                    .skip(offset)
                    .take(4)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                *items = page;
                out["total_commands"] = json!(total);
                out["offset"] = json!(offset);
                out["next_offset"] = json!((offset + 4 < total).then_some(offset + 4));
                out["complete"] = json!(offset == 0 && total <= 4);
            }
        }
        if let Some(items) = out.get_mut("commands").and_then(Value::as_array_mut) {
            let total = items.len();
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let page: Vec<_> = items.iter().skip(offset).take(4).cloned().collect();
            *items = page;
            out["total_commands"] = json!(total);
            out["offset"] = json!(offset);
            out["next_offset"] = json!((offset + 4 < total).then_some(offset + 4));
        }
        attach_contract_artifact(&mut out, &id);
    }
    if let Some(object) = out.as_object_mut() {
        if object
            .get("defaults_used")
            .is_some_and(|v| v.as_array().is_some_and(Vec::is_empty))
        {
            object.remove("defaults_used");
        }
    }
    Ok(ToolOutput::Json(artifact::canonical(&out)))
}

/// A page of records is already budgeted by the core, which lets one oversized record through
/// whole (a table or a long polyline). Past this size such a record keeps its plain fields and
/// shortens its nested data; the artifact keeps all of it.
const PAGE_LIMIT: usize = 16_000;
const RECORD_LIMIT: usize = 4_000;

fn bound_records(out: &mut Value) {
    if out.to_string().len() <= PAGE_LIMIT {
        return;
    }
    let mut cut = 0;
    let mut tables = false;
    if let Some(records) = out.get_mut("records").and_then(Value::as_array_mut) {
        for record in records {
            tables |= record["type"] == "Table";
            if record.to_string().len() <= RECORD_LIMIT {
                continue;
            }
            cut += 1;
            for field in ["data", "fields"] {
                if let Some(inner) = record.get_mut(field) {
                    *inner = shorten(inner);
                }
            }
        }
    }
    if cut > 0 {
        out["shortened"] = json!({"records":cut,"detail":"Some records are shortened to fit. Read one part with fields: [\"/pointer\"], or artifact {id, pointer} for the full record."});
    }
    if tables {
        out["hint"] = json!("Table text, row by row: qiyas_read query {kind: \"tables\"}.");
    }
}

/// The value with every long list or object inside it reduced to its size and first rows.
fn shorten(value: &Value) -> Value {
    match value {
        Value::Array(items) if value.to_string().len() > 600 => list(items),
        Value::Object(map) if value.to_string().len() > 600 => Value::Object(
            map.iter()
                .map(|(key, inner)| (key.clone(), shorten(inner)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Keep repair facts in context and the complete validator evidence in the artifact.
pub(crate) fn error(value: Value) -> Value {
    let Ok(id) = artifact::save(&value) else {
        return value;
    };
    let mut out = summary(&value, 0);
    if let Some(message) = value["message"].as_str() {
        out["message"] = json!(message.split(" Expected form:").next().unwrap_or(message));
    }
    out["status"] = json!("error");
    out["artifact"] = json!(id);
    artifact::canonical(&out)
}

fn attach_contract_artifact(value: &mut Value, id: &str) {
    if value["version"].is_string() {
        value["artifact"] = json!(id);
    }
    for key in ["commands", "operations"] {
        if let Some(items) = value.get_mut(key).and_then(Value::as_array_mut) {
            for item in items {
                attach_contract_artifact(item, id);
            }
        }
    }
}

/// Most rows of a long list the model sees in place; the rest stays in the artifact.
const ROWS: usize = 40;
const HINT: &str = "Full evidence is in the artifact; read one part with artifact {id, pointer}.";

/// One element of a long list reduced to its plain fields (text, numbers, flags), long text
/// cut, nested values named in `+` and left in the artifact. This is what a table of the
/// list would show: enough to choose, quote or total from, without paging the artifact.
fn row(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return summary(value, 3);
    };
    let mut out = serde_json::Map::new();
    let mut nested = Vec::new();
    for (key, field) in map {
        match field {
            Value::Null => {}
            Value::String(text) => {
                let mut text = text.clone();
                if text.chars().count() > 100 {
                    text = text.chars().take(100).collect::<String>() + "…";
                }
                out.insert(key.clone(), json!(text));
            }
            Value::Number(_) | Value::Bool(_) => {
                out.insert(key.clone(), field.clone());
            }
            Value::Array(items)
                if items.len() <= 6
                    && items
                        .iter()
                        .all(|v| v.is_number() || v.as_str().is_some_and(|t| t.len() <= 40)) =>
            {
                out.insert(key.clone(), field.clone());
            }
            Value::Array(items) if items.is_empty() => {}
            _ => nested.push(json!(key)),
        }
    }
    if !nested.is_empty() {
        out.insert("+".into(), Value::Array(nested));
    }
    Value::Object(out)
}

/// A long list as `count`, its first `ROWS` rows and what was left out.
fn list(items: &[Value]) -> Value {
    if items.iter().all(Value::is_object) && !items.is_empty() {
        let mut out = json!({"count":items.len(), "rows":items.iter().take(ROWS).map(row).collect::<Vec<_>>(), "detail":HINT});
        if items.len() > ROWS {
            out["more"] = json!(items.len() - ROWS);
        }
        out
    } else {
        json!({"count":items.len(), "sample":items.iter().take(SAMPLE * 4).map(|v| summary(v, 3)).collect::<Vec<_>>(), "detail":HINT})
    }
}

/// Keep small answers compatible, including exact single measurements and action handles.
fn summary(value: &Value, depth: usize) -> Value {
    if value.to_string().len() <= SMALL {
        return value.clone();
    }
    match value {
        Value::Array(items) => list(items),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if key == "defaults_used" && value.as_array().is_some_and(Vec::is_empty) {
                    continue;
                }
                let heavy = matches!(
                    key.as_str(),
                    "measurement_report"
                        | "report"
                        | "items"
                        | "geometry"
                        | "entities"
                        | "body"
                        | "records"
                        | "outcomes"
                );
                if (heavy || depth >= 3) && value.to_string().len() > SMALL {
                    match value {
                        Value::Array(items) => {
                            out.insert(key.clone(), list(items));
                        }
                        Value::Object(inner) => {
                            out.insert(
                                key.clone(),
                                json!({"count":inner.len(),"keys":inner.keys().take(30).collect::<Vec<_>>(),"detail":HINT}),
                            );
                        }
                        _ => {
                            out.insert(key.clone(), summary(value, depth + 1));
                        }
                    }
                } else {
                    out.insert(key.clone(), summary(value, depth + 1));
                }
            }
            Value::Object(out)
        }
        Value::String(text) => {
            json!({"characters":text.chars().count(),"preview":text.chars().take(320).collect::<String>(),"detail":HINT})
        }
        _ => value.clone(),
    }
}

fn collect_advice(value: &Value, found: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "advice" {
                    for advice in child.as_array().into_iter().flatten() {
                        let rule = advice
                            .get("rule")
                            .or_else(|| advice.get("code"))
                            .unwrap_or(advice)
                            .to_string();
                        found.entry(rule).or_insert_with(|| advice.clone());
                    }
                } else if key != "schema" {
                    collect_advice(child, found);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_advice(v, found)),
        _ => {}
    }
}

fn remove_advice(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("advice");
            for child in map.values_mut() {
                remove_advice(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(remove_advice),
        _ => {}
    }
}

fn check_summary(check: &Value) -> Value {
    let mut groups: BTreeMap<String, (usize, BTreeSet<u64>)> = BTreeMap::new();
    for finding in check["findings"].as_array().into_iter().flatten() {
        let rule = finding["rule"].as_str().unwrap_or("unknown").to_owned();
        let (count, handles) = groups.entry(rule).or_default();
        *count += 1;
        handles.extend(
            finding["handles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64),
        );
    }
    for (rule, count) in check["counts"].as_object().into_iter().flatten() {
        groups.entry(rule.clone()).or_default().0 = count.as_u64().unwrap_or(0) as usize;
    }
    json!({"line":check["line"],"complete":check["complete"],"not_checked":check["not_checked"],
        "layout_findings":check["layout_findings"],
        "findings":groups.into_iter().map(|(rule,(count,handles))| json!({"rule":rule,"count":count,"handles":handles.into_iter().take(SAMPLE).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}

fn compact_contracts(value: &Value) -> Value {
    let mut out = value.clone();
    if let Some(name) = value["name"].as_str() {
        let mut contract = value.clone();
        if let Some(object) = contract.as_object_mut() {
            object.remove("revision");
            object.remove("artifact");
        }
        out["version"] = json!(artifact::hash(
            artifact::canonical(&contract).to_string().as_bytes()
        ));
        out["name"] = json!(name);
        if let Some(object) = out.as_object_mut() {
            object.remove("prompts");
            object.remove("words");
            if let Some(examples) = object.get_mut("examples").and_then(Value::as_array_mut) {
                examples.truncate(1);
            }
        }
        fn schema(value: &mut Value) {
            match value {
                Value::Object(object) => {
                    // Constraints, defaults and field types remain; long prose is in the artifact.
                    for key in ["description", "title", "$schema", "examples"] {
                        object.remove(key);
                    }
                    for child in object.values_mut() {
                        schema(child);
                    }
                }
                Value::Array(items) => items.iter_mut().for_each(schema),
                _ => {}
            }
        }
        schema(&mut out["schema"]);
        if let Some(kinds) = value["kinds"].as_array().filter(|k| !k.is_empty()) {
            out.as_object_mut().unwrap().remove("schema");
            // Each kind with its required fields and one example: the first lookup answers.
            let reg = qiyas_core::registry::registry();
            let mut contracts = serde_json::Map::new();
            for kind in kinds.iter().filter_map(Value::as_str) {
                let Some(info) = reg.get(&format!("{name}.{kind}")) else {
                    contracts.insert(kind.into(), json!({}));
                    continue;
                };
                let required: Vec<_> = info.schema["required"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|field| *field != "type" && *field != "kind")
                    .collect();
                let mut entry = json!({"required":required});
                if let Some(mut example) = info.examples.first().cloned() {
                    if let Some(map) = example.as_object_mut() {
                        map.remove("type");
                        map.remove("kind");
                    }
                    if example.to_string().len() <= 400 {
                        entry["example"] = example;
                    }
                }
                contracts.insert(kind.into(), entry);
            }
            out["kinds_contract"] = Value::Object(contracts);
            out.as_object_mut().unwrap().remove("kinds");
            out["next"] = json!(
                "Each kind lists its required fields and an example (type and kind are added: {\"type\": name, \"kind\": kind}). Optional fields: commands {name: \"<name>.<kind>\"}; detail: full only for everything."
            );
        }
        if let Some(doc) = out["doc"].as_str().filter(|doc| doc.len() > 240) {
            out["doc"] = json!(doc.chars().take(240).collect::<String>());
        }
    }
    for key in ["commands", "operations"] {
        if let Some(items) = value[key].as_array() {
            out[key] = json!(items.iter().map(compact_contracts).collect::<Vec<_>>());
        }
    }
    out
}

/// Only the new reply changes; previous discovery messages never move or lose their schema.
pub(crate) fn deduplicate(value: &mut Value, known: &mut BTreeMap<String, Value>) {
    if let (Some(name), Some(version)) = (value["name"].as_str(), value["version"].as_str()) {
        let key = format!("{name}:{version}");
        if let Some(reference) = known.get(&key) {
            *value = json!({"name":name,"version":version,"known":true,"artifact":reference,
                "revision":value["revision"],"detail":"Contract already supplied. Read its artifact for the full schema."});
        } else {
            known.insert(key, value["artifact"].clone());
        }
    }
    for key in ["commands", "operations"] {
        if let Some(items) = value.get_mut(key).and_then(Value::as_array_mut) {
            for item in items {
                deduplicate(item, known);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reply(tool: &str, op: &str, args: Value, value: Value) -> Value {
        let ToolOutput::Json(value) = format(tool, op, &args, ToolOutput::Json(value)).unwrap()
        else {
            panic!("JSON result")
        };
        value
    }

    #[test]
    fn a_long_list_comes_as_rows_to_read_not_as_a_pointer_to_page_through() {
        let items: Vec<_> = (0..115)
            .map(|i| json!({"code":format!("A-{i}"),"description":"Net exterior masonry","quantity":i as f64 * 1.5,"unit":"m3",
                "lines":[{"label":"Gross","value":1.0,"sources":[{"handle":i}]}; 4],"confidence":0.5}))
            .collect();
        let evidence = json!({"revision":null,"outcome":{"result":{"items":items,"warnings":[]}}});
        let out = reply(surface::READ, "run", json!({}), evidence);
        let listed = &out["outcome"]["result"]["items"];
        assert_eq!(listed["count"], 115);
        assert_eq!(listed["rows"].as_array().unwrap().len(), ROWS);
        assert_eq!(listed["more"], 115 - ROWS);
        assert_eq!(listed["rows"][3]["code"], "A-3");
        assert_eq!(listed["rows"][3]["quantity"], 4.5);
        assert_eq!(listed["rows"][3]["unit"], "m3");
        assert_eq!(
            listed["rows"][3]["+"],
            json!(["lines"]),
            "nested parts are named, not shown"
        );
        assert!(out.to_string().len() < 12_000, "{}", out.to_string().len());
        // Any part of the evidence is one pointer away.
        let part = artifact::read(
            &json!({"id":out["artifact"],"pointer":"/outcome/result/items/3/lines/0"}),
        )
        .unwrap();
        assert!(part["data"].as_str().unwrap().contains("Gross"));
    }

    #[test]
    fn a_command_with_kinds_lists_each_kinds_required_fields_and_an_example() {
        let draw =
            serde_json::to_value(qiyas_core::registry::registry().get("draw").unwrap()).unwrap();
        let out = reply(surface::READ, "commands", json!({"name":"draw"}), draw);
        let line = &out["kinds_contract"]["line"];
        assert_eq!(line["required"], json!(["a", "b"]));
        assert!(line["example"]["a"].is_array());
        assert!(out["kinds_contract"]["polyline"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("points")));
        assert!(out.get("kinds").is_none() && out.get("schema").is_none());
        assert!(out.to_string().len() < 9_000, "{}", out.to_string().len());
    }

    #[test]
    fn one_huge_record_is_shortened_to_fit_and_points_at_the_compact_table_read() {
        let rows: Vec<_> = (0..500)
            .map(|i| json!(["D-1000x2100", "Door", "1000 x 2100", format!("{i}")]))
            .collect();
        let value = json!({"collection":"entities","total":2,"offset":0,"next_offset":null,"truncated":false,"revision":null,
            "records":[
                {"key":718,"type":"Table","layer":"A-SCHEDULES","data":{"rows":rows,"columns":4}},
                {"key":719,"type":"Line","layer":"A-WALL","length":3.0}]});
        let out = reply(surface::READ, "records", json!({}), value);
        assert!(out.to_string().len() < 16_000, "{}", out.to_string().len());
        assert_eq!(out["shortened"]["records"], 1);
        assert!(out["hint"].as_str().unwrap().contains("tables"));
        assert_eq!(out["records"][0]["data"]["columns"], 4);
        assert_eq!(out["records"][0]["data"]["rows"]["count"], 500);
        assert_eq!(out["records"][1]["length"], 3.0, "small records stay whole");
    }

    #[test]
    fn batch_envelope_preserves_counts_and_full_precision_in_paginated_evidence() {
        let evidence = json!({"revision":{"document_revision":12},"added":(1..=100).collect::<Vec<_>>(),
            "measurement_report":{"items":(0..1000).map(|i|json!({"handle":i,"area_m2":1.234567890123456})).collect::<Vec<_>>()},
            "defaults_used":[],"ui":{"handles":(1..=100).collect::<Vec<_>>()},
            "advice":[{"code":"check","message":"Check dimensions."},{"code":"check","message":"Check dimensions."}],
            "check":{"counts":{"duplicates":30},"findings":(0..30).map(|i|json!({"rule":"duplicates","handles":[i]})).collect::<Vec<_>>()}});
        let compact = reply(surface::EXECUTE, "batch", json!({}), evidence.clone());
        assert_eq!(compact["added_count"], 100);
        assert_eq!(compact["added"].as_array().unwrap().len(), 5);
        assert_eq!(compact["check"]["findings"][0]["count"], 30);
        assert_eq!(compact["advice"].as_array().unwrap().len(), 1);
        assert_eq!(compact["ui"], evidence["ui"]);
        assert!(compact.get("defaults_used").is_none());
        assert!(compact.to_string().len() < 2000);
        let mut text = String::new();
        let mut offset = 0;
        loop {
            let page = artifact::read(&json!({"id":compact["artifact"],"offset":offset})).unwrap();
            assert_eq!(page["artifact_revision"], evidence["revision"]);
            text.push_str(page["data"].as_str().unwrap());
            match page["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), evidence);
        let full = reply(
            surface::EXECUTE,
            "batch",
            json!({"detail":"full"}),
            evidence.clone(),
        );
        assert_eq!(full["measurement_report"], evidence["measurement_report"]);
    }

    #[test]
    fn full_entity_reads_keep_precision_in_mm_cm_m_and_inches() {
        for (units, metres) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
            let dir = tempfile::tempdir().unwrap();
            let mut session =
                qiyas_core::Session::new_blank(&dir.path().join("precision.dxf"), units);
            let radius = 0.123456789012345 / metres;
            let ToolOutput::Json(created) = surface::run_session(
                &mut session,
                surface::EXECUTE,
                "run",
                &json!({"command":{"type":"draw","kind":"circle","centre":[0,0],"radius":radius}}),
                qiyas_core::Source::Ai,
            )
            .unwrap() else {
                panic!("JSON edit")
            };
            let handle = created["added"][0].clone();
            let ToolOutput::Json(entity) = surface::run_session(
                &mut session,
                surface::READ,
                "query",
                &json!({"kind":"entity","handle":handle,"detail":"full"}),
                qiyas_core::Source::Ai,
            )
            .unwrap() else {
                panic!("JSON entity")
            };
            let expected = std::f64::consts::PI * radius * radius;
            let area = entity["entity"]["area"].as_f64().unwrap();
            assert!(
                (area - expected).abs() / expected < 1e-13,
                "units {units}: {area} != {expected}"
            );
            let ToolOutput::Json(measured) = surface::run_session(
                &mut session,
                surface::READ,
                "measure",
                &json!({"handle":handle,"detail":"full"}),
                qiyas_core::Source::Ai,
            )
            .unwrap() else {
                panic!("JSON measure")
            };
            assert!(
                (measured["area_m2"].as_f64().unwrap() - expected * metres * metres).abs() < 1e-13
            );
            let artifact = reply(surface::EXECUTE, "run", json!({}), created.clone());
            let undone = surface::run_session(
                &mut session,
                surface::EXECUTE,
                "undo_own",
                &json!({}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
            let ToolOutput::Json(undone) =
                format(surface::EXECUTE, "undo_own", &json!({}), undone).unwrap()
            else {
                panic!("JSON undo")
            };
            assert_eq!(undone["removed_count"], 1);
            assert_eq!(undone["removed"], json!([handle]));
            assert!(session.entity_info(handle.as_u64().unwrap(), &[]).is_err());
            let ToolOutput::Json(page) = surface::run_session(
                &mut session,
                surface::READ,
                "artifact",
                &json!({"id":artifact["artifact"]}),
                qiyas_core::Source::Ai,
            )
            .unwrap() else {
                panic!("JSON evidence")
            };
            assert_eq!(page["revision"], created["revision"]);
            assert_ne!(page["revision"], page["current_revision"]);
            let error=surface::run_session(&mut session,surface::EXECUTE,"run",&json!({"expected_revision":page["revision"],"command":{"type":"draw","kind":"line","a":[0,0],"b":[1,1]}}),qiyas_core::Source::Ai).unwrap_err();
            assert_eq!(error.json()["code"], "stale_read");
        }
    }

    #[test]
    fn contracts_are_versioned_without_revision_churn_and_repeated_only_by_reference() {
        let contract = json!({"name":"draw.line","schema":{"type":"object","properties":{"a":{"type":"array","minItems":2}}},"revision":1});
        let first = reply(surface::READ, "commands", json!({}), contract.clone());
        let mut changed = contract;
        changed["revision"] = json!(2);
        let mut next = reply(surface::READ, "commands", json!({}), changed);
        assert_eq!(first["version"], next["version"]);
        let mut known = BTreeMap::new();
        deduplicate(&mut first.clone(), &mut known);
        deduplicate(&mut next, &mut known);
        assert_eq!(next["known"], true);
        assert!(next.get("schema").is_none());
        assert_eq!(next["artifact"], first["artifact"]);
        let selected = reply(
            surface::READ,
            "measure",
            json!({"fields":["area_m2"]}),
            json!({"area_m2":1.234567890123456,"perimeter_m":4,"revision":2}),
        );
        assert_eq!(selected["area_m2"], json!(1.234567890123456));
        assert!(selected.get("perimeter_m").is_none());
    }

    #[test]
    fn discovery_pages_reach_every_contract_and_full_detail_stays_complete() {
        let evidence = json!({"commands":(0..11).map(|i|json!({"name":format!("command-{i}"),"schema":{"type":"object"}})).collect::<Vec<_>>(),"revision":null});
        let mut offset = 0;
        let mut names = BTreeSet::new();
        loop {
            let page = reply(
                surface::READ,
                "commands",
                json!({"offset":offset}),
                evidence.clone(),
            );
            for contract in page["commands"].as_array().unwrap() {
                assert!(contract["version"].is_string());
                assert!(contract["artifact"].is_string());
                names.insert(contract["name"].as_str().unwrap().to_owned());
            }
            match page["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(names.len(), 11);
        let full = reply(
            surface::READ,
            "commands",
            json!({"detail":"full"}),
            evidence.clone(),
        );
        assert_eq!(full["commands"], evidence["commands"]);
    }

    #[test]
    fn full_record_geometry_is_not_cut_at_the_native_array_display_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = qiyas_core::Session::new_blank(&dir.path().join("geometry.dxf"), 6);
        let points: Vec<_> = (0..150).map(|i| [f64::from(i), f64::from(i % 2)]).collect();
        surface::run_session(
            &mut session,
            surface::EXECUTE,
            "run",
            &json!({"command":{"type":"draw","kind":"polyline","points":points}}),
            qiyas_core::Source::Ai,
        )
        .unwrap();
        let output = surface::run_session(
            &mut session,
            surface::READ,
            "records",
            &json!({"detail":"full"}),
            qiyas_core::Source::Ai,
        )
        .unwrap();
        let ToolOutput::Json(value) =
            format(surface::READ, "records", &json!({"detail":"full"}), output).unwrap()
        else {
            panic!("JSON records")
        };
        assert_eq!(
            value["records"][0]["data"]["vertices"]
                .as_array()
                .unwrap()
                .len(),
            150
        );
        let mut offset = 0;
        let mut evidence = String::new();
        loop {
            let page = artifact::read(&json!({"id":value["artifact"],"offset":offset})).unwrap();
            evidence.push_str(page["data"].as_str().unwrap());
            match page["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        let evidence: Value = serde_json::from_str(&evidence).unwrap();
        assert_eq!(evidence["records"], value["records"]);
    }
}
