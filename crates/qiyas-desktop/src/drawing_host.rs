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
    pub expected_revision: Option<Value>,
    pub binding: Option<AppState>,
}

impl<R: Runtime> AppHost<R> {
    fn state(&self) -> AppState {
        self.binding.clone().unwrap_or_else(|| {
            self.app
                .state::<AppState>()
                .scoped(None)
                .expect("active workspace exists")
        })
    }

    fn require_visible(&self) -> Result<(), String> {
        let state = self.state();
        if crate::state::lock(&state.tabs)
            .active
            .is_some_and(|id| id != state.id)
        {
            return Err("This drawing is not on screen. Use picture capture to inspect it.".into());
        }
        Ok(())
    }

    /// The drawing on screen now, summarised for the model, with `message` for the card.
    fn shown(&self, message: String) -> Result<Value, String> {
        let _ = self.app.emit(DRAWING_SHOWN, ());
        let drawing = with_session(&self.state(), |s| Ok(summary(s))).ok();
        Ok(
            json!({ "message": message, "drawing": drawing, "workspace":self.state().id,
            "binding":"Subsequent tools still address this Mizan’s original drawing." }),
        )
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
                format!("ميزان يريد فتح هذا الرسم:\n\u{2068}{shown}\u{2069}"),
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

    fn confirm_read(&self, path: &Path) -> bool {
        #[cfg(test)]
        if let Some(confirmation) = self.app.try_state::<tests::ReadConfirmation>() {
            confirmation.paths.lock().unwrap().push(path.to_path_buf());
            return confirmation
                .answer
                .load(std::sync::atomic::Ordering::Relaxed);
        }
        let arabic = self
            .state()
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .language
            == "ar";
        let shown = path.display();
        let (text, read, cancel, title) = if arabic {
            (
                format!("ميزان يريد قراءة ملف PDF هذا:\n\u{2068}{shown}\u{2069}"),
                "قراءة",
                "إلغاء",
                "قياس",
            )
        } else {
            (
                format!("Mizan wants to read this PDF:\n{shown}"),
                "Read",
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
                read.into(),
                cancel.into(),
            ))
            .blocking_show()
    }

    fn document_allowed(&self, path: &Path) -> bool {
        let state = self.state();
        let inside = |folder: &Path| {
            folder
                .canonicalize()
                .is_ok_and(|folder| path.starts_with(folder))
        };
        let beside = |drawing: &Path| drawing.parent().is_some_and(inside);
        if tabs::snapshot(&state)
            .tabs
            .iter()
            .any(|tab| beside(Path::new(&tab.path)))
        {
            return true;
        }
        let project = crate::state::lock(&state.projects).active.clone();
        if let Some(project) = project {
            if let Ok(set) = qiyas_core::projects::load(&project) {
                if qiyas_core::sheet_set_entries(&project, &set)
                    .iter()
                    .any(|entry| beside(Path::new(&entry.full)))
                {
                    return true;
                }
            }
        }
        qiyas_core::data_home::qiyas_dir().is_ok_and(|home| inside(&home.join("attachments")))
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
        "export_folder": s.export_folder(),
        "units": facts.units,
        "sheets": facts.sheets,
        "sheet_shown": facts.sheet,
        "layers": top(&facts.layers),
        "blocks": top(&facts.blocks),
    })
}

impl<R: Runtime> DrawingHost for AppHost<R> {
    fn before_tool(&self, name: &str, input: &Value) -> Result<(), String> {
        self.before_tool_with_user(name, input, None)
    }

    fn before_tool_with_user(
        &self,
        name: &str,
        input: &Value,
        user_text: Option<&str>,
    ) -> Result<(), String> {
        if !((name == qiyas_ai::surface::READ && input["op"] == "document")
            || (name == qiyas_ai::surface::CAPTURE && input["source"] == "document"))
        {
            return Ok(());
        }
        let qiyas_ai::surface::Route::Session { args, .. } = qiyas_ai::surface::route(name, input)?
        else {
            return Ok(());
        };
        let Some(named) = args["path"].as_str() else {
            return Ok(());
        };
        let path = Path::new(named);
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            return Ok(());
        }
        let mut path = path.to_path_buf();
        if path.is_relative() {
            let state = self.state();
            let guard = crate::state::lock(&state.session);
            if let Some(session) = guard.as_ref() {
                path = session
                    .drawing
                    .path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(path);
            }
        }
        // Leave missing files and other filesystem errors to the document reader.
        let Ok(path) = path.canonicalize() else {
            return Ok(());
        };
        if user_text.is_some_and(|text| text.contains(named))
            || self.document_allowed(&path)
            || self.confirm_read(&path)
        {
            Ok(())
        } else {
            Err(format!("The user did not allow reading {}", path.display()))
        }
    }

    fn run_file(
        &self,
        name: &str,
        input: &Value,
        user_text: Option<&str>,
    ) -> Result<Value, String> {
        {
            let state = self.state();
            let guard = state.session.lock().unwrap_or_else(PoisonError::into_inner);
            qiyas_ai::host::check_expected_revision(
                guard.as_ref(),
                input.get("expected_revision"),
            )?;
        }
        let checked = Self {
            app: self.app.clone(),
            expected_revision: input.get("expected_revision").cloned(),
            binding: Some(self.state()),
        };
        qiyas_ai::host::dispatch_host(&checked, name, input, user_text)
    }
    fn has_window(&self) -> bool {
        true
    }

    fn with_sheet_authority(
        &self,
        run: &mut dyn FnMut() -> Result<qiyas_ai::ToolOutput, qiyas_ai::ToolError>,
    ) -> Result<qiyas_ai::ToolOutput, qiyas_ai::ToolError> {
        let state = self.state();
        let _turn =
            crate::commands::window_sheet_guard(&state).map_err(qiyas_ai::ToolError::Failed)?;
        run()
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
                    "mizan": t.status,
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
        Ok(json!({ "workspace":state.id, "open": open, "recent": recent }))
    }

    fn new_drawing(&self, units: &str) -> Result<Value, String> {
        let state = self.state();
        let units = insunits_of(units)?;
        if let Some(id) = tabs::reusable_blank(&state, units) {
            tabs::activate_checked(&self.app, &state, id, true, self.expected_revision.as_ref())?;
            return self.shown("Using the open empty drawing with the same units.".to_string());
        }
        let mut session = Session::new_blank(&tabs::untitled_path(&state)?, units);
        tabs::set_untitled_export_folder(&self.app, &mut session)?;
        let title = session
            .drawing
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        tabs::add_checked(&state, session, self.expected_revision.as_ref())?;
        self.shown(format!("New drawing {title} on screen, unsaved"))
    }

    fn new_named_drawing(&self, name: &str, units: &str) -> Result<Value, String> {
        let state = self.state();
        let units = insunits_of(units)?;
        let path = tabs::named_untitled_path(&state, name)?;
        let title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut session = Session::new_blank(&path, units);
        tabs::set_untitled_export_folder(&self.app, &mut session)?;
        tabs::add_checked(&state, session, self.expected_revision.as_ref())?;
        let mut result = self.shown(format!("New drawing {title} on screen, unsaved"))?;
        result["name"] = json!(title);
        Ok(result)
    }

    fn project(&self, input: &Value, source: qiyas_core::Source) -> Result<Value, String> {
        crate::projects::execute_checked(
            &self.state(),
            input,
            source,
            self.expected_revision.as_ref(),
        )
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
        if !user_named
            && !self.known(path)
            && !crate::projects::contains(&self.state().projects, path)
            && !self.confirm_open(path)
        {
            return Err(format!("The user did not allow opening {}", path.display()));
        }
        let what = crate::script::open_checked(
            &self.app,
            &self.state(),
            path,
            true,
            self.expected_revision.as_ref(),
        )?;
        self.shown(what)
    }

    fn switch(&self, id: u32) -> Result<Value, String> {
        tabs::activate_checked(
            &self.app,
            &self.state(),
            id,
            true,
            self.expected_revision.as_ref(),
        )?;
        self.shown(format!(
            "Tab {id} on screen. Your tools remain bound to drawing {}.",
            self.state().id
        ))
    }

    fn save(&self) -> Result<Value, String> {
        let state = self.state();
        let (untitled, repaired) =
            with_session(&state, |s| Ok((s.is_untitled(), read_problems(s) > 0)))?;
        if untitled || repaired {
            let _ = self.app.emit(DRAWING_SAVE, state.id);
            return Ok(asked(if untitled {
                "The drawing has no file yet: the Save dialog is open for the user to choose where"
            } else {
                "The drawing was repaired when it opened: Qiyas asks the user before saving over it"
            }));
        }
        let path = {
            let mut guard = state.session.lock().unwrap_or_else(PoisonError::into_inner);
            qiyas_ai::host::check_expected_revision(
                guard.as_ref(),
                self.expected_revision.as_ref(),
            )?;
            let session = guard.as_mut().ok_or("No drawing is open")?;
            session.save().map_err(|e| e.to_string())?;
            session.drawing.path.display().to_string()
        };
        Ok(
            json!({ "path": path, "message": format!("Saved {path}"), "ui": { "action": "redraw" } }),
        )
    }

    fn close(&self, id: Option<u32>) -> Result<Value, String> {
        let state = self.state();
        let now = tabs::snapshot(&state);
        let id = id.or(Some(state.id)).ok_or("No drawing is open")?;
        let tab = now
            .tabs
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("No tab {id}"))?;
        if tab.busy {
            let _ = self.app.emit(DRAWING_CLOSE, id);
            return Ok(asked("Mizan is working on this drawing: stop and close?"));
        }
        if tab.dirty {
            let _ = self.app.emit(DRAWING_CLOSE, id);
            return Ok(asked(&format!(
                "{} has unsaved changes: Qiyas asks the user to save, not save or cancel",
                tab.title
            )));
        }
        let title = tab.title.clone();
        tabs::close_checked(
            &self.app,
            &state,
            id,
            false,
            true,
            self.expected_revision.as_ref(),
        )?;
        self.shown(format!("Closed {title}"))
    }

    fn capture_screen(&self, max_side: u32) -> Result<Screen, String> {
        self.require_visible()?;
        self.state().ai_window.capture(&self.app, max_side)
    }

    fn select(&self, handles: &[u64], zoom: bool) -> Result<(), String> {
        let state = self.state();
        crate::state::lock(&state.context).selection = handles
            .iter()
            .map(|handle| qiyas_core::Target {
                handle: *handle,
                chain: Vec::new(),
            })
            .collect();
        if self.require_visible().is_err() {
            return Ok(());
        }
        state.ai_window.select(&self.app, handles, zoom)
    }

    fn panel_state(&self) -> Result<Value, String> {
        self.require_visible()?;
        self.state().ai_window.panels(&self.app)
    }

    fn pointed(&self) -> Result<TurnContext, String> {
        let state = self.state();
        if self.require_visible().is_err() {
            return Ok(crate::state::lock(&state.context).clone());
        }
        state.ai_window.pointed(&self.app)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;

    use qiyas_ai::DrawingHost;
    use qiyas_core::{Command, DrawSpec};
    use serde_json::{json, Value};
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

    #[derive(Default)]
    pub(super) struct ReadConfirmation {
        pub answer: std::sync::atomic::AtomicBool,
        pub paths: std::sync::Mutex<Vec<std::path::PathBuf>>,
    }

    fn document_host(app: &App<MockRuntime>, folder: &std::path::Path) -> AppHost<MockRuntime> {
        std::fs::create_dir_all(folder).unwrap();
        tabs::add(
            &app.state::<AppState>(),
            qiyas_core::Session::new_blank(&folder.join("tower-podium.dxf"), 6),
        )
        .unwrap();
        app.manage(ReadConfirmation::default());
        AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        }
    }

    fn document_calls(path: &std::path::Path) -> [(&'static str, Value); 2] {
        [
            ("qiyas_read", json!({"op":"document","args":{"path":path}})),
            (
                "qiyas_capture",
                json!({"source":"document","args":{"path":path,"page":1}}),
            ),
        ]
    }

    fn assert_document_refused(host: &AppHost<MockRuntime>, path: &std::path::Path) {
        for (name, input) in document_calls(path) {
            let mut reading_reached = false;
            let result = host.before_tool(name, &input).map(|()| {
                reading_reached = true;
            });
            assert!(!reading_reached, "the PDF read must not be reached");
            assert!(result
                .unwrap_err()
                .starts_with("The user did not allow reading "));
        }
        let confirmation = host.app.state::<ReadConfirmation>();
        assert!(!confirmation
            .answer
            .load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(confirmation.paths.lock().unwrap().len(), 2);
    }

    #[test]
    fn document_outside_open_drawings_is_refused_before_reading() {
        let (dir, app) = app();
        let host = document_host(&app, &dir.path().join("tower"));
        let pdf = dir.path().join("private.pdf");
        std::fs::write(&pdf, b"PDF contents must not be read").unwrap();
        assert_document_refused(&host, &pdf);
        assert_eq!(
            *app.state::<ReadConfirmation>().paths.lock().unwrap(),
            vec![pdf.canonicalize().unwrap(); 2]
        );
    }

    #[test]
    fn document_beside_open_drawing_is_allowed_without_asking() {
        let (dir, app) = app();
        let folder = dir.path().join("tower");
        let host = document_host(&app, &folder);
        std::fs::write(folder.join("podium-spec.pdf"), b"PDF").unwrap();
        for path in [folder.join("podium-spec.pdf"), "podium-spec.pdf".into()] {
            for (name, input) in document_calls(&path) {
                host.before_tool(name, &input).unwrap();
            }
        }
        assert!(app
            .state::<ReadConfirmation>()
            .paths
            .lock()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn document_beside_another_tab_or_project_drawing_is_allowed() {
        let (dir, app) = app();
        let host = document_host(&app, &dir.path().join("tower"));
        let bound = AppHost {
            binding: Some(host.state()),
            ..host
        };
        let other = dir.path().join("transit-hub");
        std::fs::create_dir_all(&other).unwrap();
        tabs::add(
            &app.state::<AppState>(),
            qiyas_core::Session::new_blank(&other.join("concourse.dxf"), 6),
        )
        .unwrap();
        let project = dir.path().join("megaproject.qss");
        std::fs::write(&project, serde_json::to_vec(&json!({
            "name":"Tower and transit hub", "drawings":[{"path":"project-tower/core.dxf"}], "project":{}
        })).unwrap()).unwrap();
        app.state::<AppState>().projects.lock().unwrap().active = Some(project);
        let project_folder = dir.path().join("project-tower");
        std::fs::create_dir_all(&project_folder).unwrap();
        for folder in [other, project_folder] {
            let pdf = folder.join("spec.pdf");
            std::fs::write(&pdf, b"PDF").unwrap();
            for (name, input) in document_calls(&pdf) {
                bound.before_tool(name, &input).unwrap();
            }
        }
        assert!(app
            .state::<ReadConfirmation>()
            .paths
            .lock()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn document_attachment_is_allowed_without_a_drawing() {
        let (_dir, app) = app();
        app.manage(ReadConfirmation::default());
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        };
        let attachments = qiyas_core::data_home::qiyas_dir()
            .unwrap()
            .join("attachments");
        std::fs::create_dir_all(&attachments).unwrap();
        let folder = tempfile::tempdir_in(attachments).unwrap();
        let pdf = folder.path().join("podium-spec.pdf");
        std::fs::write(&pdf, b"PDF").unwrap();
        for (name, input) in document_calls(&pdf) {
            host.before_tool(name, &input).unwrap();
        }
        assert!(app
            .state::<ReadConfirmation>()
            .paths
            .lock()
            .unwrap()
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn document_symlink_beside_drawing_cannot_escape_its_folder() {
        let (dir, app) = app();
        let folder = dir.path().join("tower");
        let host = document_host(&app, &folder);
        let outside = dir.path().join("private.pdf");
        std::fs::write(&outside, b"PDF contents must not be read").unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("podium-spec.pdf")).unwrap();
        assert_document_refused(&host, std::path::Path::new("podium-spec.pdf"));
        assert_eq!(
            *app.state::<ReadConfirmation>().paths.lock().unwrap(),
            vec![outside.canonicalize().unwrap(); 2]
        );
    }

    #[test]
    fn document_parent_components_cannot_escape_drawing_folder() {
        let (dir, app) = app();
        let host = document_host(&app, &dir.path().join("tower"));
        let outside = dir.path().join("private.pdf");
        std::fs::write(&outside, b"PDF contents must not be read").unwrap();
        assert_document_refused(&host, std::path::Path::new("../private.pdf"));
        assert_eq!(
            *app.state::<ReadConfirmation>().paths.lock().unwrap(),
            vec![outside.canonicalize().unwrap(); 2]
        );
    }

    struct DocumentProvider {
        call: std::sync::Mutex<Option<qiyas_ai::ContentBlock>>,
    }

    impl qiyas_ai::Provider for DocumentProvider {
        fn image_input(&self, _: &str) -> Option<bool> {
            Some(true)
        }

        fn stream_turn<'a>(
            &'a self,
            _: qiyas_ai::TurnRequest,
            _: qiyas_ai::provider::Sink<'a>,
        ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, qiyas_ai::AiError>>
        {
            Box::pin(async move {
                let call = self.call.lock().unwrap().take();
                Ok(qiyas_ai::TurnResult {
                    stop_reason: if call.is_some() {
                        qiyas_ai::StopReason::ToolUse
                    } else {
                        qiyas_ai::StopReason::EndTurn
                    },
                    content: vec![call.unwrap_or(qiyas_ai::ContentBlock::Text {
                        text: "Finished.".into(),
                    })],
                    usage: Default::default(),
                })
            })
        }
    }

    #[tokio::test]
    async fn document_mizan_checks_permission_before_reading_or_capture() {
        for index in 0..2 {
            let (dir, app) = app();
            let host = document_host(&app, &dir.path().join("tower"));
            let pdf = dir.path().join("private.pdf");
            std::fs::write(&pdf, b"PDF contents must not be read").unwrap();
            let state = host.state();
            let (name, input) = document_calls(&pdf)[index].clone();
            let provider = DocumentProvider {
                call: std::sync::Mutex::new(Some(qiyas_ai::ContentBlock::ToolUse {
                    id: "document".into(),
                    name: name.into(),
                    input,
                })),
            };
            let mut agent = qiyas_ai::Agent::new(
                Box::new(provider),
                state.session.clone(),
                qiyas_ai::AgentSettings::default(),
            )
            .with_host(std::sync::Arc::new(host));
            let (tx, mut rx) = tokio::sync::mpsc::channel(128);
            let (result, ()) = tokio::join!(
                agent.run_turn(
                    "Inspect the podium specification".into(),
                    tx,
                    tokio_util::sync::CancellationToken::new()
                ),
                async { while rx.recv().await.is_some() {} }
            );
            result.unwrap();
            let history = serde_json::to_string(agent.history()).unwrap();
            assert!(
                history.contains("The user did not allow reading"),
                "{history}"
            );
            assert_eq!(
                app.state::<ReadConfirmation>().paths.lock().unwrap().len(),
                1
            );
        }
    }

    #[test]
    fn document_user_named_paths_and_confirmation_allow_reading() {
        let (dir, app) = app();
        let host = document_host(&app, &dir.path().join("tower"));
        let pdf = dir.path().join("private.pdf");
        std::fs::write(&pdf, b"PDF").unwrap();
        for (name, input) in document_calls(&pdf) {
            host.before_tool_with_user(name, &input, Some(&format!("Read {}", pdf.display())))
                .unwrap();
        }
        assert!(app
            .state::<ReadConfirmation>()
            .paths
            .lock()
            .unwrap()
            .is_empty());
        assert_document_refused(&host, &pdf);
        let confirmation = app.state::<ReadConfirmation>();
        confirmation.paths.lock().unwrap().clear();
        confirmation
            .answer
            .store(true, std::sync::atomic::Ordering::Relaxed);
        for (name, input) in document_calls(&pdf) {
            host.before_tool(name, &input).unwrap();
        }
        assert_eq!(
            app.state::<ReadConfirmation>().paths.lock().unwrap().len(),
            2
        );
    }

    #[test]
    fn document_missing_and_non_pdf_paths_keep_reader_errors() {
        let (dir, app) = app();
        let host = document_host(&app, &dir.path().join("tower"));
        let text = dir.path().join("private.txt");
        std::fs::write(&text, b"text").unwrap();
        for path in [text, dir.path().join("missing.pdf")] {
            for (name, input) in document_calls(&path) {
                host.before_tool(name, &input).unwrap();
                let args = &input["args"];
                let expected = qiyas_ai::surface::run_without_drawing(name, "document", args, None)
                    .unwrap()
                    .unwrap_err()
                    .to_string();
                let actual = with_session(&host.state(), |s| {
                    Ok(qiyas_ai::surface::run_session(
                        s,
                        name,
                        "document",
                        args,
                        qiyas_core::Source::Ai,
                    ))
                })
                .unwrap()
                .unwrap_err()
                .to_string();
                assert_eq!(actual, expected);
            }
        }
        host.before_tool("qiyas_read", &json!({"op":"state"}))
            .unwrap();
        host.before_tool("qiyas_capture", &json!({"source":"picture"}))
            .unwrap();
        assert!(app
            .state::<ReadConfirmation>()
            .paths
            .lock()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn named_drawings_keep_arabic_and_choose_unused_names() {
        let (_dir, app) = app();
        let state = app.state::<AppState>();
        let path = tabs::named_untitled_path(&state, "  فيلا G+1: plans /?*<>|\\\"  ").unwrap();
        assert_eq!(path.file_name().unwrap(), "فيلا G+1 plans.dwg");
        assert_eq!(path.parent().unwrap(), state.config_dir.join("untitled"));
        assert!(tabs::named_untitled_path(&state, " ").is_err());
        assert!(tabs::named_untitled_path(&state, "/?*.").is_err());
        assert_eq!(
            tabs::named_untitled_path(&state, "CON")
                .unwrap()
                .file_name()
                .unwrap(),
            "CON drawing.dwg"
        );
        std::fs::write(&path, b"existing drawing").unwrap();
        let second = tabs::named_untitled_path(&state, "فيلا G+1 plans").unwrap();
        assert_eq!(second.file_name().unwrap(), "فيلا G+1 plans 2.dwg");
        std::fs::write(qiyas_core::autosave_path(&second), b"recovery").unwrap();
        let third = tabs::named_untitled_path(&state, "فيلا G+1 plans").unwrap();
        assert_eq!(third.file_name().unwrap(), "فيلا G+1 plans 3.dwg");
        tabs::add(&state, qiyas_core::Session::new_blank(&third, 6)).unwrap();
        assert_eq!(tabs::snapshot(&state).tabs[0].title, "فيلا G+1 plans 3.dwg");
        assert_eq!(
            tabs::named_untitled_path(&state, "فيلا G+1 plans")
                .unwrap()
                .file_name()
                .unwrap(),
            "فيلا G+1 plans 4.dwg"
        );
    }

    #[test]
    fn new_drawing_op_returns_the_requested_name_and_does_not_reuse_another_name() {
        let (_dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        };
        let first = qiyas_ai::host::run_host(
            &host,
            "new_drawing",
            &json!({"name":"Villa G+1 plans","units":"m"}),
            None,
        )
        .unwrap();
        assert_eq!(first["name"], "Villa G+1 plans");
        assert!(first["message"]
            .as_str()
            .unwrap()
            .contains("Villa G+1 plans"));
        let second = qiyas_ai::host::run_host(
            &host,
            "new_drawing",
            &json!({"name":"Villa G+1 plans","units":"m"}),
            None,
        )
        .unwrap();
        assert_eq!(second["name"], "Villa G+1 plans 2");
        let tabs = tabs::snapshot(&app.state::<AppState>());
        assert_eq!(tabs.tabs[0].title, "Villa G+1 plans.dwg");
        assert_eq!(tabs.tabs[1].title, "Villa G+1 plans 2.dwg");
    }

    #[test]
    fn deliverable_without_a_path_writes_in_the_drawing_export_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("exports");
        std::fs::create_dir_all(&folder).unwrap();
        let mut session = qiyas_core::Session::new_blank(&dir.path().join("Villa plans.dwg"), 6);
        session.set_export_directory(folder.clone());
        for (command, filename) in [
            (
                json!({"type":"deliverable","kind":"export","report":"register","format":"excel"}),
                "Villa plans - Register.xlsx",
            ),
            (
                json!({"type":"deliverable.export","kind":"register","format":"excel"}),
                "Villa plans - Register 2.xlsx",
            ),
        ] {
            let output = qiyas_ai::surface::run_session(
                &mut session,
                qiyas_ai::surface::EXECUTE,
                "run",
                &json!({"command":command}),
                qiyas_core::Source::User,
            )
            .unwrap();
            let qiyas_ai::ToolOutput::Json(result) = output else {
                panic!("export result")
            };
            let path = folder.join(filename);
            assert_eq!(result["written"], path.to_string_lossy().as_ref());
            assert!(path.is_file());
            assert_eq!(&std::fs::read(&path).unwrap()[..2], b"PK");
        }
        assert!(folder.join("Villa plans - Register.xlsx").is_file());
    }

    #[test]
    fn mizan_edits_report_whole_part_overlaps_and_arrange_clears_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = qiyas_core::Session::new_blank(&dir.path().join("Villa.dwg"), 6);
        let run = |s: &mut qiyas_core::Session, command: Value| {
            let result = qiyas_ai::surface::run_session(
                s,
                qiyas_ai::surface::EXECUTE,
                "run",
                &json!({"command":command}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
            let qiyas_ai::ToolOutput::Json(value) = result else {
                panic!("drawing result")
            };
            value
        };
        run(
            &mut session,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[10,8]}),
        );
        let table = run(
            &mut session,
            json!({"type":"table","at":[2,7],"rows":2,"cols":1,
            "row_height":0.5,"col_width":2,"data":[["Rooms"],["Living"]]}),
        );
        assert!(
            table["check"]["counts"]["part_overlap"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
        let arranged = run(&mut session, json!({"type":"arrange"}));
        assert_eq!(
            arranged["check"]["counts"]["part_overlap"]
                .as_u64()
                .unwrap_or(0),
            0
        );
        let moved = arranged["outcome"]["moves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["label"] == "Table")
            .unwrap();
        let delta = [0, 1].map(|axis| {
            moved["from"][axis].as_f64().unwrap() - moved["to"][axis].as_f64().unwrap()
        });
        let overlap = run(
            &mut session,
            json!({"type":"modify","kind":"move",
            "targets":[{"handle":moved["handles"][0]}],"delta":delta}),
        );
        assert!(
            overlap["check"]["counts"]["part_overlap"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    }

    #[test]
    fn checked_file_operations_refuse_stale_drawings_under_the_mutation_lock() {
        let (_dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        };
        host.new_drawing("m").unwrap();
        let state = app.state::<AppState>();
        let expected = with_session(&state, |s| Ok(json!(s.revision()))).unwrap();
        with_session(&state, |s| {
            s.run_input(
                &json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
                qiyas_core::Source::User,
            )
        })
        .unwrap();
        let before = json!(tabs::snapshot(&state));
        // Call the checked host directly as well as its public dispatch: these are the
        // second checks, after the tool's earlier read/validation released its lock.
        let checked = AppHost {
            app: app.handle().clone(),
            expected_revision: Some(expected.clone()),
            binding: None,
        };
        for error in [
            checked.new_drawing("m").unwrap_err(),
            checked
                .switch(before["active"].as_u64().unwrap() as u32)
                .unwrap_err(),
        ] {
            let error: Value = serde_json::from_str(&error).unwrap();
            assert_eq!(error["code"], "stale_read");
        }
        for (name, args) in [
            ("new_drawing", json!({"name":"Villa plans","units":"m"})),
            ("save_drawing", json!({})),
            ("close_drawing", json!({})),
        ] {
            let mut args = args;
            args["expected_revision"] = expected.clone();
            let error = qiyas_ai::host::run_host(&host, name, &args, None).unwrap_err();
            let error: Value = serde_json::from_str(&error).unwrap();
            assert_eq!(error["code"], "stale_read");
            assert_eq!(json!(tabs::snapshot(&state)), before);
        }
        let fresh = with_session(&state, |s| Ok(json!(s.revision()))).unwrap();
        qiyas_ai::host::run_host(
            &host,
            "new_drawing",
            &json!({"name":"Villa plans","units":"cm","expected_revision":fresh}),
            None,
        )
        .unwrap();
        assert_eq!(tabs::snapshot(&state).tabs.len(), 2);
    }

    #[test]
    fn external_sheet_authority_excludes_turn_start_without_holding_status() {
        let (_dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        };
        let state = app.state::<AppState>();
        let (_, generation) = state.ai.lock().unwrap().reserve().unwrap();
        let refused = host.with_sheet_authority(&mut || panic!("the turn owns the sheet"));
        assert!(refused
            .unwrap_err()
            .to_string()
            .contains("Mizan owns the sheet"));
        state.ai.lock().unwrap().release(None, generation);

        host.with_sheet_authority(&mut || {
            assert!(matches!(
                state.sheet_authority.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            assert!(
                state.ai.try_lock().is_ok(),
                "status remains readable during external work"
            );
            host.new_drawing("m").map_err(qiyas_ai::ToolError::Failed)?;
            let first = tabs::snapshot(&state).active.unwrap();
            host.new_drawing("mm")
                .map_err(qiyas_ai::ToolError::Failed)?;
            host.switch(first).map_err(qiyas_ai::ToolError::Failed)?;
            host.close(Some(first))
                .map_err(qiyas_ai::ToolError::Failed)?;
            Ok(qiyas_ai::ToolOutput::Json(serde_json::json!({})))
        })
        .unwrap();
        assert!(state.ai.try_lock().is_ok());
    }

    #[test]
    fn u2_new_drawing_reuses_empty_untitled_tabs_with_matching_units() {
        let (_dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        };
        let state = app.state::<AppState>();
        host.new_drawing("mm").unwrap();
        let first = tabs::snapshot(&state).active;
        let again = host.new_drawing("mm").unwrap();
        assert_eq!(tabs::snapshot(&state).tabs.len(), 1);
        assert_eq!(tabs::snapshot(&state).active, first);
        assert!(again["message"].as_str().unwrap().contains("empty"));
        host.new_drawing("m").unwrap();
        host.new_drawing("mm").unwrap();
        assert_eq!(tabs::snapshot(&state).tabs.len(), 2);
        assert_eq!(tabs::snapshot(&state).active, first);
        with_session(&state, |s| {
            s.run(Command::Draw(DrawSpec::Line {
                a: [0.0, 0.0],
                b: [1000.0, 0.0],
            }))
        })
        .unwrap();
        host.new_drawing("mm").unwrap();
        assert_eq!(
            tabs::snapshot(&state).tabs.len(),
            3,
            "a drawing with work must not be reused"
        );
    }

    #[test]
    fn u6_untitled_exports_have_a_visible_documents_destination() {
        let (_dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
        };
        let made = host.new_drawing("m").unwrap();
        // A runner without user folders (CI) gets the app's fallback, home/Documents.
        let documents = app
            .path()
            .document_dir()
            .or_else(|_| app.path().home_dir().map(|home| home.join("Documents")))
            .unwrap();
        assert_eq!(
            made["drawing"]["export_folder"],
            documents.display().to_string()
        );
        assert!(!documents.starts_with(&app.state::<AppState>().config_dir));
    }

    #[test]
    fn new_open_switch_save_and_close_keep_the_conversation() {
        let (dir, app) = app();
        let host = AppHost {
            app: app.handle().clone(),
            expected_revision: None,
            binding: None,
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
            expected_revision: None,
            binding: None,
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
            expected_revision: None,
            binding: None,
        };
        host.new_drawing("m").unwrap();
        let state = app.state::<AppState>();
        let project = with_session(&state, |s| s.save_project()).unwrap();
        assert!(!project.exists());
    }
}
