//! The MCP server's life in the app: started at launch when Settings turn it on, restarted when
//! its settings change, and stopped when they turn it off. Tool calls from outside AI apps change
//! the take-off, so each change tells the window with `takeoff:changed`; a sheet, layer or zoom
//! change sends `session:changed` with what to show.

use std::sync::{Arc, PoisonError};

use qiyas_ai::mcp::{serve_workspaces, OnMutated, OnUi, Qiyas};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio_util::sync::CancellationToken;

use crate::settings::Settings;
use crate::state::AppState;

/// Sent to the window after an MCP tool changed the take-off or the BOQ work.
pub const CHANGED: &str = "takeoff:changed";
/// Sent to the window, with the tool result's `ui`, after an MCP tool showed another sheet, hid or
/// showed layers, or asked for a zoom.
pub const SESSION_CHANGED: &str = "session:changed";

/// What the Settings dialog shows about the server.
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct McpStatus {
    pub enabled: bool,
    pub listening: bool,
    /// `127.0.0.1:<port>` while listening.
    pub addr: Option<String>,
    /// Why it could not start (the port is taken, say).
    pub error: Option<String>,
}

#[derive(Default)]
pub struct McpRun {
    /// Stops the running server; `None` when none runs.
    shutdown: Option<CancellationToken>,
    pub status: McpStatus,
}

/// The server must restart when it is turned on or off, or its port or token changed.
pub fn needs_restart(old: &Settings, new: &Settings) -> bool {
    (old.mcp_enabled, old.mcp_port, &old.mcp_token)
        != (new.mcp_enabled, new.mcp_port, &new.mcp_token)
}

/// Stops the server if one runs, then starts it with the current settings if they turn it on.
pub fn restart<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut run = state.mcp.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(old) = run.shutdown.take() {
        old.cancel();
    }
    run.status = McpStatus {
        enabled: settings.mcp_enabled,
        ..McpStatus::default()
    };
    if !settings.mcp_enabled {
        return;
    }
    let shutdown = CancellationToken::new();
    run.shutdown = Some(shutdown.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let window = app.clone();
        let resolver: qiyas_ai::mcp::WorkspaceResolver = Arc::new(move |id| {
            let state = window.state::<AppState>().scoped(Some(id))?;
            Ok(handler(&window, state))
        });
        let window = app.clone();
        let factory = Arc::new(move || {
            let state = window
                .state::<AppState>()
                .scoped(None)
                .expect("active workspace exists");
            handler(&window, state).with_workspaces(resolver.clone())
        });
        let result = serve_workspaces(
            factory,
            settings.mcp_port,
            settings.mcp_token,
            shutdown.clone(),
        )
        .await;
        let state = app.state::<AppState>();
        let mut run = state.mcp.lock().unwrap_or_else(PoisonError::into_inner);
        // Replaced or stopped while it was starting: the newer status stands.
        if shutdown.is_cancelled() {
            return;
        }
        match result {
            Ok(addr) => {
                run.status.listening = true;
                run.status.addr = Some(addr.to_string());
            }
            Err(e) => {
                eprintln!("{e}");
                run.status.error = Some(e.to_string());
                run.shutdown = None;
            }
        }
    });
}

fn handler<R: Runtime>(app: &AppHandle<R>, state: AppState) -> Qiyas {
    let id = state.id;
    let session = state.session.clone();
    let window = app.clone();
    let changed = state.clone();
    let on_mutated: OnMutated = Arc::new(move || {
        if let Ok(session) = changed.session.try_lock() {
            if let Some(session) = session.as_ref() {
                changed.publish(session);
            }
        }
        let _ = window.emit(CHANGED, serde_json::json!({"workspace":id}));
    });
    let window = app.clone();
    let on_ui: OnUi = Arc::new(move |mut ui| {
        ui["workspace"] = serde_json::json!(id);
        let _ = window.emit(SESSION_CHANGED, ui);
    });
    let host = Arc::new(crate::drawing_host::AppHost {
        app: app.clone(),
        expected_revision: None,
        binding: Some(state),
    });
    Qiyas::new(session, Some(host), on_mutated, on_ui)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_only_for_mcp_changes() {
        let old = Settings::default();
        let same = |f: fn(&mut Settings)| {
            let mut new = old.clone();
            f(&mut new);
            needs_restart(&old, &new)
        };
        assert!(!same(|_| {}));
        assert!(!same(|s| s.profile_mut().model = "other".into()));
        assert!(same(|s| s.mcp_enabled = !s.mcp_enabled));
        assert!(same(|s| s.mcp_port = 48124));
        assert!(same(|s| s.mcp_token = "f".repeat(32)));
    }
}
