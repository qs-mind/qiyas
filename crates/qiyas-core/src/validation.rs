//! Input checks shared by the command bus and every JSON caller. This deliberately implements
//! only the JSON Schema vocabulary emitted by our registry, without a validator dependency.

use serde::Serialize;
use serde_json::{json, Value};

use crate::{registry::registry, Session, Units};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputIssue {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DefaultUsed {
    pub field: String,
    pub value: Value,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<InputIssue>,
    pub warnings: Vec<InputIssue>,
    pub defaults_used: Vec<DefaultUsed>,
    pub not_checked: Vec<String>,
    /// Complete registry contracts for commands with missing fields, including nested stubs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repairs: Vec<Value>,
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            defaults_used: Vec::new(),
            not_checked: vec![
                "geometry".into(),
                "locked layers".into(),
                "handles and named objects existing".into(),
                "file access and host availability".into(),
                "values returned by earlier steps".into(),
            ],
            repairs: Vec::new(),
        }
    }
}

impl ValidationReport {
    pub(crate) fn reject_unknown_fields(&mut self) {
        let warnings = std::mem::take(&mut self.warnings);
        for issue in warnings {
            if issue.code == "unknown_field" {
                self.error(
                    &issue.code,
                    &issue.pointer,
                    issue
                        .message
                        .replace("; it is ignored", "; remove it or use a documented field"),
                );
            } else {
                self.warnings.push(issue);
            }
        }
    }

    fn deduplicate(&mut self) {
        let mut seen = std::collections::BTreeSet::new();
        self.errors
            .retain(|e| seen.insert((e.code.clone(), e.pointer.clone(), e.message.clone())));
        seen.clear();
        self.warnings
            .retain(|e| seen.insert((e.code.clone(), e.pointer.clone(), e.message.clone())));
        let mut fields = std::collections::BTreeSet::new();
        self.defaults_used
            .retain(|default| fields.insert(default.field.clone()));
        let mut notes = std::collections::BTreeSet::new();
        self.not_checked.retain(|note| notes.insert(note.clone()));
    }
    fn error(&mut self, code: &str, pointer: &str, message: impl Into<String>) {
        self.valid = false;
        self.errors.push(InputIssue {
            code: code.into(),
            pointer: pointer.into(),
            message: message.into(),
        });
    }

    pub(crate) fn append(&mut self, other: Self) {
        self.valid &= other.valid;
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self.defaults_used.extend(other.defaults_used);
        self.not_checked.extend(other.not_checked);
        self.repairs.extend(other.repairs);
    }

    pub fn message(&self) -> String {
        self.errors
            .iter()
            .map(|issue| format!("{} at {}: {}", issue.code, issue.pointer, issue.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Checks a command without changing the drawing. References are only allowed in a batch.
pub fn validate_command(value: &Value, units: Option<Units>) -> ValidationReport {
    let mut report = validate_at(value, "", units, None);
    if value["type"]
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("batch"))
    {
        batch_context_notes(&mut report);
    }
    if units.is_none() {
        report
            .not_checked
            .push("drawing units, sheet and annotation scale for automatic sizes".into());
    }
    report
}

/// Checks all batch inputs, including references to earlier step results.
pub fn validate_batch(commands: &[Value], units: Option<Units>) -> ValidationReport {
    let mut report = ValidationReport::default();
    batch_context_notes(&mut report);
    for (index, command) in commands.iter().enumerate() {
        report.append(validate_at(
            command,
            &format!("/commands/{index}"),
            units,
            Some(index),
        ));
    }
    if units.is_none() {
        report
            .not_checked
            .push("drawing units, sheet and annotation scale for automatic sizes".into());
    }
    report
}

fn batch_context_notes(report: &mut ValidationReport) {
    report.not_checked.extend([
        "defaults affected by earlier steps changing the sheet or annotation scale".into(),
        "record-specific set_records checks after earlier steps change or create records".into(),
    ]);
}

impl Session {
    /// The same dry input check with the current drawing's sizes and annotation scale.
    pub fn validate_input(&self, input: &Value) -> ValidationReport {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("validation.validate_input");
        let _read = self.evaluation_read().ok();
        let mut canonical = input.clone();
        crate::registry::canonicalize(&mut canonical);
        let mut report = validate_command(input, Some(self.drawing.units));
        let input = &canonical;
        report.append(self.validate_record_inputs(input));
        if report.valid {
            self.validate_batch_knowledge(input, &mut report);
        }
        self.resolve_input_defaults(input, "", &mut report.defaults_used);
        report.deduplicate();
        report
    }

    /// Stage knowledge in command order. The live drawing and its undo history are untouched;
    /// every kind shares the same document, including types created by schedule interpretation.
    fn validate_batch_knowledge(&self, input: &Value, report: &mut ValidationReport) {
        let Some(commands) = input.get("commands").and_then(Value::as_array) else {
            return;
        };
        let is_definition = |value: &Value| {
            matches!(
                value["type"].as_str(),
                Some(
                    "define_object_type"
                        | "redefine_object_type"
                        | "delete_object_type"
                        | "set_knowledge"
                        | "define_check"
                )
            ) || (value["type"] == "recognition"
                && matches!(value["kind"].as_str(), Some("define" | "schedule")))
                || (value["type"] == "measurement"
                    && matches!(value["kind"].as_str(), Some("set_method" | "set_mappings")))
        };
        if !commands.iter().any(is_definition) {
            return;
        }
        let mut staged =
            Session::new_blank(&self.drawing.path, self.drawing.doc.header.insertion_units);
        staged.drawing.doc = self.drawing.doc.clone();
        staged.sheet = self.sheet.clone();
        let mut source_changed = false;
        for (index, value) in commands.iter().enumerate() {
            if !is_definition(value) {
                // Geometry-dependent validation still belongs to execution, where preceding
                // drawing edits and resolved step references are available.
                source_changed |= crate::registry::parse_command(value.clone()).map_or(true, |c| {
                    crate::registry::effect_of(&c) != crate::registry::Effect::Read
                });
                continue;
            }
            let command = crate::registry::parse_command(value.clone());
            if command.is_err()
                || (source_changed && value["type"] == "recognition" && value["kind"] == "schedule")
            {
                // Do not reject later references against stale source cells or unresolved
                // step values. The transaction validates them against the actual earlier steps.
                report.not_checked.push("knowledge depending on earlier drawing edits or step results; checked in execution order".into());
                break;
            };
            if let Err(error) = staged.execute(command.expect("parsed above"), crate::Source::User)
            {
                report.error(
                    "invalid_value",
                    &format!("/commands/{index}"),
                    error.to_string(),
                );
                break;
            }
        }
    }

    fn resolve_input_defaults(&self, input: &Value, prefix: &str, defaults: &mut Vec<DefaultUsed>) {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("validation.resolve_input_defaults");
        resolve_defaults(
            input,
            prefix,
            Some(self.drawing.units),
            self.annotation_scale().1,
            defaults,
        );
        let name = input["type"].as_str().unwrap_or_default();
        let field_and_value = match name {
            "text" | "mtext" | "att_def" | "field" => Some(("height", self.default_text_height())),
            "paste_from_clipboard"
                if input["text"]
                    .as_str()
                    .is_some_and(|text| !text.starts_with(crate::clipboard::CLIPBOARD_MARK)) =>
            {
                Some(("height", self.default_text_height()))
            }
            "center_mark" => Some(("extension", self.default_text_height())),
            "grid" => Some(("bubble", self.default_text_height() * 3.5 / 2.5)),
            _ => None,
        };
        if let Some((field, value)) = field_and_value {
            if let Some(default) = defaults
                .iter_mut()
                .find(|d| d.field == format!("{prefix}/{field}"))
            {
                default.value = json!(value);
                if !self.sheet.eq_ignore_ascii_case("*Model_Space") {
                    default.unit = Some("mm".into());
                }
            }
        }
        if name == "table" {
            let data: Vec<Vec<String>> =
                serde_json::from_value(input["data"].clone()).unwrap_or_default();
            let (width, height) = self.table_sizes(
                input["col_width"].as_f64().unwrap_or(0.0),
                input["row_height"].as_f64().unwrap_or(0.0),
                &data,
            );
            for (field, value) in [("col_width", width), ("row_height", height)] {
                if input.get(field).is_none_or(|v| v.as_f64() == Some(0.0)) {
                    let path = format!("{prefix}/{field}");
                    defaults.retain(|d| d.field != path);
                    defaults.push(DefaultUsed {
                        field: path,
                        value: json!(value),
                        unit: Some(
                            if self.sheet.eq_ignore_ascii_case("*Model_Space") {
                                unit_name(self.drawing.units)
                            } else {
                                "mm"
                            }
                            .into(),
                        ),
                    });
                }
            }
        }
        let mut resolved = Vec::new();
        let unit = if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            unit_name(self.drawing.units)
        } else {
            "mm"
        };
        let mut set = |field: &str, value: Value, unit: Option<&str>| {
            resolved.push(DefaultUsed {
                field: format!("{prefix}/{field}"),
                value,
                unit: unit.map(str::to_owned),
            });
        };
        if matches!(name, "text" | "mtext")
            && input.get("style").is_none_or(|value| {
                value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty())
            })
        {
            set("style", json!(self.text_style()), None);
        }
        if name == "dim_space" && input.get("spacing").is_none_or(Value::is_null) {
            if let Ok(base) = serde_json::from_value(input["base"].clone()) {
                if let Ok(value) = self.dimension_spacing_default(&base) {
                    set("spacing", json!(value), Some(unit));
                }
            }
        }
        if name == "verify" {
            if let Some(expectations) = input["expectations"].as_array() {
                for (index, input) in expectations.iter().enumerate() {
                    if input.get("tolerance").is_none_or(Value::is_null) {
                        if let Ok(expectation) =
                            serde_json::from_value::<crate::Expectation>(input.clone())
                        {
                            let kind = input["kind"].as_str().unwrap_or_default();
                            let unit = if matches!(
                                kind,
                                "angle" | "parallel" | "perpendicular" | "horizontal" | "vertical"
                            ) {
                                "degrees".to_string()
                            } else if matches!(kind, "count" | "no_self_intersection") {
                                "count".to_string()
                            } else if kind == "area" {
                                format!(
                                    "{}²",
                                    if expectation.in_metres {
                                        "m"
                                    } else {
                                        unit_name(self.drawing.units)
                                    }
                                )
                            } else if expectation.in_metres {
                                "m".to_string()
                            } else {
                                unit_name(self.drawing.units).to_string()
                            };
                            set(
                                &format!("expectations/{index}/tolerance"),
                                json!(self.verification_default_tolerance(&expectation)),
                                Some(&unit),
                            );
                        }
                    }
                }
            }
        }
        for default in resolved {
            defaults.retain(|previous| previous.field != default.field);
            defaults.push(default);
        }
        for mut default in self.contextual_defaults(input) {
            default.field = format!("{prefix}{}", default.field);
            defaults.retain(|previous| previous.field != default.field);
            defaults.push(default);
        }
        if let Some(commands) = input.get("commands").and_then(Value::as_array) {
            for (i, command) in commands.iter().enumerate() {
                self.resolve_input_defaults(command, &format!("{prefix}/commands/{i}"), defaults);
            }
        }
    }
}

fn unit_name(units: Units) -> &'static str {
    match units {
        Units::Inches => "in",
        Units::Feet => "ft",
        Units::Centimetres => "cm",
        Units::Metres => "m",
        Units::Millimetres | Units::Unknown => "mm",
    }
}

fn validate_at(
    value: &Value,
    path: &str,
    units: Option<Units>,
    step: Option<usize>,
) -> ValidationReport {
    let mut report = ValidationReport::default();
    if !value.is_object() {
        report.error("invalid_value", path, "A command must be an object");
        return report;
    }
    let Some(name) = value.get("type").and_then(Value::as_str) else {
        report.error(
            if value.get("type").is_none() {
                "missing_field"
            } else {
                "invalid_value"
            },
            &pointer(path, "type"),
            "Give a literal command type",
        );
        return report;
    };
    let reg = registry();
    let Some(top) = reg.get(name) else {
        report.error(
            "not_found",
            &pointer(path, "type"),
            format!("No command {name}"),
        );
        return report;
    };
    let info = value
        .get("kind")
        .and_then(Value::as_str)
        .and_then(|kind| reg.get(&format!("{name}.{kind}")))
        .unwrap_or(top);
    let mut canonical = value.clone();
    crate::registry::canonicalize(&mut canonical);
    walk(&info.schema, &canonical, path, step, &mut report);
    report.warnings.extend(
        crate::registry::normalization_warnings(value)
            .into_iter()
            .map(|mut w| {
                w.pointer = format!("{path}{}", w.pointer);
                w
            }),
    );
    if info.name == "set_layer" && canonical.get("prop").is_some() {
        for key in [
            "color",
            "linetype",
            "lineweight",
            "on",
            "frozen",
            "locked",
            "plot",
        ] {
            if canonical.get(key).is_some() {
                report.error(
                    "invalid_value",
                    &pointer(path, key),
                    "Use either prop/value or named layer properties, not both",
                );
            }
        }
    }
    if info.name == "delete_entities"
        && canonical.get("targets").is_some()
        && canonical.get("handles").is_some()
    {
        report.error(
            "invalid_value",
            path,
            "Use either targets or handles, not both",
        );
    }
    if report
        .errors
        .iter()
        .any(|issue| issue.code == "missing_field")
    {
        let mut repair = crate::takeoff_kit::contract(info);
        repair["command"] = json!(info.name);
        repair["pointer"] = json!(path);
        report.repairs.push(repair);
    }
    if matches!(
        info.name.as_str(),
        "title_block" | "mview" | "set_layout_paper"
    ) {
        report.not_checked.push("active layout, its paper size, and whether annotation or viewport frames fit the paper; use new_layout or set_layout_paper for a sized sheet".into());
    }
    if info.name == "mview" {
        let fitted = canonical
            .get("model_bounds")
            .is_some_and(|value| !value.is_null());
        let required = if fitted {
            ["scale", "paper_centre"]
        } else {
            ["corner1", "corner2"]
        };
        for field in required {
            if canonical.get(field).is_none_or(Value::is_null) {
                report.error("missing_field", &pointer(path, field), if fitted {
                    "model_bounds requires scale (for example \"1:100\") and paper_centre: [x, y] in paper millimetres"
                } else {
                    "Give corner1 and corner2 as [x, y] in paper millimetres, or model_bounds with scale and paper_centre"
                });
            }
        }
        for field in if fitted {
            &["corner1", "corner2", "centre"][..]
        } else {
            &["paper_centre"][..]
        } {
            if canonical.get(field).is_some_and(|value| !value.is_null())
                && !(fitted
                    && field.starts_with("corner")
                    && canonical[*field] == json!([0.0, 0.0]))
            {
                report.error("invalid_value", &pointer(path, field), "Use either corner1/corner2 with optional centre, or model_bounds with scale and paper_centre");
            }
        }
    }
    if matches!(info.name.as_str(), "create_entities" | "define_block") {
        if let Some(entities) = canonical["entities"].as_array() {
            for (index, entity) in entities.iter().enumerate() {
                let Some(kind) = entity["type"].as_str() else {
                    continue;
                };
                let entity_path = format!("{path}/entities/{index}");
                match crate::record_edit::record_input_schema(kind) {
                    Ok(schema) => {
                        if let Err(error) = crate::record_edit::validate_entity_creation(kind) {
                            report.error(
                                "refused",
                                &format!("{entity_path}/type"),
                                error.to_string(),
                            );
                            continue;
                        }
                        if let Some(data) = entity.get("data").filter(|data| !data.is_null()) {
                            walk(
                                &schema,
                                data,
                                &format!("{entity_path}/data"),
                                step,
                                &mut report,
                            );
                        }
                    }
                    Err(error) => report.error(
                        "not_found",
                        &format!("{entity_path}/type"),
                        error.to_string(),
                    ),
                }
            }
        }
    }
    if info.effect != crate::registry::Effect::Read {
        report.reject_unknown_fields();
    }
    if info.name == "batch" {
        if let Some(commands) = canonical["commands"].as_array() {
            for (index, command) in commands.iter().enumerate() {
                report.append(validate_at(
                    command,
                    &format!("{path}/commands/{index}"),
                    units,
                    Some(index),
                ));
            }
        }
    }
    resolve_defaults(&canonical, path, units, 100.0, &mut report.defaults_used);
    report.deduplicate();
    report
}

fn matches_type(kind: &str, value: &Value) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => value.as_f64().is_some_and(|n| n.fract() == 0.0),
        _ => true,
    }
}

fn reference(value: &Value, path: &str, step: Option<usize>, out: &mut ValidationReport) -> bool {
    let Some(object) = value.as_object().filter(|o| o.contains_key("$step")) else {
        return false;
    };
    let index = object["$step"].as_u64();
    if !matches!((index, step), (Some(index), Some(step)) if index < step as u64) {
        out.error(
            "invalid_value",
            &pointer(path, "$step"),
            "A step reference must name an earlier step index",
        );
    }
    for key in object
        .keys()
        .filter(|key| !matches!(key.as_str(), "$step" | "pointer"))
    {
        out.warnings.push(InputIssue {
            code: "unknown_field".into(),
            pointer: pointer(path, key),
            message: format!("Unknown reference field {key}; it is ignored"),
        });
    }
    if object.get("pointer").is_some_and(|p| {
        !p.is_string()
            || !p
                .as_str()
                .is_some_and(|s| s.is_empty() || s.starts_with('/'))
    }) {
        out.error(
            "invalid_value",
            &pointer(path, "pointer"),
            "A reference pointer must be a JSON pointer",
        );
    }
    true
}

/// Walks the registry's schema, collecting every independent error and unknown field.
pub fn validate_schema(schema: &Value, value: &Value) -> ValidationReport {
    let mut report = ValidationReport::default();
    walk(schema, value, "", None, &mut report);
    report
}

fn scalar_choices(schema: &Value) -> Option<Vec<Value>> {
    if let Some(value) = schema
        .get("const")
        .filter(|v| !v.is_object() && !v.is_array())
    {
        return Some(vec![value.clone()]);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .all(|v| !v.is_object() && !v.is_array())
            .then(|| values.clone());
    }
    let variants = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))?
        .as_array()?;
    let mut choices = Vec::new();
    for variant in variants {
        for value in scalar_choices(variant)? {
            if !choices.contains(&value) {
                choices.push(value);
            }
        }
    }
    Some(choices)
}

fn walk(
    schema: &Value,
    value: &Value,
    path: &str,
    step: Option<usize>,
    out: &mut ValidationReport,
) {
    if reference(value, path, step, out) {
        return;
    }
    if schema == &Value::Bool(false) {
        out.error("invalid_value", path, "This value is not allowed");
        return;
    }
    if schema == &Value::Bool(true) || schema.as_object().is_some_and(|object| object.is_empty()) {
        untyped_references(value, path, step, out);
        return;
    }
    if let Some(variants) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        // Documented scalar variants may be separate const/enum branches. Report the union.
        if let Some(choices) = scalar_choices(schema) {
            if !choices.contains(value) {
                out.error(
                    "invalid_value",
                    path,
                    format!("Choose one of {}", json!(choices)),
                );
            }
        } else {
            let candidates: Vec<_> = variants
                .iter()
                .map(|variant| {
                    let mut candidate = ValidationReport::default();
                    walk(variant, value, path, step, &mut candidate);
                    (variant, candidate)
                })
                .collect();
            // Prefer a matching discriminator, so a missing field does not disguise a known kind.
            let chosen = candidates.iter().min_by_key(|(variant, report)| {
                let mut mismatches = 0;
                let mut matches = 0;
                if let Some(props) = variant.get("properties").and_then(Value::as_object) {
                    for (key, schema) in props {
                        if let Some(constant) = schema.get("const") {
                            if value.get(key) == Some(constant) {
                                matches += 1;
                            } else if value.get(key).is_some() {
                                mismatches += 1;
                            }
                        } else if value.get(key).is_some() {
                            matches += 1;
                        }
                    }
                }
                (
                    mismatches,
                    std::cmp::Reverse(matches),
                    report.errors.len(),
                    report.warnings.len(),
                )
            });
            if let Some((_, chosen)) = chosen {
                out.append(chosen.clone());
            }
        }
    }
    if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
        // Object schemas emitted for flattened enums share the same object. Merge their
        // properties first; otherwise each part would call the other part's fields unknown.
        let mut merged = json!({"properties": {}, "required": []});
        for part in parts {
            if let Some(props) = part.get("properties").and_then(Value::as_object) {
                merged["properties"]
                    .as_object_mut()
                    .unwrap()
                    .extend(props.clone());
            }
            if let Some(required) = part.get("required").and_then(Value::as_array) {
                merged["required"]
                    .as_array_mut()
                    .unwrap()
                    .extend(required.clone());
            }
            if let Some(additional) = part.get("additionalProperties") {
                merged["additionalProperties"] = additional.clone();
            }
            let mut rest = part.clone();
            if let Some(object) = rest.as_object_mut() {
                object.remove("properties");
                object.remove("required");
                object.remove("additionalProperties");
            }
            walk(&rest, value, path, step, out);
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            merged["properties"]
                .as_object_mut()
                .unwrap()
                .extend(props.clone());
        }
        walk(&merged, value, path, step, out);
    }
    if let Some(expected) = schema.get("type") {
        let valid = expected
            .as_str()
            .map(|t| matches_type(t, value))
            .unwrap_or_else(|| {
                expected.as_array().is_none_or(|types| {
                    types
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|t| matches_type(t, value))
                })
            });
        if !valid {
            out.error("invalid_value", path, format!("Expected {expected}"));
            return;
        }
    }
    if schema
        .get("const")
        .is_some_and(|constant| constant != value)
    {
        out.error(
            "invalid_value",
            path,
            format!("Expected {}", schema["const"]),
        );
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.contains(value) {
            out.error(
                "invalid_value",
                path,
                format!("Choose one of {}", schema["enum"]),
            );
        }
    }
    if let Some(number) = value.as_f64() {
        for (key, bad) in [
            (
                "minimum",
                schema["minimum"]
                    .as_f64()
                    .is_some_and(|bound| number < bound),
            ),
            (
                "maximum",
                schema["maximum"]
                    .as_f64()
                    .is_some_and(|bound| number > bound),
            ),
            (
                "exclusiveMinimum",
                schema["exclusiveMinimum"]
                    .as_f64()
                    .is_some_and(|bound| number <= bound),
            ),
            (
                "exclusiveMaximum",
                schema["exclusiveMaximum"]
                    .as_f64()
                    .is_some_and(|bound| number >= bound),
            ),
        ] {
            if bad {
                out.error(
                    "invalid_value",
                    path,
                    format!("Number violates {key} {}", schema[key]),
                );
            }
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    out.error(
                        "missing_field",
                        &pointer(path, key),
                        format!("Missing required field {key}"),
                    );
                }
            }
        }
        if schema.get("allOf").is_none() {
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (key, sub) in props {
                    if let Some(value) = object.get(key) {
                        walk(sub, value, &pointer(path, key), step, out);
                    } else if let Some(default) = sub.get("default") {
                        out.defaults_used.push(DefaultUsed {
                            field: pointer(path, key),
                            value: default.clone(),
                            unit: None,
                        });
                    }
                }
                for (key, value) in object.iter().filter(|(key, _)| !props.contains_key(*key)) {
                    match schema.get("additionalProperties") {
                        Some(Value::Bool(true)) => {
                            untyped_references(value, &pointer(path, key), step, out)
                        }
                        Some(sub @ Value::Object(_)) => {
                            walk(sub, value, &pointer(path, key), step, out)
                        }
                        _ => out.warnings.push(InputIssue {
                            code: "unknown_field".into(),
                            pointer: pointer(path, key),
                            message: format!("Unknown field {key}; it is ignored"),
                        }),
                    }
                }
            } else if let Some(sub) = schema.get("additionalProperties") {
                for (key, value) in object {
                    walk(sub, value, &pointer(path, key), step, out);
                }
            }
        }
    }
    if let Some(items) = value.as_array() {
        for (key, bad) in [
            (
                "minItems",
                schema["minItems"]
                    .as_u64()
                    .is_some_and(|n| items.len() < n as usize),
            ),
            (
                "maxItems",
                schema["maxItems"]
                    .as_u64()
                    .is_some_and(|n| items.len() > n as usize),
            ),
        ] {
            if bad {
                out.error(
                    "invalid_value",
                    path,
                    format!("Array violates {key} {}", schema[key]),
                );
            }
        }
        for (index, value) in items.iter().enumerate() {
            if let Some(sub) = schema
                .get("prefixItems")
                .and_then(|v| v.get(index))
                .or_else(|| schema.get("items"))
            {
                walk(sub, value, &pointer(path, &index.to_string()), step, out);
            }
        }
    }
}

fn untyped_references(value: &Value, path: &str, step: Option<usize>, out: &mut ValidationReport) {
    if reference(value, path, step, out) {
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                untyped_references(value, &pointer(path, key), step, out);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                untyped_references(value, &pointer(path, &index.to_string()), step, out);
            }
        }
        _ => {}
    }
}

/// Resolves conventional lengths after the schema has found omitted fields. The same values
/// are used by typed commands (zero/None selects the convention) and by the dry run.
pub(crate) fn resolve_defaults(
    input: &Value,
    path: &str,
    units: Option<Units>,
    annotation_scale: f64,
    defaults: &mut Vec<DefaultUsed>,
) {
    let name = input["type"].as_str().unwrap_or_default();
    let mut set = |field: &str, value: Value, unit: Option<&str>| {
        let field = format!("{path}/{field}");
        defaults.retain(|d| d.field != field);
        defaults.push(DefaultUsed {
            field,
            value,
            unit: unit.map(str::to_string),
        });
    };
    let automatic = |field: &str| {
        input
            .get(field)
            .is_none_or(|v| v.is_null() || v.as_f64() == Some(0.0))
    };
    let drawing_unit = units.map(unit_name);
    let length = |metres: f64| {
        units.map_or_else(
            || json!({"metres":metres, "requires":"drawing units"}),
            |units| json!(metres / units.to_metres()),
        )
    };
    let paper_length = |millimetres: f64| {
        if units.is_none() {
            json!({"paper_mm": millimetres, "requires": "drawing units, sheet and annotation scale"})
        } else {
            length(millimetres * 0.001 * annotation_scale)
        }
    };
    if matches!(name, "text" | "mtext" | "att_def" | "field") && automatic("height") {
        set("height", paper_length(2.5), drawing_unit);
    }
    if name == "paste_from_clipboard"
        && input["text"]
            .as_str()
            .is_some_and(|text| !text.starts_with(crate::clipboard::CLIPBOARD_MARK))
        && input.get("height").is_none_or(Value::is_null)
    {
        set("height", paper_length(2.5), drawing_unit);
    }
    if (name == "comment" || name == "draw" && input["kind"] == "revcloud")
        && input.get("arc_len").is_none_or(Value::is_null)
    {
        if let Ok(mut points) =
            serde_json::from_value::<crate::PointRun<true>>(input["points"].clone())
        {
            if let Some(units) = units {
                points.normalize(units, true);
            }
            set(
                "arc_len",
                json!(crate::draw::default_cloud_arc_len(&points)),
                drawing_unit.or(Some("drawing_units")),
            );
        }
    }
    if name == "edit" && input["kind"] == "overkill" && input.get("tol").is_none_or(Value::is_null)
    {
        set(
            "tol",
            units.map_or_else(
                || json!({"metres":1e-9,"requires":"drawing units"}),
                |units| json!(crate::modify2::default_overkill_tol(units)),
            ),
            drawing_unit,
        );
    }
    if name == "dimension"
        && input["kind"] == "linear"
        && input.get("rotation").is_none_or(Value::is_null)
    {
        if let (Ok(p1), Ok(p2), Ok(line_point)) = (
            serde_json::from_value(input["p1"].clone()),
            serde_json::from_value(input["p2"].clone()),
            serde_json::from_value(input["line_point"].clone()),
        ) {
            set(
                "rotation",
                json!(crate::dimension::auto_angle(p1, p2, line_point).to_degrees()),
                Some("degrees"),
            );
        }
    }
    if name == "records" && input["query"].get("limit").is_none_or(Value::is_null) {
        set("query/limit", json!(20), None);
    }
    if name == "title_block" {
        for (field, value) in [
            ("margin_mm", crate::layouts::default_margin()),
            ("binding_mm", crate::layouts::default_binding()),
        ] {
            if input.get(field).is_none() {
                set(field, json!(value), Some("mm"));
            }
        }
    }
    if name == "walls" {
        let height = input["height"].as_f64().unwrap_or(3.0);
        if input.get("height").is_none() {
            set("height", json!(height), Some("m"));
        }
        if let Some(openings) = input["openings"].as_array() {
            for (i, opening) in openings.iter().enumerate() {
                let run_height = opening["run"]
                    .as_u64()
                    .and_then(|run| input["runs"].get(run as usize))
                    .and_then(|run| run["height"].as_f64())
                    .unwrap_or(height);
                for (field, value) in [
                    (
                        "sill",
                        if opening["kind"] == "window" {
                            0.9
                        } else {
                            0.0
                        },
                    ),
                    ("head", run_height.min(2.1)),
                ] {
                    if opening.get(field).is_none_or(Value::is_null) {
                        set(&format!("openings/{i}/{field}"), json!(value), Some("m"));
                    }
                }
            }
        }
        for (field, value) in [
            ("outline", "A-WALL"),
            ("hatch", "A-WALL-PATT"),
            ("centreline", "A-WALL-CNTR"),
            ("doors", "A-DOOR"),
            ("windows", "A-GLAZ"),
        ] {
            if input["layers"].get(field).is_none_or(Value::is_null) {
                set(&format!("layers/{field}"), json!(value), None);
            }
        }
    }
    if name == "center_mark" && input.get("extension").is_none_or(Value::is_null) {
        set("extension", paper_length(2.5), drawing_unit);
    }
    if name == "grid" && input.get("bubble").is_none_or(Value::is_null) {
        set("bubble", paper_length(3.5), drawing_unit);
    }
    if matches!(name, "rooms" | "label_spaces")
        && input["query"]
            .get("subtract_islands")
            .is_none_or(Value::is_null)
    {
        set("query/subtract_islands", json!(true), None);
    }
    if name == "label_spaces" && input.get("decimals").is_none_or(Value::is_null) {
        set("decimals", json!(2), None);
    }
    if name == "draw" && input["kind"] == "mline" && input.get("scale").is_none_or(Value::is_null) {
        set("scale", length(0.02), drawing_unit);
    }
    if name == "draw" && input["kind"] == "mline" && input.get("style").is_none_or(Value::is_null) {
        set("style", json!("Standard"), None);
    }
}
