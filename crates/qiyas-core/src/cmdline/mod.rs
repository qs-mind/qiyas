//! Typed commands, shared by the window, scripts and the AI. The registry supplies the words
//! and schemas; this machine only knows argument shapes. It never reads or changes a session.
//!
//! Required fields follow the schema's `required` order, then optional fields follow the
//! schema's properties. `x-prompts` overrides that order with field names or objects containing
//! `field`, `prompt`, `required`, `default`, `options`, `by_word`, `min`, `max`, `close`, `direction` or
//! `relative_to`. A `close`
//! field names the boolean set by C/Close; `relative_to` reads a through-point as a direction.

mod input;

use serde_json::{json, Map, Value};

use crate::registry::{parse_command, registry, CommandInfo};
use crate::{Command, Ucs};
use input::{integer, number, point, tokenize};

/// A completed command, the next question, or an explicit cancellation.
#[derive(Debug, Clone)]
pub enum Step {
    Command(Command),
    Prompt(String),
    Cancelled,
}

/// The state of one command line. Hosts supply the current UCS and the last picked point.
#[derive(Debug, Clone, Default)]
pub struct Machine {
    active: Option<Pending>,
    ucs: Ucs,
    last: Option<[f64; 2]>,
    completed_input: Option<Value>,
}

#[derive(Debug, Clone)]
struct Pending {
    name: String,
    fields: Vec<Argument>,
    next: usize,
    values: Map<String, Value>,
    before: Option<[f64; 2]>,
}

#[derive(Debug, Clone)]
struct Argument {
    name: String,
    schema: Value,
    metadata: Value,
    required: bool,
}

impl Machine {
    pub fn set_ucs(&mut self, ucs: Ucs) {
        self.ucs = ucs;
    }

    pub fn set_last(&mut self, point: Option<[f64; 2]>) {
        self.last = point;
    }

    pub fn last(&self) -> Option<[f64; 2]> {
        self.last
    }

    /// Takes the completed wire input, preserving omitted fields for command validation.
    pub fn take_input(&mut self) -> Option<Value> {
        self.completed_input.take()
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Cancels the pending arguments, keeping the UCS and last point.
    pub fn cancel(&mut self) {
        self.active = None;
        self.completed_input = None;
    }

    /// Reads a whole command or the answer to its next prompt. Fixed arguments complete as
    /// soon as they are present. A point list on the command's own line ends with that line;
    /// across prompt lines, Enter ends it and C closes it when its metadata allows that.
    /// Errors leave both the pending arguments and the last point unchanged for correction.
    pub fn step(&mut self, line: &str) -> Result<Step, String> {
        let mut next = self.clone();
        next.completed_input = None;
        let step = next.advance(line)?;
        *self = next;
        Ok(step)
    }

    fn advance(&mut self, line: &str) -> Result<Step, String> {
        let line = line.trim();
        if line.eq_ignore_ascii_case("ESC")
            || line.eq_ignore_ascii_case("CANCEL")
            || line == "\u{1b}"
        {
            self.cancel();
            return Ok(Step::Cancelled);
        }
        let mut words = tokenize(line);
        let starting = self.active.is_none();
        if starting {
            if words.is_empty() {
                return Ok(Step::Prompt("Command:".into()));
            }
            let word = words.remove(0);
            let info = registry()
                .by_word(&word)
                .ok_or_else(|| format!("unknown command {word}"))?;
            self.active = Some(Pending::new(info, &word, self.last)?);
        }
        let finish_list = (starting && !words.is_empty()) || line.is_empty();
        let mut pending = self.active.take().expect("a command is active");
        let mut index = 0;
        while pending.next < pending.fields.len() {
            let arg = pending.fields[pending.next].clone();
            if pending.values.contains_key(&arg.name) && !arg.is_list() {
                pending.next += 1;
                continue;
            }
            if words.get(index).is_none()
                && !arg.required
                && !pending.values.contains_key(&arg.name)
            {
                if let Some(default) = arg
                    .metadata
                    .get("default")
                    .filter(|default| Some(*default) != arg.schema.get("default"))
                {
                    pending.values.insert(arg.name.clone(), default.clone());
                }
                pending.next += 1;
                continue;
            }
            if arg.is_list() {
                let done = self.list(&mut pending, &arg, &words, &mut index, finish_list)?;
                if done {
                    pending.next += 1;
                    continue;
                }
                let prompt = arg.prompt();
                self.active = Some(pending);
                return Ok(Step::Prompt(prompt));
            }
            let Some(word) = words.get(index) else {
                if let Some(default) = arg.default() {
                    pending.values.insert(arg.name.clone(), default.clone());
                    pending.next += 1;
                    continue;
                }
                if !arg.required {
                    pending.next += 1;
                    continue;
                }
                let prompt = arg.prompt();
                self.active = Some(pending);
                return Ok(Step::Prompt(prompt));
            };
            let value = self.argument(&arg, word, &pending.values)?;
            pending.values.insert(arg.name, value);
            pending.next += 1;
            index += 1;
        }
        if let Some(extra) = words.get(index) {
            return Err(format!("{}: unexpected argument {extra}", pending.name));
        }
        let input = Value::Object(pending.values);
        let command = parse_command(input.clone())?;
        self.completed_input = Some(input);
        Ok(Step::Command(command))
    }

    fn argument(
        &mut self,
        arg: &Argument,
        word: &str,
        values: &Map<String, Value>,
    ) -> Result<Value, String> {
        if let Some(options) = arg.metadata.get("options").and_then(Value::as_object) {
            return options
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(word))
                .map(|(_, value)| value.clone())
                .ok_or_else(|| {
                    format!(
                        "{}: choose {}",
                        arg.name,
                        options.keys().cloned().collect::<Vec<_>>().join("/")
                    )
                });
        }
        if is_point(&arg.schema) {
            if arg.metadata.get("direction").and_then(Value::as_bool) == Some(true) {
                return point(
                    word,
                    Some([0., 0.]),
                    Ucs {
                        origin: [0., 0.],
                        ..self.ucs
                    },
                )
                .map(|p| json!(p));
            }
            let p = point(word, self.last, self.ucs)?;
            self.last = Some(p);
            if let Some(base) = arg.metadata.get("relative_to").and_then(Value::as_str) {
                let base = values
                    .get(base)
                    .and_then(value_point)
                    .ok_or_else(|| format!("{}: missing reference point {base}", arg.name))?;
                return Ok(json!([p[0] - base[0], p[1] - base[1]]));
            }
            return Ok(json!(p));
        }
        scalar(&arg.schema, word).map_err(|why| format!("{}: {why}", arg.name))
    }

    fn list(
        &mut self,
        pending: &mut Pending,
        arg: &Argument,
        words: &[String],
        index: &mut usize,
        finish: bool,
    ) -> Result<bool, String> {
        let min = arg.limit("min", "minItems").unwrap_or(0);
        let max = arg.limit("max", "maxItems");
        let item = arg.schema.get("items").unwrap_or(&Value::Null);
        let points = is_point(item);
        let mut entries = pending
            .values
            .get(&arg.name)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut closed = false;
        while let Some(word) = words.get(*index) {
            if max.is_some_and(|max| entries.len() >= max) {
                break;
            }
            if points && (word.eq_ignore_ascii_case("C") || word.eq_ignore_ascii_case("CLOSE")) {
                let Some(field) = arg.metadata.get("close").and_then(Value::as_str) else {
                    return Err(format!("{} does not take Close", pending.name));
                };
                if entries.len() < min {
                    return Err(format!("{} needs at least {min} points", arg.name));
                }
                pending.values.insert(field.into(), Value::Bool(true));
                *index += 1;
                closed = true;
                break;
            }
            if points && (word.eq_ignore_ascii_case("U") || word.eq_ignore_ascii_case("UNDO")) {
                entries.pop();
                self.last = entries.last().and_then(value_point).or(pending.before);
                *index += 1;
                continue;
            }
            let value = if points {
                match point(word, self.last, self.ucs) {
                    Ok(p) => {
                        self.last = Some(p);
                        json!(p)
                    }
                    Err(_) if entries.len() >= min && pending.next + 1 < pending.fields.len() => {
                        break
                    }
                    Err(error) => return Err(error),
                }
            } else {
                scalar(item, word).map_err(|why| format!("{}: {why}", arg.name))?
            };
            entries.push(value);
            *index += 1;
        }
        let enough = entries.len() >= min;
        let full = max.is_some_and(|max| entries.len() >= max);
        let next_field = *index < words.len() && pending.next + 1 < pending.fields.len();
        let done = closed || full || (enough && (finish || next_field));
        if finish && !enough && words.is_empty() {
            return Err(format!("{} needs at least {min} values", arg.name));
        }
        pending
            .values
            .insert(arg.name.clone(), Value::Array(entries));
        Ok(done)
    }
}

impl Pending {
    fn new(info: &CommandInfo, word: &str, before: Option<[f64; 2]>) -> Result<Self, String> {
        let mut properties = Map::new();
        let mut required = Vec::new();
        properties_of(&info.schema, &mut properties, &mut required);
        let mut values = Map::new();
        let mut fields = Vec::new();
        for (name, schema) in &properties {
            let constant = schema.get("const").or_else(|| {
                matches!(name.as_str(), "type" | "kind")
                    .then(|| schema.get("enum")?.as_array()?.first())
                    .flatten()
            });
            if let Some(value) = constant {
                values.insert(name.clone(), value.clone());
            }
        }
        let mut order: Vec<Value> = info.prompts.clone();
        let specified: Vec<String> = order
            .iter()
            .filter_map(field_name)
            .map(str::to_string)
            .collect();
        order.extend(
            required
                .iter()
                .chain(properties.keys())
                .filter(|name| !specified.contains(name))
                .map(|name| Value::String(name.clone())),
        );
        for metadata in order {
            let name = field_name(&metadata)
                .ok_or_else(|| format!("{}: invalid prompt metadata", info.name))?;
            if values.contains_key(name) || fields.iter().any(|arg: &Argument| arg.name == name) {
                continue;
            }
            let schema = properties
                .get(name)
                .ok_or_else(|| format!("{}: prompt field {name} is missing", info.name))?;
            if let Some(value) = metadata
                .get("by_word")
                .and_then(Value::as_object)
                .and_then(|options| {
                    options
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(word))
                })
                .map(|(_, value)| value)
            {
                values.insert(name.into(), value.clone());
                continue;
            }
            let required = metadata
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| required.iter().any(|field| field == name));
            // Optional complex data stays available through JSON commands. A simple typed
            // command does not ask for arrays of objects or nested records.
            if !required && !supported(schema) && !specified.iter().any(|field| field == name) {
                continue;
            }
            fields.push(Argument {
                name: name.into(),
                schema: schema.clone(),
                metadata,
                required,
            });
        }
        Ok(Self {
            name: info.name.clone(),
            fields,
            next: 0,
            values,
            before,
        })
    }
}

impl Argument {
    fn is_list(&self) -> bool {
        has_type(&self.schema, "array") && !is_point(&self.schema)
    }

    fn default(&self) -> Option<&Value> {
        self.metadata.get("default").or_else(|| {
            if self.metadata.get("required") == Some(&Value::Bool(true)) {
                None
            } else {
                self.schema.get("default")
            }
        })
    }

    fn limit(&self, metadata: &str, schema: &str) -> Option<usize> {
        self.metadata
            .get(metadata)
            .or_else(|| self.schema.get(schema))
            .and_then(Value::as_u64)
            .map(|n| n as usize)
    }

    fn prompt(&self) -> String {
        if let Some(text) = self.metadata.get("prompt").and_then(Value::as_str) {
            return text.to_string();
        }
        let name = self.name.replace('_', " ");
        let options = self
            .metadata
            .get("options")
            .and_then(Value::as_object)
            .map(|options| options.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_else(|| {
                enum_values(&self.schema)
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            });
        if options.is_empty() {
            format!("{name}:")
        } else {
            format!("{name} [{}]:", options.join("/"))
        }
    }
}

fn field_name(metadata: &Value) -> Option<&str> {
    metadata
        .as_str()
        .or_else(|| metadata.get("field").and_then(Value::as_str))
}

fn properties_of(schema: &Value, properties: &mut Map<String, Value>, required: &mut Vec<String>) {
    if let Some(fields) = schema.get("properties").and_then(Value::as_object) {
        properties.extend(fields.clone());
    }
    if let Some(names) = schema.get("required").and_then(Value::as_array) {
        required.extend(names.iter().filter_map(Value::as_str).map(str::to_string));
    }
    for part in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        properties_of(part, properties, required);
    }
}

fn has_type(schema: &Value, kind: &str) -> bool {
    schema.get("type").is_some_and(|value| {
        value == kind
            || value
                .as_array()
                .is_some_and(|types| types.iter().any(|t| t == kind))
    })
}

fn is_point(schema: &Value) -> bool {
    has_type(schema, "array")
        && schema.get("minItems").and_then(Value::as_u64) == Some(2)
        && schema.get("maxItems").and_then(Value::as_u64) == Some(2)
        && schema
            .get("items")
            .is_some_and(|item| has_type(item, "number"))
}

fn value_point(value: &Value) -> Option<[f64; 2]> {
    Some([value.get(0)?.as_f64()?, value.get(1)?.as_f64()?])
}

fn supported(schema: &Value) -> bool {
    ["string", "number", "integer", "boolean"]
        .iter()
        .any(|kind| has_type(schema, kind))
        || is_point(schema)
        || schema.get("items").is_some_and(supported)
        || !enum_values(schema).is_empty()
}

fn enum_values(schema: &Value) -> Vec<Value> {
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        return options.clone();
    }
    schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|schema| {
            schema
                .get("const")
                .cloned()
                .into_iter()
                .chain(enum_values(schema))
        })
        .collect()
}

fn scalar(schema: &Value, word: &str) -> Result<Value, String> {
    let choices = enum_values(schema);
    if !choices.is_empty() {
        return choices
            .iter()
            .find(|choice| {
                choice
                    .as_str()
                    .is_some_and(|text| text.eq_ignore_ascii_case(word))
            })
            .cloned()
            .ok_or_else(|| {
                format!(
                    "choose {}",
                    choices
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("/")
                )
            });
    }
    if has_type(schema, "string") {
        return Ok(Value::String(word.to_string()));
    }
    if has_type(schema, "integer") {
        return integer(word)
            .map(Value::Number)
            .ok_or_else(|| format!("{word} is not a whole number in the integer range"));
    }
    if has_type(schema, "number") {
        let value = number(word).ok_or_else(|| format!("{word} is not a number"))?;
        return Ok(json!(value));
    }
    if has_type(schema, "boolean") {
        return match word.to_ascii_uppercase().as_str() {
            "TRUE" | "YES" | "ON" | "1" => Ok(Value::Bool(true)),
            "FALSE" | "NO" | "OFF" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("{word} is not Yes/No")),
        };
    }
    if word.eq_ignore_ascii_case("null") && has_type(schema, "null") {
        return Ok(Value::Null);
    }
    Err("this argument needs a JSON command".into())
}
