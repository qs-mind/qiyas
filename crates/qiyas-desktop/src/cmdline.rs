//! Typed command prompts share the core machine; drawing tools still belong to the window.

use std::sync::{Mutex, PoisonError};

use qiyas_core::cmdline::{Machine, Step};
use qiyas_core::registry::{registry, CommandWord};
use qiyas_core::Ucs;
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Default)]
pub struct CommandLineState(Mutex<TypedLine>);

#[derive(Default)]
struct TypedLine {
    tab: Option<u32>,
    machine: Machine,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Command {
        command: serde_json::Value,
        last: Option<[f64; 2]>,
    },
    Prompt {
        prompt: String,
        last: Option<[f64; 2]>,
    },
    Cancelled,
}

impl TypedLine {
    fn step(
        &mut self,
        tab: Option<u32>,
        ucs: Ucs,
        line: &str,
        last: Option<[f64; 2]>,
    ) -> Result<Reply, String> {
        if self.tab != tab {
            self.machine = Machine::default();
            self.tab = tab;
        }
        self.machine.set_ucs(ucs);
        if !self.machine.is_active() && last.is_some() {
            self.machine.set_last(last);
        }
        Ok(match self.machine.step(line)? {
            Step::Command(_) => Reply::Command {
                command: self
                    .machine
                    .take_input()
                    .expect("a completed command has wire input"),
                last: self.machine.last(),
            },
            Step::Prompt(prompt) => Reply::Prompt {
                prompt,
                last: self.machine.last(),
            },
            Step::Cancelled => Reply::Cancelled,
        })
    }
}

/// All typed words, including aliases, with their registry command and documentation.
#[tauri::command]
pub async fn command_words() -> Vec<CommandWord> {
    registry().words()
}

/// Reads a typed line without executing it. The window applies a completed command through its
/// usual command path, which refreshes the drawing and saves the project.
#[tauri::command]
pub async fn cmdline_step(
    workspace: Option<u32>,

    line: String,
    last: Option<[f64; 2]>,
    state: State<'_, AppState>,
    typed: State<'_, CommandLineState>,
) -> Result<Reply, String> {
    let state = state.scoped(workspace)?;
    let ucs = crate::commands::with_session(&state, |session| Ok(session.ucs))?;
    typed
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .step(Some(state.id), ucs, &line, last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_ipc_input_keeps_omitted_text_defaults() {
        let mut typed = TypedLine::default();
        let Reply::Command { command, .. } = typed
            .step(Some(1), Ucs::default(), "TEXT 0,0 Room", None)
            .unwrap()
        else {
            panic!("expected a completed command");
        };
        assert_eq!(command["value"], "Room");
        assert!(command.get("height").is_none());
        assert!(command.get("rotation").is_none());
    }

    #[test]
    fn a_drawing_switch_drops_the_previous_prompt_and_last_point() {
        let mut typed = TypedLine::default();
        assert!(matches!(
            typed.step(Some(1), Ucs::default(), "LINE 0,0", None),
            Ok(Reply::Prompt { .. })
        ));
        assert!(typed.step(Some(2), Ucs::default(), "@1,0", None).is_err());
        assert!(matches!(
            typed.step(Some(2), Ucs::default(), "LINE 1,1 2,2", None),
            Ok(Reply::Command { .. })
        ));
    }

    #[test]
    fn a_typed_command_takes_the_windows_last_point_and_current_ucs() {
        let mut typed = TypedLine::default();
        let ucs = Ucs {
            origin: [10.0, 20.0],
            x_axis: [0.0, 1.0],
        };
        let Reply::Command { command, last } = typed
            .step(Some(1), ucs, "LINE @2,0 @3,0", Some([1.0, 1.0]))
            .unwrap()
        else {
            panic!("a complete line must yield its command");
        };
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["a"], serde_json::json!([1.0, 3.0]));
        assert_eq!(value["b"], serde_json::json!([1.0, 6.0]));
        assert_eq!(last, Some([1.0, 6.0]));
    }

    #[test]
    fn ipc_lines_without_a_last_point_keep_the_machines_point() {
        let mut typed = TypedLine::default();
        typed
            .step(Some(1), Ucs::default(), "LINE 0,0 1,1", None)
            .unwrap();
        let Reply::Command { command, .. } = typed
            .step(Some(1), Ucs::default(), "LINE @1,0 @0,1", None)
            .unwrap()
        else {
            panic!("relative input must keep the previous point");
        };
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["a"], serde_json::json!([2.0, 1.0]));
        assert_eq!(value["b"], serde_json::json!([2.0, 2.0]));
    }
}
