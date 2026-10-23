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
        notes.join("; ")
    }
}

impl Serialize for Reported {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(&self.outcome).map_err(serde::ser::Error::custom)?;
        metadata(&mut value, &self.defaults_used, &self.warnings);
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
        if self.tx_depth == 0 {
            self.last_change = crate::Change::default();
            self.handle_replacements.clear();
        }
        let mut input = raw.clone();
        crate::registry::canonicalize(&mut input);
        self.resolve_input_handles(&mut input)?;
        let report = self.validate_input(&input);
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
                outcome,
                defaults_used: report.defaults_used,
                warnings: report.warnings,
            }),
        };
        result.map_err(|error| with_warnings(error, &warnings))
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
            let mut outcomes = Vec::new();
            for (index, input) in commands
                .as_array()
                .expect("validated batch commands")
                .iter()
                .enumerate()
            {
                let step = session
                    .run_input(input, source)
                    .map_err(|error| CoreError::Step {
                        index,
                        label: crate::registry::parse_command(input.clone())
                            .map_or_else(|_| "input".into(), |command| command.label()),
                        error: Box::new(error),
                    })?;
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
