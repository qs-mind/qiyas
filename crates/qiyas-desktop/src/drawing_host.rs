//! The file tools of Mizan and the MCP server (`qiyas_ai::DrawingHost`) over the app's tabs. They
//! keep the assistant's conversation (it chose the drawing), tell the window what to show
//! (`DRAWING_SHOWN`), and leave every choice that could lose work or pick a new file to the user:
//! a file nobody named or opened before is confirmed in a native dialog, and Save As, a save over
//! a repaired drawing and closing unsaved changes go through the window's own flows
//! (`DRAWING_SAVE`, `DRAWING_CLOSE`).

use std::path::Path;
use std::sync::PoisonError;

use qiyas_ai::host::asked;
use qiyas_ai::prompt::DrawingFacts;
use qiyas_ai::{DrawingHost, Screen, TurnContext};
use qiyas_core::Session;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::commands::{insunits_of, with_session};
use crate::script::read_problems;
use crate::settings::same_file;
use crate::state::AppState;
use crate::tabs;

/// Sent when the assistant put another drawing on screen (or closed the last): the window shows
/// the one the core has now.
pub const DRAWING_SHOWN: &str = "drawing:shown";
/// Sent when the assistant asked to save a drawing the window must ask about first (untitled:
/// Save As; repaired on open: the warning): the window runs its Save.
pub const DRAWING_SAVE: &str = "drawing:save";
/// Sent with a tab id when the assistant asked to close a drawing with unsaved changes: the
/// window asks Save, Don't Save or Cancel.
pub const DRAWING_CLOSE: &str = "drawing:close";

/// Most layers and blocks a file tool's answer lists.
const MAX_LISTED: usize = 30;

pub struct AppHost<R: Runtime> {
    pub app: AppHandle<R>,
}

impl<R: Runtime> AppHost<R> {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    /// The drawing on screen now, summarised for the model, with `message` for the card.
    fn shown(&self, message: String) -> Result<Value, String> {
        let _ = self.app.emit(DRAWING_SHOWN, ());
        let drawing = with_session(&self.state(), |s| Ok(summary(s))).ok();
        Ok(json!({ "message": message, "drawing": drawing }))
    }

    /// Whether `path` is an open tab or a recent drawing: the user has opened it before.
    fn known(&self, path: &Path) -> bool {
        let state = self.state();
        if tabs::find_path(&state, path).is_some() {
            return true;
        }
        let settings = state
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        settings
            .recents
            .iter()
            .any(|r| same_file(Path::new(r), path))
    }

    /// Asks the user in a native dialog, in the window's language, whether Mizan may open `path`.
    fn confirm_open(&self, path: &Path) -> bool {
        let arabic = self
            .state()
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .language
            == "ar";
        let shown = path.display();
        let (text, open, cancel, title) = if arabic {
            (
                format!("ميزان يريد فتح هذا المخطط:\n\u{2068}{shown}\u{2069}"),
                "فتح",
                "إلغاء",
                "قياس",
            )
        } else {
            (
                format!("Mizan wants to open this drawing:\n{shown}"),
                "Open",
                "Cancel",
                "Qiyas",
            )
        };
        self.app
            .dialog()
            .message(text)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                open.into(),
                cancel.into(),
            ))
            .blocking_show()
    }
}

/// What the model needs of a drawing it just got on screen: the system prompt's summary is of
/// the one before.
fn summary(s: &Session) -> Value {
    let facts = DrawingFacts::of(s);
    let top = |list: &[(String, usize)]| -> Vec<Value> {
        list.iter()
            .take(MAX_LISTED)
            .map(|(name, n)| json!([name, n]))
            .collect()
    };
    json!({
        "file": facts.file,
        "untitled": s.is_untitled(),
        "units": facts.units,
        "sheets": facts.sheets,
        "sheet_shown": facts.sheet,
        "layers": top(&facts.layers),
        "blocks": top(&facts.blocks),
    })
}

impl<R: Runtime> DrawingHost for AppHost<R> {
    fn has_window(&self) -> bool {
        true
    }

    fn list(&self) -> Result<Value, String> {
        let state = self.state();
        let now = tabs::snapshot(&state);
        let open: Vec<Value> = now
            .tabs
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "title": t.title,
                    "path": if t.untitled { Value::Null } else { json!(t.path) },
                    "unsaved_changes": t.dirty,
                    "untitled": t.untitled,
                    "on_screen": now.active == Some(t.id),
                })
            })
            .collect();
        let recent: Vec<Value> = state
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recents
            .iter()
            .map(|p| json!({ "path": p, "exists": Path::new(p).is_file() }))
            .collect();
        Ok(json!({ "open": open, "recent": recent }))
    }

    fn new_drawing(&self, units: &str) -> Result<Value, String> {
        let state = self.state();
        let session = Session::new_blank(&tabs::untitled_path(&state)?, insunits_of(units)?);
        let title = session
            .drawing
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        tabs::add(&state, session);
        self.shown(format!("New drawing {title} on screen, unsaved"))
    }

    fn open(&self, path: &str, user_named: bool) -> Result<Value, String> {
        let path = Path::new(path);
        let drawing = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"));
        if !drawing {
            return Err("Only .dwg and .dxf drawings open".into());
        }
        if !path.is_file() {
            return Err(format!("{} does not exist", path.display()));
        }
        if !user_named && !self.known(path) && !self.confirm_open(path) {
            return Err(format!("The user did not allow opening {}", path.display()));
        }
        let what = crate::script::open(&self.app, &self.state(), path, true)?;
        self.shown(what)
    }

    fn switch(&self, id: u32) -> Result<Value, String> {
        tabs::activate(&self.app, &self.state(), id, true)?;
        self.shown(format!("Tab {id} on screen"))
    }

    fn save(&self) -> Result<Value, String> {
        let state = self.state();
        let (untitled, repaired) =
            with_session(&state, |s| Ok((s.is_untitled(), read_problems(s) > 0)))?;
        if untitled || repaired {
            let _ = self.app.emit(DRAWING_SAVE, ());
            return Ok(asked(if untitled {
                "The drawing has no file yet: the Save dialog is open for the user to choose where"
            } else {
                "The drawing was repaired when it opened: Qiyas asks the user before saving over it"
            }));
        }
        let path = with_session(&state, |s| {
            s.save()?;
            Ok(s.drawing.path.display().to_string())
        })?;
        Ok(json!({ "message": format!("Saved {path}"), "ui": { "action": "redraw" } }))
    }

    fn close(&self, id: Option<u32>) -> Result<Value, String> {
        let state = self.state();
        let now = tabs::snapshot(&state);
        let id = id.or(now.active).ok_or("No drawing is open")?;
        let tab = now
            .tabs
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("No tab {id}"))?;
        if tab.dirty {
            let _ = self.app.emit(DRAWING_CLOSE, id);
            return Ok(asked(&format!(
                "{} has unsaved changes: Qiyas asks the user to save, not save or cancel",
                tab.title
            )));
        }
        let title = tab.title.clone();
        tabs::close(&self.app, &state, id, false, true)?;
        self.shown(format!("Closed {title}"))
    }

    fn capture_screen(&self, max_side: u32) -> Result<Screen, String> {
        self.state().ai_window.capture(&self.app, max_side)
    }

    fn select(&self, handles: &[u64], zoom: bool) -> Result<(), String> {
        self.state().ai_window.select(&self.app, handles, zoom)
    }

    fn pointed(&self) -> Result<TurnContext, String> {
        self.state().ai_window.pointed(&self.app)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;

    use qiyas_ai::DrawingHost;
    use qiyas_core::{Command, DrawSpec};
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::{App, Manager};

    use super::AppHost;
    use crate::commands::with_session;
    use crate::state::AppState;
    use crate::tabs;

    fn app() -> (tempfile::TempDir, App<MockRuntime>) {
        let dir = tempfile::tempdir().unwrap();
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        app.manage(AppState::new(dir.path().join("config")));
        (dir, app)
    }

    fn generation(app: &App<MockRuntime>) -> u64 {
        app.state::<AppState>()
            .ai
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .generation()
    }

    #[test]
    fn new_open_switch_save_and_close_keep_the_conversation() {
        let (dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
        };
        let state = app.state::<AppState>();
        let before = generation(&app);

        // New: an untitled tab in the app's folder, summarised for the model.
        let made = host.new_drawing("mm").unwrap();
        assert_eq!(made["drawing"]["units"], "millimetres");
        assert_eq!(made["drawing"]["untitled"], true);
        let now = tabs::snapshot(&state);
        assert_eq!(now.tabs.len(), 1);
        assert!(now.tabs[0].untitled);
        assert!(now.tabs[0].path.contains("untitled"));
        assert_eq!(now.tabs[0].title, "Untitled-1.dwg");
        // A second new drawing takes the next name.
        host.new_drawing("m").unwrap();
        assert_eq!(tabs::snapshot(&state).tabs[1].title, "Untitled-2.dwg");

        // Open a file the user named: no question asked.
        let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
        let opened = host.open(&path.display().to_string(), true).unwrap();
        assert!(opened["message"].as_str().unwrap().starts_with("opened"));
        assert_eq!(opened["drawing"]["file"], "sample.dxf");
        // Not a drawing, or not there: refused before anything is asked.
        assert!(host.open("/etc/passwd", false).is_err());
        assert!(host
            .open(&dir.path().join("gone.dwg").display().to_string(), false)
            .is_err());

        // The listing has the three tabs and the recent drawing.
        let list = host.list().unwrap();
        assert_eq!(list["open"].as_array().unwrap().len(), 3);
        assert_eq!(list["open"][0]["path"], serde_json::Value::Null);
        assert_eq!(list["open"][2]["on_screen"], true);
        assert_eq!(list["recent"][0]["exists"], true);

        // Save over the sample: written, no question.
        let saved = host.save().unwrap();
        assert!(saved["message"].as_str().unwrap().starts_with("Saved"));

        // Switch to the first new drawing; its save is the user's (Save As in the window).
        let first = tabs::snapshot(&state).tabs[0].id;
        host.switch(first).unwrap();
        assert_eq!(host.save().unwrap()["asked_user"], true);

        // Unsaved changes: closing is asked in the window, the tab stays.
        with_session(&state, |s| {
            s.run(Command::Draw(DrawSpec::Line {
                a: [0.0, 0.0],
                b: [1.0, 0.0],
            }))
        })
        .unwrap();
        assert_eq!(host.close(None).unwrap()["asked_user"], true);
        assert_eq!(tabs::snapshot(&state).tabs.len(), 3);
        // A clean one closes at once.
        let second = tabs::snapshot(&state).tabs[1].id;
        host.close(Some(second)).unwrap();
        assert_eq!(tabs::snapshot(&state).tabs.len(), 2);

        assert_eq!(generation(&app), before, "the conversation went on");
    }

    /// Qiyas closed before an untitled drawing's first Save As: at the next launch its autosave
    /// is found in the app's folder, and recovered it is still untitled (Save goes to Save As).
    #[test]
    fn an_untitled_drawings_crash_autosave_is_found_and_recovered_untitled() {
        let (dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
        };
        host.new_drawing("m").unwrap();
        let state = app.state::<AppState>();
        let placeholder = with_session(&state, |s| {
            s.run(Command::Draw(DrawSpec::Line {
                a: [0.0, 0.0],
                b: [1.0, 0.0],
            }))?;
            s.autosave()?;
            Ok(s.drawing.path.clone())
        })
        .unwrap();
        drop(app);

        // The next launch, on the same config folder.
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        app.manage(AppState::new(dir.path().join("config")));
        let state = app.state::<AppState>();
        let found = tauri::async_runtime::block_on(crate::files::untitled_autosaves(state.clone()))
            .unwrap();
        assert_eq!(found, [placeholder.display().to_string()]);
        let info = tauri::async_runtime::block_on(crate::files::recover_autosave(
            found[0].clone(),
            app.handle().clone(),
            state,
        ))
        .unwrap();
        assert!(info.untitled, "recovered, still untitled");
    }

    #[test]
    fn an_untitled_drawing_never_writes_its_project_file_beside_the_placeholder() {
        let (_dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
        };
        host.new_drawing("m").unwrap();
        let state = app.state::<AppState>();
        let project = with_session(&state, |s| s.save_project()).unwrap();
        assert!(!project.exists());
    }
}
