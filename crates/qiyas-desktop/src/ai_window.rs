//! The window's half of Mizan's window tools (`qiyas_ai::window`). The core asks the window and
//! waits for its reply (`ai_window_reply`): a picture of its main view (`ai:capture`), or what
//! the user points at now (`ai:pointed`: the selection, a marked area, the view). It also asks
//! the window to select entities (`ai:select`), with no reply.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use qiyas_ai::{Screen, TurnContext};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::state::AppState;

/// Asks for a picture of the main view: `{ id, maxSide }`, answered with `{ png, bounds }`.
pub const CAPTURE: &str = "ai:capture";
/// Asks what the user points at: `{ id }`, answered with a `TurnContext`.
pub const POINTED: &str = "ai:pointed";
/// Read visible panels, without opening or changing any of them.
pub const PANELS: &str = "ai:panels";
/// Asks the window to select entities: `{ handles, zoom }`.
pub const SELECT: &str = "ai:select";
/// How long the window may take to answer.
const WAIT: Duration = Duration::from_secs(5);

/// The questions put to the window and not answered yet.
#[derive(Default)]
pub struct AiWindow {
    waiting: Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>,
    next: AtomicU64,
}

/// What the window sends for `ai:capture`.
#[derive(Deserialize)]
struct Captured {
    /// Base64.
    png: String,
    bounds: [[f64; 2]; 2],
}

impl AiWindow {
    /// Sends `event` with `payload` and an `id`, and waits (at most `WAIT`) for the window's
    /// reply. Blocks: call it off the async workers.
    fn ask<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        event: &str,
        mut payload: Value,
    ) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        payload["id"] = json!(id);
        let (tx, rx) = mpsc::channel();
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        let got = app
            .emit(event, payload)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                rx.recv_timeout(WAIT)
                    .map_err(|_| "The Qiyas window did not answer in time".to_string())
            });
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        got?
    }

    /// Hands the reply `id` to the question waiting for it; false when none waits (too late).
    fn answer(&self, id: u64, reply: Result<Value, String>) -> bool {
        let waiting = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        waiting.is_some_and(|tx| tx.send(reply).is_ok())
    }

    /// A picture of the main view, at most `max_side` pixels a side.
    pub fn capture<R: Runtime>(&self, app: &AppHandle<R>, max_side: u32) -> Result<Screen, String> {
        let reply = self.ask(app, CAPTURE, json!({ "maxSide": max_side }))?;
        let Captured { png, bounds } =
            serde_json::from_value(reply).map_err(|e| format!("The window's picture: {e}"))?;
        Ok(Screen {
            png: qiyas_ai::from_base64(&png)?,
            bounds,
        })
    }

    /// What the user points at now.
    pub fn pointed<R: Runtime>(&self, app: &AppHandle<R>) -> Result<TurnContext, String> {
        let reply = self.ask(app, POINTED, json!({}))?;
        serde_json::from_value(reply).map_err(|e| format!("What the window points at: {e}"))
    }

    pub fn panels<R: Runtime>(&self, app: &AppHandle<R>) -> Result<Value, String> {
        self.ask(app, PANELS, json!({}))
    }

    /// Asks the window to select `handles`, zooming to them with `zoom`.
    pub fn select<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        handles: &[u64],
        zoom: bool,
    ) -> Result<(), String> {
        app.emit(SELECT, json!({ "handles": handles, "zoom": zoom }))
            .map_err(|e| e.to_string())
    }
}

/// The window's reply to question `id` (`ai:capture`, `ai:pointed`): `value`, or why it has none.
#[tauri::command]
pub async fn ai_window_reply(
    id: u64,
    value: Option<Value>,
    error: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let reply = match (value, error) {
        (_, Some(why)) => Err(why),
        (Some(value), None) => Ok(value),
        (None, None) => Err("The window sent nothing".to_string()),
    };
    state.ai_window.answer(id, reply);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::{Listener, Manager};

    use super::*;

    /// An app whose window answers `event` with what `reply` makes of the question.
    fn app_answering(
        event: &'static str,
        reply: impl Fn(&Value) -> (Option<Value>, Option<String>) + Send + Sync + 'static,
    ) -> (
        tempfile::TempDir,
        tauri::App<tauri::test::MockRuntime>,
        Arc<Mutex<Vec<Value>>>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        app.manage(AppState::new(dir.path().join("config")));
        let handle = app.handle().clone();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        let reply = Arc::new(reply);
        handle.clone().listen(event, move |e| {
            let question: Value = serde_json::from_str(e.payload()).unwrap();
            seen.lock().unwrap().push(question.clone());
            let id = question["id"].as_u64().unwrap();
            let (value, error) = reply(&question);
            let app = handle.clone();
            std::thread::spawn(move || {
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(ai_window_reply(id, value, error, state)).unwrap();
            });
        });
        (dir, app, asked)
    }

    #[test]
    fn a_capture_waits_for_the_windows_picture() {
        let (_dir, app, asked) = app_answering(CAPTURE, |_| {
            (
                Some(json!({ "png": "iVBORw0KGgo=", "bounds": [[0, 0], [10, 5]] })),
                None,
            )
        });
        let handle = app.handle();
        let screen = handle
            .state::<AppState>()
            .ai_window
            .capture(handle, 800)
            .unwrap();
        assert_eq!(&screen.png[..4], b"\x89PNG");
        assert_eq!(screen.bounds, [[0.0, 0.0], [10.0, 5.0]]);
        assert_eq!(asked.lock().unwrap()[0]["maxSide"], 800);
    }

    #[test]
    fn the_window_says_what_the_user_points_at_or_why_not() {
        let (_dir, app, _) = app_answering(POINTED, |_| {
            (
                Some(json!({ "selection": [{ "handle": 7 }], "view": [[0, 0], [1, 1]] })),
                None,
            )
        });
        let handle = app.handle();
        let pointed = handle
            .state::<AppState>()
            .ai_window
            .pointed(handle)
            .unwrap();
        assert_eq!(pointed.selection[0].handle, 7);
        assert_eq!(pointed.view, Some([[0.0, 0.0], [1.0, 1.0]]));

        let (_dir, app, _) =
            app_answering(CAPTURE, |_| (None, Some("No drawing is on screen".into())));
        let handle = app.handle();
        let err = handle
            .state::<AppState>()
            .ai_window
            .capture(handle, 800)
            .unwrap_err();
        assert_eq!(err, "No drawing is on screen");
        // A reply nobody waits for is dropped.
        assert!(!handle
            .state::<AppState>()
            .ai_window
            .answer(999, Err("late".into())));
    }
    #[test]
    fn panels_use_the_existing_window_reply_channel() {
        let (_dir, app, asked) = app_answering(PANELS, |_| {
            (
                Some(json!({"open_panel":"assistant","layers":{"view":"flat"}})),
                None,
            )
        });
        let state = app.state::<AppState>();
        let panels = state.ai_window.panels(app.handle()).unwrap();
        assert_eq!(panels["layers"]["view"], "flat");
        assert_eq!(asked.lock().unwrap().len(), 1);
    }
}
