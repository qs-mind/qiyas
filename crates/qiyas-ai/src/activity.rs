//! Presentation metadata for the window, independent of command execution and model prose.
use qiyas_core::registry::registry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub category: String,
    pub effect: String,
}

/// Registry metadata is the source for every command, including newly registered commands.
/// Surface operations without a command use the tool's read/view/edit contract.
pub fn describe(name: &str, input: &Value) -> Activity {
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
            category: info.category.clone(),
            effect: serde_json::to_value(effect)
                .expect("registry effect")
                .as_str()
                .unwrap()
                .into(),
        };
    }
    Activity {
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

/// Counts are data, never extracted from English summaries or guessed from a command name.
pub fn count(value: &Value) -> Option<u64> {
    let value = value
        .get("outcome")
        .filter(|v| v.is_object())
        .unwrap_or(value);
    value.get("total").and_then(Value::as_u64).or_else(|| {
        value
            .get("handles")
            .and_then(Value::as_array)
            .map(|v| v.len() as u64)
    })
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
}
