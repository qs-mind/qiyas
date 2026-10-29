//! Metadata about batch size and quantity families, supplied as advice only.
use std::collections::BTreeSet;

use serde_json::Value;

use crate::surface;

pub(crate) const MAX_STEPS: usize = 12;
const SPLIT: &str = "This batch spans multiple quantity families or more than 12 commands. / تشمل هذه الدفعة أكثر من مجموعة كميات أو أكثر من ١٢ أمرًا.";

#[derive(Default)]
pub(crate) struct Batch {
    families: BTreeSet<&'static str>,
    steps: usize,
}

impl Batch {
    pub(crate) fn read(name: &str, input: &Value) -> Self {
        let mut batch = Self::default();
        if name != surface::EXECUTE {
            return batch;
        }
        let Ok(args) = surface::args_of(input) else {
            return batch;
        };
        match input["op"].as_str() {
            Some("batch") => {
                batch.visit(&serde_json::json!({"type":"batch","commands":args["commands"]}))
            }
            Some("run") => batch.visit(&args["command"]),
            Some("type") => {
                if let Some(line) = args["line"].as_str() {
                    if let Ok(qiyas_core::cmdline::Step::Command(command)) =
                        qiyas_core::cmdline::Machine::default().step(line)
                    {
                        batch.visit(&serde_json::json!(command));
                    }
                }
            }
            _ => {}
        }
        batch
    }

    fn visit(&mut self, raw: &Value) {
        let mut command = match raw {
            Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            _ => raw.clone(),
        };
        qiyas_core::registry::canonicalize(&mut command);
        if let Some(commands) = command["commands"].as_array() {
            for child in commands {
                self.visit(child);
            }
        } else {
            self.steps += 1;
            if let Some(phase) = qiyas_core::takeoff_kit::phase(&command) {
                self.families.insert(phase);
            }
        }
    }

    pub(crate) fn advice(&self) -> Option<&'static str> {
        // Many objects of one family (every room of a tower floor) are one step: a cap on
        // them would turn one goal into dozens of requests.
        (self.families.len() > 1 || self.steps > MAX_STEPS).then_some(SPLIT)
    }

    pub(crate) fn takeoff_edit(&self) -> bool {
        self.families.iter().any(|family| *family != "report")
    }
}

/// A successful scoped read is enough evidence for advice. Explicit failed checks are not.
pub(crate) fn checked(name: &str, input: &Value, result: &Value) -> Option<bool> {
    fn failed(value: &Value) -> bool {
        match value {
            Value::Object(fields) => fields.iter().any(|(key, value)| {
                (key == "passed" && value == false)
                    || (key == "severity" && value == "error")
                    || failed(value)
            }),
            Value::Array(values) => values.iter().any(failed),
            _ => false,
        }
    }
    (name == surface::READ
        && matches!(
            input["op"].as_str(),
            Some(
                "digest"
                    | "state"
                    | "records"
                    | "explain"
                    | "takeoff_check"
                    | "check"
                    | "brief_check"
                    | "run"
            )
        ))
    .then(|| !failed(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn families_and_limits_cover_nested_string_and_typed_commands() {
        let kit = qiyas_core::takeoff_kit::kit();
        for phase in ["recognition", "method", "mappings", "report"] {
            assert!(Batch::read(
                surface::EXECUTE,
                &json!({"op":"batch","args":kit["phases"][phase]})
            )
            .advice()
            .is_none());
        }
        let commands = kit["phases"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|v| v["commands"].as_array().unwrap().clone())
            .collect::<Vec<_>>();
        for input in [
            json!({"op":"batch","args":{"commands":commands}}),
            json!({"op":"run","args":{"command":json!({"type":"batch","commands":commands}).to_string()}}),
            json!({"op":"batch","args":{"commands":vec![json!({"type":"new_layer","name":"Hospital"});13]}}),
        ] {
            assert!(Batch::read(surface::EXECUTE, &input)
                .advice()
                .unwrap()
                .contains("This batch spans multiple quantity families"));
        }
        assert!(Batch::read(
            surface::EXECUTE,
            &json!({"op":"run","args":{"command":{"type":"place_objects","objects":vec![json!({"type":"Observed"});300]}}})
        )
        .advice()
        .is_none());
        assert!(
            Batch::read(
                surface::EXECUTE,
                &json!({"op":"type","args":{"line":"PLACEOBJECTS {\"objects\":[]}"}})
            )
            .steps
                <= 1
        );
    }

    #[test]
    fn failed_checks_and_discovery_are_not_completion_evidence() {
        assert_ne!(
            checked(
                surface::READ,
                &json!({"op":"takeoff_check"}),
                &json!({"passed":false})
            ),
            Some(true)
        );
        assert_ne!(
            checked(surface::READ, &json!({"op":"commands"}), &json!({})),
            Some(true)
        );
        assert_eq!(
            checked(surface::READ, &json!({"op":"records"}), &json!({"rows":[]})),
            Some(true)
        );
    }
}
