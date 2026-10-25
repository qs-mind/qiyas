//! Stable drawing workspaces. Tab changes never move sessions or cancel turns.
use crate::settings::same_file;
use crate::state::{lock, AppState};
use qiyas_core::{DrawingInfo, Session};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Runtime, State};

pub const UNSAVED: &str = "unsaved";
pub const WORKING: &str = "mizan_working";

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct TabInfo {
    pub id: u32,
    pub path: String,
    pub title: String,
    pub dirty: bool,
    pub untitled: bool,
    pub busy: bool,
    pub status: String,
    pub away: Option<String>,
}
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Tabs {
    pub tabs: Vec<TabInfo>,
    pub active: Option<u32>,
}

/// Where a new drawing stands until its first Save As: `<config>/untitled/Untitled-N.dwg`, with
/// the smallest N no open tab, autosave or conversation history already uses.
/// A crash's work and an earlier drawing's chats are never assigned to a new drawing.
pub(crate) fn untitled_path(state: &AppState) -> Result<PathBuf, String> {
    let dir = state.config_dir.join("untitled");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let open: Vec<String> = snapshot(state).tabs.into_iter().map(|t| t.path).collect();
    let history = crate::conversations::Store::new(&state.config_dir).drawing_paths()?;
    (1u32..)
        .map(|n| dir.join(format!("Untitled-{n}.dwg")))
        .find(|p| {
            !open.contains(&p.display().to_string())
                && !qiyas_core::autosave_path(p).exists()
                && !history.iter().any(|old| crate::settings::same_file(old, p))
        })
        .ok_or_else(|| "No name left for a new drawing".to_string())
}

/// Metadata refresh never waits for a busy drawing. It returns the last committed state.
pub(crate) fn snapshot(state: &AppState) -> Tabs {
    let (active, workspaces) = {
        let tabs = lock(&state.tabs);
        (
            tabs.active,
            tabs.order
                .iter()
                .filter_map(|id| tabs.workspaces.get(id).cloned())
                .collect::<Vec<_>>(),
        )
    };
    let tabs = workspaces
        .into_iter()
        .filter_map(|workspace| {
            if let Ok(session) = workspace.session.try_lock() {
                if let Some(session) = session.as_ref() {
                    workspace.publish(session);
                }
            }
            let busy = lock(&workspace.ai).busy();
            let (status, away) = {
                let view = lock(&workspace.view);
                (view.status.to_string(), view.away.clone())
            };
            let info = lock(&workspace.published).clone()?;
            Some({
                let path = Path::new(&info.path);
                TabInfo {
                    id: workspace.id,
                    path: info.path.clone(),
                    title: path
                        .file_name()
                        .map_or_else(|| info.path.clone(), |n| n.to_string_lossy().into_owned()),
                    dirty: workspace.dirty.load(std::sync::atomic::Ordering::Relaxed),
                    untitled: info.untitled,
                    busy,
                    status,
                    away,
                }
            })
        })
        .collect();
    Tabs { tabs, active }
}

pub(crate) fn find_path(state: &AppState, path: &Path) -> Option<u32> {
    snapshot(state)
        .tabs
        .into_iter()
        .find(|t| same_file(Path::new(&t.path), path))
        .map(|t| t.id)
}

/// An empty untitled drawing with matching units can serve another new-drawing request.
/// The drawing on screen is tried first. A drawing Mizan is working in is never reused.
pub(crate) fn reusable_blank(state: &AppState, units: i16) -> Option<u32> {
    let (active, mut workspaces) = {
        let tabs = lock(&state.tabs);
        (
            tabs.active,
            tabs.order
                .iter()
                .filter_map(|id| tabs.workspaces.get(id).cloned())
                .collect::<Vec<_>>(),
        )
    };
    workspaces.sort_by_key(|w| Some(w.id) != active);
    workspaces
        .into_iter()
        .find(|w| {
            !lock(&w.ai).busy()
                && w.session
                    .try_lock()
                    .ok()
                    .is_some_and(|s| s.as_ref().is_some_and(|s| s.is_reusable_blank(units)))
        })
        .map(|w| w.id)
}

/// Choose a visible export folder while the drawing still has no saved file.
pub(crate) fn set_untitled_export_folder<R: Runtime>(
    app: &AppHandle<R>,
    session: &mut Session,
) -> Result<(), String> {
    use tauri::Manager;
    let directory = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir().map(|home| home.join("Documents")))
        .map_err(|e| format!("Choose a location with Save As before exporting: {e}"))?;
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("The Documents folder cannot be opened: {e}"))?;
    session.set_export_directory(directory);
    Ok(())
}

pub(crate) fn add(state: &AppState, session: Session) -> Result<u32, String> {
    add_checked(state, session, None)
}

pub(crate) fn add_checked(
    state: &AppState,
    mut session: Session,
    expected: Option<&serde_json::Value>,
) -> Result<u32, String> {
    check_expected(state, expected)?;
    let name = lock(&state.settings).measurement_method.clone();
    let method = crate::methods::load_at(&crate::methods::qiyas_dir()?, name.as_deref())?;
    crate::methods::apply(&mut session, method.as_ref()).map_err(|e| e.to_string())?;
    let existing = state.scoped(None)?;
    if existing.closed.load(std::sync::atomic::Ordering::Acquire) {
        return Err("The drawing is closed".into());
    }
    // A no-drawing conversation adopts its first drawing without replacing its session Arc.
    let empty = lock(&existing.published).is_none();
    let workspace = if empty {
        existing.workspace.clone()
    } else {
        let id = {
            let mut tabs = lock(&state.tabs);
            let id = tabs.next_id;
            tabs.next_id += 1;
            id
        };
        state.make_workspace(id)
    };
    workspace
        .closed
        .store(false, std::sync::atomic::Ordering::Release);
    workspace.publish(&session);
    // A newly visible drawing has a committed canvas available for a later reload mid-tool.
    *lock(&workspace.render) = crate::state::PublishedRender {
        version: session.batch_version(),
        batch: session.batch_bytes_if(None, None),
        atlas: Some(session.atlas_bytes(None)),
    };
    lock(&workspace.pictures).show(session.epoch());
    *lock(&workspace.session) = Some(session);
    let id = workspace.id;
    let mut tabs = lock(&state.tabs);
    tabs.workspaces.insert(id, workspace);
    if !tabs.order.contains(&id) {
        tabs.order.push(id);
    }
    tabs.active = Some(id);
    Ok(id)
}

fn check_expected(state: &AppState, expected: Option<&serde_json::Value>) -> Result<(), String> {
    if expected.is_none() {
        return Ok(());
    }
    let state = state.scoped(None)?;
    let session = lock(&state.session);
    qiyas_ai::host::check_expected_revision(session.as_ref(), expected)
}

pub(crate) fn activate<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    id: u32,
    keep_conversation: bool,
) -> Result<(), String> {
    activate_checked(app, state, id, keep_conversation, None)
}
pub(crate) fn activate_checked<R: Runtime>(
    _app: &AppHandle<R>,
    state: &AppState,
    id: u32,
    _keep_conversation: bool,
    expected: Option<&serde_json::Value>,
) -> Result<(), String> {
    check_expected(state, expected)?;
    let mut tabs = lock(&state.tabs);
    if !tabs.workspaces.contains_key(&id) {
        return Err(format!("No tab {id}"));
    }
    tabs.active = Some(id);
    Ok(())
}

pub(crate) fn close<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    id: u32,
    force: bool,
    keep_conversation: bool,
) -> Result<(), String> {
    close_checked(app, state, id, force, keep_conversation, None)
}
pub(crate) fn close_checked<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    id: u32,
    force: bool,
    _keep_conversation: bool,
    expected: Option<&serde_json::Value>,
) -> Result<(), String> {
    check_expected(state, expected)?;
    let target = state.scoped(Some(id))?;
    let mut turn = lock(&target.ai);
    // Stopping a turn is a separate explicit action. Force only means discard unsaved edits.
    if turn.busy() {
        return Err(WORKING.into());
    }
    let mut session = match target.session.try_lock() {
        Ok(session) => session,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Err("workspace_busy".into()),
    };
    if !force && session.as_ref().is_some_and(Session::is_dirty) {
        return Err(UNSAVED.into());
    }
    if let Some(s) = session.as_ref().filter(|s| s.is_dirty()) {
        qiyas_core::discard_autosave(&s.drawing.path).map_err(|e| e.to_string())?;
    }
    target
        .closed
        .store(true, std::sync::atomic::Ordering::Release);
    turn.new_conversation();
    *lock(&target.view) = Default::default();
    *lock(&target.context) = Default::default();
    *lock(&target.render) = Default::default();
    lock(&target.reads).clear();
    *session = None;
    *lock(&target.published) = None;
    drop(session);
    drop(turn);
    let lobby_id = {
        let mut tabs = lock(&state.tabs);
        if tabs.lobby.as_ref().is_some_and(|lobby| lobby.id == id) {
            let next = tabs.next_id;
            tabs.next_id += 1;
            Some(next)
        } else {
            None
        }
    };
    let lobby = lobby_id.map(|id| state.make_workspace(id));
    let mut tabs = lock(&state.tabs);
    if let Some(lobby) = lobby {
        tabs.lobby = Some(lobby);
    }
    let pos = tabs
        .order
        .iter()
        .position(|t| *t == id)
        .ok_or_else(|| format!("No tab {id}"))?;
    tabs.order.remove(pos);
    tabs.workspaces.remove(&id);
    if tabs.active == Some(id) {
        tabs.active = tabs.order.get(pos).or_else(|| tabs.order.last()).copied();
    }
    drop(tabs);
    let _ = app.emit("workspace:closed", id);
    Ok(())
}

#[tauri::command]
pub async fn tabs(state: State<'_, AppState>) -> Result<Tabs, String> {
    Ok(snapshot(&state))
}

#[tauri::command]
pub async fn activate_tab<R: Runtime>(
    id: u32,
    keep_conversation: Option<bool>,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    activate(&app, &state, id, keep_conversation.unwrap_or(false))?;
    let state = state.scoped(Some(id))?;
    let info = lock(&state.published).clone().ok_or("No drawing is open")?;
    Ok(info)
}
#[tauri::command]
pub async fn close_tab<R: Runtime>(
    id: u32,
    force: Option<bool>,
    keep_conversation: Option<bool>,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Tabs, String> {
    close(
        &app,
        &state,
        id,
        force.unwrap_or(false),
        keep_conversation.unwrap_or(false),
    )?;
    Ok(snapshot(&state))
}
