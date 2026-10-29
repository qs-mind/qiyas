//! The JSON command boundary. Wire input keeps its omitted fields here; the command bus and
//! its outcomes remain ordinary enums for native Rust callers.

use serde::Serialize;
use serde_json::{json, Value};

use crate::validation::{DefaultUsed, InputIssue, ValidationReport};
use crate::{Command, CoreError, Outcome, Session, Source};

/// A successful wire command, with the input conventions and ignored fields it used.
#[derive(Debug, Clone)]
pub struct Reported {
    pub outcome: Outcome,
    pub revision: crate::Revision,
    pub advice: Vec<Value>,
    pub handle_map: std::collections::BTreeMap<u64, Vec<u64>>,
    pub defaults_used: Vec<DefaultUsed>,
    pub warnings: Vec<InputIssue>,
}

impl Reported {
    /// A readable version of the same metadata for scripts and command-line logs.
    pub fn input_notes(&self) -> String {
        let defaults: Vec<_> = self
            .defaults_used
            .iter()
            .map(|default| {
                format!(
                    "{}={}{}",
                    default.field,
                    default.value,
                    default
                        .unit
                        .as_ref()
                        .map_or_else(String::new, |unit| format!(" {unit}"))
                )
            })
            .collect();
        let mut notes = Vec::new();
        if !defaults.is_empty() {
            notes.push(format!("Defaults: {}", defaults.join(", ")));
        }
        notes.extend(
            self.warnings
                .iter()
                .map(|warning| format!("Warning {}: {}", warning.pointer, warning.message)),
        );
        notes.extend(
            self.advice
                .iter()
                .filter_map(|a| a["message"].as_str())
                .map(|m| format!("Advice: {m}")),
        );
        notes.join("; ")
    }
}

impl Serialize for Reported {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(&self.outcome).map_err(serde::ser::Error::custom)?;
        metadata(&mut value, &self.defaults_used, &self.warnings);
        value["revision"] = json!(self.revision);
        value["advice"] = json!(self.advice);
        value["handle_map"] = json!(self.handle_map);
        value.serialize(serializer)
    }
}

/// Keep the existing wire shape, including each batch step's metadata. Prefixes in the
/// aggregate report are the single source for both the whole batch and its individual steps.
fn metadata(value: &mut Value, defaults: &[DefaultUsed], warnings: &[InputIssue]) {
    if defaults.is_empty() && warnings.is_empty() {
        return;
    }
    if value["type"] == "batch" {
        if let Some(outcomes) = value["outcomes"].as_array_mut() {
            for (index, outcome) in outcomes.iter_mut().enumerate() {
                let prefix = format!("/commands/{index}/");
                let defaults: Vec<_> = defaults
                    .iter()
                    .filter_map(|default| {
                        let field = default.field.strip_prefix(&prefix)?;
                        Some(DefaultUsed {
                            field: format!("/{field}"),
                            ..default.clone()
                        })
                    })
                    .collect();
                let warnings: Vec<_> = warnings
                    .iter()
                    .filter_map(|warning| {
                        let pointer = warning.pointer.strip_prefix(&prefix)?;
                        Some(InputIssue {
                            pointer: format!("/{pointer}"),
                            ..warning.clone()
                        })
                    })
                    .collect();
                metadata(outcome, &defaults, &warnings);
            }
        }
    }
    value["defaults_used"] = json!(defaults);
    value["warnings"] = json!(warnings);
}

pub(crate) fn with_warnings(error: CoreError, warnings: &[InputIssue]) -> CoreError {
    if warnings.is_empty() {
        error
    } else {
        CoreError::InputWarnings {
            error: Box::new(error),
            warnings: warnings.to_vec(),
        }
    }
}

impl Session {
    /// Validates the complete raw input, reads a plain command, resolves omitted defaults in
    /// the current session, and executes it. Every JSON boundary uses this entry point.
    pub fn run_input(&mut self, raw: &Value, source: Source) -> Result<Reported, CoreError> {
        let mut input = raw.clone();
        crate::registry::canonicalize(&mut input);
        let targeted = input["sheet"]
            .as_str()
            .filter(|_| {
                input["type"] == "batch"
                    || input["type"]
                        .as_str()
                        .and_then(|name| crate::registry::registry().get(name))
                        .is_some_and(|info| info.effect == crate::registry::Effect::Edit)
            })
            .map(str::to_owned);
        let Some(sheet) = targeted else {
            return self.run_input_here(raw, source);
        };
        // The read token describes the caller's context, before the temporary edit sheet.
        if let Some(expected) = input
            .as_object_mut()
            .and_then(|o| o.remove("expected_revision"))
        {
            self.check_revision(&expected)?;
        }
        let block = self.sheet_block(Some(&sheet));
        if self.block_edit.is_some() && !block.eq_ignore_ascii_case(&self.sheet) {
            return Err(CoreError::InvalidCommand(
                "Finish editing the block before targeting another sheet.".into(),
            ));
        }
        let previous = self.sheet.clone();
        self.set_sheet(&block)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_input_here(&input, source)
        }));
        if self.set_sheet(&previous).is_err() {
            let _ = self.set_sheet("*Model_Space");
        }
        match result {
            Ok(result) => result.map(|mut report| {
                report.revision = self.revision();
                report
            }),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn run_input_here(&mut self, raw: &Value, source: Source) -> Result<Reported, CoreError> {
        if self.tx_depth == 0 {
            self.last_change = crate::Change::default();
            self.last_advice.clear();
            self.handle_replacements.clear();
        }
        let mut input = raw.clone();
        if let Some(expected) = input
            .as_object_mut()
            .and_then(|o| o.remove("expected_revision"))
        {
            self.check_revision(&expected)?;
        }
        crate::registry::canonicalize(&mut input);
        self.resolve_input_handles(&mut input)?;
        let mut report = self.validate_input(&input);
        report
            .warnings
            .extend(crate::registry::normalization_warnings(raw));
        let parsed = serde_path_to_error::deserialize::<_, Command>(input.clone());
        if !report.valid {
            let mut message = report.message();
            if let Err(error) = parsed {
                message.push_str(&format!("; {} at {}", error.inner(), error.path()));
            }
            return Err(with_warnings(
                CoreError::InvalidCommand(message),
                &report.warnings,
            ));
        }
        let command = parsed.map_err(|error| {
            with_warnings(
                CoreError::InvalidCommand(format!("{} at {}", error.inner(), error.path())),
                &report.warnings,
            )
        })?;
        let warnings = report.warnings.clone();
        let result = match command {
            Command::Batch { layer, color, .. } => {
                if self.tx_depth > 0 {
                    Err(CoreError::InvalidCommand(
                        "A batch can't run inside another batch".into(),
                    ))
                } else {
                    self.run_input_batch(&input["commands"], layer, color, source, report)
                }
            }
            command => self.run_command(command, source).map(|outcome| Reported {
                revision: self.revision(),
                advice: self.last_advice.clone(),
                handle_map: self.handle_map(),
                outcome,
                defaults_used: report.defaults_used,
                warnings: report.warnings,
            }),
        };
        result
            .map(|mut report| {
                report.revision = self.revision();
                report.advice = self.last_advice.clone();
                report.handle_map = self.handle_map();
                report
            })
            .map_err(|error| with_warnings(error, &warnings))
    }

    /// Runs a batch consisting solely of literal opening additions. Other batches keep
    /// ordinary sequential execution, including all step references and mixed edits.
    /// Each result is the final shared wall state, in the original input order.
    pub fn run_opening_batch(
        &mut self,
        inputs: &[Value],
        source: Source,
    ) -> Option<Result<Vec<Reported>, CoreError>> {
        if inputs.len() < 2
            || inputs.iter().any(|input| {
                input.get("sheet").is_some_and(|sheet| !sheet.is_null())
                    || input["type"].as_str().is_none_or(|name| {
                        !name.eq_ignore_ascii_case("add_wall_openings")
                            && !name.eq_ignore_ascii_case("add-wall-openings")
                    })
            })
        {
            return None;
        }
        let original = inputs;
        let mut inputs = inputs.to_vec();
        inputs.iter_mut().for_each(crate::registry::canonicalize);
        if inputs.len() < 2
            || inputs.iter().any(|input| {
                input["type"] != "add_wall_openings"
                    || !input["centreline"].is_u64()
                    || serde_json::from_value::<Command>(input.clone()).is_err()
            })
        {
            return None;
        }
        Some(self.transaction(source, "add_wall_openings", |session| {
            let mut commands = Vec::new();
            let mut reports = Vec::new();
            for (index, input) in inputs.iter().enumerate() {
                let mut input = input.clone();
                session.resolve_input_handles(&mut input)?;
                let mut report = session.validate_input(&input);
                report
                    .warnings
                    .extend(crate::registry::normalization_warnings(&original[index]));
                if !report.valid {
                    return Err(CoreError::Step {
                        index,
                        label: "add_wall_openings".into(),
                        error: Box::new(CoreError::InvalidCommand(report.message())),
                    });
                }
                let command: Command = serde_json::from_value(input)
                    .map_err(|error| CoreError::InvalidCommand(error.to_string()))?;
                session.check_block_edit(&command)?;
                let Command::AddWallOpenings {
                    centreline,
                    openings,
                } = command
                else {
                    unreachable!("checked opening command")
                };
                commands.push((centreline, openings));
                reports.push(report);
            }
            let results =
                session.record_document_change(source, "add_wall_openings", true, true, |s| {
                    s.add_wall_openings_many(&commands, source)
                })?;
            Ok(results
                .into_iter()
                .zip(reports)
                .map(|(result, report)| Reported {
                    revision: session.revision(),
                    advice: session.last_advice.clone(),
                    handle_map: session.handle_map(),
                    outcome: Outcome::Walls { result },
                    defaults_used: report.defaults_used,
                    warnings: report.warnings,
                })
                .collect())
        }))
    }

    fn run_input_batch(
        &mut self,
        commands: &Value,
        layer: Option<String>,
        color: Option<String>,
        source: Source,
        mut report: ValidationReport,
    ) -> Result<Reported, CoreError> {
        // An earlier step may change the sheet, scale or geometry used by a later default.
        // Replace the dry prediction with each step's report from its original raw input.
        report
            .defaults_used
            .retain(|default| !default.field.starts_with("/commands/"));
        report
            .warnings
            .retain(|warning| !warning.pointer.starts_with("/commands/"));
        self.batch_with(source, "batch", layer, color, |session| {
            let inputs = commands.as_array().expect("validated batch commands");
            let mut grouped = session
                .run_opening_batch(inputs, source)
                .transpose()?
                .map(Vec::into_iter);
            let mut outcomes = Vec::new();
            for (index, input) in commands
                .as_array()
                .expect("validated batch commands")
                .iter()
                .enumerate()
            {
                let step = if let Some(grouped) = &mut grouped {
                    grouped.next().expect("one result per input")
                } else {
                    session
                        .run_input(input, source)
                        .map_err(|error| CoreError::Step {
                            index,
                            label: crate::registry::parse_command(input.clone())
                                .map_or_else(|_| "input".into(), |command| command.label()),
                            error: Box::new(error),
                        })?
                };
                outcomes.push(step.outcome);
                report
                    .defaults_used
                    .extend(step.defaults_used.into_iter().map(|mut default| {
                        default.field = format!("/commands/{index}{}", default.field);
                        default
                    }));
                report
                    .warnings
                    .extend(step.warnings.into_iter().map(|mut warning| {
                        warning.pointer = format!("/commands/{index}{}", warning.pointer);
                        warning
                    }));
            }
            Ok(Reported {
                revision: session.revision(),
                advice: session.last_advice.clone(),
                handle_map: session.handle_map(),
                outcome: Outcome::Batch {
                    outcomes,
                    added: session.last_change.added.clone(),
                },
                defaults_used: report.defaults_used,
                warnings: report.warnings,
            })
        })
    }
}
