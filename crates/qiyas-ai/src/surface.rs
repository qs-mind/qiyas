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
//!   typed command lines (`type`), selecting and viewing, the drawing files and the BOQ files.
//!   `plan` reports task phases without changing the drawing or adding an undo step.
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
pub const READ_OPS: [&str; 22] = [
    "explain",
    "digest",
    "takeoff_kit",
    "workflow",
    "takeoff_check",
    "brief_check",
    "document",
    "state",
    "capabilities",
    "validate",
    "commands",
    "records",
    "record_schema",
    "query",
    "measure",
    "run",
    "check",
    "history",
    "events",
    "drawings",
    "selection",
    "panel_state",
    "rooms",
];

pub const EXECUTE_OPS: [&str; 20] = [
    "plan",
    "finish",
    "run",
    "type",
    "batch",
    "set_records",
    "create_entities",
    "undo_own",
    "redo_own",
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

/// Every operation declares its environment here, shared by Mizan and MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    Nothing,
    Drawing,
    Window,
    Host,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Operation {
    pub tool: &'static str,
    pub op: &'static str,
    pub needs: &'static [Need],
    /// The private runner, when this is a host or window operation.
    pub runner: Option<&'static str>,
}

macro_rules! operations {
    ($(($tool:ident, $op:literal, [$($need:ident),+] $(, $runner:literal)?)),* $(,)?) => {
        pub const OPERATIONS: &[Operation] = &[$(Operation {
            tool: $tool, op: $op, needs: &[$(Need::$need),+], runner: operations!(@runner $($runner)?),
        }),*];
    };
    (@runner $runner:literal) => {Some($runner)};
    (@runner) => {None};
}
operations![
    (READ, "document", [Nothing]),
    (READ, "state", [Drawing]),
    (READ, "explain", [Nothing]),
    (READ, "digest", [Drawing]),
    (READ, "takeoff_kit", [Nothing]),
    (READ, "workflow", [Nothing]),
    (READ, "takeoff_check", [Drawing]),
    (READ, "capabilities", [Nothing]),
    (READ, "commands", [Nothing]),
    (READ, "record_schema", [Nothing]),
    (READ, "validate", [Nothing]),
    (READ, "records", [Drawing]),
    (READ, "query", [Drawing]),
    (READ, "measure", [Drawing]),
    (READ, "run", [Drawing]),
    (READ, "check", [Drawing]),
    (READ, "history", [Drawing]),
    (READ, "events", [Drawing]),
    (READ, "rooms", [Drawing]),
    (READ, "brief_check", [Drawing]),
    (READ, "drawings", [Host], "list_drawings"),
    (READ, "selection", [Window, Drawing], "get_selection"),
    (READ, "panel_state", [Window], "panel_state"),
    (EXECUTE, "plan", [Nothing]),
    (EXECUTE, "run", [Drawing]),
    (EXECUTE, "type", [Drawing]),
    (EXECUTE, "batch", [Drawing]),
    (EXECUTE, "set_records", [Drawing]),
    (EXECUTE, "create_entities", [Drawing]),
    (EXECUTE, "undo_own", [Drawing]),
    (EXECUTE, "redo_own", [Drawing]),
    (EXECUTE, "view", [Drawing]),
    (EXECUTE, "import_boq", [Drawing]),
    (EXECUTE, "compare", [Drawing]),
    (EXECUTE, "export_takeoff", [Drawing]),
    (EXECUTE, "export_conflicts", [Drawing]),
    (EXECUTE, "select", [Window, Drawing], "select_entities"),
    (EXECUTE, "new_drawing", [Host], "new_drawing"),
    (EXECUTE, "open_drawing", [Host], "open_drawing"),
    (EXECUTE, "switch_drawing", [Host], "switch_drawing"),
    (EXECUTE, "finish", [Host], "finish"),
    (EXECUTE, "save_drawing", [Host], "save_drawing"),
    (EXECUTE, "close_drawing", [Host], "close_drawing"),
    (CAPTURE, "screen", [Window, Drawing], "view_screen"),
    (CAPTURE, "picture", [Drawing]),
    (CAPTURE, "document", [Nothing]),
];

pub fn operation(tool: &str, op: &str) -> Option<&'static Operation> {
    OPERATIONS
        .iter()
        .find(|entry| entry.tool == tool && entry.op == op)
}

/// Check only the environment declared by the operation, before asking its runner.
pub fn check_need(
    tool: &str,
    op: &str,
    drawing: bool,
    window: bool,
    host: bool,
) -> Result<(), ToolError> {
    let entry = operation(tool, op).ok_or_else(|| bad(format!("{tool} has no op {op}")))?;
    let missing = entry.needs.iter().find_map(|need| match need {
        Need::Nothing => None,
        Need::Drawing if !drawing => Some(("no_drawing", crate::mcp::NO_DRAWING)),
        Need::Window if !window => Some(("needs_window", crate::host::NO_WINDOW)),
        Need::Host if !host => Some(("needs_host", crate::host::NO_HOST)),
        _ => None,
    });
    match missing {
        Some((code, message)) => Err(ToolError::Structured {
            code: code.into(),
            message: message.into(),
            details: json!({}),
        }),
        None => Ok(()),
    }
}

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
    let mut revision = qiyas_core::revision_schema();
    // Provider strict mode omits numeric bounds; native validation still checks them.
    for field in revision["properties"].as_object_mut().unwrap().values_mut() {
        field.as_object_mut().unwrap().remove("minimum");
    }
    revision["required"] = json!(revision["properties"]
        .as_object()
        .unwrap()
        .keys()
        .collect::<Vec<_>>());
    let args = json!({
        "type": "string",
        "description": "The op's arguments as a JSON object written as a string, e.g. \"{\\\"name\\\": \\\"draw.line\\\"}\"; \"{}\" for none."
    });
    vec![
        ToolSpec {
            name: READ,
            description: "Read anything, change nothing. ops: workflow ({language?: en|ar}: quantity take-off guidance, only for quantity jobs); explain ({item, project?}: generated:<code>, boq:<code>, takeoff:<id>, dimension:<decimal handle>:<rule>; project path and item code for project quantities; returns dimensions, elements, rules, citations and confidence); digest ({offset?}: compact drawing evidence, method, suggested picture and takeoff_kit link; start take-off here); takeoff_kit ({}: all take-off contracts and a complete batch from registry examples); takeoff_check ({offset?, expectations?, end_report?}: explanation coverage, unmatched schedule rules, uncertainties and verify); document ({path, pages?}: PDF page text, one-based pages; scans need a document capture); state (units, sheet, scale, current layer, take-off and BOQ); capabilities; validate ({command} or {commands}: check all inputs without running); commands ({} the index by category, {name} or {names:[...]} schemas and examples in one call, {search}); records ({collection, type, layer, handles, names, where, fields, detail}); record_schema ({name}: an entity type, layer, text_style, header); query ({kind: near|window|entity|texts|tables|dimensions|blocks|boundary|layers, …}); measure ({handle} or {points}: numbers only, no take-off row); run ({command}: any command that only reads); check ({handles, rules}: the drawing check); history; events ({after?, session?}: completed commits, changed handles and a resync flag); drawings; selection; panel_state ({}: visible panels, layer folders, selected rows and open controls; no changes); rooms ({text_layer|seeds}); brief_check ({expectations:[{id,text,verify:{…}} or {id,text,read:{op,args,pointer,expected,tolerance?,subtract?}}]}: check the user’s own brief against measured values, never domain rules).",
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
            description: "Plan the task or change the drawing, take-off or view. ops: plan ({phases: [{id, title, status: pending|active|done|skipped}]}: full checklist, stable ids, one active phase; no drawing change or undo step); type ({line}: a complete typed command, e.g. PLINE 0,0 10,0 C; words and aliases come from commands; an incomplete line returns a prompt, then resend the complete line); run ({command, layer?, color?}: any command of the index, e.g. {\"command\": {\"type\": \"draw\", \"kind\": \"line\", \"a\": [0,0], \"b\": [5,0]}}); batch ({commands: [1 to 200 commands], layer, color}: one undo step, all or nothing; a value {\"$step\": i, \"pointer\": \"/added/0\"} takes what step i gave); set_records ({collection, updates: [{key, path, value}]}); create_entities ({entities: [{type, data}]}); undo_own and redo_own ({}: only your own newest step); select ({handles, zoom?}); view ({handles} or {bounds}, optional sheet); new_drawing ({units: \"m\"|\"cm\"|\"mm\"}); open_drawing ({path}); switch_drawing ({id}: tab id from qiyas_read drawings); save_drawing ({}: saves the current drawing); finish ({answer?}: records completion and a headless end report; never saves the drawing); close_drawing ({id?}: defaults to current tab); import_boq ({path}); compare ({}); export_takeoff and export_conflicts ({path?}: workbook in the drawing's folder, a default name when omitted). Use expected_revision from the read you act on, in args or the envelope. Every result carries revision; stale_read refuses an edit and lists changes. Core edits report added, changed, removed, handle_map and checks. Advice is information; Mizan decides.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "op": { "type": "string", "enum": EXECUTE_OPS },
                    "expected_revision": {"anyOf": [revision, {"type":"null"}]},
                    "args": args,
                },
                "required": ["op", "args", "expected_revision"],
                "additionalProperties": false,
            }),
            mutating: true,
        },
        ToolSpec {
            name: CAPTURE,
            description: "Look: source document renders a PDF page ({path, page, width?}) without changing the drawing; coordinates in its caption are page points for recognition.vision. Source screen is what the user sees now; source picture draws a sheet or part of it ({sheet, bounds, width, highlight, marks}). With marks, entities get numbered labels whose handles the caption lists: act on those handles, never on ones guessed from a picture. Zoom in with bounds until the text reads.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "enum": ["screen", "picture", "document"] },
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
    if name == EXECUTE && op == "type" {
        let args = args_of(input).ok()?;
        let word = args["line"].as_str()?.split_whitespace().next()?;
        return registry().by_word(word).map(|info| info.name.clone());
    }
    if name == EXECUTE && op == "run" {
        let args = args_of(input).ok()?;
        let command = command_value(&args["command"]).ok()?;
        return command_info(&command).map(|info| info.name.clone());
    }
    Some(op.to_string())
}

/// Execute operations change things, except planning, selecting and viewing.
pub fn mutating(name: &str, input: &Value) -> bool {
    name == EXECUTE && !matches!(input["op"].as_str(), Some("plan" | "select" | "view"))
}

/// A full snapshot of the phases in the current task. It belongs to the conversation,
/// not the drawing, and never creates an undo step.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub phases: Vec<PlanPhase>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanPhase {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    Active,
    Done,
    Skipped,
}

pub fn read_plan(args: &Value) -> Result<Plan, ToolError> {
    let mut args = args.clone();
    if let Some(map) = args.as_object_mut() {
        map.remove("expected_revision");
    }
    let plan: Plan = read_args(&args, "plan")?;
    if plan.phases.is_empty() || plan.phases.len() > 24 {
        return Err(bad("A plan needs 1 to 24 phases."));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut active = 0;
    for phase in &plan.phases {
        if phase.id.trim().is_empty() || phase.id.chars().count() > 64 || !ids.insert(&phase.id) {
            return Err(bad(
                "plan phase ids must be unique, nonempty and at most 64 characters",
            ));
        }
        if phase.title.trim().is_empty() || phase.title.chars().count() > 160 {
            return Err(bad("plan phase titles must have 1 to 160 characters"));
        }
        active += usize::from(phase.status == PlanStatus::Active);
    }
    if active > 1 {
        return Err(bad(
            "plan has more than one active phase: work one phase at a time",
        ));
    }
    Ok(plan)
}

pub(super) fn plan_info() -> Value {
    json!({
        "name": "plan",
        "tool": EXECUTE,
        "op": "plan",
        "doc": "Consider small phases in the user's language before drawing: grid, walls, openings, rooms, dimensions, sheet as needed. Use one batch per phase and fix by handle. Send the full list with stable ids on each update; at most one phase is active. Capture and check a phase before marking it done. The conversation keeps the plan; no drawing change or undo step.",
        "changes": false,
        "undo": false,
        "schema": {
            "type": "object", "additionalProperties": false, "required": ["phases"],
            "properties": {"phases": {
                "type": "array", "minItems": 1, "maxItems": 24,
                "items": {"type": "object", "additionalProperties": false,
                    "required": ["id", "title", "status"],
                    "properties": {
                        "id": {"type": "string", "minLength": 1, "maxLength": 64},
                        "title": {"type": "string", "minLength": 1, "maxLength": 160},
                        "status": {"type": "string", "enum": ["pending", "active", "done", "skipped"]}
                    }
                }
            }}
        },
        "examples": [{"phases": [
            {"id": "plan", "title": "Check sizes", "status": "active"},
            {"id": "geometry", "title": "Draw geometry", "status": "pending"},
            {"id": "annotations", "title": "Dimensions and table", "status": "pending"}
        ]}]
    })
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
        Some("type") => args["line"]
            .as_str()
            .and_then(|line| line.split_whitespace().next())
            .and_then(|word| registry().by_word(word))
            .map_or(0, |info| usize::from(info.destructive)),
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

/// External callers must share the host's turn authority before changing the selected sheet
/// or replacing its drawing. Reads and ordinary geometry edits do not need that authority.
pub(crate) fn needs_sheet_authority(route: &Route) -> bool {
    fn changes(raw: &Value) -> bool {
        let Ok(command) = command_value(raw) else {
            return false;
        };
        match command_info(&command).map(|info| info.name.as_str()) {
            Some("set_sheet") => true,
            Some("batch") => command["commands"]
                .as_array()
                .is_some_and(|commands| commands.iter().any(changes)),
            _ => false,
        }
    }
    match route {
        Route::Host { tool, .. } => crate::host::changes_drawing(tool),
        Route::Session { tool, op, args } if tool == EXECUTE => match op.as_str() {
            "run" => changes(&args["command"]),
            "batch" => args["commands"]
                .as_array()
                .is_some_and(|commands| commands.iter().any(changes)),
            "type" => args["line"].as_str().is_some_and(|line| {
                matches!(
                    line.trim().to_ascii_uppercase().as_str(),
                    "UNDO" | "U" | "REDO"
                ) || matches!(
                    qiyas_core::cmdline::Machine::default().step(line),
                    Ok(qiyas_core::cmdline::Step::Command(command)) if changes(&json!(command))
                )
            }),
            "view" => args["sheet"].is_string(),
            "undo_own" | "redo_own" => true,
            _ => false,
        },
        _ => false,
    }
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
    if !is_surface_tool(name) {
        return Err(format!("No tool named {name}"));
    }
    let (normalized, _) =
        crate::normalise::envelope(name, input).map_err(|e| e.json().to_string())?;
    let input = &normalized;
    let field = if name == CAPTURE { "source" } else { "op" };
    let mut envelope = tools()
        .into_iter()
        .find(|tool| tool.name == name)
        .expect("surface tool")
        .input_schema;
    envelope["properties"]["args"] =
        json!({"type":["object","string","null"],"additionalProperties":true});
    envelope["required"] = json!([field]);
    let mut probe = input.clone();
    if probe["args"].is_object() {
        probe["args"] = json!({});
    }
    let report = qiyas_core::validation::validate_schema(&envelope, &probe);
    if !report.valid {
        return Err(ToolError::Structured {
            code: report.errors[0].code.clone(),
            message: format!("{name}: {}. Expected form: {envelope}. Example: {{{field:?}: {:?}, \"args\": \"{{}}\"}}", report.message(), if name == CAPTURE { "picture" } else { "state" }),
            details: json!({"errors":report.errors,"schema":envelope,"example":{field:if name == CAPTURE { "picture" } else { "state" },"args":"{}"}}),
        }.json().to_string());
    }
    let args = args_of(input).map_err(|message| ToolError::Structured {
        code: "invalid_value".into(),
        message: format!("/args: {message}. Expected form: a JSON object or its JSON string. Example: {{\"args\":\"{{}}\"}}"),
        details: json!({"errors":[{"code":"invalid_value","pointer":"/args","message":message}],"schema":envelope["properties"]["args"],"example":{"args":"{}"}}),
    }.json().to_string())?;
    let op = input
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} needs {field}"))?
        .to_string();
    let args = read_handle_values(&args);
    if let Some(entry) = operation(name, &op) {
        let (normalized, _) =
            crate::normalise::args(name, &op, &args).map_err(|e| e.json().to_string())?;
        crate::surface_args::validate(name, &op, &normalized)
            .map_err(|error| error.json().to_string())?;
        if matches!(name, READ | EXECUTE) && op == "run" && args["command"]["type"] == "project" {
            let command = qiyas_core::registry::parse_command(args["command"].clone())?;
            if name == READ && effect_of(&command) != Effect::Read {
                return Err("Use qiyas_execute for a project edit".into());
            }
            let mut input = args["command"].clone();
            if let Some(expected) = args.get("expected_revision") {
                input["expected_revision"] = expected.clone();
            }
            return Ok(Route::Host {
                tool: "project",
                input,
            });
        }
        return Ok(match (entry.needs.first(), entry.runner) {
            (Some(Need::Host), Some(tool)) => Route::Host { tool, input: args },
            (Some(Need::Window), Some(tool)) => Route::Window { tool, input: args },
            _ => Route::Session {
                tool: name.into(),
                op,
                args,
            },
        });
    }
    match (name, op.as_str()) {
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

/// Registry reads and conversation plans work before a drawing has been opened.
pub fn run_without_drawing(
    tool: &str,
    op: &str,
    args: &Value,
    units: Option<qiyas_core::Units>,
) -> Option<Result<ToolOutput, ToolError>> {
    if let Err(error) = crate::surface_args::validate(tool, op, args) {
        return Some(Err(error));
    }
    if tool == EXECUTE && op == "plan" {
        if let Err(error) = crate::revision::check(None, args) {
            return Some(Err(error));
        }
        return Some(read_plan(args).map(|plan| {
            let mut out = json!({"plan":plan,"revision":null});
            if plan.phases.len() == 1 { crate::advice::append(&mut out, json!({"code":"small_phases","message":"Consider smaller phases with a capture and check after each. Mizan decides the plan."})); }
            ToolOutput::Json(out)
        }));
    }
    if tool == CAPTURE && op == "document" {
        return Some(crate::revision::stamp(
            crate::document::capture(args),
            Value::Null,
        ));
    }
    if tool != READ {
        return None;
    }
    let value = match op {
        "explain" if args["project"].is_string() => qiyas_core::trust::explain_project(
            args["project"].as_str().unwrap(),
            args["item"].as_str().unwrap_or_default(),
        )
        .map_err(Into::into),
        "document" => crate::document::read(args),
        "workflow" => Ok(
            json!({"workflow":crate::prompt::takeoff_workflow(crate::prompt::Lang::from_code(args["language"].as_str().unwrap_or("en")))}),
        ),
        "takeoff_kit" if units.is_none() => {
            let mut kit = qiyas_core::takeoff_kit::kit();
            finish_kit(&mut kit);
            Ok(kit)
        }
        "capabilities" => Ok(capabilities()),
        "commands" => commands(args),
        "record_schema" => args["name"]
            .as_str()
            .ok_or_else(|| bad("record_schema needs {name}"))
            .and_then(|name| qiyas_core::record_edit::record_schema(name).map_err(Into::into)),
        "validate" => validate(args, units, None),
        _ => return None,
    };
    Some(crate::revision::stamp(
        value.map(ToolOutput::Json),
        Value::Null,
    ))
}

fn batch_commands_schema() -> Value {
    json!({"type":"array", "minItems":1, "maxItems":MAX_BATCH})
}

fn batch_limits(
    input: &Value,
    prefix: &str,
    report: &mut qiyas_core::validation::ValidationReport,
) {
    if input["type"]
        .as_str()
        .is_none_or(|name| !name.eq_ignore_ascii_case("batch"))
    {
        return;
    }
    let Some(commands) = input["commands"].as_array() else {
        return;
    };
    let size =
        qiyas_core::validation::validate_schema(&batch_commands_schema(), &input["commands"]);
    report.valid &= size.valid;
    report
        .errors
        .extend(size.errors.into_iter().map(|mut issue| {
            issue.pointer = format!("{prefix}/commands{}", issue.pointer);
            issue
        }));
    for (index, command) in commands.iter().enumerate() {
        batch_limits(command, &format!("{prefix}/commands/{index}"), report);
    }
}

fn validate(
    args: &Value,
    units: Option<qiyas_core::Units>,
    session: Option<&Session>,
) -> Result<Value, ToolError> {
    if args.get("command").is_some() && args.get("commands").is_some() {
        return Err(bad("validate takes command or commands, not both"));
    }
    let mut report = if let Some(commands) = args.get("commands") {
        let command = json!({"type":"batch", "commands":commands});
        match session {
            Some(session) => session.validate_input(&command),
            None => qiyas_core::validation::validate_command(&command, units),
        }
    } else if let Some(command) = args.get("command") {
        let command = match command {
            Value::String(text) => serde_json::from_str(text)
                .map_err(|e| bad(format!("command is not valid JSON: {e}")))?,
            other => other.clone(),
        };
        match session {
            Some(session) => session.validate_input(&command),
            None => qiyas_core::validation::validate_command(&command, units),
        }
    } else {
        return Err(bad("validate needs {command} or {commands}"));
    };
    let command = if args.get("commands").is_some() {
        json!({"type":"batch", "commands":args["commands"]})
    } else if let Some(text) = args["command"].as_str() {
        serde_json::from_str(text).unwrap_or_default()
    } else {
        args["command"].clone()
    };
    batch_limits(&command, "", &mut report);
    Ok(json!(report))
}

/// Runs a session op of `tool` for `source`.
pub fn run_session(
    session: &mut Session,
    tool: &str,
    op: &str,
    args: &Value,
    source: Source,
) -> Result<ToolOutput, ToolError> {
    run_session_with_method(
        session,
        tool,
        op,
        args,
        source,
        &crate::MeasurementMethod::default(),
    )
}

/// The in-app agent supplies the selected method without storing app settings in a drawing.
pub fn run_session_with_method(
    session: &mut Session,
    tool: &str,
    op: &str,
    args: &Value,
    source: Source,
    method: &crate::MeasurementMethod,
) -> Result<ToolOutput, ToolError> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (mut args, warnings) = crate::normalise::args(tool, op, args)?;
        let before = session.revision();
        let result = session
            .with_automation_sheet(|session| {
                if tool == EXECUTE {
                    crate::revision::check(Some(session), &args)?;
                    args.as_object_mut().unwrap().remove("expected_revision");
                }
                run_working_session(session, tool, op, &args, source, method)
            })
            .map(|out| crate::normalise::output(out, &warnings));
        if result.is_ok() {
            session.record_tool_revision(before);
        }
        result
    }))
    .unwrap_or_else(|_| {
        Err(ToolError::Failed(
            "The tool stopped unexpectedly. You can keep working.".into(),
        ))
    });
    crate::revision::stamp(result, json!(session.revision()))
}

fn run_working_session(
    session: &mut Session,
    tool: &str,
    op: &str,
    args: &Value,
    source: Source,
    method: &crate::MeasurementMethod,
) -> Result<ToolOutput, ToolError> {
    let normalized = read_handle_values(args);
    let args = &normalized;
    let argument_warnings = crate::surface_args::validate(tool, op, args)?;
    if tool == READ && op == "validate" {
        return validate(args, Some(session.drawing.units), Some(session)).map(ToolOutput::Json);
    }
    if op == "document" && matches!(tool, READ | CAPTURE) {
        let mut args = args.clone();
        if let Some(path) = args["path"].as_str() {
            let path = std::path::Path::new(path);
            if path.is_relative() {
                args["path"] = json!(session
                    .drawing
                    .path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join(path));
            }
        }
        return if tool == READ {
            crate::document::read(&args).map(ToolOutput::Json)
        } else {
            crate::document::capture(&args)
        };
    }
    if let Some(result) = run_without_drawing(tool, op, args, Some(session.drawing.units)) {
        return result;
    }
    let json = |mut v: Value| {
        // Mutating registry operations report their own warnings, with command-relative paths.
        // These read shortcuts have no run_input outcome, so retain their argument warnings here.
        if tool == READ
            && matches!(op, "records" | "check" | "rooms")
            && !argument_warnings.is_empty()
        {
            v["warnings"] = json!(argument_warnings);
        }
        Ok(ToolOutput::Json(v))
    };
    match (tool, op) {
        (READ, "explain") => {
            let item = args["item"]
                .as_str()
                .ok_or_else(|| bad("item is required"))?;
            json(if let Some(path) = args["project"].as_str() {
                qiyas_core::trust::explain_project(path, item)?
            } else {
                session.explain(item)?
            })
        }
        (READ, "brief_check") => json(crate::brief_check::run(session, args, source)?),
        (READ, "document") => json(crate::document::read(args)?),
        (CAPTURE, "document") => crate::document::capture(args),
        (READ, "digest") => {
            let mut digest =
                session.drawing_digest(args["offset"].as_u64().unwrap_or(0) as usize)?;
            let selected = crate::MeasurementMethod::for_session(session, method)?;
            digest["measurement_method"]["name"] = json!(selected.name);
            digest["measurement_method"]["rules_loaded"] = json!(selected.rules_loaded);
            json(digest)
        }
        (READ, "workflow") => json(
            json!({"workflow": crate::prompt::takeoff_workflow(crate::prompt::Lang::from_code(args["language"].as_str().unwrap_or("en")))}),
        ),
        (READ, "takeoff_kit") => {
            let mut kit = qiyas_core::takeoff_kit::kit();
            let digest = session.drawing_digest(0)?;
            fn bind(value: &mut Value, names: &Value) {
                match value {
                    Value::String(s) if s == "$layer" || s == "$block" => {
                        if let Some(name) = names[s.trim_start_matches('$')].as_str() {
                            *s = name.into();
                        }
                    }
                    Value::Object(fields) => fields.values_mut().for_each(|v| bind(v, names)),
                    Value::Array(values) => values.iter_mut().for_each(|v| bind(v, names)),
                    _ => {}
                }
            }
            bind(&mut kit["batch"], &digest["example_names"]);
            let selected = crate::MeasurementMethod::for_session(session, method)?;
            if selected.name.is_some() {
                kit["measurement_method"] = selected.state();
            }
            let selection = session.measurement_selection()?;
            let scope = if selection.project.is_some() {
                "project"
            } else {
                "settings"
            };
            if let Some(name) = &selected.name {
                kit["measurement_method"]["scope"] = json!(scope);
                let commands = kit["batch"]["commands"].as_array_mut().expect("kit batch");
                if selected.rules_loaded {
                    // Existing rules may be large. Reuse them without resending or replacing them.
                    commands.retain(|command| {
                        !(command["type"] == "measurement" && command["kind"] == "set_method")
                    });
                } else {
                    for command in commands {
                        if command["type"] == "measurement" && command["kind"] == "set_method" {
                            command["scope"] = json!(scope);
                            command["method"] =
                                json!(qiyas_core::measurement::RuleSet::named(name));
                        }
                    }
                }
            }
            finish_kit(&mut kit);
            json(kit)
        }
        (READ, "takeoff_check") => {
            let expectations: Vec<qiyas_core::verify::Expectation> = serde_json::from_value(
                args.get("expectations")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )
            .map_err(|e| bad(format!("expectations: {e}")))?;
            json(session.takeoff_check_with_report(
                args["offset"].as_u64().unwrap_or(0) as usize,
                &expectations,
                args["end_report"].as_str().unwrap_or(""),
            )?)
        }
        (READ, "state") => {
            let mut value = state(session);
            value["measurement_method"] =
                crate::MeasurementMethod::for_session(session, method)?.state();
            let selection = session.measurement_selection()?;
            value["measurement_scope"] = json!(if selection.project.is_some() {
                "project"
            } else {
                "settings"
            });
            json(value)
        }
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
            let (command, input) = prepare_command(session, &args["command"])?;
            if effect_of(&command) != Effect::Read {
                return Err(bad(format!(
                    "{} changes things: run it with qiyas_execute run",
                    command.label()
                )));
            }
            let outcome = session.run_input(&input, source)?;
            let mut out = json!({ "outcome": outcome });
            copy_input_metadata(&mut out);
            json(out)
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
        (READ, "events") => json(session.events_since(
            args["after"].as_u64().unwrap_or(0),
            args["session"].as_str(),
        )),
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
            let (command, input) = prepare_command(session, &args["command"])?;
            let result = if args["layer"].is_string() || args["color"].is_string() {
                let layer = args["layer"].as_str().map(str::to_owned);
                let color = args["color"].as_str().map(str::to_owned);
                let wrapped =
                    json!({"type":"batch","commands":[input],"layer":layer,"color":color});
                run_one(
                    session,
                    Command::Batch {
                        commands: vec![command],
                        layer,
                        color,
                    },
                    &wrapped,
                    source,
                )?
            } else {
                run_one(session, command, &input, source)?
            };
            json(result)
        }
        (EXECUTE, "type") => {
            let line = args["line"]
                .as_str()
                .ok_or_else(|| bad("type needs {line}: a complete typed command line"))?;
            if matches!(
                line.trim().to_ascii_uppercase().as_str(),
                "UNDO" | "U" | "REDO"
            ) {
                let op = if line.trim().eq_ignore_ascii_case("REDO") {
                    "redo_own"
                } else {
                    "undo_own"
                };
                return run_session_with_method(session, EXECUTE, op, &json!({}), source, method);
            }
            let mut machine = qiyas_core::cmdline::Machine::default();
            machine.set_ucs(session.ucs);
            match machine.step(line).map_err(bad)? {
                qiyas_core::cmdline::Step::Command(command) => {
                    // Apply the same internal-command and folder rules as `run`.
                    let raw = machine.take_input().unwrap_or_else(|| json!(command));
                    let (command, input) = prepare_command(session, &raw)?;
                    json(run_one(session, command, &input, source)?)
                }
                qiyas_core::cmdline::Step::Prompt(prompt) => json(json!({
                    "ok": true,
                    "complete": false,
                    "prompt": prompt,
                    "next": "Send type again with the complete command line, including its word and answers; nothing ran.",
                })),
                qiyas_core::cmdline::Step::Cancelled => {
                    json(json!({ "ok": true, "cancelled": true }))
                }
            }
        }
        (EXECUTE, "set_records") => {
            let mut body = args.clone();
            body["type"] = json!("set_records");
            let (command, input) = prepare_command(session, &body)?;
            json(run_one(session, command, &input, source)?)
        }
        (EXECUTE, "create_entities") => {
            let mut body = args.clone();
            body["type"] = json!("create_entities");
            let (command, input) = prepare_command(session, &body)?;
            json(run_one(session, command, &input, source)?)
        }
        (EXECUTE, "batch") => json(batch(session, args, source)?),
        (EXECUTE, "redo_own") => {
            let step = session.history_steps().1.first().cloned();
            let view = ViewState::of(session);
            let redone = step.as_ref().is_some_and(|step| step.source == source) && session.redo();
            let mut out = json!({"redone": redone, "no_change": !redone,
                "step": if redone { step } else { None },
                "why": if redone { Value::Null } else { json!("The next step is not yours, or there is none") },
                "ui": {"action": "redraw"}});
            view.update_ui(session, &mut out);
            json(out)
        }
        (EXECUTE, "undo_own") => {
            let label = session.history_steps().0.last().map(|s| s.label.clone());
            let view = ViewState::of(session);
            let undone = session.undo_if(source);
            let mut out = json!({
                "undone": undone,
            "no_change": !undone,
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
        "image_input": crate::models::image_state(None),
        "local_date": chrono::Local::now().format("%Y-%m-%d").to_string(),
        "file": session.drawing.path.file_name().map(|f| f.to_string_lossy().to_string()),
        "units": info.units,
        "metres_per_unit": to_m,
        "text_height_2_5mm": if session.sheet.eq_ignore_ascii_case("*Model_Space") { 0.0025 / to_m * factor } else { 2.5 },
        "sheet_units": crate::execute::sheet_units(session, &session.sheet),
        "paper": info.paper,
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
        "operation_needs": OPERATIONS,
        "operation_arguments": OPERATIONS.iter().map(|operation| {
            let (schema, example) = crate::surface_args::contract(operation.tool, operation.op);
            json!({"tool":operation.tool,"op":operation.op,"schema":schema,"example":example})
        }).collect::<Vec<_>>(),
        "error_codes": crate::errors::CODES,
        "validate": {"args": "{command} or {commands}", "commands_schema": batch_commands_schema(), "changes": false, "step_references": "Only earlier step indexes are allowed; pointed results are checked when running"},
        "execute_ops": EXECUTE_OPS,
        "plan": plan_info(),
        "typed_commands": {
            "args": { "line": "PLINE 0,0 10,0 C" },
            "words": "qiyas_read commands returns each command's words and aliases",
            "coordinates": "Drawing units in the current UCS; @ uses the preceding point in this line",
            "continuation": "An incomplete line returns a prompt; resend the complete line",
        },
        "capture": ["screen", "picture"],
        "query_kinds": QUERY_KINDS,
        "record_collections": ["entities", "layers", "linetypes", "text_styles", "dim_styles", "blocks", "layouts", "viewports", "groups", "views", "header", "layer_states", "xrefs", "takeoff", "boq", "conflicts", "schedules", "comments", "constraints", "parameters", "objects", "object_types", "knowledge_types", "knowledge_levels", "knowledge_tables", "knowledge_rules", "knowledge_sheets", "knowledge_mappings", "knowledge_views", "knowledge_measurement", "knowledge_checks", "knowledge_queries", "knowledge_reports", "knowledge_placement"],
        "creatable_entity_types": qiyas_core::record_edit::CREATABLE,
        "check_rules": qiyas_core::check::RULES,
        "commands_by_category": categories,
        "limits": { "records_page": 20, "records_page_max": 200, "records_bytes": qiyas_core::records::MAX_BYTES, "result_bytes": 40_960, "batch_commands": MAX_BATCH },
    })
}

/// `commands`: the index, one command, a category or a search.
fn finish_kit(kit: &mut Value) {
    let (schema, args) = crate::surface_args::contract(READ, "takeoff_check");
    kit["self_check"] = json!({"required":schema["required"],"example":{"tool":READ,"op":"takeoff_check","args":args}});
    // Reserve room for the read revision by avoiding duplicate prose in this compact kit.
    kit["end_report"] = json!({"BOQ":"measurement.report.items","method":"measurement.report.selection","unresolved":"takeoff_check"});
    kit["measurement_rules"] = json!("basis: geometric or rule:<cited note name>; factors on named_handles. Counts are whole numbers. Exclude <handle>: <reason> (note <handle>).");
}

fn commands(args: &Value) -> Result<Value, ToolError> {
    let reg = registry();
    if let Some(names) = args["names"].as_array() {
        let results = names
            .iter()
            .map(|name| commands(&json!({"name":name})))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(json!({"commands":results}));
    }
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        if name.eq_ignore_ascii_case("plan") {
            return Ok(plan_info());
        }
        let normalized = name.trim().to_ascii_lowercase();
        let operations: Vec<_> = OPERATIONS.iter().filter(|operation| {
            normalized == format!("{}.{}", operation.tool, operation.op)
                || (normalized == operation.op && reg.get(name).is_none())
        }).map(|operation| {
            let (schema, example) = crate::surface_args::contract(operation.tool, operation.op);
            json!({"name":format!("{}.{}", operation.tool, operation.op),"tool":operation.tool,"op":operation.op,"schema":schema,"example":example})
        }).collect();
        if operations.len() == 1 {
            return Ok(operations[0].clone());
        }
        if !operations.is_empty() {
            return Ok(json!({"operations": operations}));
        }
        let info = reg
            .get(name)
            .or_else(|| reg.by_word(name))
            .filter(|c| !c.internal)
            .ok_or_else(|| {
                bad(format!(
                    "No command {name}; close ones: {}",
                    close_commands(name).join(", ")
                ))
            })?;
        if let Some(entry) = qiyas_core::takeoff_kit::short_entry(info) {
            return Ok(
                json!({"name":info.name,"doc":info.summary,"in_takeoff_kit":true,"complete":true,"entry":entry}),
            );
        }
        return Ok(json!({
            "name": info.name,
            "words": info.words,
            "prompts": info.prompts,
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
            .map(|c| json!({ "name": c.name, "words": c.words, "summary": c.summary, "effect": c.effect }))
            .collect()
    };
    if let Some(query) = args.get("search").and_then(Value::as_str) {
        let mut answer = json!({ "commands": list(reg.search(query, SEARCH_LIMIT)) });
        let query = query.to_lowercase();
        answer["operations"] = json!(OPERATIONS
            .iter()
            .filter(|o| format!("{}.{}", o.tool, o.op).contains(&query))
            .map(|o| json!({"name":format!("{}.{}",o.tool,o.op),"tool":o.tool,"op":o.op}))
            .collect::<Vec<_>>());
        if "plan phases pending active done skipped".contains(&query) {
            answer["operations"]
                .as_array_mut()
                .expect("operation list")
                .push(plan_info());
        }
        return Ok(answer);
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
        "index": crate::prompt::command_index(),
        "operations": std::iter::once(plan_info()).chain(OPERATIONS.iter().filter(|o| o.op != "plan").map(|o| json!({"name":format!("{}.{}",o.tool,o.op),"tool":o.tool,"op":o.op}))).collect::<Vec<_>>(),
        "next": "qiyas_read commands {\"name\": \"…\"} gives a command or operation schema and examples",
    }))
}

/// Normalize handle shortcuts before operation validation, for MCP and direct callers alike.
fn read_handle_values(args: &Value) -> Value {
    let mut value = args.clone();
    if let Some(handle) = qiyas_core::registry::handle_number(&value["handle"]) {
        value["handle"] = json!(handle);
    }
    for key in ["handles", "chain", "highlight"] {
        if let Some(handles) = value.get_mut(key).and_then(Value::as_array_mut) {
            for handle in handles {
                if let Some(number) = qiyas_core::registry::handle_number(handle) {
                    *handle = json!(number);
                }
            }
        }
    }
    value
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
    let limit = rest
        .as_object_mut()
        .and_then(|o| o.remove("limit"))
        .and_then(|v| v.as_u64());
    let within: Option<[[f64; 2]; 2]> = args
        .get("within")
        .filter(|v| !v.is_null())
        .map(|v| read_args(v, "query within"))
        .transpose()?;
    if kind != "texts" {
        rest.as_object_mut().unwrap().remove("within");
    }
    let inside = |p: [f64; 2]| {
        within.is_none_or(|b| {
            (0..2).all(|i| p[i] >= b[0][i].min(b[1][i]) && p[i] <= b[0][i].max(b[1][i]))
        })
    };
    let page = |session: &Session, rows: Vec<Value>| {
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        crate::execute::filtered_query_page(session, kind, args["sheet"].as_str(), rows, offset)
    };
    let result = match kind {
        "near" => {
            let p: [f64; 2] = read_args(&args["point"], "query near: point")?;
            let r = args["radius"]
                .as_f64()
                .unwrap_or(0.5 / crate::execute::sheet_metres_per_unit(session));
            let mut box_ = [[p[0] - r, p[1] - r], [p[0] + r, p[1] + r]];
            if let Some(b) = within {
                for i in 0..2 {
                    box_[0][i] = box_[0][i].max(b[0][i].min(b[1][i]));
                    box_[1][i] = box_[1][i].min(b[0][i].max(b[1][i]));
                }
                if (0..2).any(|i| box_[0][i] > box_[1][i]) {
                    return page(session, vec![]);
                }
            }
            let mut find = json!({ "within": box_ });
            for k in ["layer", "type", "limit", "sheet", "offset"] {
                if let Some(v) = args.get(k) {
                    find[if k == "type" { "kind" } else { k }] = v.clone();
                }
            }
            execute(session, "find_entities", &find, source)
        }
        "window" => {
            let mut find = json!({ "within": within });
            for k in ["layer", "type", "limit", "offset", "sheet"] {
                if let Some(v) = args.get(k) {
                    find[if k == "type" { "kind" } else { k }] = v.clone();
                }
            }
            execute(session, "find_entities", &find, source)
        }
        "entity" => {
            let handle = args["handle"].as_u64().unwrap();
            let chain: Vec<u64> = args
                .get("chain")
                .map(|v| read_args(v, "chain"))
                .transpose()?
                .unwrap_or_default();
            if within
                .map(|b| session.entity_in_query_box(handle, &chain, b))
                .transpose()?
                .is_some_and(|inside| !inside)
            {
                Ok(ToolOutput::Json(json!({"entity":null,"matched":false})))
            } else {
                execute(session, "get_entity", &rest, source)
            }
        }
        "texts" => execute(session, "read_text", &rest, source),
        "tables" if within.is_some() => page(
            session,
            session
                .read_tables_within(args["sheet"].as_str(), within)
                .into_iter()
                .map(|r| json!(r))
                .collect(),
        ),
        "dimensions" if within.is_some() => page(
            session,
            session
                .read_dimensions(args["sheet"].as_str())
                .into_iter()
                .filter(|r| inside(r.at))
                .map(|r| json!(r))
                .collect(),
        ),
        "blocks" if within.is_some() => page(
            session,
            session
                .block_summary_within(args["sheet"].as_str(), within.unwrap())
                .into_iter()
                .map(|r| json!(r))
                .collect(),
        ),
        "layers" if within.is_some() => {
            let mut filter = qiyas_core::EntityFilter {
                within,
                limit: 500,
                ..Default::default()
            };
            let mut names = std::collections::BTreeSet::new();
            loop {
                let found = session.find_entities(&filter);
                names.extend(found.rows.into_iter().map(|r| r.layer.to_lowercase()));
                let Some(next) = found.next_offset else { break };
                filter.offset = next;
            }
            page(
                session,
                session
                    .layers()
                    .into_iter()
                    .filter(|l| names.contains(&l.name.to_lowercase()))
                    .map(|l| json!(l))
                    .collect(),
            )
        }
        "tables" => execute(session, "read_tables", &rest, source),
        "dimensions" => execute(session, "read_dimensions", &rest, source),
        "blocks" => execute(session, "block_summary", &rest, source),
        "layers" => execute(session, "list_layers", &rest, source),
        "boundary" => {
            let p: [f64; 2] = read_args(&args["point"], "query boundary: point")?;
            if !inside(p) {
                return Ok(ToolOutput::Json(json!({"ring":null,"matched":false})));
            }
            let to_m = crate::execute::sheet_metres_per_unit(session);
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
    };
    result.map(|mut result| {
        if !matches!(kind, "near" | "window") {
            if let (Some(limit), ToolOutput::Json(v)) = (limit, &mut result) {
                if let Some(rows) = v["rows"].as_array_mut() {
                    rows.truncate(limit.min(500) as usize);
                    let end = args["offset"].as_u64().unwrap_or(0) + rows.len() as u64;
                    v["next_offset"] = if end < v["total"].as_u64().unwrap_or(0) && limit > 0 {
                        json!(end)
                    } else {
                        Value::Null
                    };
                    crate::normalise::append(
                        v,
                        &[qiyas_core::validation::InputIssue {
                            code: "normalised_input".into(),
                            pointer: "/args/limit".into(),
                            message: "Normalised query limit to a page size (at most 500 rows)."
                                .into(),
                        }],
                    );
                }
            }
        }
        result
    })
}

/// `measure`: the length and area of an entity or of points, in metres, without a take-off row.
fn measure(session: &Session, args: &Value) -> Result<Value, ToolError> {
    let to_m = crate::execute::sheet_metres_per_unit(session);
    if let Some(handle) = args.get("handle") {
        let h = handle
            .as_u64()
            .ok_or_else(|| bad("Give a decimal handle or an explicit 0x hexadecimal handle"))?;
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
            "units": if session.sheet.eq_ignore_ascii_case("*Model_Space") { session.info().units } else { "paper millimetres".into() },
        }));
    }
    let mut points: qiyas_core::PointRun = read_args(&args["points"], "measure: points")?;
    let closed = points.normalize(
        session.drawing.units,
        args["closed"].as_bool().unwrap_or(false),
    );
    if points.len() < 2 {
        return Err(bad("measure needs {handle} or {points} (2 or more)"));
    }
    let length: f64 = points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
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
    prepare_command(session, raw).map(|(command, _)| command)
}

/// Keep the accepted input beside its typed command; serialization would fill omitted fields.
fn prepare_command(session: &Session, raw: &Value) -> Result<(Command, Value), ToolError> {
    let mut value = command_value(raw).map_err(|error| {
        let value = match raw {
            Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| raw.clone()),
            _ => raw.clone(),
        };
        let report = session.validate_input(&value);
        ToolError::Structured {
            code: report
                .errors
                .first()
                .map_or("invalid_value".into(), |issue| issue.code.clone()),
            message: format!("{error}; {}", report.message()),
            details: json!({"errors": report.errors, "warnings": report.warnings}),
        }
    })?;
    let kind = value["type"].as_str().unwrap_or_default().to_string();
    let Some(info) = command_info(&value) else {
        let report = session.validate_input(&value);
        return Err(ToolError::Structured {
            code: "not_found".into(),
            message: format!(
                "No command {kind}; close ones: {}",
                close_commands(&kind).join(", ")
            ),
            details: json!({"errors": report.errors, "warnings": report.warnings, "repairs": report.repairs, "did_you_mean": close_commands(&kind)}),
        });
    };
    if info.internal {
        return Err(bad(format!(
            "{kind} is run by Qiyas itself (import_boq and compare do it)"
        )));
    }
    // Use the registry's spelling after a lookup that accepts case and hyphens.
    qiyas_core::registry::canonicalize(&mut value);
    session
        .resolve_input_handles(&mut value)
        .map_err(|e| bad(e.to_string()))?;
    let mut report = session.validate_input(&value);
    batch_limits(&value, "", &mut report);
    if !report.valid {
        let serde_error = qiyas_core::registry::parse_command(value.clone())
            .err()
            .map(|e| e.to_string());
        return Err(ToolError::Structured {
            code: report
                .errors
                .first()
                .map_or("invalid_value".into(), |issue| issue.code.clone()),
            message: format!(
                "{}: {}. {} Its schema: {} Example: {}",
                info.name,
                report
                    .errors
                    .iter()
                    .map(|issue| format!("{}: {}", issue.pointer, issue.message))
                    .collect::<Vec<_>>()
                    .join("; "),
                serde_error.unwrap_or_default(),
                info.schema,
                info.examples.first().cloned().unwrap_or_default()
            ),
            details: json!({"errors": report.errors, "warnings": report.warnings, "repairs": report.repairs, "schema": info.schema, "example": info.examples.first()}),
        });
    }
    if info.name == "batch" {
        if let Some(commands) = value["commands"].as_array_mut() {
            for (index, command) in commands.iter_mut().enumerate() {
                parse_command(session, command)
                    .map_err(|error| bad(format!("Batch step {index}: {error}")))?;
                // Keep omitted fields omitted so the core can report their defaults.
                *command = command_value(command)?;
                qiyas_core::registry::canonicalize(command);
                resolve_paths(session, command)?;
            }
        }
    }
    resolve_paths(session, &mut value)?;
    let command = qiyas_core::registry::parse_command(value.clone()).map_err(|error| {
        let path = input_path(&info.schema, &value, "");
        bad(format!(
            "{}: {error}{}. Its schema: {} Example: {}",
            info.name,
            path.map_or_else(String::new, |path| format!(" (field {path})")),
            info.schema,
            info.examples.first().cloned().unwrap_or_default()
        ))
    })?;
    Ok((command, value))
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
        .map(|command| {
            let short = command.name.rsplit('.').next().unwrap_or(&command.name);
            (
                distance(&name, &command.name).min(distance(&name, short)),
                command.name.as_str(),
            )
        })
        .collect();
    ranked.extend(OPERATIONS.iter().map(|o| (distance(&name, o.op), o.op)));
    ranked.sort_unstable();
    ranked.dedup();
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
    if kind == "batch" {
        if let Some(commands) = value["commands"].as_array_mut() {
            for command in commands {
                *command = command_value(command)?;
                qiyas_core::registry::canonicalize(command);
                resolve_paths(session, command)?;
            }
        }
        return Ok(());
    }
    if matches!(
        kind,
        "set_records" | "records" | "create_entities" | "deliverable" | "deliverable.export"
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
fn run_one(
    session: &mut Session,
    command: Command,
    input: &Value,
    source: Source,
) -> Result<Value, ToolError> {
    let label = command.label();
    let effect = effect_of(&command);
    let history = session.history_revision();
    let view = ViewState::of(session);
    let outcome = session.run_input(input, source)?;
    qiyas_boq::deliverables::complete(&outcome.outcome).map_err(bad)?;
    let outputs = if label == "project.run" {
        Some(qiyas_boq::project::outputs(&outcome.outcome, None).map_err(bad)?)
    } else {
        None
    };
    let mut out = changed(session, &label, effect, history, json!(outcome));
    if let Some(outputs) = outputs {
        out["outputs"] = outputs;
    }
    if label == "set_sheet" {
        out["ui"] = json!({ "action": "show_sheet", "sheet": session.sheet });
    }
    view.update_ui(session, &mut out);
    Ok(out)
}

fn copy_input_metadata(out: &mut Value) {
    for field in ["defaults_used", "warnings", "advice"] {
        if let Some(value) = out["outcome"].get(field).cloned() {
            out[field] = value;
        }
    }
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
        "no_change": effect == Effect::Edit && session.history_revision() == history_before,
        "effect": effect,
        "outcome": outcome,
        "handle_map": session.handle_map(),
        "added": change.added,
        "changed": change.changed,
        "removed": change.removed,
        "step": step.map(|s| json!({ "label": s.label, "source": s.source, "undoable": true })),
    });
    copy_input_metadata(&mut out);
    if !touched.is_empty() {
        if session.drawing.entities_in(&session.sheet).count() <= CHECK_UP_TO {
            let report = session.check_drawing(&qiyas_core::check::CheckScope {
                handles: touched.clone(),
                ..Default::default()
            });
            out["check"] = json!({
                "line": report.line,
                "complete": report.complete,
                "layout_findings": report.counts.iter().filter(|(r,_)| qiyas_core::check::layout_rule(r)).map(|(_,n)| n).sum::<usize>(),
                "not_checked": report.not_checked,
                "findings": report.findings.iter().take(CHECK_FINDINGS).collect::<Vec<_>>(),
            });
        } else {
            out["check"] = json!({ "complete": false, "line": "Not checked: a large sheet. qiyas_read check {\"handles\": […]} checks what you changed" });
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
    let commands = args["commands"].as_array().cloned().unwrap_or_default();
    let mut batch_input = args.clone();
    batch_input["type"] = json!("batch");
    let mut report = session.validate_input(&batch_input);
    batch_limits(&batch_input, "", &mut report);
    if !report.valid {
        return Err(ToolError::Structured {
            code: report
                .errors
                .first()
                .map_or("invalid_value".into(), |issue| issue.code.clone()),
            message: report
                .errors
                .iter()
                .map(|issue| format!("{}: {}", issue.pointer, issue.message))
                .collect::<Vec<_>>()
                .join("; "),
            details: json!({
                "errors": report.errors, "warnings": report.warnings, "repairs": report.repairs,
                "schema": registry().get("batch").map(|info| &info.schema),
                "example": registry().get("batch").and_then(|info| info.examples.first()),
                "commands": commands.iter().enumerate().filter_map(|(index, command)| {
                    let info = command_info(command)?;
                    let prefix = format!("/commands/{index}/");
                    report.errors.iter().any(|issue| issue.pointer.starts_with(&prefix) || issue.pointer == prefix.trim_end_matches('/')).then(|| json!({
                        "index": index, "schema": info.schema, "example": info.examples.first(),
                        "serde_error": qiyas_core::registry::parse_command(command.clone()).err(),
                    }))
                }).collect::<Vec<_>>(),
            }),
        });
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
        if let Some(grouped) = s.run_opening_batch(&commands, source) {
            let grouped = grouped?;
            let change = s.last_change().clone();
            for outcome in grouped {
                answers.push(json!({
                    "command":"add_wall_openings", "outcome":outcome,
                    "shared_wall_result":true,
                    "added":change.added, "changed":change.changed, "removed":change.removed,
                }));
            }
            return Ok(());
        }
        for (i, raw) in commands.iter().enumerate() {
            let input_error = |error: ToolError| CoreError::Step {
                index: i,
                label: "input".into(),
                error: Box::new(CoreError::InvalidCommand(error.json().to_string())),
            };
            let mut raw = command_value(raw).map_err(input_error)?;
            bind(&mut raw, &answers).map_err(|why| {
                input_error(ToolError::Structured {
                    code: "not_found".into(),
                    message: why,
                    details: json!({}),
                })
            })?;
            let (command, input) = prepare_command(s, &raw).map_err(input_error)?;
            let label = command.label();
            let (outcome, change) = s
                .transaction(source, &label, |s| {
                    let outcome = s.run_input(&input, source)?;
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
    if let Err(error) = ran {
        let mut details = ToolError::from(error).json();
        if !details["warnings"].is_array() {
            details["warnings"] = json!([]);
        }
        let warnings = details["warnings"].as_array_mut().unwrap();
        for warning in &report.warnings {
            let warning = json!(warning);
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
        return Err(ToolError::Structured {
            code: details["code"].as_str().unwrap_or("failed").into(),
            message: details["message"].as_str().unwrap_or_default().into(),
            details,
        });
    }
    let mut defaults_used: Vec<Value> = report
        .defaults_used
        .iter()
        .filter(|entry| !entry.field.starts_with("/commands/"))
        .map(|entry| json!(entry))
        .collect();
    let mut warnings: Vec<Value> = report
        .warnings
        .iter()
        .filter(|entry| {
            !entry.pointer.starts_with("/commands/")
                || reference_warning(&batch_input, &entry.pointer)
        })
        .map(|entry| json!(entry))
        .collect();
    for (index, answer) in answers.iter().enumerate() {
        for (field, target, pointer_field) in [
            ("defaults_used", &mut defaults_used, "field"),
            ("warnings", &mut warnings, "pointer"),
        ] {
            if let Some(entries) = answer["outcome"][field].as_array() {
                for entry in entries {
                    let mut entry = entry.clone();
                    if let Some(pointer) = entry[pointer_field].as_str() {
                        entry[pointer_field] = json!(format!("/commands/{index}{pointer}"));
                    }
                    target.push(entry);
                }
            }
        }
    }
    let mut out = changed(
        session,
        "batch",
        Effect::Edit,
        history,
        json!({ "steps": answers, "defaults_used": defaults_used, "warnings": warnings }),
    );
    view.update_ui(session, &mut out);
    Ok(out)
}

/// Binding replaces reference objects; keep their unknown-field warnings from the dry check.
fn reference_warning(input: &Value, pointer: &str) -> bool {
    let Some((parent, _)) = pointer.rsplit_once('/') else {
        return false;
    };
    input
        .pointer(parent)
        .is_some_and(|value| value.get("$step").is_some())
}

/// Replaces each `{"$step": i, "pointer": "/added/0"}` in `value` with what it points at in
/// step i's answer.
fn bind(value: &mut Value, answers: &[Value]) -> Result<(), String> {
    match value {
        Value::Object(map) if map.contains_key("$step") => {
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
        qiyas_boq::export_takeoff_with_queries(
            &path,
            &session.takeoff.items,
            &totals,
            &stem,
            &session
                .measurement_report()
                .map_err(|e| ToolError::Failed(e.to_string()))?,
            &session.conflicts,
        )
        .map_err(|e| ToolError::Failed(e.to_string()))?;
    } else {
        qiyas_boq::report::export_conflicts(&path, &session.conflicts)
            .map_err(|e| ToolError::Failed(e.to_string()))?;
    }
    Ok(json!({ "written": path.to_string_lossy() }))
}
