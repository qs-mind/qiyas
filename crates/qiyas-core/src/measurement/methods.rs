//! Resolve optional local rules by their declared name, never by a user-supplied path.
use std::fs;
use std::path::Path;

use super::{same_method_name, MeasurementCommand, MethodScope, RuleSet};
use crate::{Command, CoreError, Session};

pub fn load_method_at(home: &Path, name: Option<&str>) -> Result<Option<RuleSet>, String> {
    let Some(name) = name else { return Ok(None) };
    let placeholder = RuleSet::named(name);
    placeholder.validate().map_err(|e| e.to_string())?;
    let entries = match fs::read_dir(home.join("methods")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some(placeholder)),
        Err(e) => return Err(e.to_string()),
    };
    let mut found = None;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|e| e != "json") || !path.is_file() {
            continue;
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !value["name"]
            .as_str()
            .is_some_and(|declared| same_method_name(declared, name))
        {
            continue;
        }
        let method: RuleSet = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let method = method.bare();
        method.validate().map_err(|e| e.to_string())?;
        if found.replace(method).is_some() {
            return Err(format!("More than one measurement method is named {name}"));
        }
    }
    Ok(Some(found.unwrap_or(placeholder)))
}

impl Session {
    /// Reopening a name-only selection must retain rules already authored in this drawing.
    pub fn apply_settings_method(&mut self, method: Option<&RuleSet>) -> Result<(), CoreError> {
        let selection = self.measurement_selection()?;
        if selection.settings.as_ref() == method
            || method
                .zip(selection.settings.as_ref())
                .is_some_and(|(next, saved)| {
                    // A name-only choice of the same method keeps what the drawing has: its
                    // rules, or an older spelling of the name ("POMI (RICS)"), which every
                    // report shows bare. Rewriting it would mark an untouched drawing changed.
                    same_method_name(&next.name, &saved.name) && next.rules.is_empty()
                })
        {
            return Ok(());
        }
        self.run(Command::Measurement(Box::new(
            MeasurementCommand::SetMethod {
                scope: MethodScope::Settings,
                method: method.cloned().map(Box::new),
            },
        )))?;
        Ok(())
    }
}
