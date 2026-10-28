//! Presentation metadata for the window, independent of command execution and model prose.
use qiyas_core::registry::registry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub category: String,
    pub effect: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
}

/// Translation keys and scalar data only. A batch holds at most three grouped parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    pub key: String,
    pub params: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Detail>,
}

/// Registry metadata is the source for every command, including newly registered commands.
/// Surface operations without a command use the tool's read/view/edit contract.
pub fn describe(name: &str, input: &Value) -> Activity {
    let mut activity = classify(name, input);
    activity.detail = detail(name, input, &Value::Null, &activity);
    activity
}

/// Called before the result goes to the window; never reads a model's English summary.
pub fn refine(name: &str, input: &Value, result: &Value) -> Activity {
    let mut activity = classify(name, input);
    activity.detail = detail(name, input, result, &activity);
    if let Some(detail) = &mut activity.detail {
        if result["no_change"] == true {
            detail.key = "activity.line.unchanged".into();
        } else if result["asked_user"] == true {
            detail.key = "activity.line.waiting".into();
        }
    }
    activity
}

fn classify(name: &str, input: &Value) -> Activity {
    let args = input.get("args").and_then(|v| match v {
        Value::String(text) => serde_json::from_str::<Value>(text).ok(),
        Value::Object(_) => Some(v.clone()),
        _ => None,
    });
    let command = args.as_ref().and_then(|a| a.get("command"));
    let parsed = command
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    let command = parsed.as_ref().or(command);
    let label = command
        .and_then(|c| {
            let base = c.get("type")?.as_str()?;
            let full = c
                .get("kind")
                .and_then(Value::as_str)
                .map(|kind| format!("{base}.{kind}"));
            full.filter(|name| registry().get(name).is_some())
                .or_else(|| Some(base.to_string()))
        })
        .or_else(|| crate::surface::label(name, input));
    if let Some(info) = label.as_deref().and_then(|label| registry().get(label)) {
        let effect = command
            .and_then(|value| serde_json::from_value::<qiyas_core::Command>(value.clone()).ok())
            .map(|command| qiyas_core::registry::effect_of(&command))
            .unwrap_or(info.effect);
        return Activity {
            detail: None,
            category: info.category.clone(),
            effect: serde_json::to_value(effect)
                .expect("registry effect")
                .as_str()
                .unwrap()
                .into(),
        };
    }
    Activity {
        detail: None,
        category: "drawing".into(),
        effect: if name != crate::surface::EXECUTE {
            "read"
        } else if crate::surface::mutating(name, input) {
            "edit"
        } else {
            "view"
        }
        .into(),
    }
}

fn decoded(value: &Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        _ => value.clone(),
    }
}

fn line(verb: &str, what: &str) -> Detail {
    Detail {
        key: format!("activity.line.{verb}"),
        params: BTreeMap::from([("what".into(), Value::String(what.into()))]),
        parts: Vec::new(),
    }
}

fn put(detail: &mut Detail, key: &str, value: &Value) {
    if value.is_number() || value.as_str().is_some_and(|s| !s.is_empty()) {
        detail.params.insert(key.into(), value.clone());
    }
}

fn first<'a>(values: &[&'a Value]) -> &'a Value {
    values
        .iter()
        .copied()
        .find(|v| !v.is_null())
        .unwrap_or(&Value::Null)
}

fn outcome(result: &Value) -> &Value {
    result
        .get("outcome")
        .filter(|v| v.is_object())
        .unwrap_or(result)
}

fn length(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_array().map(|a| a.len() as u64))
}

/// These are wire type names, not words extracted from prose.
fn noun(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "entities" | "objects" => "item",
        "walls" | "wall" => "wall",
        "three_point_arc" => "arc",
        "solid2d" => "solid",
        "ground_line" => "ground",
        "foundation_level" => "foundation",
        "material_hatch" => "material",
        "lwpolyline" | "polyline2d" | "polyline3d" => "polyline",
        "mtext" | "texts" => "text",
        "insert" | "blocks" => "block",
        "layers" => "layer",
        "layouts" | "sheets" | "knowledge_sheets" => "sheet",
        "tables" | "schedules" | "knowledge_tables" => "table",
        "dimensions" => "dimension",
        "rooms" => "room",
        "viewports" => "viewport",
        "groups" => "group",
        "comments" => "note",
        "parameters" => "parameter",
        "constraints" => "constraint",
        "xrefs" => "xref",
        "views" | "knowledge_views" => "view",
        "knowledge_levels" => "floor",
        "levels" => "level",
        "knowledge_rules" | "rules" => "rule",
        "knowledge_mappings" | "mappings" => "mapping",
        "knowledge_checks" => "check",
        "knowledge_queries" => "query",
        "knowledge_reports" => "report",
        "knowledge_measurement" | "measurement" => "takeoff",
        "knowledge_placement" => "placement",
        "knowledge_types" | "object_types" => "object_type",
        other => return other.to_string(),
    }
    .into()
}

fn scope(detail: &mut Detail, args: &Value, result: &Value) {
    put(
        detail,
        "layer",
        first(&[&result["layer"], &args["layer"], &args["layers"]["outline"]]),
    );
    put(detail, "sheet", first(&[&args["sheet"], &result["sheet"]]));
    put(
        detail,
        "name",
        first(&[&result["name"], &args["name"], &args["title"]]),
    );
    if let Some(names) = args["names"].as_array() {
        let names: Vec<_> = names.iter().filter_map(Value::as_str).collect();
        if !names.is_empty() {
            put(detail, "name", &Value::String(names.join(", ")));
        }
    }
    put(detail, "block", &args["block"]);
}

fn detail(name: &str, input: &Value, result: &Value, activity: &Activity) -> Option<Detail> {
    let args = decoded(&input["args"]);
    let op = input["op"].as_str().unwrap_or_default();
    if name == crate::surface::CAPTURE {
        let mut d = line("capture", "view");
        scope(&mut d, &args, result);
        put(
            &mut d,
            "name",
            first(&[&args["name"], &args["sheet"], &result["sheet"]]),
        );
        if let Some(path) = args["path"].as_str() {
            put(
                &mut d,
                "file",
                &Value::String(path.rsplit(['/', '\\']).next().unwrap_or(path).into()),
            );
        }
        put(&mut d, "page", &args["page"]);
        d.params.remove("sheet");
        return Some(d);
    }
    if op == "run" {
        let mut command = decoded(&args["command"]);
        if let Some(map) = command.as_object_mut() {
            for field in ["layer", "sheet"] {
                if !map.contains_key(field) && !args[field].is_null() {
                    map.insert(field.into(), args[field].clone());
                }
            }
        }
        // The surface wraps a single command in a batch to apply the requested layer/colour.
        let mut result = result.clone();
        if command["type"] != "batch" && (args["layer"].is_string() || args["color"].is_string()) {
            if let Some(single) = result.pointer("/outcome/outcomes/0").cloned() {
                result["outcome"] = single;
            }
        }
        return command_detail(&command, &result, activity);
    }
    if op == "batch" {
        return Some(batch_detail(&args, result, activity));
    }
    if op == "records" {
        return command_detail(
            &serde_json::json!({"type":"records", "query":args}),
            result,
            activity,
        );
    }
    if matches!(op, "set_records" | "create_entities") {
        let mut command = args.clone();
        if let Some(map) = command.as_object_mut() {
            map.insert("type".into(), Value::String(op.into()));
        }
        return command_detail(&command, result, activity);
    }
    let mut d = match op {
        "commands" => {
            let mut d = line("lookup", "command");
            let names = args["names"].as_array().map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            put(
                &mut d,
                "name",
                &names.map(Value::String).unwrap_or_else(|| {
                    first(&[&args["name"], &args["query"], &args["category"]]).clone()
                }),
            );
            return Some(d);
        }
        "plan" => {
            let phases = first(&[&result["plan"]["phases"], &args["phases"]]);
            let mut d = line("plan", "plan");
            if let Some(phases) = phases.as_array() {
                put(
                    &mut d,
                    "n",
                    &Value::from(phases.iter().filter(|p| p["status"] == "done").count()),
                );
                put(&mut d, "total", &Value::from(phases.len()));
            }
            return Some(d);
        }
        "save_drawing" => line("save", "drawing"),
        "open_drawing" => line("open", "drawing"),
        "close_drawing" => line("close", "drawing"),
        "switch_drawing" | "view" => line("show", "view"),
        "new_drawing" => line("make", "drawing"),
        "import_boq" => line("import", "boq"),
        "export_takeoff" | "export_conflicts" | "export_evidence" | "plot" => {
            line("export", "file")
        }
        "check" | "brief_check" | "takeoff_check" => line(
            "check",
            match op {
                "brief_check" => "brief",
                "takeoff_check" => "takeoff",
                _ => "drawing",
            },
        ),
        "compare" => line("compare", "boq"),
        "select" => line("select", "item"),
        "undo_own" => line("undo", "change"),
        "redo_own" => line("redo", "change"),
        "ask" => line("ask", "question"),
        "finish" => line("finish", "work"),
        "type" => {
            let mut d = line("run", "command");
            put(&mut d, "name", &args["line"]);
            return Some(d);
        }
        "state" => line("read", "state"),
        "document" => line("read", "drawing"),
        "selection" => line("read", "selection"),
        "drawings" => line("read", "drawing"),
        "history" | "events" => line("read", "history"),
        "panel_state" => line("read", "panel"),
        "rooms" => line("read", "room"),
        "capabilities" => line("read", "capability"),
        "record_schema" => line("read", "record_schema"),
        "artifact" => line("read", "output"),
        "explain" => line("read", "explanation"),
        "digest" => line("read", "overview"),
        "takeoff_kit" => line("read", "takeoff_method"),
        "workflow" => line("read", "workflow"),
        "validate" => line("check", "command"),
        "query" => line("read", "query"),
        "measure" => line("measure", "item"),
        _ => return None,
    };
    scope(&mut d, &args, result);
    let file = first(&[
        &result["written"],
        &result["saved"],
        &result["path"],
        &args["path"],
    ]);
    if let Some(path) = file.as_str() {
        put(
            &mut d,
            "file",
            &Value::String(path.rsplit(['/', '\\']).next().unwrap_or(path).into()),
        );
    }
    if let Some(n) = count(result).or_else(|| length(&args["handles"])) {
        put(&mut d, "n", &Value::from(n));
    }
    if matches!(op, "check" | "brief_check" | "takeoff_check") {
        if let Some(n) = length(first(&[&result["findings"], &result["report"]["findings"]])) {
            put(&mut d, "notes", &Value::from(n));
        }
    }
    Some(d)
}

fn command_detail(command: &Value, result: &Value, activity: &Activity) -> Option<Detail> {
    let mut normalized = command.clone();
    qiyas_core::registry::canonicalize(&mut normalized);
    let command = &normalized;
    let kind = command["type"].as_str()?;
    if kind == "batch" {
        return Some(batch_detail(command, result, activity));
    }
    if kind == "records" {
        let query = command.get("query").unwrap_or(command);
        let collection = first(&[&query["collection"], &outcome(result)["collection"]])
            .as_str()
            .unwrap_or("entities");
        let what = query["type"]
            .as_str()
            .filter(|s| *s != "records")
            .unwrap_or(collection);
        let mut d = line("read", &noun(what));
        scope(&mut d, query, result);
        if let Some(n) = count(result) {
            put(&mut d, "n", &Value::from(n));
        }
        // Keep the collection as well when a type filter makes the object more specific.
        if what != collection && collection != "entities" {
            put(&mut d, "collection", &Value::String(noun(collection)));
        }
        return Some(d);
    }
    if kind == "create_entities" {
        let commands: Vec<Value> = command["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entity| {
                serde_json::json!({"type":"draw", "kind":entity["type"],
                "layer":entity.pointer("/data/common/layer"), "sheet":command["sheet"]})
            })
            .collect();
        return Some(batch_detail(
            &serde_json::json!({"commands":commands}),
            result,
            activity,
        ));
    }
    let out = outcome(result);
    let report = out
        .get("report")
        .or_else(|| out.get("result"))
        .unwrap_or(out);
    let subkind = command["kind"].as_str().unwrap_or_default();
    let (verb, what) = match kind {
        "draw" | "symbol" => ("draw", noun(subkind)),
        "wall" | "walls" => ("draw", "wall".into()),
        "drafting_knowledge" => ("read", "drafting_guidance".into()),
        "modify" => (
            match subkind {
                "move" => "move",
                "copy" => "copy",
                "rotate" => "rotate",
                "scale" => "scale",
                "erase" => "remove",
                "mirror" => "mirror",
                _ => "edit",
            },
            noun(
                first(&[&out["entity_type"], &command["entity_type"]])
                    .as_str()
                    .unwrap_or("item"),
            ),
        ),
        "erase" | "delete_entities" => ("remove", "item".into()),
        "text" | "mtext" => ("add", "text".into()),
        "edit_text" | "replace_text" => ("edit", "text".into()),
        "make_sheet" => ("make", "sheet".into()),
        "table" | "object_schedule" | "room_schedule" | "opening_schedule" => {
            ("add", "table".into())
        }
        "edit_cell" => ("edit", "cell".into()),
        "space_check" => ("check", "room_size".into()),
        "check_drawing" => ("check", "drawing".into()),
        "set_records" => (
            "edit",
            noun(command["collection"].as_str().unwrap_or("entities")),
        ),
        "measure_area" => ("measure", "area".into()),
        "measure_length" => ("measure", "length".into()),
        "dimension" => ("add", "dimension".into()),
        "set_sheet" => ("show", "sheet".into()),
        "new_layout" => ("make", "sheet".into()),
        "rename_layout" => ("rename", "sheet".into()),
        "set_layout_paper" => ("edit", "sheet".into()),
        "layer" | "add_layer" | "new_layer" => ("add", "layer".into()),
        "delete_layer" => ("remove", "layer".into()),
        "set_layer" | "set_layer_hidden" => ("edit", "layer".into()),
        "rename_layer" => ("rename", "layer".into()),
        "create_group" => ("make", "group".into()),
        "add_to_group" | "remove_from_group" => ("edit", "group".into()),
        "delete_group" => ("remove", "group".into()),
        "create_block" | "define_block" => ("make", "block".into()),
        "insert_block" => ("add", "block".into()),
        "redefine_block" | "edit_block_in_place" => ("edit", "block".into()),
        "write_block" => ("export", "block".into()),
        "save_view" => ("save", "view".into()),
        "delete_view" => ("remove", "view".into()),
        "comment" | "add_markup" => ("add", "note".into()),
        "list_comments" => ("read", "note".into()),
        "rooms" => ("read", "room".into()),
        "measure_room" => ("measure", "room".into()),
        "measure_distance" => ("measure", "length".into()),
        "add_measurement" => ("add", "takeoff".into()),
        "remove_measurement" => ("remove", "takeoff".into()),
        "list_constraints" => ("read", "constraint".into()),
        "list_parameters" => ("read", "parameter".into()),
        "add_constraint" | "add_dim_constraint" => ("add", "constraint".into()),
        "delete_constraint" | "delete_constraints_on" => ("remove", "constraint".into()),
        "set_parameter" => ("edit", "parameter".into()),
        "record_schema" => ("read", "record_schema".into()),
        "deliverable" => ("export", "file".into()),
        _ => {
            // Keep the registry name for commands without a dedicated sentence. The UI reuses
            // its translated command words where available; it never guesses from a summary.
            let mut d = line(
                if activity.effect == "read" {
                    "inspect"
                } else {
                    "run"
                },
                "command",
            );
            let label = if subkind.is_empty() {
                kind.into()
            } else {
                format!("{kind}.{subkind}")
            };
            put(&mut d, "command", &Value::String(label));
            scope(&mut d, command, result);
            put(
                &mut d,
                "file",
                &command["path"]
                    .as_str()
                    .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p))
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            );
            if let Some(n) = count(result) {
                put(&mut d, "n", &Value::from(n));
            }
            return Some(d);
        }
    };
    let mut d = line(verb, &what);
    scope(&mut d, command, result);
    if kind == "set_sheet" {
        put(&mut d, "name", &command["block"]);
        d.params.remove("block");
    }
    if kind == "rename_layer" || kind == "rename_layout" {
        put(&mut d, "name", &command["from"]);
        put(&mut d, "to", &command["to"]);
    }
    if kind == "make_sheet" {
        put(
            &mut d,
            "name",
            first(&[&report["layout"], &report["name"], &command["name"]]),
        );
        put(
            &mut d,
            "paper",
            first(&[&report["paper"], &command["paper"]]),
        );
        put(
            &mut d,
            "scale",
            first(&[&report["scale"], &command["scale"]]),
        );
    } else if what == "table" {
        if let Some(rows) = length(first(&[&report["rows"], &command["rows"]])) {
            put(&mut d, "rows", &Value::from(rows));
        }
        if let Some(rows) = command["data"].as_array() {
            d.params
                .entry("rows".into())
                .or_insert(Value::from(rows.len()));
        }
    } else if kind == "space_check" || kind == "check_drawing" {
        if let Some(n) = length(&report["findings"]) {
            put(&mut d, "notes", &Value::from(n));
        }
    } else {
        // A wall is a run, not every line/hatch generated to draw that run.
        let n = if what == "wall" {
            length(&command["runs"])
        } else if kind == "symbol" {
            length(&command["levels"]).or(Some(1))
        } else if matches!(verb, "draw" | "add") {
            length(first(&[&result["added_count"], &out["added_count"]]))
                .or_else(|| length(&out["handles"]))
                .or_else(|| (kind == "draw" || what == "text" || kind == "dimension").then_some(1))
        } else {
            length(first(&[&result["changed_count"], &out["changed_count"]]))
                .or_else(|| {
                    length(
                        &result[if verb == "remove" {
                            "removed"
                        } else {
                            "changed"
                        }],
                    )
                })
                .or_else(|| length(&command["handles"]))
                .or_else(|| length(&command["targets"]))
                .or_else(|| length(&command["updates"]))
                .or_else(|| command.get("handle").map(|_| 1))
        };
        if let Some(n) = n {
            put(&mut d, "n", &Value::from(n));
        }
    }
    if let Some(path) =
        first(&[&result["written"], &out["export"]["path"], &command["path"]]).as_str()
    {
        put(
            &mut d,
            "file",
            &Value::String(path.rsplit(['/', '\\']).next().unwrap_or(path).into()),
        );
    }
    Some(d)
}

fn batch_detail(args: &Value, result: &Value, activity: &Activity) -> Detail {
    let mut batch = line("batch", "step");
    let steps = first(&[
        &result["steps"],
        &result["outcome"]["steps"],
        &result["outcome"]["outcomes"],
    ]);
    let mut groups: Vec<(Detail, usize)> = Vec::new();
    for (i, command) in args["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut command = decoded(command);
        if let Some(map) = command.as_object_mut() {
            for field in ["layer", "sheet"] {
                if !map.contains_key(field) && !args[field].is_null() {
                    map.insert(field.into(), args[field].clone());
                }
            }
        }
        let answer = steps.get(i).unwrap_or(&Value::Null);
        let Some(mut part) = command_detail(&command, answer, activity) else {
            continue;
        };
        let n = part.params.remove("n").and_then(|n| n.as_u64());
        if let Some((existing, step_count)) = groups.iter_mut().find(|(existing, _)| {
            let mut comparable = existing.clone();
            comparable.params.remove("n");
            n.is_some() && comparable == part
        }) {
            *step_count += 1;
            if let Some(n) = n {
                let old = existing
                    .params
                    .get("n")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                put(existing, "n", &Value::from(old + n));
            }
        } else {
            if let Some(n) = n {
                put(&mut part, "n", &Value::from(n));
            }
            groups.push((part, 1));
        }
    }
    let more: usize = groups.iter().skip(3).map(|(_, steps)| steps).sum();
    batch.parts = groups.into_iter().take(3).map(|(part, _)| part).collect();
    if more > 0 {
        put(&mut batch, "more", &Value::from(more));
    }
    batch
}

/// Counts are data, never extracted from English summaries or guessed from a command name.
pub fn count(value: &Value) -> Option<u64> {
    let out = outcome(value);
    ["total", "added_count", "changed_count"]
        .into_iter()
        .find_map(|key| length(first(&[&value[key], &out[key]])))
        .or_else(|| length(&out["handles"]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn commands_use_registry_metadata_including_reads() {
        let en: Value = serde_json::from_str(include_str!("../../../src/i18n/en.json")).unwrap();
        let ar: Value = serde_json::from_str(include_str!("../../../src/i18n/ar.json")).unwrap();
        for command in registry().commands() {
            let key = format!("activity.subject.{}", command.category);
            assert!(en[&key].is_string(), "English: {key}");
            assert!(ar[&key].is_string(), "Arabic: {key}");
            let Some(example) = command.examples.first() else {
                continue;
            };
            let activity = describe(
                crate::surface::EXECUTE,
                &json!({"op": "run", "args": json!({"command": example}).to_string()}),
            );
            assert_eq!(activity.category, command.category, "{}", command.name);
        }
        let read = describe(
            crate::surface::READ,
            &json!({"op": "records", "args": "{}"}),
        );
        assert_eq!(read.effect, "read");
        let dry_run = describe(
            crate::surface::EXECUTE,
            &json!({"op": "run", "args": {"command": {"type": "purge", "dry_run": true}}}),
        );
        assert_eq!(dry_run.effect, "read");
        assert_eq!(count(&json!({"total": 12})), Some(12));
        assert_eq!(count(&json!({"message": "12 found"})), None);
    }
    #[test]
    fn batches_group_counts_by_kind_and_keep_layer_and_overflow() {
        let commands: Vec<Value> = ["line", "line", "line", "line", "arc", "arc"]
            .into_iter()
            .map(|kind| json!({"type":"draw", "kind":kind}))
            .collect();
        let input = json!({"op":"batch", "args":{"layer":"A-WALL", "commands":commands}});
        let running = describe(crate::surface::EXECUTE, &input).detail.unwrap();
        assert_eq!(running.key, "activity.line.batch");
        assert_eq!(running.parts.len(), 2);
        assert_eq!(running.parts[0].params["n"], 4);
        assert_eq!(running.parts[0].params["what"], "line");
        assert_eq!(running.parts[0].params["layer"], "A-WALL");
        assert_eq!(running.parts[1].params["n"], 2);
        let done = refine(
            crate::surface::EXECUTE,
            &input,
            &json!({"steps":[
                {"outcome":{"handles":[1,2]}}, {"outcome":{"handles":[3]}},
                {"outcome":{"handles":[4]}}, {"outcome":{"handles":[5]}},
                {"outcome":{"handles":[6]}}, {"outcome":{"handles":[7]}}
            ]}),
        )
        .detail
        .unwrap();
        assert_eq!(done.parts[0].params["n"], 5);
        let many = describe(
            crate::surface::EXECUTE,
            &json!({"op":"run", "args":{"command":{
                "type":"batch", "commands":[
                    {"type":"draw", "kind":"line"}, {"type":"draw", "kind":"arc"},
                    {"type":"draw", "kind":"circle"}, {"type":"draw", "kind":"point"},
                    {"type":"draw", "kind":"point"}
                ]
            }}}),
        )
        .detail
        .unwrap();
        assert_eq!(many.parts.len(), 3);
        assert_eq!(many.params["more"], 2);
    }

    #[test]
    fn sheet_details_use_the_result_name_paper_and_scale() {
        let input = json!({"op":"run", "args":json!({"command":json!({
            "type":"make_sheet", "name":"A-101 Ground floor"
        }).to_string()}).to_string()});
        let running = describe(crate::surface::EXECUTE, &input).detail.unwrap();
        assert_eq!(running.params["name"], "A-101 Ground floor");
        assert!(!running.params.contains_key("paper"));
        let done = refine(crate::surface::EXECUTE, &input, &json!({"outcome":{
            "type":"sheet", "report":{"layout":"A-101 Ground floor", "paper":"A1", "scale":"1:100"}
        }})).detail.unwrap();
        assert_eq!(done.key, "activity.line.make");
        assert_eq!(done.params["paper"], "A1");
        assert_eq!(done.params["scale"], "1:100");
    }

    #[test]
    fn records_name_the_collection_type_layer_and_count() {
        let input = json!({"op":"records", "args":{"collection":"entities", "type":"Wall", "layer":"A-WALL"}});
        let done = refine(crate::surface::READ, &input, &json!({"total":37}))
            .detail
            .unwrap();
        assert_eq!(done.key, "activity.line.read");
        assert_eq!(done.params["what"], "wall");
        assert_eq!(done.params["n"], 37);
        assert_eq!(done.params["layer"], "A-WALL");
        let layers = describe(
            crate::surface::READ,
            &json!({"op":"records", "args":"{\"collection\":\"layers\"}"}),
        )
        .detail
        .unwrap();
        assert_eq!(layers.params["what"], "layer");
        assert!(!layers.params.contains_key("n"));
        let tables = refine(crate::surface::READ, &json!({"op":"run", "args":{"command":{
            "type":"records", "query":{"collection":"schedules", "layer":"A-TABLE", "sheet":"Ground floor"}
        }}}), &json!({"outcome":{"total":6}})).detail.unwrap();
        assert_eq!(tables.params["what"], "table");
        assert_eq!(tables.params["sheet"], "Ground floor");
        assert_eq!(tables.params["layer"], "A-TABLE");
        assert_eq!(tables.params["n"], 6);
    }

    #[test]
    fn commands_lookup_and_export_name_their_objects() {
        let lookup = describe(
            crate::surface::READ,
            &json!({"op":"commands", "args":{"names":["make_sheet", "table"]}}),
        )
        .detail
        .unwrap();
        assert_eq!(lookup.key, "activity.line.lookup");
        assert_eq!(lookup.params["name"], "make_sheet, table");
        let export = refine(
            crate::surface::EXECUTE,
            &json!({"op":"export_takeoff", "args":{}}),
            &json!({"written":"outputs/boq.xlsx"}),
        )
        .detail
        .unwrap();
        assert_eq!(export.key, "activity.line.export");
        assert_eq!(export.params["file"], "boq.xlsx");
        let save = refine(
            crate::surface::EXECUTE,
            &json!({"op":"save_drawing", "args":{}}),
            &json!({"path":"drawings/Villa.dwg"}),
        )
        .detail
        .unwrap();
        assert_eq!(save.params["file"], "Villa.dwg");
        let no_prose = refine(
            crate::surface::EXECUTE,
            &json!({"op":"export_takeoff", "args":{}}),
            &json!({"message":"Exported hidden.xlsx"}),
        )
        .detail
        .unwrap();
        assert!(!no_prose.params.contains_key("file"));
    }

    #[test]
    fn walls_plan_and_checks_use_semantic_counts() {
        let walls = refine(
            crate::surface::EXECUTE,
            &json!({"op":"run", "args":{"command":{
                "type":"walls", "runs":[{"points":[[0,0],[1,0]]}, {"points":[[1,0],[1,1]]}],
                "layers":{"outline":"A-WALL"}
            }}}),
            &json!({"outcome":{"handles":[1,2,3,4,5,6]}}),
        )
        .detail
        .unwrap();
        assert_eq!(walls.params["n"], 2);
        assert_eq!(walls.params["layer"], "A-WALL");
        let plan = describe(
            crate::surface::EXECUTE,
            &json!({"op":"plan", "args":{"phases":[
                {"status":"done"}, {"status":"active"}, {"status":"pending"}
            ]}}),
        )
        .detail
        .unwrap();
        assert_eq!(plan.params["n"], 1);
        assert_eq!(plan.params["total"], 3);
        let check = refine(
            crate::surface::READ,
            &json!({"op":"run", "args":{"command":{"type":"space_check"}}}),
            &json!({"outcome":{"result":{"findings":[{},{}]}}}),
        )
        .detail
        .unwrap();
        assert_eq!(check.params["what"], "room_size");
        assert_eq!(check.params["notes"], 2);
    }

    #[test]
    fn command_aliases_and_run_scope_keep_result_details() {
        let d = refine(
            crate::surface::EXECUTE,
            &json!({"op":"run", "args":{
                "layer":"A-WALL", "command":{"type":"draw.line", "a":[0,0], "b":[1,0]}
            }}),
            &json!({"outcome":{"type":"batch", "outcomes":[{"type":"added", "handles":[1]}]}}),
        )
        .detail
        .unwrap();
        assert_eq!(d.key, "activity.line.draw");
        assert_eq!(d.params["what"], "line");
        assert_eq!(d.params["layer"], "A-WALL");
        assert_eq!(d.params["n"], 1);
        let unchanged = refine(
            crate::surface::EXECUTE,
            &json!({"op":"run", "args":{"command":{
                "type":"modify", "kind":"move", "targets":[{"handle":1}]
            }}}),
            &json!({"no_change":true,"changed_count":0}),
        )
        .detail
        .unwrap();
        assert_eq!(unchanged.key, "activity.line.unchanged");
        assert_eq!(unchanged.params["n"], 0);
    }

    #[test]
    fn saved_activity_without_detail_is_still_readable_and_keys_match() {
        let old: Activity =
            serde_json::from_value(json!({"category":"draw", "effect":"edit"})).unwrap();
        assert_eq!(old.detail, None);
        let en: Value = serde_json::from_str(include_str!("../../../src/i18n/en.json")).unwrap();
        let ar: Value = serde_json::from_str(include_str!("../../../src/i18n/ar.json")).unwrap();
        for (source, other) in [(&en, &ar), (&ar, &en)] {
            for key in source
                .as_object()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with("activity."))
            {
                assert!(other[key].is_string(), "Missing translation: {key}");
            }
        }
        fn check_keys(detail: &Detail, en: &Value, ar: &Value) {
            if detail.key != "activity.line.batch" {
                for status in ["running", "done"] {
                    let key = format!("{}.{status}", detail.key);
                    assert!(en[&key].is_string(), "English: {key}");
                    assert!(ar[&key].is_string(), "Arabic: {key}");
                }
            }
            for part in &detail.parts {
                check_keys(part, en, ar);
            }
        }
        for command in registry().commands() {
            for example in &command.examples {
                let activity = describe(
                    crate::surface::EXECUTE,
                    &json!({"op":"run", "args":{"command":example}}),
                );
                if let Some(detail) = activity.detail {
                    check_keys(&detail, &en, &ar);
                }
            }
        }
    }
}
