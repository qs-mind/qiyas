//! The drawing files around the session: which drawings are open, new, open, switch, save and
//! close. The app does these (`DrawingHost`, implemented by `qiyas-desktop` over its tabs); the
//! agent and the MCP server reach them through the six file tools, outside the session's lock
//! (the app's tabs are locked before the session). The user stays in charge: anything that could
//! lose work or pick a new file (Save As, closing unsaved changes, opening a file the user has not
//! named or opened before) goes through the window, where the user decides.

use serde_json::{json, Value};

use crate::context::TurnContext;

/// The tools run by the host, not on the session.
pub const HOST_TOOLS: [&str; 8] = [
    "finish",
    "project",
    "list_drawings",
    "new_drawing",
    "open_drawing",
    "switch_drawing",
    "save_drawing",
    "close_drawing",
];

/// Whether `name` is one of `HOST_TOOLS`.
pub fn is_host_tool(name: &str) -> bool {
    HOST_TOOLS.contains(&name)
}

/// Whether the host tool `name` may put another drawing on screen (or none): the turn's drawing
/// is then the one on screen after it.
pub fn changes_drawing(name: &str) -> bool {
    matches!(
        name,
        "new_drawing" | "open_drawing" | "switch_drawing" | "close_drawing"
    )
}

/// The app's side of the file tools. Each answer is JSON for the model; an `Err` is said to it
/// as the tool's error.
pub trait DrawingHost: Send + Sync {
    /// Check and record a tool call before any file access or mutation.
    fn before_tool(&self, _name: &str, _input: &Value) -> Result<(), String> {
        Ok(())
    }
    /// Record completion only. Saving is a separate, explicit operation.
    fn finish(&self, _answer: &str) -> Result<Value, String> {
        Ok(
            json!({"completed": true, "message": "Work finished. This action does not save the drawing."}),
        )
    }

    /// Registry project commands run in a project workspace, including before a drawing opens.
    fn project(&self, _input: &Value, _source: qiyas_core::Source) -> Result<Value, String> {
        Err("This host does not supply a project workspace".into())
    }
    /// Whether this host supplies the window operations as well as file operations.
    fn has_window(&self) -> bool {
        false
    }

    /// Runs an external sheet or drawing change while the host's turn ownership is held.
    /// The in-app assistant uses its own authority; MCP asks here before taking session locks.
    fn with_sheet_authority(
        &self,
        run: &mut dyn FnMut() -> Result<crate::ToolOutput, crate::ToolError>,
    ) -> Result<crate::ToolOutput, crate::ToolError> {
        run()
    }

    /// The open drawings' tabs (id, title, path, unsaved changes, untitled, on screen) and the
    /// recent drawings (path, whether the file is still there).
    fn list(&self) -> Result<Value, String>;
    /// A new, empty drawing in `units` ("m", "cm" or "mm") on screen in a new tab.
    fn new_drawing(&self, units: &str) -> Result<Value, String>;
    /// Opens the drawing at `path` (or shows its tab). `user_named`: the user wrote this path in
    /// their message. A path that is neither that, a recent drawing nor an open tab is asked
    /// about in the window first.
    fn open(&self, path: &str, user_named: bool) -> Result<Value, String>;
    /// Shows tab `id`.
    fn switch(&self, id: u32) -> Result<Value, String>;
    /// Saves the drawing on screen over its own file; an untitled one opens the window's Save As
    /// for the user instead.
    fn save(&self) -> Result<Value, String>;
    /// Closes tab `id` (the one on screen when `None`); one with unsaved changes is asked about
    /// in the window (Save, Don't Save, Cancel) instead.
    fn close(&self, id: Option<u32>) -> Result<Value, String>;
    /// What the main view shows now, as the window draws it: a PNG at most `max_side` pixels a
    /// side, and the drawing bounds it shows (see `crate::window`).
    fn capture_screen(&self, max_side: u32) -> Result<Screen, String> {
        let _ = max_side;
        Err(NO_WINDOW.into())
    }
    /// Makes `handles` (entities at the top of the sheet on screen) the Select tool's selection
    /// in the window, zooming to them with `zoom`.
    fn select(&self, handles: &[u64], zoom: bool) -> Result<(), String> {
        let _ = (handles, zoom);
        Err(NO_WINDOW.into())
    }
    /// The visible panels and their current state, read without changing the window.
    fn panel_state(&self) -> Result<Value, String> {
        Err(NO_WINDOW.into())
    }
    /// What the user points at in the window now: the selection, a marked region, the view.
    fn pointed(&self) -> Result<TurnContext, String> {
        Err(NO_WINDOW.into())
    }
}

/// A picture of the main view: a PNG, and `[[min_x, min_y], [max_x, max_y]]` of the drawing it
/// shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub png: Vec<u8>,
    pub bounds: [[f64; 2]; 2],
}

/// What a window tool answers when there is no window to ask.
pub const NO_WINDOW: &str =
    "No Qiyas window is connected. Visible panels and the screen cannot be read here.";

/// Runs the host tool `name` with `input`. `user_text` is the user's message of this turn (the
/// agent's; `None` for MCP, which has none): a path in it counts as named by the user.
pub fn run_host(
    host: &dyn DrawingHost,
    name: &str,
    input: &Value,
    user_text: Option<&str>,
) -> Result<Value, String> {
    match name {
        "finish" => host.finish(input["answer"].as_str().unwrap_or_default()),
        "project" => host.project(
            input,
            if user_text.is_some() {
                qiyas_core::Source::Ai
            } else {
                qiyas_core::Source::Mcp
            },
        ),
        "list_drawings" => host.list(),
        "new_drawing" => match input.get("units").and_then(Value::as_str) {
            Some(units @ ("m" | "cm" | "mm")) => host.new_drawing(units),
            _ => Err("units must be \"m\", \"cm\" or \"mm\"; ask the user which".into()),
        },
        "open_drawing" => {
            let path = input
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .ok_or("path is required")?;
            let user_named = user_text.is_some_and(|text| text.contains(path));
            host.open(path, user_named)
        }
        "switch_drawing" => host.switch(tab_id(input)?.ok_or("id is required")?),
        "save_drawing" => host.save(),
        "close_drawing" => host.close(tab_id(input)?),
        other => Err(format!("{other} is not a file tool")),
    }
}

/// The optional tab `id` of `input`.
fn tab_id(input: &Value) -> Result<Option<u32>, String> {
    match input.get("id") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| "id must be a tab id from qiyas_read drawings".to_string()),
    }
}

/// What a file tool answers when there is no host (a test, or a client without the app).
pub const NO_HOST: &str = "Drawings can't be opened, made or saved from here; ask the user to use \
                           New, Open and Save in Qiyas";

/// The answer of a host tool that only asked the window to do something.
pub fn asked(what: &str) -> Value {
    json!({ "asked_user": true, "message": what })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
    }

    impl DrawingHost for Fake {
        fn list(&self) -> Result<Value, String> {
            self.calls.lock().unwrap().push("list".into());
            Ok(json!({}))
        }
        fn new_drawing(&self, units: &str) -> Result<Value, String> {
            self.calls.lock().unwrap().push(format!("new {units}"));
            Ok(json!({}))
        }
        fn open(&self, path: &str, user_named: bool) -> Result<Value, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("open {path} {user_named}"));
            Ok(json!({}))
        }
        fn switch(&self, id: u32) -> Result<Value, String> {
            self.calls.lock().unwrap().push(format!("switch {id}"));
            Ok(json!({}))
        }
        fn save(&self) -> Result<Value, String> {
            self.calls.lock().unwrap().push("save".into());
            Ok(json!({}))
        }
        fn close(&self, id: Option<u32>) -> Result<Value, String> {
            self.calls.lock().unwrap().push(format!("close {id:?}"));
            Ok(json!({}))
        }
    }

    #[test]
    fn inputs_are_checked_and_a_path_counts_as_named_only_when_the_user_wrote_it() {
        let host = Fake::default();
        let run =
            |name: &str, input: Value, text: Option<&str>| run_host(&host, name, &input, text);
        assert!(run("new_drawing", json!({}), None).is_err());
        assert!(run("new_drawing", json!({ "units": "km" }), None).is_err());
        run("new_drawing", json!({ "units": "cm" }), None).unwrap();
        run(
            "open_drawing",
            json!({ "path": "/jobs/villa.dwg" }),
            Some("open /jobs/villa.dwg please"),
        )
        .unwrap();
        run(
            "open_drawing",
            json!({ "path": "/etc/x.dwg" }),
            Some("open the villa"),
        )
        .unwrap();
        run("open_drawing", json!({ "path": "/jobs/villa.dwg" }), None).unwrap();
        assert!(run("open_drawing", json!({ "path": "  " }), None).is_err());
        assert!(run("switch_drawing", json!({}), None).is_err());
        assert!(run("switch_drawing", json!({ "id": -1 }), None).is_err());
        run("switch_drawing", json!({ "id": 2 }), None).unwrap();
        run("close_drawing", json!({}), None).unwrap();
        run("close_drawing", json!({ "id": 3 }), None).unwrap();
        assert_eq!(
            *host.calls.lock().unwrap(),
            [
                "new cm",
                "open /jobs/villa.dwg true",
                "open /etc/x.dwg false",
                "open /jobs/villa.dwg false",
                "switch 2",
                "close None",
                "close Some(3)",
            ]
        );
    }
    #[test]
    fn finish_never_calls_save_or_opens_a_dialog() {
        let host = Fake::default();
        let result =
            run_host(&host, "finish", &json!({"answer":"Drawing complete"}), None).unwrap();
        assert_eq!(result["completed"], true);
        assert!(result.get("saved").is_none());
        assert!(host.calls.lock().unwrap().is_empty());
    }
}
