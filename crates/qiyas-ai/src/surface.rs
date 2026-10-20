//! Mizan's and MCP's tools: three generic tools over the self-describing core, the way Open CAD
//! Studio's automation works (read for ideas only; nothing copied). Nothing here is written per
//! feature: a command added to the core with its doc comment, category and example is offered
//! here at once.
//!
//! - `qiyas_read {op, args}`: the state, what Qiyas can do (`commands`: the registry's index,
//!   one command's schema and examples, or a search), records and their schemas, queries
//!   (entities near a point, in a window, texts, tables, dimensions, blocks, rooms, a boundary),
//!   measuring without a take-off row, running any read-only command, the drawing check, the
//!   history, the drawings and what the user selected.
//! - `qiyas_execute {op, args}`: any command (`run`), several as one undo step with each able to
//!   use what an earlier one made (`batch`), records set and entities made, undoing its own step,
//!   selecting and viewing, the drawing files and the BOQ files.
//! - `qiyas_capture {source, args}`: a picture of the screen or of any part of a sheet.
//!
//! `args` is a JSON object written as a string, so each tool's schema stays strict and closed for
//! every provider (Claude's strict mode refuses open schemas, Gemini empty objects); an object is
//! taken too. After a change, the answer says what was added, changed and removed, the step, the
//! drawing check on what it touched, and what the window should do (`ui`).

use serde_json::{json, Value};

use qiyas_core::registry::{effect_of, registry, Effect};
use qiyas_core::{Command, CoreError, Outcome, Session, Source};

use crate::execute::{execute, ToolError, ToolOutput};
use crate::tools::ToolSpec;

pub const READ: &str = "qiyas_read";
pub const EXECUTE: &str = "qiyas_execute";
pub const CAPTURE: &str = "qiyas_capture";

/// The read ops, in the order the schema lists them.
pub const READ_OPS: [&str; 13] = [
    "state",
    "capabilities",
    "commands",
    "records",
    "record_schema",
    "query",
    "measure",
    "run",
    "check",
    "history",
    "drawings",
    "selection",
    "rooms",
];

pub const EXECUTE_OPS: [&str; 16] = [
    "run",
    "batch",
    "set_records",
    "create_entities",
    "undo_own",
    "select",
    "view",
    "new_drawing",
    "open_drawing",
    "switch_drawing",
    "save_drawing",
    "close_drawing",
    "import_boq",
    "compare",
    "export_takeoff",
    "export_conflicts",
];

pub const QUERY_KINDS: [&str; 9] = [
    "near",
    "window",
    "entity",
    "texts",
    "tables",
    "dimensions",
    "blocks",
    "boundary",
    "layers",
];

/// The three tools.
pub fn tools() -> Vec<ToolSpec> {
    let args = json!({
        "type": "string",
        "description": "The op's arguments as a JSON object written as a string, e.g. \"{\\\"name\\\": \\\"draw.line\\\"}\"; \"{}\" for none."
    });
    vec![
        ToolSpec {
            name: READ,
            description: "Read anything, change nothing. ops: state (units, sheet, scale, current layer, take-off and BOQ); capabilities; commands ({} the index by category, {name} one command's schema and examples, {search}); records ({collection, type, layer, handles, names, where, fields, detail}); record_schema ({name}: an entity type, layer, text_style, header); query ({kind: near|window|entity|texts|tables|dimensions|blocks|boundary|layers, …}); measure ({handle} or {points}: numbers only, no take-off row); run ({command}: any command that only reads); check ({handles, rules}: the drawing check); history; drawings; selection; rooms ({text_layer|seeds}).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "op": { "type": "string", "enum": READ_OPS },
                    "args": args,
                },
                "required": ["op", "args"],
                "additionalProperties": false,
            }),
            mutating: false,
        },
        ToolSpec {
            name: EXECUTE,
            description: "Change the drawing, take-off or view. ops: run ({command}: any command of the index, e.g. {\"command\": {\"type\": \"draw\", \"kind\": \"line\", \"a\": [0,0], \"b\": [5,0]}}); batch ({commands: [...], layer, color}: one undo step, all or nothing; a value {\"$step\": i, \"pointer\": \"/added/0\"} takes what step i gave); set_records ({collection, updates: [{key, path, value}]}); create_entities ({entities: [{type, data}]}); undo_own ({}); select ({handles, zoom?}); view ({handles} or {bounds}, optional sheet); new_drawing ({units: \"m\"|\"cm\"|\"mm\"}); open_drawing ({path}); switch_drawing ({id}: tab id from qiyas_read drawings); save_drawing ({}: saves the current drawing); close_drawing ({id?}: defaults to current tab); import_boq ({path}); compare ({}); export_takeoff and export_conflicts ({path?}: workbook in the drawing's folder, a default name when omitted). Core edits are undoable and answer with what they added, changed and removed and a check of what they touched.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "op": { "type": "string", "enum": EXECUTE_OPS },
                    "args": args,
                },
                "required": ["op", "args"],
                "additionalProperties": false,
            }),
            mutating: true,
        },
        ToolSpec {
            name: CAPTURE,
            description: "Look: source screen is what the user sees now; source picture draws a sheet or part of it ({sheet, bounds, width, highlight, marks}). With marks, entities get numbered labels whose handles the caption lists: act on those handles, never on ones guessed from a picture. Zoom in with bounds until the text reads.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "enum": ["screen", "picture"] },
                    "args": args,
                },
                "required": ["source", "args"],
                "additionalProperties": false,
            }),
            mutating: false,
        },
    ]
}

/// Whether `name` is one of the three tools.
pub fn is_surface_tool(name: &str) -> bool {
    matches!(name, READ | EXECUTE | CAPTURE)
}

/// The operation's label for the window's tool card.
pub fn label(name: &str, input: &Value) -> Option<String> {
    if name == CAPTURE {
        return Some("capture".to_string());
    }
    if !is_surface_tool(name) {
        return None;
    }
    let op = input.get("op")?.as_str()?;
    if name == EXECUTE && op == "run" {
        let args = args_of(input).ok()?;
        let command = command_value(&args["command"]).ok()?;
        return command_info(&command).map(|info| info.name.clone());
    }
    Some(op.to_string())
}

/// Execute operations change things, except selecting and viewing.
pub fn mutating(name: &str, input: &Value) -> bool {
    name == EXECUTE && !matches!(input["op"].as_str(), Some("select" | "view"))
}

/// Counts destructive core commands before a call runs, including commands inside a batch.
/// It reads their registry metadata without deserializing unresolved step references.
pub fn destructive_count(name: &str, input: &Value) -> usize {
    if name != EXECUTE {
        return 0;
    }
    let Ok(args) = args_of(input) else {
        return 0;
    };
    fn count(raw: &Value) -> usize {
        let Ok(command) = command_value(raw) else {
            return 0;
        };
        let Some(info) = command_info(&command) else {
            return 0;
        };
        usize::from(info.destructive)
            + if info.name == "batch" {
                command["commands"]
                    .as_array()
                    .map_or(0, |commands| commands.iter().map(count).sum())
            } else {
                0
            }
    }
    match input["op"].as_str() {
        Some("run") => count(&args["command"]),
        Some("batch") => args["commands"]
            .as_array()
            .map_or(0, |commands| commands.iter().map(count).sum()),
        Some("set_records" | "create_entities") => registry()
            .get(input["op"].as_str().unwrap_or_default())
            .map_or(0, |info| usize::from(info.destructive)),
        _ => 0,
    }
}

/// Where a call goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    /// Run on the session (`run_session`).
    Session {
        tool: String,
        op: String,
        args: Value,
    },
    /// A window tool (`crate::window`) with its input.
    Window { tool: &'static str, input: Value },
    /// A file tool (`crate::host`) with its input.
    Host { tool: &'static str, input: Value },
}

/// The op's arguments: an object, a JSON object written as a string, or nothing.
pub fn args_of(input: &Value) -> Result<Value, String> {
    match input.get("args") {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::Object(_)) => Ok(input["args"].clone()),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(json!({})),
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Ok(Value::Object(map)),
            Ok(_) => Err("args must be a JSON object, e.g. \"{}\"".to_string()),
            Err(e) => Err(format!("args is not valid JSON: {e}")),
        },
        Some(_) => Err("args must be a JSON object written as a string".to_string()),
    }
}

/// Where the call `name` with `input` goes.
pub fn route(name: &str, input: &Value) -> Result<Route, String> {
    let args = args_of(input)?;
    let field = if name == CAPTURE { "source" } else { "op" };
    let op = input
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} needs {field}"))?
        .to_string();
    let host = |tool: &'static str| {
        Ok(Route::Host {
            tool,
            input: args.clone(),
        })
    };
    let window = |tool: &'static str| {
        Ok(Route::Window {
            tool,
            input: args.clone(),
        })
    };
    match (name, op.as_str()) {
        (READ, "drawings") => host("list_drawings"),
        (READ, "selection") => window("get_selection"),
        (EXECUTE, "select") => window("select_entities"),
        (EXECUTE, "new_drawing") => host("new_drawing"),
        (EXECUTE, "open_drawing") => host("open_drawing"),
        (EXECUTE, "switch_drawing") => host("switch_drawing"),
        (EXECUTE, "save_drawing") => host("save_drawing"),
        (EXECUTE, "close_drawing") => host("close_drawing"),
        (CAPTURE, "screen") => window("view_screen"),
        (READ, op) if READ_OPS.contains(&op) => Ok(Route::Session {
            tool: READ.into(),
            op: op.into(),
            args,
        }),
        (EXECUTE, op) if EXECUTE_OPS.contains(&op) => Ok(Route::Session {
            tool: EXECUTE.into(),
            op: op.into(),
            args,
        }),
        (CAPTURE, "picture") => Ok(Route::Session {
            tool: CAPTURE.into(),
            op: "picture".into(),
            args,
        }),
        (READ, op) => Err(format!(
            "qiyas_read has no op {op}: {}",
            READ_OPS.join(", ")
        )),
        (EXECUTE, op) => Err(format!(
            "qiyas_execute has no op {op}: {}",
            EXECUTE_OPS.join(", ")
        )),
        (CAPTURE, op) => Err(format!(
            "qiyas_capture's source is screen or picture, not {op}"
        )),
        _ => Err(format!("No tool named {name}")),
    }
}

// ---- On the session ----

/// Most findings of the check a change answers with.
const CHECK_FINDINGS: usize = 8;
/// A sheet with more entities than this is not checked after every change (ask for the check).
const CHECK_UP_TO: usize = 20_000;
/// Most commands one batch takes.
const MAX_BATCH: usize = 200;
/// Most commands a search lists.
const SEARCH_LIMIT: usize = 15;

fn bad(why: impl Into<String>) -> ToolError {
    ToolError::BadInput(why.into())
}

/// Reads `args` as a `T`, naming the field that does not fit.
fn read_args<T: serde::de::DeserializeOwned>(args: &Value, what: &str) -> Result<T, ToolError> {
    serde_json::from_value(args.clone()).map_err(|e| bad(format!("{what}: {e}")))
}

/// Runs a session op of `tool` for `source`.
pub fn run_session(
    session: &mut Session,
    tool: &str,
    op: &str,
    args: &Value,
    source: Source,
) -> Result<ToolOutput, ToolError> {
    let json = |v: Value| Ok(ToolOutput::Json(v));
    match (tool, op) {
        (READ, "state") => json(state(session)),
        (READ, "capabilities") => json(capabilities()),
        (READ, "commands") => json(commands(args)?),
        (READ, "records") => {
            let query: qiyas_core::records::RecordQuery = read_args(args, "records")?;
            json(serde_json::to_value(session.records(&query)?).unwrap_or_default())
        }
        (READ, "record_schema") => {
            let name = args["name"]
                .as_str()
                .ok_or_else(|| bad("record_schema needs {name}"))?;
            json(qiyas_core::record_edit::record_schema(name)?)
        }
        (READ, "query") => query(session, args, source),
        (READ, "measure") => json(measure(session, args)?),
        (READ, "run") => {
            let command = parse_command(session, &args["command"])?;
            if effect_of(&command) != Effect::Read {
                return Err(bad(format!(
                    "{} changes things: run it with qiyas_execute run",
                    command.label()
                )));
            }
            let outcome = session.run_as(command, source)?;
            json(json!({ "outcome": outcome }))
        }
        (READ, "check") => {
            let scope: qiyas_core::check::CheckScope = read_args(args, "check")?;
            let Outcome::Checked { report } =
                session.run_as(Command::CheckDrawing { scope }, source)?
            else {
                return Err(ToolError::Failed(
                    "The drawing check returned no report".into(),
                ));
            };
            json(json!(report))
        }
        (READ, "history") => {
            let (undo, redo) = session.history_steps();
            json(json!({ "undo": undo, "redo": redo }))
        }
        (READ, "rooms") => {
            let query: qiyas_core::rooms::RoomsQuery = read_args(args, "rooms")?;
            let Outcome::Rooms { report } = session.run_as(Command::Rooms { query }, source)?
            else {
                return Err(ToolError::Failed(
                    "The room query returned no report".into(),
                ));
            };
            json(json!(report))
        }
        (EXECUTE, "run") => {
            let command = parse_command(session, &args["command"])?;
            json(run_one(session, command, source)?)
        }
        (EXECUTE, "set_records") => {
            let mut body = args.clone();
            body["type"] = json!("set_records");
            let command = parse_command(session, &body)?;
            json(run_one(session, command, source)?)
        }
        (EXECUTE, "create_entities") => {
            let mut body = args.clone();
            body["type"] = json!("create_entities");
            let command = parse_command(session, &body)?;
            json(run_one(session, command, source)?)
        }
        (EXECUTE, "batch") => json(batch(session, args, source)?),
        (EXECUTE, "undo_own") => {
            let label = session.history_steps().0.last().map(|s| s.label.clone());
            let view = ViewState::of(session);
            let undone = session.undo_if(source);
            let mut out = json!({
                "undone": undone,
                "step": if undone { label } else { None },
                "why": if undone { Value::Null } else { json!("The last step is not yours, or there is none") },
                "ui": { "action": "redraw" },
            });
            view.update_ui(session, &mut out);
            json(out)
        }
        (EXECUTE, "view") => {
            if let Some(sheet) = args.get("sheet").and_then(Value::as_str) {
                execute(session, "set_sheet", &json!({ "sheet": sheet }), source)?;
            }
            let mut zoom = json!({});
            for k in ["handles", "bounds"] {
                if let Some(v) = args.get(k) {
                    zoom[k] = v.clone();
                }
            }
            if zoom.as_object().is_some_and(|m| m.is_empty()) {
                return json(
                    json!({ "sheet": session.sheet, "ui": { "action": "show_sheet", "sheet": session.sheet } }),
                );
            }
            let mut out = execute(session, "zoom_to", &zoom, source)?;
            if args.get("sheet").and_then(Value::as_str).is_some() {
                if let ToolOutput::Json(value) = &mut out {
                    value["sheet"] = json!(session.sheet);
                    value["ui"]["sheet"] = json!(session.sheet);
                }
            }
            Ok(out)
        }
        (EXECUTE, "import_boq") => execute(session, "import_boq", args, source),
        (EXECUTE, "compare") => execute(session, "compare_boq", args, source),
        (EXECUTE, "export_takeoff") | (EXECUTE, "export_conflicts") => {
            json(export(session, op, args)?)
        }
        (CAPTURE, "picture") => execute(session, "view_image", args, source),
        _ => Err(bad(format!("{tool} has no op {op}"))),
    }
}

/// What is true now: the drawing, its units and scale, the sheet, the current layer, the
/// take-off and the BOQ, the last step.
fn state(session: &Session) -> Value {
    let info = session.info();
    let (scale, factor) = session.annotation_scale();
    let to_m = session.drawing.units.to_metres();
    json!({
        "file": session.drawing.path.file_name().map(|f| f.to_string_lossy().to_string()),
        "units": info.units,
        "metres_per_unit": to_m,
        "text_height_2_5mm": 0.0025 / to_m * factor,
        "annotation_scale": scale,
        "sheet": session.sheet,
        "sheets": info.sheets,
        "current": session.current,
        "isolated_layer": info.isolated_layer,
        "entities_on_sheet": session.drawing.entities_in(&session.sheet).count(),
        "takeoff_rows": session.takeoff.items.len(),
        "boq_items": session.boq.as_ref().map(|b| b.items.len()),
        "conflicts": session.conflicts.len(),
        "last_step": session.history_steps().0.last(),
        "unsaved_changes": session.is_dirty(),
    })
}

/// What Qiyas can do, read from the registry.
fn capabilities() -> Value {
    let mut categories: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for c in registry()
        .commands()
        .iter()
        .filter(|c| !c.internal && !c.name.contains('.'))
    {
        categories
            .entry(c.category.clone())
            .or_default()
            .push(c.name.clone());
    }
    json!({
        "read_ops": READ_OPS,
        "execute_ops": EXECUTE_OPS,
        "capture": ["screen", "picture"],
        "query_kinds": QUERY_KINDS,
        "record_collections": ["entities", "layers", "linetypes", "text_styles", "dim_styles", "blocks", "layouts", "viewports", "groups", "views", "header", "layer_states", "xrefs", "takeoff", "boq", "conflicts", "schedules", "comments", "constraints", "parameters"],
        "creatable_entity_types": qiyas_core::record_edit::CREATABLE,
        "check_rules": qiyas_core::check::RULES,
        "commands_by_category": categories,
        "limits": { "records_page": 20, "records_page_max": 200, "result_bytes": 40_960, "batch_commands": MAX_BATCH },
    })
}

/// `commands`: the index, one command, a category or a search.
fn commands(args: &Value) -> Result<Value, ToolError> {
    let reg = registry();
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        let info = reg.get(name).filter(|c| !c.internal).ok_or_else(|| {
            bad(format!(
                "No command {name}; close ones: {}",
                close_commands(name).join(", ")
            ))
        })?;
        return Ok(json!({
            "name": info.name,
            "category": info.category,
            "doc": info.doc,
            "effect": info.effect,
            "undo": info.undo,
            "destructive": info.destructive,
            "batchable": info.batchable,
            "needs": info.needs,
            "kinds": info.kinds,
            "schema": info.schema,
            "examples": info.examples.iter().take(2).collect::<Vec<_>>(),
        }));
    }
    let list = |cs: Vec<&qiyas_core::registry::CommandInfo>| -> Vec<Value> {
        cs.into_iter()
            .map(|c| json!({ "name": c.name, "summary": c.summary, "effect": c.effect }))
            .collect()
    };
    if let Some(query) = args.get("search").and_then(Value::as_str) {
        return Ok(json!({ "commands": list(reg.search(query, SEARCH_LIMIT)) }));
    }
    if let Some(category) = args.get("category").and_then(Value::as_str) {
        let cs = reg
            .commands()
            .iter()
            .filter(|c| !c.internal && c.category.eq_ignore_ascii_case(category))
            .collect();
        return Ok(json!({ "commands": list(cs) }));
    }
    Ok(json!({
        "index": reg.index_text(),
        "next": "qiyas_read commands {\"name\": \"…\"} gives a command's schema and examples",
    }))
}

/// `query`: entities near a point or in a window, one entity, texts, tables, dimensions,
/// blocks, the boundary round a point, the layers.
fn query(session: &mut Session, args: &Value, source: Source) -> Result<ToolOutput, ToolError> {
    let kind = args["kind"]
        .as_str()
        .ok_or_else(|| bad(format!("query needs {{kind}}: {}", QUERY_KINDS.join(", "))))?;
    let mut rest = args.clone();
    if let Some(map) = rest.as_object_mut() {
        map.remove("kind");
    }
    match kind {
        "near" => {
            let p: [f64; 2] = read_args(&args["point"], "query near: point")?;
            let r = args["radius"]
                .as_f64()
                .unwrap_or(0.5 / session.drawing.units.to_metres());
            let mut find = json!({ "within": [[p[0] - r, p[1] - r], [p[0] + r, p[1] + r]] });
            for k in ["layer", "type", "limit"] {
                if let Some(v) = args.get(k) {
                    find[if k == "type" { "kind" } else { k }] = v.clone();
                }
            }
            execute(session, "find_entities", &find, source)
        }
        "window" => {
            let mut find = json!({ "within": args.get("bounds").cloned().unwrap_or(Value::Null) });
            for k in ["layer", "type", "limit", "offset", "sheet"] {
                if let Some(v) = args.get(k) {
                    find[if k == "type" { "kind" } else { k }] = v.clone();
                }
            }
            execute(session, "find_entities", &find, source)
        }
        "entity" => execute(session, "get_entity", &rest, source),
        "texts" => execute(session, "read_text", &rest, source),
        "tables" => execute(session, "read_tables", &rest, source),
        "dimensions" => execute(session, "read_dimensions", &rest, source),
        "blocks" => execute(session, "block_summary", &rest, source),
        "layers" => execute(session, "list_layers", &rest, source),
        "boundary" => {
            let p: [f64; 2] = read_args(&args["point"], "query boundary: point")?;
            let to_m = session.drawing.units.to_metres();
            match session.boundary_at(p, 1.0 / to_m) {
                qiyas_core::Boundary::Ring {
                    ring,
                    holes,
                    bulges,
                } => {
                    let area = ring_area(&ring, bulges.first().map_or(&[][..], Vec::as_slice));
                    Ok(ToolOutput::Json(json!({
                        "ring": ring,
                        "bulges": bulges.first(),
                        "islands": holes.len(),
                        "area_m2": area * to_m * to_m,
                    })))
                }
                qiyas_core::Boundary::None => Err(bad("No closed region round that point")),
                qiyas_core::Boundary::TooDense => Err(bad(
                    "Too much line work round that point: zoom in (a smaller region)",
                )),
            }
        }
        other => Err(bad(format!(
            "query has no kind {other}: {}",
            QUERY_KINDS.join(", ")
        ))),
    }
}

/// `measure`: the length and area of an entity or of points, in metres, without a take-off row.
fn measure(session: &Session, args: &Value) -> Result<Value, ToolError> {
    let to_m = session.drawing.units.to_metres();
    if let Some(h) = args.get("handle").and_then(Value::as_u64) {
        let chain: Vec<u64> = args
            .get("chain")
            .map(|c| read_args(c, "chain"))
            .transpose()?
            .unwrap_or_default();
        let info = session.entity_info(h, &chain)?;
        return Ok(json!({
            "handle": h,
            "length_m": info.length.map(|length| length * to_m),
            "area_m2": info.area.map(|area| area * to_m * to_m),
            "entity": info,
            "units": session.info().units,
        }));
    }
    let points: Vec<[f64; 2]> = read_args(&args["points"], "measure: points")?;
    if points.len() < 2 {
        return Err(bad("measure needs {handle} or {points} (2 or more)"));
    }
    let length: f64 = points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
    let closed = args["closed"].as_bool().unwrap_or(false);
    let mut out = json!({ "length_m": length * to_m });
    if closed && points.len() >= 3 {
        let ring_len = length + {
            let (a, b) = (points[points.len() - 1], points[0]);
            (b[0] - a[0]).hypot(b[1] - a[1])
        };
        out["perimeter_m"] = json!(ring_len * to_m);
        out["area_m2"] = json!(ring_area(&points, &[]) * to_m * to_m);
    }
    Ok(out)
}

fn ring_area(ring: &[[f64; 2]], bulges: &[f64]) -> f64 {
    let n = ring.len();
    let mut twice = 0.0;
    let mut segments = 0.0;
    for i in 0..n {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        twice += p[0] * q[1] - q[0] * p[1];
        let b = bulges.get(i).copied().unwrap_or(0.0);
        if b != 0.0 {
            let chord = (q[0] - p[0]).hypot(q[1] - p[1]);
            let angle = 4.0 * b.abs().atan();
            let radius = chord * (1.0 + b * b) / (4.0 * b.abs());
            segments += radius * radius / 2.0 * (angle - angle.sin()) * b.signum();
        }
    }
    (twice / 2.0 + segments).abs()
}

/// A command from the model: an object or a JSON object written as a string. A wrong one is
/// answered with where it went wrong, the command's schema and an example; an unknown type with
/// the close ones. File paths start in the drawing's folder and may not leave it. Commands
/// Qiyas runs itself (`x-internal`) are refused.
pub fn parse_command(session: &Session, raw: &Value) -> Result<Command, ToolError> {
    let mut value = command_value(raw)?;
    let kind = value["type"].as_str().unwrap_or_default().to_string();
    let Some(info) = command_info(&value) else {
        return Err(bad(format!(
            "No command {kind}; close ones: {}",
            close_commands(&kind).join(", ")
        )));
    };
    if info.internal {
        return Err(bad(format!(
            "{kind} is run by Qiyas itself (import_boq and compare do it)"
        )));
    }
    // Use the registry's spelling after a lookup that accepts case and hyphens.
    let mut parts = info.name.split('.');
    value["type"] = json!(parts.next().unwrap_or_default());
    if let Some(kind) = parts.next() {
        value["kind"] = json!(kind);
    }
    if info.name == "batch" {
        if let Some(commands) = value["commands"].as_array_mut() {
            if commands.is_empty() || commands.len() > MAX_BATCH {
                return Err(bad(format!("A batch takes 1 to {MAX_BATCH} commands")));
            }
            for (index, command) in commands.iter_mut().enumerate() {
                let parsed = parse_command(session, command)
                    .map_err(|error| bad(format!("Batch step {index}: {error}")))?;
                *command = json!(parsed);
            }
        }
    }
    resolve_paths(session, &mut value)?;
    qiyas_core::registry::parse_command(value.clone()).map_err(|error| {
        let path = input_path(&info.schema, &value, "");
        bad(format!(
            "{}: {error}{}. Its schema: {} Example: {}",
            info.name,
            path.map_or_else(String::new, |path| format!(" (field {path})")),
            info.schema,
            info.examples.first().cloned().unwrap_or_default()
        ))
    })
}

/// Reads a command object before binding its fields. Its type and kind must be literal.
fn command_value(raw: &Value) -> Result<Value, ToolError> {
    let value = match raw {
        Value::String(text) => serde_json::from_str::<Value>(text)
            .map_err(|e| bad(format!("command is not valid JSON: {e}")))?,
        Value::Null => {
            return Err(bad(
                "Give {\"command\": {\"type\": …}}; qiyas_read commands lists them",
            ))
        }
        other => other.clone(),
    };
    if value.get("type").and_then(Value::as_str).is_none() {
        return Err(bad(
            "A command needs a literal \"type\"; qiyas_read commands lists them",
        ));
    }
    if value.get("kind").is_some_and(|kind| !kind.is_string()) {
        return Err(bad("A command's \"kind\" must be a literal string"));
    }
    Ok(value)
}

fn command_info(value: &Value) -> Option<&'static qiyas_core::registry::CommandInfo> {
    let kind = value.get("type")?.as_str()?;
    let reg = registry();
    let name = match value.get("kind").and_then(Value::as_str) {
        Some(k) if reg.get(&format!("{kind}.{k}")).is_some() => format!("{kind}.{k}"),
        _ => kind.to_string(),
    };
    reg.get(&name)
}

/// Close wire names, including a typo that a full-text search cannot match.
fn close_commands(name: &str) -> Vec<&'static str> {
    fn distance(a: &str, b: &str) -> usize {
        let mut row: Vec<usize> = (0..=b.chars().count()).collect();
        for (i, a) in a.chars().enumerate() {
            let mut previous = row[0];
            row[0] = i + 1;
            for (j, b) in b.chars().enumerate() {
                let above = row[j + 1];
                row[j + 1] = (row[j] + 1)
                    .min(above + 1)
                    .min(previous + usize::from(a != b));
                previous = above;
            }
        }
        *row.last().unwrap_or(&0)
    }
    let name = name.trim().to_ascii_lowercase().replace('-', "_");
    let mut ranked: Vec<_> = registry()
        .commands()
        .iter()
        .filter(|command| !command.internal)
        .map(|command| (distance(&name, &command.name), command.name.as_str()))
        .collect();
    ranked.sort_unstable();
    ranked.into_iter().take(5).map(|(_, name)| name).collect()
}

/// Serde's tagged enums sometimes lose their inner field path. The schema keeps that path.
fn input_path(schema: &Value, value: &Value, path: &str) -> Option<String> {
    for choices in ["anyOf", "oneOf"] {
        if let Some(choices) = schema[choices].as_array() {
            let errors: Vec<_> = choices
                .iter()
                .map(|choice| input_path(choice, value, path))
                .collect();
            if errors.iter().any(Option::is_none) {
                return None;
            }
            return errors.into_iter().flatten().max_by_key(String::len);
        }
    }
    let matches_type = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    };
    if schema["type"]
        .as_str()
        .is_some_and(|kind| !matches_type(kind))
        || schema["type"]
            .as_array()
            .is_some_and(|kinds| !kinds.iter().filter_map(Value::as_str).any(matches_type))
        || schema
            .get("const")
            .is_some_and(|expected| value != expected)
        || schema["enum"]
            .as_array()
            .is_some_and(|choices| !choices.contains(value))
    {
        return Some(if path.is_empty() {
            "/".into()
        } else {
            path.into()
        });
    }
    if let Some(properties) = schema["properties"].as_object() {
        for (key, child) in properties {
            let child_path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            if let Some(value) = value.get(key) {
                if let Some(path) = input_path(child, value, &child_path) {
                    return Some(path);
                }
            } else if schema["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|name| name == key))
            {
                return Some(child_path);
            }
        }
    }
    if let Some(values) = value.as_array() {
        for (i, value) in values.iter().enumerate() {
            let child = schema["prefixItems"].get(i).or_else(|| schema.get("items"));
            if let Some(child) = child {
                if let Some(path) = input_path(child, value, &format!("{path}/{i}")) {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// File fields belong to the command, not arbitrary record data or JSON pointer paths.
fn resolve_paths(session: &Session, value: &mut Value) -> Result<(), ToolError> {
    let kind = value["type"].as_str().unwrap_or_default();
    if matches!(
        kind,
        "batch" | "set_records" | "records" | "create_entities"
    ) {
        return Ok(());
    }
    if let Some(map) = value.as_object_mut() {
        for (key, value) in map {
            if (key == "path" || key.ends_with("_path")) && value.is_string() {
                let full = crate::execute::resolve_in_drawing_folder(
                    session,
                    value.as_str().unwrap_or_default(),
                )?;
                *value = json!(full.to_string_lossy());
            }
        }
    }
    Ok(())
}

/// The view fields the window needs when a command changes them.
struct ViewState {
    sheet: String,
    hidden: Vec<String>,
}

impl ViewState {
    fn of(session: &Session) -> Self {
        Self {
            sheet: session.sheet.clone(),
            hidden: session.hidden_layers(),
        }
    }

    fn update_ui(self, session: &Session, out: &mut Value) {
        if self.sheet != session.sheet {
            out["ui"]["action"] = json!("show_sheet");
            out["ui"]["sheet"] = json!(session.sheet);
        }
        let hidden = session.hidden_layers();
        if self.hidden != hidden {
            if out["ui"]["action"] != "show_sheet" {
                out["ui"]["action"] = json!("redraw");
            }
            out["ui"]["hidden"] = json!(hidden);
        }
    }
}

/// Runs one command and answers with what it did (`changed`).
fn run_one(session: &mut Session, command: Command, source: Source) -> Result<Value, ToolError> {
    let label = command.label();
    let effect = effect_of(&command);
    let history = session.history_revision();
    let view = ViewState::of(session);
    let outcome = session.run_as(command, source)?;
    let mut out = changed(session, &label, effect, history, json!(outcome));
    if label == "set_sheet" {
        out["ui"] = json!({ "action": "show_sheet", "sheet": session.sheet });
    }
    view.update_ui(session, &mut out);
    Ok(out)
}

/// The answer to a change: the outcome, the entities added, changed and removed, the step, the
/// check of what it touched, and what the window should do.
fn changed(
    session: &Session,
    label: &str,
    effect: Effect,
    history_before: u64,
    outcome: Value,
) -> Value {
    let change = session.last_change().clone();
    let (undo, _) = session.history_steps();
    let step = (session.history_revision() != history_before)
        .then(|| undo.last().cloned())
        .flatten();
    let touched: Vec<u64> = change
        .added
        .iter()
        .chain(&change.changed)
        .copied()
        .collect();
    let mut out = json!({
        "ok": true,
        "command": label,
        "effect": effect,
        "outcome": outcome,
        "added": change.added,
        "changed": change.changed,
        "removed": change.removed,
        "step": step.map(|s| json!({ "label": s.label, "source": s.source, "undoable": true })),
    });
    if !touched.is_empty() {
        if session.drawing.entities_in(&session.sheet).count() <= CHECK_UP_TO {
            let report = session.check_drawing(&qiyas_core::check::CheckScope {
                handles: touched.clone(),
                ..Default::default()
            });
            out["check"] = json!({
                "line": report.line,
                "findings": report.findings.iter().take(CHECK_FINDINGS).collect::<Vec<_>>(),
            });
        } else {
            out["check"] = json!({ "line": "Not checked: a large sheet. qiyas_read check {\"handles\": […]} checks what you changed" });
        }
        out["ui"] = json!({ "action": "reveal", "sheet": session.sheet, "handles": touched });
    } else if !change.removed.is_empty() || effect == Effect::View {
        out["ui"] = json!({ "action": "redraw" });
    }
    out
}

/// `batch`: commands as one undo step, all or nothing, on `layer` in `color` when given; a value
/// `{"$step": i, "pointer": "/added/0"}` anywhere in a command takes what step i answered.
fn batch(session: &mut Session, args: &Value, source: Source) -> Result<Value, ToolError> {
    let commands = args["commands"]
        .as_array()
        .ok_or_else(|| bad("batch needs {commands: [...]}"))?
        .clone();
    if commands.is_empty() || commands.len() > MAX_BATCH {
        return Err(bad(format!("A batch takes 1 to {MAX_BATCH} commands")));
    }
    let layer = args
        .get("layer")
        .and_then(Value::as_str)
        .map(str::to_string);
    let color = args
        .get("color")
        .and_then(Value::as_str)
        .map(str::to_string);
    let history = session.history_revision();
    let view = ViewState::of(session);
    let mut answers: Vec<Value> = Vec::new();
    let ran = session.batch_with(source, "batch", layer, color, |s| {
        for (i, raw) in commands.iter().enumerate() {
            let mut raw = command_value(raw)
                .map_err(|e| CoreError::InvalidCommand(format!("Batch step {i}: {e}")))?;
            bind(&mut raw, &answers)
                .map_err(|why| CoreError::InvalidCommand(format!("Batch step {i}: {why}")))?;
            let command = parse_command(s, &raw)
                .map_err(|e| CoreError::InvalidCommand(format!("Batch step {i}: {e}")))?;
            let label = command.label();
            let (outcome, change) = s
                .transaction(source, &label, |s| {
                    let outcome = s.run_as(command, source)?;
                    Ok((outcome, s.last_change().clone()))
                })
                .map_err(|error| CoreError::Step {
                    index: i,
                    label: label.clone(),
                    error: Box::new(error),
                })?;
            answers.push(json!({
                "command": label,
                "outcome": outcome,
                "added": change.added,
                "changed": change.changed,
                "removed": change.removed,
            }));
        }
        Ok(())
    });
    ran?;
    let mut out = changed(
        session,
        "batch",
        Effect::Edit,
        history,
        json!({ "steps": answers }),
    );
    view.update_ui(session, &mut out);
    Ok(out)
}

/// Replaces each `{"$step": i, "pointer": "/added/0"}` in `value` with what it points at in
/// step i's answer.
fn bind(value: &mut Value, answers: &[Value]) -> Result<(), String> {
    match value {
        Value::Object(map) if map.contains_key("$step") => {
            if map.keys().any(|key| key != "$step" && key != "pointer") {
                return Err("A step reference takes only $step and pointer".into());
            }
            let i = map["$step"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or("$step must be a step number")?;
            let pointer = match map.get("pointer") {
                Some(Value::String(pointer)) => pointer.as_str(),
                None => "/added/0",
                Some(_) => return Err("A step reference's pointer must be a string".into()),
            };
            let answer = answers
                .get(i)
                .ok_or_else(|| format!("$step {i} has not run yet (steps count from 0)"))?;
            *value = answer.pointer(pointer).cloned().ok_or_else(|| {
                format!("step {i}'s answer has nothing at {pointer}: it is {answer}")
            })?;
            Ok(())
        }
        Value::Object(map) => map.values_mut().try_for_each(|v| bind(v, answers)),
        Value::Array(items) => items.iter_mut().try_for_each(|v| bind(v, answers)),
        _ => Ok(()),
    }
}

/// `export_takeoff`, `export_conflicts`: a workbook in the drawing's folder.
fn export(session: &Session, op: &str, args: &Value) -> Result<Value, ToolError> {
    let stem = session
        .drawing
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "drawing".to_string());
    let default = if op == "export_takeoff" {
        format!("{stem} take-off.xlsx")
    } else {
        format!("{stem} conflicts.xlsx")
    };
    let name = args.get("path").and_then(Value::as_str).unwrap_or(&default);
    let path = crate::execute::resolve_in_drawing_folder(session, name)?;
    if op == "export_takeoff" {
        let totals = session.takeoff.totals();
        qiyas_boq::export_takeoff(&path, &session.takeoff.items, &totals, &stem)
            .map_err(|e| ToolError::Failed(e.to_string()))?;
    } else {
        qiyas_boq::report::export_conflicts(&path, &session.conflicts)
            .map_err(|e| ToolError::Failed(e.to_string()))?;
    }
    Ok(json!({ "written": path.to_string_lossy() }))
}
