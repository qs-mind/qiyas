//! The command registry: every `Command` described from its own JSON schema, which schemars
//! generates from the enum, its doc comments, its examples and its `x-…` extensions. Mizan, MCP,
//! the command line and scripts read this one list, so a command added with its doc comment,
//! category and example shows everywhere; `tests/registry.rs` refuses one without them.
//!
//! The extensions on a `Command` variant:
//! - `x-category` (required): where it belongs ("draw", "layers", "takeoff", ...).
//! - `x-effect`: for a command that is not an undo step, what it does instead: "view" (view state:
//!   layers shown, the sheet, the UCS), "file" (writes a file), "session" (opens or closes an
//!   edit). Left out, it reads only. An undo step is an "edit" whatever this says.
//! - `x-destructive`: it removes or replaces what someone made.
//! - `x-batch: false`: `Command::Batch` refuses it.
//! - `x-internal`: hosts run it (the BOQ import, the comparison); never offered to the AI.
//! - `x-needs`: what it needs besides the drawing ("pdf": PDF support, "clipboard").
//! - `x-words`: command-line words and aliases, upper case, the full word first.
//! - `x-prompts`: ordered field names or objects with `field` and `prompt`, overriding the
//!   schema's argument order and prompt text for the typed command line.
//!
//! A command whose payload is another tagged enum (`Draw(DrawSpec)`) has kinds: `draw.line`,
//! `draw.circle`, each described and exampled on its own.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use schemars::{schema_for, Schema};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::commands::Part;
use crate::Command;

/// What running a command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Reads only: the session is as it was.
    Read,
    /// Changes the view state (layers shown, the sheet, the UCS, the drawing state new entities
    /// take); not an undo step.
    View,
    /// An undo step.
    Edit,
    /// Writes a file; not an undo step.
    File,
    /// Opens or closes an edit (a block edited in place); not an undo step.
    Session,
}

/// One command (or one kind of a command with kinds, `draw.line`).
#[derive(Debug, Clone, Serialize)]
pub struct CommandInfo {
    /// The wire name (`"type"`), with the kind after a dot for a kind: `draw.line`.
    pub name: String,
    pub category: String,
    /// Optional workflow stage declared on the command or kind schema.
    pub takeoff_step: Option<String>,
    /// Compact take-off contract order, example index and optional batch order.
    pub takeoff_kit: Option<Value>,
    /// The doc comment's first sentence.
    pub summary: String,
    /// The whole doc comment.
    pub doc: String,
    /// Command-line words and aliases, the full word first.
    pub words: Vec<String>,
    /// Ordered prompt overrides from `x-prompts`; the schema supplies omitted field details.
    pub prompts: Vec<Value>,
    pub effect: Effect,
    /// The part of the session its undo step keeps ("takeoff", "boq", "conflicts", "drawing",
    /// "drawing+tables", "drawing+tables+takeoff"), worked out from its first example; none when
    /// it is not an undo step.
    pub undo: Option<String>,
    pub destructive: bool,
    pub batchable: bool,
    pub internal: bool,
    pub needs: Option<String>,
    /// The JSON schema of the whole command (`"type"` and, for a kind, `"kind"` included), with
    /// every definition it uses inlined; `Command::Batch`'s commands are left open.
    pub schema: Value,
    /// Whole commands, ready to run.
    pub examples: Vec<Value>,
    /// For a command with kinds: their names (`line`, `circle`, ...).
    pub kinds: Vec<String>,
    /// What `Registry::search` looks in: the name, category, doc, and the schema's property
    /// names, values and descriptions, in lower case.
    #[serde(skip)]
    search_text: String,
}

/// Every command, in the enum's order, each followed by its kinds.
pub struct Registry {
    commands: Vec<CommandInfo>,
    by_name: BTreeMap<String, usize>,
    by_word: BTreeMap<String, usize>,
}

/// A command-line word, its registry command and the text shown beside a suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct CommandWord {
    pub word: String,
    pub command: String,
    pub summary: String,
}

/// The registry, built once from `Command`'s schema.
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::build)
}

/// The JSON schema of `Command`, as generated.
pub fn command_schema() -> Schema {
    schema_for!(Command)
}

/// Reads a command, keeping the field path in a bad input's error.
pub fn parse_command(mut value: Value) -> Result<Command, String> {
    let report = crate::validation::validate_command(&value, None);
    if !report.valid {
        let serde_error = serde_path_to_error::deserialize::<_, Command>(value)
            .err()
            .map(|error| format!("; {} at {}", error.inner(), error.path()))
            .unwrap_or_default();
        let warnings = if report.warnings.is_empty() {
            String::new()
        } else {
            format!(
                "; warnings: {}",
                serde_json::to_string(&report.warnings).unwrap_or_default()
            )
        };
        return Err(format!("{}{serde_error}{warnings}", report.message()));
    }
    canonicalize(&mut value);
    serde_path_to_error::deserialize(value)
        .map_err(|error| format!("{} at {}", error.inner(), error.path()))
}

pub(crate) fn canonicalize(value: &mut Value) {
    let Some(name) = value["type"].as_str() else {
        return;
    };
    let Some(top) = registry().get(name) else {
        return;
    };
    let info = value["kind"]
        .as_str()
        .and_then(|kind| registry().get(&format!("{name}.{kind}")))
        .unwrap_or(top);
    let mut parts = info.name.split('.');
    value["type"] = json!(parts.next().unwrap_or_default());
    if let Some(kind) = parts.next() {
        value["kind"] = json!(kind);
    }
    if let Some(commands) = value.get_mut("commands").and_then(Value::as_array_mut) {
        for command in commands {
            canonicalize(command);
        }
    }
}

impl Registry {
    fn build() -> Registry {
        let root = command_schema().to_value();
        let defs = root
            .get("$defs")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut commands = Vec::new();
        for variant in variants(&root) {
            let Some(name) = tag_of(&variant, "type") else {
                continue;
            };
            let meta = Meta::of(&variant);
            let schema = inline(&variant, &defs, 0);
            let examples = examples_of(&variant);
            let kinds_of = kind_variants(&variant, &defs);
            let kinds: Vec<String> = kinds_of.iter().filter_map(|k| tag_of(k, "kind")).collect();
            let doc = description(&variant);
            commands.push(CommandInfo {
                summary: summary(&doc),
                doc,
                words: words_of(&variant),
                prompts: prompts_of(&variant),
                takeoff_step: variant["x-takeoff-step"].as_str().map(str::to_owned),
                takeoff_kit: variant.get("x-takeoff-kit").cloned(),
                effect: effect(&meta, &examples),
                undo: undo_of(&examples),
                category: meta.category.clone(),
                destructive: meta.destructive,
                batchable: meta.batchable,
                internal: meta.internal,
                needs: meta.needs.clone(),
                schema,
                examples: examples.clone(),
                kinds,
                search_text: String::new(),
                name: name.clone(),
            });
            for kind in kinds_of {
                let Some(kind_name) = tag_of(&kind, "kind") else {
                    continue;
                };
                let examples: Vec<Value> = examples_of(&kind)
                    .into_iter()
                    .map(|mut e| {
                        if let Some(obj) = e.as_object_mut() {
                            obj.insert("type".into(), json!(name));
                        }
                        e
                    })
                    .collect();
                let mut schema = inline(&kind, &defs, 0);
                if let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) {
                    props.insert("type".into(), json!({ "const": name }));
                }
                if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
                    required.insert(0, json!("type"));
                }
                let doc = description(&kind);
                let doc = if doc.is_empty() {
                    commands
                        .last()
                        .map(|c: &CommandInfo| c.doc.clone())
                        .unwrap_or_default()
                } else {
                    doc
                };
                commands.push(CommandInfo {
                    name: format!("{name}.{kind_name}"),
                    summary: summary(&doc),
                    doc,
                    words: words_of(&kind),
                    prompts: prompts_of(&kind),
                    takeoff_step: kind["x-takeoff-step"].as_str().map(str::to_owned),
                    takeoff_kit: kind.get("x-takeoff-kit").cloned(),
                    effect: effect(&meta, &examples),
                    undo: undo_of(&examples),
                    category: meta.category.clone(),
                    destructive: meta.destructive,
                    batchable: meta.batchable,
                    internal: meta.internal,
                    needs: meta.needs.clone(),
                    schema,
                    examples,
                    kinds: Vec::new(),
                    search_text: String::new(),
                });
            }
        }
        for c in &mut commands {
            describe_defaults(&mut c.schema);
            let mut words = String::new();
            schema_words(&c.schema, &mut words);
            c.search_text = format!(
                "{} {} {} {} {words}",
                c.name,
                c.category,
                c.doc,
                c.words.join(" ")
            )
            .to_lowercase();
        }
        let by_name = commands
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();
        let by_word = commands
            .iter()
            .enumerate()
            .flat_map(|(i, c)| c.words.iter().map(move |word| (word.clone(), i)))
            .collect();
        Registry {
            commands,
            by_name,
            by_word,
        }
    }

    /// Every command, each followed by its kinds.
    pub fn commands(&self) -> &[CommandInfo] {
        &self.commands
    }

    /// A command by its wire name (`draw`, `draw.line`), in any case, `-` or `_` alike.
    pub fn get(&self, name: &str) -> Option<&CommandInfo> {
        let key = name.trim().to_ascii_lowercase().replace('-', "_");
        self.by_name.get(&key).map(|&i| &self.commands[i])
    }

    /// A command by one of its command-line words or aliases, in any case.
    pub fn by_word(&self, word: &str) -> Option<&CommandInfo> {
        self.by_word
            .get(&word.trim().to_ascii_uppercase())
            .map(|&i| &self.commands[i])
    }

    /// Every command-line word and alias, in registry order, for suggestions and customisation.
    pub fn words(&self) -> Vec<CommandWord> {
        self.commands
            .iter()
            .filter(|command| !command.internal)
            .flat_map(|command| {
                command.words.iter().map(move |word| CommandWord {
                    word: word.clone(),
                    command: command.name.clone(),
                    summary: command.summary.clone(),
                })
            })
            .collect()
    }

    /// The commands whose name, category or doc holds every word of `query` (any case), names
    /// matching first, at most `limit`. Internal commands are left out.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&CommandInfo> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(|w| w.to_lowercase().replace('-', "_"))
            .collect();
        let mut found: Vec<(u8, usize)> = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.internal)
            .filter_map(|(i, c)| {
                let name = c.name.to_lowercase();
                let text = &c.search_text;
                if words.is_empty() || !words.iter().all(|w| text.contains(w.as_str())) {
                    return None;
                }
                let rank = if words.iter().all(|w| name.contains(w.as_str())) {
                    0
                } else if words.iter().any(|w| name.contains(w.as_str())) {
                    1
                } else {
                    2
                };
                Some((rank, i))
            })
            .collect();
        found.sort();
        found
            .into_iter()
            .take(limit)
            .map(|(_, i)| &self.commands[i])
            .collect()
    }

    /// The commands by category, one line each (`draw: draw (line, polyline, ...), text, ...`),
    /// internal ones left out: the index the AI's prompt carries. Deterministic, so it caches.
    pub fn index_text(&self) -> String {
        let mut by_category: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for c in self
            .commands
            .iter()
            .filter(|c| !c.internal && !c.name.contains('.'))
        {
            let entry = if c.kinds.is_empty() {
                c.name.clone()
            } else {
                format!("{} ({})", c.name, c.kinds.join(", "))
            };
            by_category.entry(&c.category).or_default().push(entry);
        }
        let categories = by_category
            .into_iter()
            .map(|(category, names)| format!("{category}: {}", names.join(", ")))
            .collect::<Vec<_>>()
            .join("\n");
        let mut workflow = BTreeMap::<&str, Vec<&str>>::new();
        for c in self.commands.iter().filter(|c| !c.internal) {
            if let Some(step) = c.takeoff_step.as_deref() {
                workflow.entry(step).or_default().push(&c.name);
            }
        }
        let workflow = workflow
            .into_iter()
            .map(|(step, names)| format!("Take-off workflow / {step}: {}", names.join(", ")))
            .collect::<Vec<_>>()
            .join("\n");
        format!("Take-off workflow / 1 read (qiyas_read): digest, takeoff_kit\n{workflow}\n{categories}")
    }
}

/// Schemars supplies serde defaults; make their convention visible even in a field-only view.
fn describe_defaults(schema: &mut Value) {
    match schema {
        Value::Object(object) => {
            if let Some(default) = object.get("default").cloned() {
                let description = object.entry("description").or_insert_with(|| json!(""));
                if let Some(text) = description
                    .as_str()
                    .filter(|s| !s.to_lowercase().contains("default"))
                {
                    let convention = if default.is_null() {
                        "Default: null selects the command's automatic convention.".into()
                    } else {
                        format!("Default: {default} when omitted.")
                    };
                    *description = json!(format!("{text} {convention}").trim());
                }
            }
            for value in object.values_mut() {
                describe_defaults(value);
            }
        }
        Value::Array(items) => {
            for item in items {
                describe_defaults(item);
            }
        }
        _ => {}
    }
}

/// What a command's extensions say.
struct Meta {
    category: String,
    effect: Option<String>,
    destructive: bool,
    batchable: bool,
    internal: bool,
    needs: Option<String>,
}

impl Meta {
    fn of(variant: &Value) -> Meta {
        let s = |k: &str| variant.get(k).and_then(Value::as_str).map(str::to_string);
        let b = |k: &str| variant.get(k).and_then(Value::as_bool);
        Meta {
            category: s("x-category").unwrap_or_default(),
            effect: s("x-effect"),
            destructive: b("x-destructive").unwrap_or(false),
            batchable: b("x-batch").unwrap_or(true),
            internal: b("x-internal").unwrap_or(false),
            needs: s("x-needs"),
        }
    }
}

/// The variants of a tagged enum's schema (`oneOf`, or `anyOf`).
fn variants(schema: &Value) -> Vec<Value> {
    ["oneOf", "anyOf"]
        .iter()
        .find_map(|k| schema.get(*k).and_then(Value::as_array))
        .cloned()
        .unwrap_or_default()
}

/// The constant value of the tag property `tag` (`"type"`, `"kind"`) of a variant's schema,
/// looking into `allOf` when the variant is made of parts.
fn tag_of(variant: &Value, tag: &str) -> Option<String> {
    let direct = variant
        .get("properties")
        .and_then(|p| p.get(tag))
        .and_then(|t| {
            t.get("const")
                .or_else(|| t.get("enum").and_then(|e| e.get(0)))
        })
        .and_then(Value::as_str);
    if let Some(name) = direct {
        return Some(name.to_string());
    }
    for part in variant
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = tag_of(part, tag) {
            return Some(name);
        }
    }
    None
}

/// The kinds of a command whose payload is another tagged enum (`Draw(DrawSpec)`): that enum's
/// variants.
fn kind_variants(variant: &Value, defs: &Map<String, Value>) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_refs(variant, &mut refs);
    for r in refs {
        if let Some(def) = def_of(&r, defs) {
            let kinds = variants(def);
            if !kinds.is_empty() && kinds.iter().all(|k| tag_of(k, "kind").is_some()) {
                // Only a payload that is the whole command, not an enum one field holds.
                if variant.get("$ref").is_some()
                    || variant
                        .get("allOf")
                        .and_then(Value::as_array)
                        .is_some_and(|a| a.iter().any(|p| p.get("$ref").is_some()))
                {
                    return kinds;
                }
            }
        }
    }
    Vec::new()
}

/// The `$ref`s directly on a schema or in its `allOf`.
fn collect_refs(schema: &Value, out: &mut Vec<String>) {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        out.push(r.to_string());
    }
    for part in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        collect_refs(part, out);
    }
}

fn def_of<'a>(reference: &str, defs: &'a Map<String, Value>) -> Option<&'a Value> {
    reference
        .strip_prefix("#/$defs/")
        .and_then(|name| defs.get(name))
}

/// How deep definitions are inlined: `Command` inside `Batch` is the only recursion.
const MAX_DEPTH: usize = 8;

/// `schema` with every `$ref` replaced by its definition (the recursive `Command` by an open
/// object), and the registry's own `x-…` keys, examples and number formats left out.
fn inline(schema: &Value, defs: &Map<String, Value>, depth: usize) -> Value {
    match schema {
        Value::Object(map) => {
            if let Some(r) = map.get("$ref").and_then(Value::as_str) {
                let target = if r == "#" || r.ends_with("/Command") || depth >= MAX_DEPTH {
                    None
                } else {
                    def_of(r, defs)
                };
                let mut out = match target {
                    Some(def) => inline(def, defs, depth + 1),
                    None => json!({
                        "type": "object",
                        "description": "Any command, as {\"type\": ...} (see the commands list)."
                    }),
                };
                // Keys beside the reference (a description, a default) win.
                if let Value::Object(out_map) = &mut out {
                    for (k, v) in map {
                        if k != "$ref" && !k.starts_with("x-") && k != "examples" {
                            let sibling = inline(v, defs, depth + 1);
                            match (k.as_str(), out_map.get_mut(k), &sibling) {
                                (
                                    "properties",
                                    Some(Value::Object(existing)),
                                    Value::Object(properties),
                                ) => {
                                    existing.extend(properties.clone());
                                }
                                (
                                    "required",
                                    Some(Value::Array(existing)),
                                    Value::Array(required),
                                ) => {
                                    for field in required {
                                        if !existing.contains(field) {
                                            existing.push(field.clone());
                                        }
                                    }
                                }
                                _ => {
                                    out_map.insert(k.clone(), sibling);
                                }
                            }
                        }
                    }
                }
                return out;
            }
            let mut out = Map::new();
            for (k, v) in map {
                // `format` ("double", "uint64") only repeats what `type` says.
                if k.starts_with("x-")
                    || k == "examples"
                    || k == "$defs"
                    || k == "$schema"
                    || k == "format"
                {
                    continue;
                }
                out.insert(k.clone(), inline(v, defs, depth));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| inline(v, defs, depth)).collect()),
        other => other.clone(),
    }
}

/// A schema's property names, string values (`enum`, `const`) and descriptions, for search.
fn schema_words(schema: &Value, out: &mut String) {
    match schema {
        Value::Object(map) => {
            for (k, v) in map {
                match (k.as_str(), v) {
                    ("properties", Value::Object(props)) => {
                        for (name, sub) in props {
                            out.push_str(name);
                            out.push(' ');
                            schema_words(sub, out);
                        }
                    }
                    ("description" | "const", Value::String(s)) => {
                        out.push_str(s);
                        out.push(' ');
                    }
                    _ => schema_words(v, out),
                }
            }
        }
        Value::Array(items) => {
            for v in items {
                if let Value::String(s) = v {
                    out.push_str(s);
                    out.push(' ');
                } else {
                    schema_words(v, out);
                }
            }
        }
        _ => {}
    }
}

fn examples_of(variant: &Value) -> Vec<Value> {
    variant
        .get("examples")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn words_of(variant: &Value) -> Vec<String> {
    variant
        .get("x-words")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn prompts_of(variant: &Value) -> Vec<Value> {
    variant
        .get("x-prompts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn description(variant: &Value) -> String {
    let title = variant.get("title").and_then(Value::as_str).unwrap_or("");
    let body = variant
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    match (title.is_empty(), body.is_empty()) {
        (false, false) => format!("{title}\n\n{body}"),
        (false, true) => title.to_string(),
        _ => body.to_string(),
    }
}

/// The first sentence of a doc comment, on one line.
fn summary(doc: &str) -> String {
    let flat = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = flat.len();
    for (i, _) in flat.match_indices(". ") {
        // Not after a one-letter abbreviation or a number ("e.g. ", "1. ").
        let before = &flat[..i];
        let word = before.rsplit(' ').next().unwrap_or("");
        if word.len() > 2 && !word.ends_with("e.g") && !word.ends_with("i.e") {
            end = i + 1;
            break;
        }
    }
    flat[..end].trim().to_string()
}

fn effect(meta: &Meta, examples: &[Value]) -> Effect {
    if undo_of(examples).is_some() {
        return Effect::Edit;
    }
    match meta.effect.as_deref() {
        Some("view") => Effect::View,
        Some("file") => Effect::File,
        Some("session") => Effect::Session,
        Some("edit") => Effect::Edit,
        _ => Effect::Read,
    }
}

/// The part the first example's undo step keeps.
fn undo_of(examples: &[Value]) -> Option<String> {
    let command: Command = serde_json::from_value(examples.first()?.clone()).ok()?;
    command.changes().map(|part| part_name(&part).to_string())
}

fn part_name(part: &Part) -> &'static str {
    match part {
        Part::ProjectFile => "project",
        Part::Takeoff => "takeoff",
        Part::Boq => "boq",
        Part::Conflicts => "conflicts",
        Part::Document {
            tables: false,
            takeoff: false,
        } => "drawing",
        Part::Document {
            tables: true,
            takeoff: false,
        } => "drawing+tables",
        Part::Document { .. } => "drawing+tables+takeoff",
    }
}

/// What running `command` does: `Effect::Edit` when it is an undo step, else what its registry
/// entry says (a dry-run purge reads; a real one edits).
pub fn effect_of(command: &Command) -> Effect {
    if command.changes().is_some() {
        return Effect::Edit;
    }
    let name = serde_json::to_value(command)
        .ok()
        .and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_string));
    match name.as_deref().and_then(|n| registry().get(n)) {
        Some(info) if info.effect != Effect::Edit => info.effect,
        _ => Effect::Read,
    }
}
