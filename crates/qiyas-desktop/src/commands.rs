//! IPC commands for the frontend. Each one locks the session, does its work and maps errors to text.
//! Drawing IPC resolves its workspace before awaiting. Long work uses blocking workers;
//! session try-locks keep another drawing responsive. No lock is held across an `.await`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, MutexGuard, PoisonError};

use qiyas_ai::conversation::{Conversation, TranscriptEntry};
use qiyas_ai::prompt::Lang;
use qiyas_ai::{Agent, AgentEvent, AgentLog};
use qiyas_core::{
    Boq, Command, Conflict, CoreError, Drafting, DrawingInfo, EntityInfo, Measurement, NamedView,
    Outcome, Picked, Reported, Session, Snap, Source,
};
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Runtime, State};
use tokio::sync::mpsc;

use crate::ai::{self, AiRun, Turn};
use crate::files;
use crate::mcp::{self, McpStatus};
use crate::secrets;
use crate::settings::{self, Settings};
use crate::state::AppState;
use crate::tabs;

/// Runs `f` on the open session, or fails with "No drawing is open".
pub(crate) fn with_session<R>(
    state: &AppState,
    f: impl FnOnce(&mut Session) -> Result<R, CoreError>,
) -> Result<R, String> {
    let state = state.scoped(None)?;
    // A busy drawing keeps its last committed canvas. The window queues the original request.
    let mut guard = match state.session.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Err("workspace_busy".into()),
    };
    let session = guard.as_mut().ok_or("No drawing is open")?;
    session
        .automation_sheet
        .get_or_insert_with(|| session.sheet.clone());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(session)))
        .map_err(|_| "The command stopped unexpectedly. You can keep working.".to_string())?
        .map_err(|e| e.to_string());
    state.publish(session);
    result
}

pub(crate) fn cached_read<T: serde::Serialize + serde::de::DeserializeOwned>(
    state: &AppState,
    key: &'static str,
    read: impl FnOnce(&mut Session) -> Result<T, CoreError>,
) -> Result<T, String> {
    match with_session(state, read) {
        Ok(value) => {
            crate::state::lock(&state.reads).insert(
                key,
                serde_json::to_value(&value).map_err(|e| e.to_string())?,
            );
            Ok(value)
        }
        Err(error) if error == "workspace_busy" => crate::state::lock(&state.reads)
            .get(key)
            .cloned()
            .ok_or(error)
            .and_then(|value| serde_json::from_value(value).map_err(|e| e.to_string())),
        Err(error) => Err(error),
    }
}

/// Opens a workspace or shows its existing tab. Its Mizan keeps the same conversation.
#[tauri::command]
pub async fn open_drawing<R: Runtime>(
    path: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    let state = state.scoped(None)?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(id) = tabs::find_path(&state, Path::new(&path)) {
            tabs::activate(&app, &state, id, false)?;
            remember_recent(&state, &path);
            return crate::state::lock(&state.scoped(Some(id))?.published)
                .clone()
                .ok_or("No drawing is open".into());
        }
        // Read the file before taking the lock, so a slow open doesn't block other commands.
        let session = Session::open(Path::new(&path)).map_err(|e| e.to_string())?;
        let id = tabs::add(&state, session)?;
        let state = state.scoped(Some(id))?;
        remember_recent(&state, &path);
        with_session(&state, |s| Ok(s.info()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The header's INSUNITS for a new drawing's units: "m", "cm" or "mm".
pub(crate) fn insunits_of(units: &str) -> Result<i16, String> {
    match units {
        "m" => Ok(6),
        "cm" => Ok(5),
        "mm" => Ok(4),
        other => Err(format!("Units must be m, cm or mm, not {other}")),
    }
}

/// A new, empty drawing in `units` ("m", "cm" or "mm") in a new tab, untitled until its first
/// Save As (`tabs::untitled_path`); the assistant starts a new conversation about it. Not a
/// recent drawing until it is saved.
#[tauri::command]
pub async fn new_drawing<R: Runtime>(
    units: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    let units = insunits_of(&units)?;
    if let Some(id) = tabs::reusable_blank(&state, units) {
        tabs::activate(&app, &state, id, false)?;
        let state = state.scoped(Some(id))?;
        return with_session(&state, |s| Ok(s.info()));
    }
    let state = state.scoped(None)?;
    let mut session = Session::new_blank(&tabs::untitled_path(&state)?, units);
    tabs::set_untitled_export_folder(&app, &mut session)?;
    let id = tabs::add(&state, session)?;
    let state = state.scoped(Some(id))?;
    with_session(&state, |s| Ok(s.info()))
}

/// Puts `path` first in the recent list and writes the settings; a failed write is only logged.
pub(crate) fn remember_recent(state: &AppState, path: &str) {
    let mut settings = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    settings.push_recent(path);
    if let Err(e) = settings::save(&state.config_dir, &settings) {
        eprintln!("The recent drawings could not be saved: {e}");
    }
}

/// One drawing of the recent list.
#[derive(Serialize)]
pub struct Recent {
    path: String,
    /// The file name, and the folder it is in.
    name: String,
    folder: String,
    /// The file is not there any more.
    missing: bool,
}

/// The drawings opened or saved lately, most recent first.
#[tauri::command]
pub async fn recents(state: State<'_, AppState>) -> Result<Vec<Recent>, String> {
    let paths = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recents
        .clone();
    Ok(paths
        .into_iter()
        .map(|path| {
            let p = Path::new(&path);
            Recent {
                name: p
                    .file_name()
                    .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned()),
                folder: p
                    .parent()
                    .map(|f| f.display().to_string())
                    .unwrap_or_default(),
                missing: std::fs::metadata(p).is_err(),
                path,
            }
        })
        .collect())
}

/// Empties the recent list.
#[tauri::command]
pub async fn clear_recents(state: State<'_, AppState>) -> Result<(), String> {
    let mut settings = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut next = settings.clone();
    next.recents.clear();
    settings::save(&state.config_dir, &next)
        .map_err(|e| format!("The settings could not be saved: {e}"))?;
    *settings = next;
    Ok(())
}

/// The drawing's named views of model space.
#[tauri::command]
pub async fn views(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<NamedView>, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.views()))
}

#[tauri::command]
pub async fn drawing_info(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    let state = state.scoped(workspace)?;
    if let Ok(session) = state.session.try_lock() {
        if let Some(s) = session.as_ref() {
            state.publish(s);
        }
    }
    let info = crate::state::lock(&state.published)
        .clone()
        .ok_or("No drawing is open".to_string());
    info
}

#[tauri::command]
pub async fn set_sheet(
    workspace: Option<u32>,
    block: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| s.set_sheet(&block))
}

pub(crate) fn window_sheet_guard(state: &AppState) -> Result<MutexGuard<'_, ()>, String> {
    let authority = state
        .sheet_authority
        .try_lock()
        .map_err(|_| "workspace_busy".to_string())?;
    let turn = ai_run(state);
    if turn.busy() {
        return Err("Mizan owns the sheet during its turn; wait until the turn finishes".into());
    }
    Ok(authority)
}

#[tauri::command]
pub async fn set_layer_hidden(
    workspace: Option<u32>,

    name: String,
    hidden: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| {
        s.set_layer_hidden(&name, hidden);
        Ok(())
    })
}

/// Looks for every xref again and reopens it; returns the drawing's info with the new list.
#[tauri::command]
pub async fn reload_xrefs(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| {
        s.reload_xrefs();
        Ok(s.info())
    })
}

/// Uses the file `path` for the xref `name` (`None` goes back to looking for it), reloads the
/// xrefs and returns the drawing's info. The window saves the project after, which remembers it.
#[tauri::command]
pub async fn set_xref_path(
    workspace: Option<u32>,

    name: String,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| {
        s.set_xref_path(&name, path.as_deref().map(Path::new))?;
        Ok(s.info())
    })
}

/// Names the current sheet's line batch; it changes only when the batch would.
#[tauri::command]
pub async fn batch_version(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let state = state.scoped(workspace)?;
    match with_session(&state, |s| Ok(s.batch_version())) {
        Err(error) if error == "workspace_busy" => Ok(crate::state::lock(&state.render).version),
        result => result,
    }
}

/// The current sheet's batch (lines and text glyph quads), in the binary layout of
/// `Batch::to_bytes`. When `if_version` is the current `batch_version`, only the 8 bytes
/// `NOT_MODIFIED`: the window keeps its batch. `px_per_unit` is the zoom to draw the glyphs for
/// (screen pixels per drawing unit); left out, the batch in hand keeps its zoom.
#[tauri::command]
pub async fn render_batch(
    workspace: Option<u32>,

    if_version: Option<u64>,
    px_per_unit: Option<f64>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = with_session(&state, |s| {
            let bytes = batch_response(s, if_version, px_per_unit);
            if bytes != NOT_MODIFIED {
                let mut cache = crate::state::lock(&state.render);
                cache.batch = Some(bytes.clone());
                cache.atlas = Some(s.atlas_bytes(None));
                cache.version = s.batch_version();
            }
            Ok(Response::new(bytes))
        });
        match result {
            Err(error) if error == "workspace_busy" => crate::state::lock(&state.render)
                .batch
                .clone()
                .map(Response::new)
                .ok_or(error),
            result => result,
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Names the batch of the sheet `sheet` as `render_batch` names the current sheet's.
#[tauri::command]
pub async fn sheet_batch_version(
    workspace: Option<u32>,
    sheet: String,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.sheet_batch_version(&sheet)))
}

/// `render_batch` for the sheet `sheet`, which stays off screen: model space for the viewports
/// of a layout, or a split window's second view (`Session::sheet_batch_bytes_if`).
#[tauri::command]
pub async fn sheet_batch(
    workspace: Option<u32>,

    sheet: String,
    if_version: Option<u64>,
    px_per_unit: Option<f64>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&state, |s| {
            Ok(Response::new(
                s.sheet_batch_bytes_if(&sheet, if_version, px_per_unit)
                    .unwrap_or_else(|| NOT_MODIFIED.to_vec()),
            ))
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The glyph atlas pages changed after `since_version` (all when left out), in the binary layout
/// of `Atlas::to_bytes`: `u64` version, `u32` page count, then per page `u32` index, width,
/// height and byte length and the R8 bytes.
#[tauri::command]
pub async fn atlas_pages(
    workspace: Option<u32>,

    since_version: Option<u64>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        match with_session(&state, |s| Ok(Response::new(s.atlas_bytes(since_version)))) {
            Err(error) if error == "workspace_busy" => crate::state::lock(&state.render)
                .atlas
                .clone()
                .map(Response::new)
                .ok_or(error),
            result => result,
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// "QINM" and four zero bytes, in place of a batch that is unchanged. A batch starts with "BYIQ".
const NOT_MODIFIED: [u8; 8] = [0x51, 0x49, 0x4E, 0x4D, 0, 0, 0, 0];

fn batch_response(session: &Session, if_version: Option<u64>, px_per_unit: Option<f64>) -> Vec<u8> {
    session
        .batch_bytes_if(if_version, px_per_unit)
        .unwrap_or_else(|| NOT_MODIFIED.to_vec())
}

#[tauri::command]
pub async fn pick(
    workspace: Option<u32>,

    x: f64,
    y: f64,
    tol: f64,
    state: State<'_, AppState>,
) -> Result<Option<Picked>, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || with_session(&state, |s| Ok(s.pick([x, y], tol))))
        .await
        .map_err(|error| error.to_string())?
}

/// The closed area around (x, y) the drawn lines make, for a hatch: looked for `start` around
/// it first, then further (see `Session::boundary_at`).
#[tauri::command]
pub async fn boundary_at(
    workspace: Option<u32>,

    x: f64,
    y: f64,
    start: f64,
    state: State<'_, AppState>,
) -> Result<qiyas_core::Boundary, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&state, |s| Ok(s.boundary_at([x, y], start)))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The entities a selection box from (`min_x`, `min_y`) to (`max_x`, `max_y`) picks: wholly
/// inside it, or with `crossing` touching it (see `Session::pick_window`).
#[tauri::command]
pub async fn pick_window(
    workspace: Option<u32>,

    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    crossing: bool,
    state: State<'_, AppState>,
) -> Result<Vec<Picked>, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&state, |s| {
            Ok(s.pick_window([[min_x, min_y], [max_x, max_y]], crossing))
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The entities a selection polygon `points` (drawing coordinates) picks: wholly inside it, or
/// with `crossing` touching it (see `Session::pick_polygon`).
#[tauri::command]
pub async fn pick_polygon(
    workspace: Option<u32>,

    points: Vec<[f64; 2]>,
    crossing: bool,
    state: State<'_, AppState>,
) -> Result<Vec<Picked>, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&state, |s| Ok(s.pick_polygon(&points, crossing)))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The object snap within `tol` (drawing units) of `(x, y)`, with the session's snap modes;
/// `from_x`, `from_y` is the previous point, for perpendicular snaps. None while snaps are off.
#[tauri::command]
pub async fn snap(
    workspace: Option<u32>,

    x: f64,
    y: f64,
    tol: f64,
    from_x: Option<f64>,
    from_y: Option<f64>,
    state: State<'_, AppState>,
) -> Result<Option<Snap>, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        let from = from_x.zip(from_y).map(|(fx, fy)| [fx, fy]);
        with_session(&state, |s| {
            Ok(s.drafting
                .osnap
                .then(|| s.snap([x, y], tol, &s.drafting.snap_modes, from))
                .flatten())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The drawing's drafting settings: snaps, ortho, polar, grid and display precision.
#[tauri::command]
pub async fn get_drafting(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Drafting, String> {
    let state = state.scoped(workspace)?;
    cached_read(&state, "drafting", |s| Ok(s.drafting.clone()))
}

/// Replaces the drafting settings (not an undo step); the window saves the project after.
#[tauri::command]
pub async fn set_drafting(
    workspace: Option<u32>,
    drafting: Drafting,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| {
        s.run(Command::SetDrafting { drafting }).map(|_| ())
    })
}

/// One entity; `chain` (outermost block reference first) for an entity inside blocks.
#[tauri::command]
pub async fn entity_info(
    workspace: Option<u32>,

    handle: u64,
    chain: Option<Vec<u64>>,
    state: State<'_, AppState>,
) -> Result<EntityInfo, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| {
        s.entity_info(handle, chain.as_deref().unwrap_or_default())
    })
}

/// The cell `[row, column]` of the table `handle` under (x, y), drawing coordinates; a merged
/// cell by its first cell. `None` outside it.
#[tauri::command]
pub async fn table_cell_at(
    workspace: Option<u32>,

    handle: u64,
    x: f64,
    y: f64,
    state: State<'_, AppState>,
) -> Result<Option<[usize; 2]>, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.table_cell_at(handle, [x, y])))
}

/// Runs one command on the command bus; see `qiyas_core::Command`. A paste without text takes
/// the system clipboard's; a copy's text goes onto the system clipboard (and comes back empty).
#[tauri::command]
pub async fn run_command(
    workspace: Option<u32>,

    mut cmd: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<Reported, String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = cmd.clone();
        if let Some(fields) = command.as_object_mut() {
            fields.remove("expected_revision");
        }
        let parsed = qiyas_core::registry::parse_command(command)?;
        if matches!(parsed, Command::PasteFromClipboard { text: None, .. }) {
            cmd["text"] = serde_json::Value::String(files::clipboard_text(&state)?);
        }
        let mut outcome = with_session(&state, |s| s.run_input(&cmd, Source::User))?;
        qiyas_boq::deliverables::complete(&outcome.outcome)?;
        if let Outcome::Clipboard { text, .. } = &mut outcome.outcome {
            files::set_clipboard_text(&state, text)?;
            text.clear();
        }
        Ok(outcome)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Undoes the last take-off change; false when there was none.
#[tauri::command]
pub async fn undo(workspace: Option<u32>, state: State<'_, AppState>) -> Result<bool, String> {
    let state = state.scoped(workspace)?;
    let _turn = window_sheet_guard(&state)?;
    with_session(&state, |s| Ok(s.undo()))
}

/// Redoes the last undone take-off change; false when there was none.
#[tauri::command]
pub async fn redo(workspace: Option<u32>, state: State<'_, AppState>) -> Result<bool, String> {
    let state = state.scoped(workspace)?;
    let _turn = window_sheet_guard(&state)?;
    with_session(&state, |s| Ok(s.redo()))
}

#[tauri::command]
pub async fn takeoff(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<Measurement>, String> {
    let state = state.scoped(workspace)?;
    cached_read(&state, "takeoff", |s| Ok(s.takeoff.items.clone()))
}

#[tauri::command]
pub async fn totals(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let state = state.scoped(workspace)?;
    cached_read(&state, "totals", |s| {
        Ok(serde_json::json!(s.takeoff.totals()))
    })
}

/// Saves the take-off to the drawing's `.qiyas` project file; returns the path written.
#[tauri::command]
pub async fn save_project(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.save_project()?.display().to_string()))
        .map_err(|e| format!("The take-off could not be saved: {e}"))
}

/// Saves the drawing over its own file, measurements embedded, and its project file.
#[tauri::command]
pub async fn save_drawing(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || with_session(&state, |s| s.save()))
        .await
        .map_err(|error| error.to_string())?
}

/// Saves the drawing to `path` (.dwg or .dxf), measurements embedded, with its project file next
/// to it; the session then works on the new file, which goes first in the recent list.
#[tauri::command]
pub async fn save_drawing_as(
    workspace: Option<u32>,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    tauri::async_runtime::spawn_blocking(move || {
        save_as_with_history(&state, Path::new(&path))?;
        remember_recent(&state, &path);
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn save_as_with_history(state: &AppState, path: &Path) -> Result<(), String> {
    let mut run = ai_run(state);
    let from = with_session(state, |s| {
        let from = s.drawing.path.clone();
        s.save_as(path)?;
        Ok(from)
    })?;
    crate::conversations::Store::new(&state.config_dir).follow_drawing(&from, path)?;
    run.drawing_saved_as(path.to_path_buf());
    Ok(())
}

/// Writes the take-off and its totals to an Excel workbook at `path`.
#[tauri::command]
pub async fn export_takeoff(
    workspace: Option<u32>,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    // Copy the take-off out, so writing the file doesn't hold the lock.
    let (items, totals, title, report, queries) = with_session(&state, |s| {
        let title = match s.drawing.path.file_name() {
            Some(name) => format!("{} take-off", name.to_string_lossy()),
            None => "Take-off".to_string(),
        };
        Ok((
            s.takeoff.items.clone(),
            s.takeoff.totals(),
            title,
            s.measurement_report()?,
            s.conflicts.clone(),
        ))
    })?;
    qiyas_boq::export_takeoff_with_queries(
        Path::new(&path),
        &items,
        &totals,
        &title,
        &report,
        &queries,
    )
    .map_err(|e| e.to_string())
}

/// Writes a table of extracted data (`Command::ExtractData`) to an Excel workbook at `path`, one
/// sheet "Data".
#[tauri::command]
pub async fn export_table(
    path: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
) -> Result<(), String> {
    qiyas_boq::export_table(Path::new(&path), &columns, &rows).map_err(|e| e.to_string())
}

/// Reads the BOQ in the Excel workbook at `path` and makes it the drawing's BOQ (an undo step);
/// returns it.
#[tauri::command]
pub async fn import_boq(
    workspace: Option<u32>,
    path: String,
    state: State<'_, AppState>,
) -> Result<Boq, String> {
    let state = state.scoped(workspace)?;
    // Read the workbook before taking the lock, so a slow read doesn't block other commands.
    let boq = qiyas_boq::import_boq(Path::new(&path)).map_err(|e| e.to_string())?;
    with_session(&state, |s| {
        s.run(Command::SetBoq { boq: boq.clone() })?;
        Ok(boq)
    })
}

/// The drawing's BOQ; null when none is imported.
#[tauri::command]
pub async fn boq(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Option<Boq>, String> {
    let state = state.scoped(workspace)?;
    cached_read(&state, "boq", |s| Ok(s.boq.clone()))
}

#[tauri::command]
pub async fn explain_item(
    workspace: Option<u32>,

    item: String,
    project: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let state = state.scoped(workspace)?;
    if let Some(path) = project {
        return qiyas_core::trust::explain_project(&path, &item).map_err(|e| e.to_string());
    }
    with_session(&state, |s| s.explain(&item))
}

/// The live generated BOQ and traceable dimension sheets, separate from the imported BOQ.
#[tauri::command]
pub async fn measurement_report(
    workspace: Option<u32>,

    state: State<'_, AppState>,
) -> Result<qiyas_core::measurement::MeasurementReport, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| s.measurement_report())
}

/// Compares the BOQ and the take-off with the schedules and block counts of every sheet and keeps
/// the conflicts (an undo step); returns them. `tolerance_default` is the allowed difference in
/// percent for BOQ items with none of their own (5 when null).
#[tauri::command]
pub async fn compare(
    workspace: Option<u32>,

    tolerance_default: Option<f64>,
    state: State<'_, AppState>,
) -> Result<Vec<Conflict>, String> {
    let state = state.scoped(workspace)?;
    let tolerance = tolerance_default.unwrap_or(qiyas_takeoff::DEFAULT_TOLERANCE);
    with_session(&state, |s| {
        qiyas_takeoff::compare_session(s, tolerance, Source::User)
    })
}

/// The conflicts of the last comparison, with their statuses and notes.
#[tauri::command]
pub async fn conflicts(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<Conflict>, String> {
    let state = state.scoped(workspace)?;
    cached_read(&state, "conflicts", |s| Ok(s.conflicts.clone()))
}

/// Writes the conflicts to an Excel workbook at `path`.
#[tauri::command]
pub async fn export_conflicts(
    workspace: Option<u32>,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    // Copy the conflicts out, so writing the file doesn't hold the lock.
    let conflicts = with_session(&state, |s| Ok(s.conflicts.clone()))?;
    qiyas_boq::report::export_conflicts(Path::new(&path), &conflicts).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone())
}

/// The user's command line aliases (alias to command), over the built-in ones.
#[tauri::command]
pub async fn aliases(state: State<'_, AppState>) -> Result<BTreeMap<String, String>, String> {
    Ok(state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .aliases
        .clone())
}

/// Checks and keeps defaults for new workspaces: written to disk first, then cached.
/// MCP turned on or off, or another port or token,
/// restarts the MCP server. The recent list, the tool palette and the time of the last update
/// check stay as the app has them: the dialog's copy may be older.
#[tauri::command]
pub async fn set_settings<R: Runtime>(
    mut settings: Settings,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    settings::validate(&settings)?;
    let old = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if old.providers.openai_compatible.base_url != settings.providers.openai_compatible.base_url {
        keychain(move || secrets::migrate(&old)).await?;
    }
    let mcp_changed = {
        let mut cached = state
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        settings.recents = cached.recents.clone();
        settings.tool_palette = cached.tool_palette.clone();
        settings.last_update_check = cached.last_update_check;
        if cached.measurement_method != settings.measurement_method {
            let method = crate::methods::load_at(
                &crate::methods::qiyas_dir()?,
                settings.measurement_method.as_deref(),
            )?;
            let workspaces = state.workspaces();
            let mut guards = workspaces.iter().map(|workspace| workspace.session.try_lock()
                .map_err(|_| "Mizan is working. Try changing the measurement method after it finishes.".to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            let mut sessions: Vec<_> = guards.iter_mut().filter_map(|s| s.as_mut()).collect();
            crate::methods::apply_all(&mut sessions, method.as_ref(), || {
                settings::save(&state.config_dir, &settings).map_err(qiyas_core::CoreError::from)
            })
            .map_err(|e| e.to_string())?;
        } else {
            settings::save(&state.config_dir, &settings)
                .map_err(|e| format!("The settings could not be saved: {e}"))?;
        }
        let mcp_changed = mcp::needs_restart(&cached, &settings);
        *cached = settings;
        mcp_changed
    };
    {
        let lobby = { crate::state::lock(&state.tabs).lobby.clone() };
        let lobby = lobby.unwrap_or_else(|| state.workspace.clone());
        let run = crate::state::lock(&lobby.ai);
        if !run.busy()
            && run.conversation_id().is_none()
            && crate::state::lock(&lobby.published).is_none()
        {
            *crate::state::lock(&lobby.provider) = crate::state::lock(&state.settings).clone();
        }
    }
    if mcp_changed {
        mcp::restart(&app);
    }
    let _ = app.emit("measurement:changed", ());
    Ok(())
}

/// Runs a keychain call off the async workers: the OS may show a prompt and wait.
async fn keychain<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// The models the provider offers, for the settings' list: `provider` and `base_url` as chosen in
/// the dialog (the saved ones when left out). The key is read from the keychain and never sent
/// to the window. When the service can't be asked, the built-in list with why.
#[tauri::command]
pub async fn ai_models(
    provider: Option<String>,
    base_url: Option<String>,
    typed_key: Option<String>,
    state: State<'_, AppState>,
) -> Result<qiyas_ai::models::ModelList, String> {
    use qiyas_ai::models::{model_list, ModelSource};
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if matches!(
        provider.as_deref().unwrap_or(settings.provider.name()),
        "claude_code" | "chatgpt_plan"
    ) {
        return Ok(qiyas_ai::models::ModelList {
            models: Vec::new(),
            from_service: false,
            error: None,
        });
    }
    let anthropic = match provider.as_deref() {
        Some(p) => p == "anthropic",
        None => settings.provider == settings::Provider::Anthropic,
    };
    let name = if anthropic {
        settings::Provider::Anthropic.name()
    } else {
        settings::Provider::OpenaiCompatible.name()
    };
    // A draft key is explicit and never saved here. Without one, Goal A's saved-address
    // check runs before any keychain read. Never pair a saved key with a draft destination.
    let typed_key = typed_key.filter(|key| !key.trim().is_empty());
    let requested = if typed_key.is_some() {
        if anthropic {
            Ok("https://api.anthropic.com/v1".into())
        } else {
            qiyas_ai::endpoint::normalize(base_url.as_deref().unwrap_or(""))
        }
    } else {
        secrets::discovery_address(
            &settings,
            name,
            if anthropic { None } else { base_url.as_deref() },
        )
    };
    let requested = match requested {
        Ok(base) => base,
        Err(error) => {
            return Ok(qiyas_ai::models::ModelList {
                models: Vec::new(),
                from_service: false,
                error: Some(error),
            })
        }
    };
    let key = match typed_key {
        Some(key) => Some(key),
        None => keychain(move || secrets::get_saved(&settings, name)).await?,
    };
    let source = if anthropic {
        ModelSource::Anthropic
    } else {
        ModelSource::OpenAiCompatible {
            base_url: &requested,
        }
    };
    Ok(model_list(source, key.as_deref()).await)
}

/// Checks only documented CLI metadata when the user presses Check in Settings.
#[tauri::command]
pub async fn claude_code_status(
    executable: Option<String>,
) -> Result<qiyas_ai::claude_code::Status, String> {
    let home = qiyas_core::data_home::qiyas_dir()?;
    qiyas_ai::claude_code::status(executable.as_deref(), &home)
        .await
        .map_err(|e| e.for_window())
}

/// Stores the provider's API key in the OS keychain. The key never comes back to the window. The
/// assistant's next message starts a new conversation with it.
#[tauri::command]
pub async fn set_api_key<R: Runtime>(
    provider: String,
    key: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let saved = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let affected = provider.clone();
    let base = secrets::address(&saved, &provider)?;
    keychain(move || {
        secrets::migrate(&saved)?;
        secrets::set_at(&provider, &base, &key)
    })
    .await?;
    refresh_credentials(&state, &affected);
    let _ = app.emit("workspace:changed", ());
    Ok(())
}

#[tauri::command]
pub async fn has_api_key(provider: String, state: State<'_, AppState>) -> Result<bool, String> {
    let saved = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    keychain(move || Ok(secrets::get_saved(&saved, &provider)?.is_some())).await
}

#[tauri::command]
pub async fn delete_api_key<R: Runtime>(
    provider: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let saved = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let affected = provider.clone();
    let base = secrets::address(&saved, &provider)?;
    keychain(move || {
        secrets::migrate(&saved)?;
        secrets::delete_at(&provider, &base)
    })
    .await?;
    refresh_credentials(&state, &affected);
    let _ = app.emit("workspace:changed", ());
    Ok(())
}

fn refresh_credentials(state: &AppState, provider: &str) {
    let workspaces = state.conversations();
    for workspace in workspaces {
        let mut run = crate::state::lock(&workspace.ai);
        if crate::state::lock(&workspace.provider).provider.name() == provider {
            run.rebuild_provider();
        }
    }
}

/// The folder of the running app, where the `qiyas-mcp` bridge sits next to it. In a dev build
/// (`pnpm tauri dev`) that is `target/debug`, where `cargo build -p qiyas-mcp` puts the bridge.
#[tauri::command]
pub async fn app_dir() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("The app's folder is unknown")?;
    Ok(dir.display().to_string())
}

/// Whether the MCP server is on, listening, or why it could not start.
#[tauri::command]
pub async fn mcp_status(state: State<'_, AppState>) -> Result<McpStatus, String> {
    Ok(state
        .mcp
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .status
        .clone())
}

// ---- The in-app assistant ----

pub(crate) fn ai_run(state: &AppState) -> MutexGuard<'_, AiRun> {
    state.ai.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Saves and resets the assistant's conversation (cancelling a running turn), then tells the window, which
/// clears its transcript on `ai:cleared`.
pub(crate) fn new_conversation<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    ai_run(state).new_conversation();
    let event = {
        let mut view = crate::state::lock(&state.view);
        view.transcript.clear();
        view.status = "idle";
        view.error = None;
        view.away = None;
        view.record(state.id, serde_json::json!({"type":"clear"}))
    };
    let _ = app.emit("workspace:event", event);
    let _ = app.emit("ai:cleared", serde_json::json!({"workspace":state.id}));
}

/// Sent as `ai:done` when a turn has ended; `error` when it failed.
#[derive(Serialize, Clone)]
struct AiDone {
    error: Option<String>,
    workspace: u32,
}

/// Sends `text` to the assistant, with what the user points at (`context`: the selection, a
/// marked region, the view) and the pictures they attached (`images`), and returns at once; the
/// turn runs in the background and sends each `AgentEvent` as `ai:event`, then `ai:done`. Fails
/// with "busy" while a turn runs, and with "no_api_key" or "no_base_url" when the settings can't
/// reach a provider.
#[tauri::command]
pub async fn ai_send<R: Runtime>(
    workspace: Option<u32>,

    text: String,
    pointed: Option<Vec<String>>,
    context: Option<qiyas_ai::TurnContext>,
    images: Option<Vec<qiyas_ai::UserImage>>,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    let images = images.unwrap_or_default();
    if text.trim().is_empty() && images.is_empty() {
        return Err("The message is empty".into());
    }
    if images.len() > qiyas_ai::context::MAX_USER_IMAGES {
        return Err(format!(
            "At most {} pictures in one message",
            qiyas_ai::context::MAX_USER_IMAGES
        ));
    }
    if let Some(why) = images.iter().find_map(qiyas_ai::UserImage::refused) {
        return Err(why);
    }
    let message = qiyas_ai::UserMessage {
        text,
        pointed: pointed.unwrap_or_default(),
        context: context.unwrap_or_default(),
        images,
    };
    let (kept, generation) = {
        let _authority = state
            .sheet_authority
            .try_lock()
            .map_err(|_| "workspace_busy".to_string())?;
        let mut run = ai_run(&state);
        if state.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err("The drawing is closed".into());
        }
        run.reserve()?
    };
    // Gives the claim back on every way out before the turn starts, a panic included.
    let mut claim = Claim {
        state: &state,
        generation,
        armed: true,
        kept,
    };
    // Start this job on the sheet the user was viewing; later browsing is independent.
    {
        let mut session = state
            .session
            .try_lock()
            .map_err(|_| "workspace_busy".to_string())?;
        if let Some(session) = session.as_mut() {
            session.automation_sheet = Some(session.sheet.clone());
        }
    }
    let language = ai_run(&state).language();
    let mut settings = state.turn_settings();
    if ai::mock_provider(&state.session).is_none() {
        crate::subscriptions::ensure_plan_model(&state, &mut settings).await?;
    }
    let previous = ai_run(&state).checkpoint_conversation();
    let mut agent = match claim.kept.take() {
        Some(agent) => agent,
        // Its file tools run on the app's tabs.
        None => new_agent(&state, &settings, language, previous)
            .await?
            .with_host(Arc::new(crate::drawing_host::AppHost {
                app: app.clone(),
                expected_revision: None,
                binding: Some(state.clone()),
            })),
    };
    // Effort may have changed since the last message.
    agent.set_settings(ai::agent_settings(&settings, language));
    let turn = ai_run(&state).start(agent, generation);
    // `start` settles the claim either way.
    claim.armed = false;
    let turn = turn?;
    let configured = crate::state::lock(&state.settings).max_concurrent_turns;
    let memory_limited = state.turn_limit() < configured;
    let event = {
        let mut view = crate::state::lock(&state.view);
        view.error = None;
        view.status = "queued";
        view.away = None;
        view.transcript.push(TranscriptEntry::User {
            text: message.text.clone(),
            pointed: message.pointed.clone(),
            images: vec![qiyas_ai::conversation::PICTURE_REMOVED.to_string(); message.images.len()],
        });
        view.record(state.id, serde_json::json!({"type":"queued","text":message.text,"pointed":message.pointed,"memory_limited":memory_limited}))
    };
    let _ = app.emit("workspace:event", event);
    tauri::async_runtime::spawn(supervise(app, state.clone(), turn, message));
    Ok(())
}

/// A turn claimed by `ai_send` but not started yet.
struct Claim<'a> {
    state: &'a AppState,
    generation: u64,
    armed: bool,
    kept: Option<Agent>,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if self.armed {
            ai_run(self.state).release(self.kept.take(), self.generation);
        }
    }
}

/// A new agent for the settings' provider, with its key from the keychain (or the mock).
async fn new_agent(
    state: &AppState,
    settings: &Settings,
    language: Lang,
    conversation: Option<Conversation>,
) -> Result<Agent, String> {
    #[cfg(test)]
    let test_provider = ai_run(state).test_provider.clone();
    #[cfg(test)]
    if let Some(make) = test_provider {
        let country = settings.country.clone();
        let settings = ai::agent_settings(settings, language);
        return Ok(match conversation {
            Some(conversation) => {
                Agent::resume(conversation, make(), state.session.clone(), settings)
            }
            None => Agent::new(make(), state.session.clone(), settings),
        }
        .with_country(country));
    }
    let (provider, name) = match ai::mock_provider(&state.session) {
        Some(mock) => (mock, "mock"),
        None if settings.provider == settings::Provider::ChatgptPlan => {
            (crate::subscriptions::provider()?, "chatgpt_plan")
        }
        None if settings.provider == settings::Provider::ClaudeCode => {
            let provider = qiyas_ai::claude_code::ClaudeCode::new(
                settings.claude_executable.as_deref(),
                qiyas_core::data_home::qiyas_dir()?,
                state.session.clone(),
            )
            .map_err(|e| e.for_window())?;
            (
                Box::new(provider) as Box<dyn qiyas_ai::Provider>,
                "claude_code",
            )
        }
        None => {
            let name = settings.provider.name();
            let saved = settings.clone();
            let key = match keychain(move || secrets::get_saved(&saved, name)).await {
                Ok(key) => key,
                // A local OpenAI-compatible server needs no key, so no keychain is fine.
                Err(_) if settings.provider == settings::Provider::OpenaiCompatible => None,
                Err(e) => return Err(e),
            };
            (ai::build_provider(settings, key)?, name)
        }
    };
    let log = AgentLog::new(ai::log_path(&state.config_dir));
    log.write(
        "NEW CONVERSATION",
        &ai::conversation_entry(name, settings, language),
    );
    let country = settings.country.clone();
    let settings = ai::agent_settings(settings, language);
    let agent = match conversation {
        Some(conversation) => {
            Agent::resume(conversation, provider, state.session.clone(), settings)
        }
        None => Agent::new(provider, state.session.clone(), settings),
    };
    Ok(agent.with_country(country).with_log(log))
}

/// Runs one turn, forwarding its events to the window while its conversation is current.
/// Runs the turn on its own task and always ends it with `ai:done`: if the task panics, the
/// conversation is dropped and the error is `CRASHED`.
async fn supervise<R: Runtime>(
    app: AppHandle<R>,
    state: AppState,
    turn: Turn,
    message: qiyas_ai::UserMessage,
) {
    let generation = turn.generation;
    let claude = crate::state::lock(&state.provider).provider == settings::Provider::ClaudeCode;
    let permit = state
        .turn_queue
        .acquire(claude, &turn.cancel, || state.turn_limit())
        .await;
    if permit.is_none() {
        ai_run(&state).finish(turn.agent, generation);
        if ai_run(&state).generation() == generation {
            let event = crate::state::lock(&state.view).agent_event(
                state.id,
                AgentEvent::Stopped {
                    reason: "cancelled".into(),
                },
            );
            let _ = app.emit("workspace:event", event);
            workspace_finished(&app, &state, None);
        }
        return;
    }
    let event = {
        let mut view = crate::state::lock(&state.view);
        view.status = "working";
        view.record(state.id, serde_json::json!({"type":"working"}))
    };
    let _ = app.emit("workspace:event", event);
    let task = tauri::async_runtime::spawn(run_turn(app.clone(), state.clone(), turn, message));
    let mut completion_generation = generation;
    let error = match task.await {
        Ok((agent, result)) => {
            ai_run(&state).finish(agent, generation);
            result.err().map(|e| e.for_window())
        }
        Err(_) => {
            let mut run = ai_run(&state);
            if run.generation() == generation {
                run.crashed();
                completion_generation = run.generation();
            } else {
                run.release(None, generation);
            }
            Some(ai::CRASHED.to_string())
        }
    };
    if ai_run(&state).generation() == completion_generation {
        workspace_finished(&app, &state, error);
    }
    drop(permit);
}

fn workspace_finished<R: Runtime>(app: &AppHandle<R>, state: &AppState, error: Option<String>) {
    if let Ok(session) = state.session.try_lock() {
        if let Some(session) = session.as_ref() {
            state.publish(session);
        }
    }
    let hidden = crate::state::lock(&state.tabs).active != Some(state.id);
    let event = {
        let mut view = crate::state::lock(&state.view);
        if error.is_some() {
            view.status = "failed";
        } else if view.status != "waiting" {
            view.status = "done";
        }
        view.error = error.clone();
        if hidden {
            view.away = Some(
                view.transcript
                    .iter()
                    .rev()
                    .find_map(|entry| match entry {
                        TranscriptEntry::Event {
                            event: AgentEvent::Text { text },
                        } => Some(text.chars().take(240).collect()),
                        _ => None,
                    })
                    .unwrap_or_default(),
            );
        }
        let status = view.status;
        view.record(
            state.id,
            serde_json::json!({"type":"end","status":status,"error":error}),
        )
    };
    let _ = app.emit("workspace:event", event);
    let _ = app.emit(
        "ai:done",
        AiDone {
            error,
            workspace: state.id,
        },
    );
}

#[tauri::command]
pub async fn workspace_attach(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let state = state.scoped(workspace)?;
    let mut snapshot = crate::state::lock(&state.view).snapshot(state.id);
    let settings = crate::state::lock(&state.provider).clone();
    snapshot["context"] = serde_json::json!(*crate::state::lock(&state.context));
    snapshot["choice"] = serde_json::json!({"provider":settings.provider,"model":settings.profile().model,"effort":settings.profile().effort,"base_url":settings.profile().base_url});
    let configured = crate::state::lock(&state.settings).max_concurrent_turns;
    snapshot["memory_limited"] = serde_json::json!(state.turn_limit() < configured);
    Ok(snapshot)
}

#[tauri::command]
pub async fn workspace_view_state(
    workspace: Option<u32>,
    context: qiyas_ai::TurnContext,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    if context.selection.len() > 100_000
        || context
            .region
            .as_ref()
            .is_some_and(|r| r.points.len() > 10_000)
    {
        return Err("The selection is too large".into());
    }
    *crate::state::lock(&state.context) = context;
    Ok(())
}

#[tauri::command]
pub async fn workspace_events(
    workspace: Option<u32>,
    after: u64,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let state = state.scoped(workspace)?;
    let events = crate::state::lock(&state.view).since(after);
    Ok(events)
}

#[tauri::command]
pub async fn workspace_provider<R: Runtime>(
    workspace: Option<u32>,
    provider: settings::Provider,
    model: String,
    effort: Option<String>,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    let mut run = ai_run(&state);
    if run.busy() {
        return Err("busy".into());
    }
    let old = crate::state::lock(&state.provider).clone();
    let mut next = if old.provider == provider {
        old.clone()
    } else {
        crate::state::lock(&state.settings).clone()
    };
    next.provider = provider;
    next.profile_mut().model = model;
    next.profile_mut().effort = effort;
    settings::validate(&next)?;
    // Preserve the conversation through Agent::resume, which removes incompatible provider memory.
    if ai::answerer_changed(&old, &next) {
        run.rebuild_provider();
    }
    *crate::state::lock(&state.provider) = next;
    drop(run);
    let _ = app.emit("workspace:changed", state.id);
    Ok(())
}

/// Runs one turn, forwarding its events to the window while its conversation is current, and
/// gives the agent back.
async fn run_turn<R: Runtime>(
    app: AppHandle<R>,
    state: AppState,
    turn: Turn,
    message: qiyas_ai::UserMessage,
) -> (Agent, Result<(), qiyas_ai::AiError>) {
    let Turn {
        mut agent,
        cancel,
        generation,
    } = turn;
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(64);
    let forward = async {
        let mut first = true;
        let mut opening_saved = false;
        let mut opening = tokio::time::interval(std::time::Duration::from_millis(25));
        loop {
            let event = tokio::select! {
                event = rx.recv() => {
                    let Some(event) = event else { break };
                    event
                }
                _ = opening.tick(), if !opening_saved => {
                    let run = ai_run(&state);
                    if run.generation() != generation {
                        opening_saved = true;
                    } else if run.conversation_id().is_some_and(|id| !id.is_empty()) {
                        // The first request may wait on the network without emitting anything.
                        // Keep its user message recoverable if the process exits at that point.
                        run.save_logged();
                        opening_saved = true;
                    }
                    continue;
                }
            };
            let run = ai_run(&state);
            if run.generation() == generation {
                // Persist the opening message and each completed step, including during a long
                // turn. Text deltas reach the checkpoint at the end of their response.
                if first
                    || matches!(
                        event,
                        AgentEvent::ToolResult { .. }
                            | AgentEvent::Done { .. }
                            | AgentEvent::Stopped { .. }
                            | AgentEvent::AwaitingApproval { .. }
                            | AgentEvent::Question { .. }
                            | AgentEvent::Refused { .. }
                    )
                {
                    run.save_logged();
                    opening_saved = true;
                }
                first = false;
                let window_event =
                    crate::state::lock(&state.view).agent_event(state.id, event.clone());
                let _ = app.emit("workspace:event", window_event);
                let mut payload = serde_json::to_value(&event).expect("agent event serializes");
                payload["workspace"] = serde_json::json!(state.id);
                let _ = app.emit("ai:event", payload);
            }
        }
    };
    // The agent drops the sender when the turn ends, which ends `forward`.
    let (result, ()) = tokio::join!(agent.run_turn_with(message, tx, cancel), forward);
    (agent, result)
}

/// Undoes the assistant's last change, only if it is still the newest change to the take-off.
/// False when the user (or nothing) changed it since.
#[tauri::command]
pub async fn ai_undo(workspace: Option<u32>, state: State<'_, AppState>) -> Result<bool, String> {
    let state = state.scoped(workspace)?;
    let _turn = window_sheet_guard(&state)?;
    with_session(&state, |s| Ok(s.undo_if(qiyas_core::Source::Ai)))
}

/// Stops the running turn; it ends with a "cancelled" stop.
#[tauri::command]
pub async fn ai_cancel(workspace: Option<u32>, state: State<'_, AppState>) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    ai_run(&state).cancel();
    Ok(())
}

/// Starts a new conversation.
#[tauri::command]
pub async fn ai_clear<R: Runtime>(
    workspace: Option<u32>,

    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    new_conversation(&app, &state);
    Ok(())
}

#[derive(Serialize)]
pub struct ConversationSummary {
    #[serde(flatten)]
    summary: crate::conversations::Summary,
    drawing_exists: bool,
}

/// A small index only: conversation bodies are loaded when the user chooses Continue.
#[tauri::command]
pub async fn ai_conversations(
    drawing: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ConversationSummary>, String> {
    for workspace in state.conversations() {
        crate::state::lock(&workspace.ai).save()?;
    }
    crate::conversations::Store::new(&state.config_dir)
        .list(drawing.as_deref().map(Path::new))
        .map(|entries| {
            entries
                .into_iter()
                .map(|summary| ConversationSummary {
                    drawing_exists: summary.drawing.as_ref().is_some_and(|path| path.is_file()),
                    summary,
                })
                .collect()
        })
}

#[derive(Serialize)]
pub struct ResumedConversation {
    transcript: Vec<TranscriptEntry>,
    drawing: Option<std::path::PathBuf>,
}

/// The window first activates or opens the drawing through the usual autosave-aware path.
#[tauri::command]
pub async fn ai_resume<R: Runtime>(
    workspace: Option<u32>,

    id: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<ResumedConversation, String> {
    let state = state.scoped(workspace)?;
    let (kept, generation, language, conversation) = {
        let mut run = ai_run(&state);
        if run.busy() {
            return Err("busy".into());
        }
        run.save()?;
        let conversation = crate::conversations::Store::new(&state.config_dir).load(&id)?;
        let (kept, generation) = run.reserve()?;
        (kept, generation, run.language(), conversation)
    };
    let mut claim = Claim {
        state: &state,
        generation,
        armed: true,
        kept,
    };
    let response = ResumedConversation {
        transcript: conversation.transcript.clone(),
        drawing: conversation.drawing.clone(),
    };
    let mut settings = state.turn_settings();
    if ai::mock_provider(&state.session).is_none() {
        crate::subscriptions::ensure_plan_model(&state, &mut settings).await?;
    }
    let agent = new_agent(&state, &settings, language, Some(conversation))
        .await?
        .with_host(Arc::new(crate::drawing_host::AppHost {
            app: app.clone(),
            expected_revision: None,
            binding: Some(state.clone()),
        }));
    let mut run = ai_run(&state);
    if run.generation() != generation {
        return Err(ai::RESTARTED.into());
    }
    run.new_conversation();
    run.resume(agent);
    {
        let mut view = crate::state::lock(&state.view);
        view.transcript = response.transcript.clone();
        view.status = "done";
        view.error = None;
        view.away = None;
    }
    claim.armed = false;
    Ok(response)
}

#[tauri::command]
pub async fn ai_delete_conversation<R: Runtime>(
    id: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let workspaces = state.conversations();
    let mut runs = workspaces
        .iter()
        .map(|workspace| crate::state::lock(&workspace.ai))
        .collect::<Vec<_>>();
    if runs
        .iter()
        .any(|run| run.conversation_id().as_deref() == Some(&id) && run.busy())
    {
        return Err("busy".into());
    }
    let mut cleared = Vec::new();
    for (workspace, run) in workspaces.iter().zip(runs.iter_mut()) {
        if run.conversation_id().as_deref() == Some(&id) {
            run.new_conversation();
            cleared.push(workspace.id);
        }
    }
    drop(runs);
    for id in cleared {
        new_conversation(&app, &state.scoped(Some(id))?);
    }
    crate::conversations::Store::new(&state.config_dir).delete(&id)
}

#[tauri::command]
pub async fn ai_delete_conversations<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let workspaces = state.conversations();
    let mut runs = workspaces
        .iter()
        .map(|workspace| crate::state::lock(&workspace.ai))
        .collect::<Vec<_>>();
    if runs.iter().any(|run| run.busy()) {
        return Err("busy".into());
    }
    for run in &mut runs {
        run.new_conversation();
    }
    crate::conversations::Store::new(&state.config_dir).delete_all()?;
    drop(runs);
    for workspace in workspaces {
        new_conversation(&app, &state.scoped(Some(workspace.id))?);
    }
    Ok(())
}

/// The window's language ("en" or "ar") for the assistant's prompt; a change starts a new
/// conversation.
#[tauri::command]
pub async fn ai_set_language<R: Runtime>(
    workspace: Option<u32>,

    lang: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    let changed = {
        let mut run = ai_run(&state);
        // Reloading or switching a translated window must not interrupt a running Mizan.
        if run.busy() {
            return Ok(());
        }
        run.set_language(Lang::from_code(&lang))
    };
    if changed {
        new_conversation(&app, &state);
    }
    Ok(())
}

/// The end of the agent log, and where the whole of it is.
#[derive(Serialize)]
pub struct AiLog {
    path: String,
    text: String,
}

/// The newest part of the agent log (empty before the first conversation).
#[tauri::command]
pub async fn ai_log(state: State<'_, AppState>) -> Result<AiLog, String> {
    let path = ai::log_path(&state.config_dir);
    Ok(AiLog {
        text: ai::log_tail(&path).map_err(|e| e.to_string())?,
        path: path.display().to_string(),
    })
}

#[derive(Serialize)]
pub struct AiStatus {
    /// "anthropic", "openai_compatible", or "mock".
    provider: String,
    model: String,
    /// A key for the provider is in the keychain.
    has_key: bool,
    busy: bool,
    message_count: usize,
}

#[tauri::command]
pub async fn ai_status(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<AiStatus, String> {
    let state = state.scoped(workspace)?;
    let settings = state
        .provider
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mock = ai::mock_provider(&state.session).is_some();
    let has_key = if mock {
        true
    } else {
        let name = settings.provider.name();
        let saved = settings.clone();
        keychain(move || secrets::get_saved(&saved, name))
            .await
            .is_ok_and(|key| key.is_some())
    };
    let run = ai_run(&state);
    Ok(AiStatus {
        provider: if mock {
            "mock"
        } else {
            settings.provider.name()
        }
        .to_string(),
        model: if mock {
            "mock".into()
        } else {
            settings.profile().model.clone()
        },
        has_key,
        busy: run.busy(),
        message_count: run.message_count(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::webview::InvokeRequest;
    use tauri::{App, Listener, Manager, WebviewWindow, WebviewWindowBuilder};

    use super::*;

    /// Give the timeout its own clock, so blocked IPC or Tauri workers cannot stop it.
    fn ai_test(name: &'static str, test: impl FnOnce() + Send + 'static) {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(60),
                    tauri::async_runtime::spawn_blocking(test),
                )
                .await
                .unwrap_or_else(|_| panic!("{name} timed out after 60 seconds"))
                .unwrap_or_else(|error| panic!("{name} failed: {error}"));
            });
    }

    struct Ipc {
        _dir: tempfile::TempDir,
        _app: App<MockRuntime>,
        webview: WebviewWindow<MockRuntime>,
        /// (event name, JSON payload) as the window would get them.
        events: Receiver<(String, Value)>,
    }

    #[test]
    fn window_browsing_preserves_the_assistants_sheet() {
        ai_test("window_browsing", || {
            let ipc = Ipc::open();
            let state = ipc._app.state::<AppState>();
            let layout = with_session(&state, |s| {
                Ok(s.info()
                    .sheets
                    .into_iter()
                    .find(|s| !s.is_model)
                    .unwrap()
                    .block)
            })
            .unwrap();
            let (agent, generation) = ai_run(&state).reserve().unwrap();
            with_session(&state, |s| {
                s.automation_sheet = Some(layout.clone());
                Ok(())
            })
            .unwrap();
            for (name, input) in [
                ("set_sheet", json!({"block":"*Model_Space"})),
                (
                    "run_command",
                    json!({"cmd":{"type":"set_sheet","block":"*Model_Space"}}),
                ),
                (
                    "run_command",
                    json!({"cmd":{"type":"batch","commands":[{"type":"set_sheet","block":"*Model_Space"}]}}),
                ),
            ] {
                ipc.call(name, input).unwrap();
            }
            for name in ["undo", "redo", "ai_undo"] {
                assert!(ipc.call(name, json!({})).is_err());
            }
            with_session(&state, |s| {
                let before = s.history_steps().0.len();
                let result = qiyas_ai::surface::run_session(
                    s,
                    "qiyas_execute",
                    "run",
                    &json!({"command":{"type":"draw","kind":"line","a":[0,0],"b":[10,0]}}),
                    Source::Ai,
                )
                .unwrap();
                let qiyas_ai::ToolOutput::Json(value) = result else {
                    panic!("json")
                };
                assert_eq!(value["ui"]["sheet"], layout);
                assert_eq!(s.sheet, "*Model_Space");
                assert_eq!(s.automation_sheet.as_deref(), Some(layout.as_str()));
                let handle = value["added"][0].as_u64().unwrap();
                assert!(s
                    .drawing
                    .entities_in(&layout)
                    .any(|e| e.common().handle.value() == handle));
                assert_eq!(s.history_steps().0.len(), before + 1);
                assert!(qiyas_ai::surface::run_session(
                    s,
                    "qiyas_execute",
                    "run",
                    &json!({"command":{"type":"set_sheet","block":"missing"}}),
                    Source::Ai
                )
                .is_err());
                assert_eq!(s.sheet, "*Model_Space");
                Ok(())
            })
            .unwrap();
            ipc.bytes("render_batch", json!({}));
            ipc.bytes("sheet_batch", json!({"sheet":layout}));
            ai_run(&state).release(agent, generation);
        });
    }

    impl Ipc {
        /// The app on the mock runtime with the canned AI provider and the sample drawing open.
        fn open() -> Ipc {
            std::env::set_var("QIYAS_AI_MOCK", "1");
            let dir = tempfile::tempdir().unwrap();
            let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
            let app = mock_builder()
                .invoke_handler(tauri::generate_handler![
                    open_drawing,
                    new_drawing,
                    drawing_info,
                    recents,
                    clear_recents,
                    views,
                    save_drawing,
                    save_drawing_as,
                    crate::tabs::tabs,
                    crate::tabs::activate_tab,
                    crate::tabs::close_tab,
                    takeoff,
                    run_command,
                    undo,
                    redo,
                    boq,
                    compare,
                    conflicts,
                    export_conflicts,
                    ai_undo,
                    ai_send,
                    workspace_attach,
                    workspace_provider,
                    workspace_events,
                    ai_cancel,
                    ai_clear,
                    ai_conversations,
                    ai_resume,
                    ai_delete_conversation,
                    ai_delete_conversations,
                    ai_status,
                    ai_log,
                    get_settings,
                    set_settings,
                    mcp_status,
                    pick,
                    snap,
                    get_drafting,
                    set_drafting,
                    render_batch,
                    set_sheet,
                    crate::pdf::pdf_page_count,
                    crate::pdf::pdf_thumbnails,
                    crate::pdf::attach_pdf,
                    crate::pdf::is_dirty,
                    crate::pdf::calibrate,
                    crate::pdf::underlay_image,
                    crate::pdf::image_size,
                    crate::pdf::attach_image,
                    crate::pdf::image_display,
                    crate::blocks::blocks,
                    crate::blocks::block_thumbnail,
                    crate::blocks::drawing_blocks,
                    crate::blocks::drawing_layers,
                    crate::blocks::write_block,
                    crate::blocks::set_tool_palette,
                    sheet_batch,
                    sheet_batch_version,
                    crate::files::paper_views,
                    crate::files::autosave_drawings,
                    crate::files::recoverable_drawings,
                    crate::files::recover_autosave,
                    crate::files::discard_autosave,
                    crate::files::recover_drawing,
                    crate::files::sheet_set_open,
                    crate::files::sheet_set_save,
                    crate::files::sheet_set_entry,
                    crate::files::sheet_set_from_tabs,
                    crate::projects::project_command,
                    crate::projects::project_history
                ])
                .build(mock_context(noop_assets()))
                .unwrap();
            app.manage(AppState::new(dir.path().join("config")));
            let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .unwrap();
            let (tx, events) = channel();
            for name in ["ai:event", "ai:done", "ai:cleared", crate::pdf::PROGRESS] {
                let tx = tx.clone();
                app.listen_any(name, move |e| {
                    let mut payload: Value =
                        serde_json::from_str(e.payload()).unwrap_or(Value::Null);
                    if let Some(object) = payload.as_object_mut() {
                        object.remove("workspace");
                    }
                    if name == crate::pdf::PROGRESS {
                        payload = payload.get("progress").cloned().unwrap_or(payload);
                    }
                    let _ = tx.send((name.to_string(), payload));
                });
            }
            let ipc = Ipc {
                _dir: dir,
                _app: app,
                webview,
                events,
            };
            ipc.call("open_drawing", json!({ "path": path })).unwrap();
            assert!(
                ipc.events.try_recv().is_err(),
                "opening must not clear another conversation"
            );
            ipc
        }

        fn call(&self, cmd: &str, body: Value) -> Result<Value, Value> {
            let request = InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                // The app's own origin, so the commands are allowed.
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            get_ipc_response(&self.webview, request).map(|b| b.deserialize().unwrap())
        }

        /// A command that answers raw bytes.
        fn bytes(&self, cmd: &str, body: Value) -> Vec<u8> {
            let request = InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            match get_ipc_response(&self.webview, request).unwrap() {
                InvokeResponseBody::Raw(bytes) => bytes,
                InvokeResponseBody::Json(json) => panic!("not bytes: {json}"),
            }
        }

        fn next(&self) -> (String, Value) {
            self.events.recv_timeout(Duration::from_secs(10)).unwrap()
        }

        /// The `ai:event` payloads up to `ai:done`, and its payload.
        fn turn(&self) -> (Vec<Value>, Value) {
            let mut events = Vec::new();
            loop {
                match self.next() {
                    (name, done) if name == "ai:done" => return (events, done),
                    (_, event) => events.push(event),
                }
            }
        }
    }

    #[test]
    fn autosave_covers_every_tab_and_recovery_opens_the_autosave_in_a_tab() {
        let ipc = Ipc::open();
        let first = ipc.call("drawing_info", json!({})).unwrap()["path"]
            .as_str()
            .unwrap()
            .to_string();
        let line = json!({ "cmd": { "type": "draw", "kind": "line", "a": [0, 0], "b": [5, 5] } });
        ipc.call("run_command", line.clone()).unwrap();
        // A second drawing in its own tab, also changed: the first one is parked.
        let folder = std::path::Path::new(&first).parent().unwrap();
        let second = qiyas_core::fixtures::write_sample(folder, "dwg");
        ipc.call("open_drawing", json!({ "path": second })).unwrap();
        ipc.call("run_command", line).unwrap();

        let run = ipc.call("autosave_drawings", json!({})).unwrap();
        assert_eq!(run["written"].as_array().unwrap().len(), 2, "{run}");
        assert!(run["failed"].as_array().unwrap().is_empty());
        let found = ipc
            .call(
                "recoverable_drawings",
                json!({ "paths": [first, second.display().to_string()] }),
            )
            .unwrap();
        // Both autosaves are newer than their drawings.
        assert_eq!(found.as_array().unwrap().len(), 2, "{found}");

        let recovered = ipc
            .call("recover_autosave", json!({ "path": first }))
            .unwrap();
        assert!(recovered["path"]
            .as_str()
            .unwrap()
            .ends_with("sample (recovered).dxf"));
        let tabs = ipc.call("tabs", json!({})).unwrap();
        assert_eq!(tabs["tabs"].as_array().unwrap().len(), 3);
        assert_eq!(tabs["tabs"][2]["dirty"], json!(true));

        ipc.call("discard_autosave", json!({ "path": first }))
            .unwrap();
        let found = ipc
            .call("recoverable_drawings", json!({ "paths": [first] }))
            .unwrap();
        assert_eq!(found, json!([]));

        // A sheet set of the open drawings, written beside them.
        let qss = folder.join("Set.qss").display().to_string();
        let made = ipc
            .call("sheet_set_from_tabs", json!({ "qss": qss, "name": "Set" }))
            .unwrap();
        let set = made["set"].clone();
        assert_eq!(set["drawings"].as_array().unwrap().len(), 2, "{made}");
        let view = ipc
            .call("sheet_set_save", json!({ "path": qss, "set": set }))
            .unwrap();
        assert_eq!(view["entries"][0]["missing"], json!(false));
        assert_eq!(view["set"]["drawings"][0]["path"], json!("sample.dxf"));
        assert_eq!(
            ipc.call("sheet_set_open", json!({ "path": qss })).unwrap()["set"],
            view["set"]
        );
    }

    #[test]
    fn project_ipc_runs_exports_and_preserves_undo_and_unsaved_members() {
        const NAME: &str =
            "commands::tests::project_ipc_runs_exports_and_preserves_undo_and_unsaved_members";
        if std::env::var("QIYAS_PROJECT_IPC_CHILD").as_deref() != Ok(NAME) {
            let home = tempfile::tempdir().unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture"])
                .env("QIYAS_PROJECT_IPC_CHILD", NAME)
                .env("QIYAS_HOME", home.path())
                .status()
                .unwrap();
            assert!(result.success());
            return;
        }
        let ipc = Ipc::open();
        let drawing = ipc.call("drawing_info", json!({})).unwrap()["path"].clone();
        let command = |value: Value| ipc.call("project_command", json!({"command":value}));
        let created =
            command(json!({"type":"project","kind":"create","name":"IPC project"})).unwrap();
        let path = created["result"]["path"].clone();
        assert!(path.is_string(), "{created}");
        assert_eq!(created["result"]["active_project"], path);
        command(json!({"type":"project","kind":"add_drawing","path":path,"drawing":drawing}))
            .unwrap();
        let listed = command(json!({"type":"project","kind":"list"})).unwrap();
        assert_eq!(listed["result"]["projects"].as_array().unwrap().len(), 1);
        assert_eq!(listed["result"]["active_project"], path);
        ipc.call(
            "run_command",
            json!({"cmd":{"type":"draw","kind":"line","a":[0,0],"b":[5,5]}}),
        )
        .unwrap();
        let rejected = command(json!({"type":"project","kind":"run","path":path})).unwrap_err();
        assert!(rejected.as_str().unwrap().contains("Save"), "{rejected}");
        ipc.call("save_drawing", json!({})).unwrap();
        let run = command(json!({"type":"project","kind":"run","path":path})).unwrap();
        assert!(matches!(
            run["result"]["set"]["project"]["drawings"][0]["status"].as_str(),
            Some("ok" | "warning")
        ));
        assert!(std::path::Path::new(run["outputs"]["workbook"].as_str().unwrap()).is_file());
        assert!(std::path::Path::new(run["outputs"]["morning_report"].as_str().unwrap()).is_file());
        assert_eq!(
            ipc.call("project_history", json!({"redo":false})).unwrap(),
            true
        );
        let undone = command(json!({"type":"project","kind":"open","path":path})).unwrap();
        assert!(undone["result"]["set"]["project"]["drawings"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            undone["result"]["set"]["drawings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            ipc.call("project_history", json!({"redo":true})).unwrap(),
            true
        );
        let redone = command(json!({"type":"project","kind":"open","path":path})).unwrap();
        assert_eq!(
            redone["result"]["set"]["project"],
            run["result"]["set"]["project"]
        );
    }

    #[test]
    fn closing_without_saving_deletes_the_autosave() {
        let ipc = Ipc::open();
        let path = ipc.call("drawing_info", json!({})).unwrap()["path"]
            .as_str()
            .unwrap()
            .to_string();
        let line = json!({ "cmd": { "type": "draw", "kind": "line", "a": [0, 0], "b": [5, 5] } });
        ipc.call("run_command", line).unwrap();
        let run = ipc.call("autosave_drawings", json!({})).unwrap();
        let written = run["written"][0].as_str().unwrap().to_string();
        assert!(std::path::Path::new(&written).exists());
        let id = ipc.call("tabs", json!({})).unwrap()["active"].clone();
        // Refused without force: the window asks first.
        assert!(ipc.call("close_tab", json!({ "id": id })).is_err());
        assert!(std::path::Path::new(&written).exists());
        // Don't Save.
        ipc.call("close_tab", json!({ "id": id, "force": true }))
            .unwrap();
        assert!(!std::path::Path::new(&written).exists());
        let found = ipc
            .call("recoverable_drawings", json!({ "paths": [path] }))
            .unwrap();
        assert_eq!(found, json!([]));
    }

    #[test]
    fn model_space_draws_for_a_layout_without_leaving_it() {
        let ipc = Ipc::open();
        let info = ipc.call("drawing_info", json!({})).unwrap();
        let layout = info["sheets"][1]["block"].as_str().unwrap().to_string();
        let batch = ipc.bytes("sheet_batch", json!({ "sheet": "*Model_Space" }));
        assert_eq!(&batch[..4], b"BYIQ");
        let version = ipc
            .call("sheet_batch_version", json!({ "sheet": "*Model_Space" }))
            .unwrap();
        let same = ipc.bytes(
            "sheet_batch",
            json!({ "sheet": "*Model_Space", "ifVersion": version }),
        );
        assert_eq!(same, NOT_MODIFIED.to_vec());
        assert_eq!(
            ipc.call("paper_views", json!({ "sheet": layout })).unwrap(),
            json!([])
        );
    }

    #[test]
    fn blocks_their_thumbnails_other_drawings_and_the_tool_palette() {
        let ipc = Ipc::open();
        let blocks = ipc.call("blocks", json!({})).unwrap();
        assert_eq!(blocks, json!([{ "name": "DOOR", "attdefs": [] }]));
        let png = ipc.bytes("block_thumbnail", json!({ "name": "DOOR", "px": 64 }));
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(ipc
            .call("block_thumbnail", json!({ "name": "NOPE", "px": 64 }))
            .is_err());

        // The Design Center lists another drawing's blocks.
        let sample = ipc._dir.path().join("sample.dxf");
        let names = ipc
            .call("drawing_blocks", json!({ "path": sample }))
            .unwrap();
        assert_eq!(names, json!([{ "name": "DOOR", "attdefs": [] }]));
        // The layer translator reads another drawing's layers.
        let layers = ipc
            .call("drawing_layers", json!({ "path": sample }))
            .unwrap();
        let wall = layers
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "A-WALL")
            .unwrap();
        assert_eq!(wall["color_name"], "ACI 1");

        // A block written where the Save dialog chose, even outside the drawing's folder.
        let elsewhere = tempfile::tempdir().unwrap();
        let out = elsewhere.path().join("door.dwg");
        ipc.call("write_block", json!({ "name": "DOOR", "path": out }))
            .unwrap();
        assert!(out.exists());

        // The tool palette is kept in the settings; the Settings dialog's copy never drops it.
        let items = json!([
            { "kind": "block", "label": "Door", "value": "DOOR" },
            { "kind": "command", "label": "Line", "value": "LINE" }
        ]);
        ipc.call("set_tool_palette", json!({ "items": items }))
            .unwrap();
        let mut settings = ipc.call("get_settings", json!({})).unwrap();
        assert_eq!(settings["tool_palette"], items);
        settings["tool_palette"] = json!([]);
        ipc.call("set_settings", json!({ "settings": settings }))
            .unwrap();
        assert_eq!(
            ipc.call("get_settings", json!({})).unwrap()["tool_palette"],
            items
        );
        let bad = json!([{ "kind": "block", "label": " ", "value": "DOOR" }]);
        assert!(ipc
            .call("set_tool_palette", json!({ "items": bad }))
            .is_err());
    }

    #[test]
    fn a_turn_streams_its_events_and_measures_on_the_session() {
        ai_test(
            "a_turn_streams_its_events_and_measures_on_the_session",
            || {
                let ipc = Ipc::open();
                ipc.call("ai_send", json!({ "text": "Measure the wall" }))
                    .unwrap();
                assert_eq!(
                    ipc.call("ai_send", json!({ "text": "And again" })),
                    Err(json!("busy")),
                    "one turn at a time"
                );
                let (events, done) = ipc.turn();
                assert_eq!(done, json!({ "error": null }));

                let kinds: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
                let first_call = kinds.iter().position(|k| *k == "tool_call").unwrap();
                assert!(kinds[..first_call]
                    .iter()
                    .all(|k| matches!(*k, "text" | "cache_usage")));
                assert!(kinds[..first_call].contains(&"cache_usage"));
                assert_eq!(kinds[first_call + 1], "tool_result");
                assert_eq!(kinds.last(), Some(&"done"));
                let call = &events[first_call];
                assert_eq!(call["name"], "qiyas_execute");
                assert_eq!(call["label"], "measure_entity");
                assert_eq!(call["mutating"], true);
                let result = &events[first_call + 1];
                assert_eq!(result["is_error"], false, "{result}");
                assert_eq!(result["handles"].as_array().unwrap().len(), 1);

                let items = ipc.call("takeoff", json!({})).unwrap();
                assert_eq!(items[0]["source"], "ai");

                let status = ipc.call("ai_status", json!({})).unwrap();
                assert_eq!(status["provider"], "mock");
                assert_eq!(status["busy"], false);
                assert_eq!(status["message_count"], 4);

                ipc.call("ai_clear", json!({})).unwrap();
                assert_eq!(ipc.next().0, "ai:cleared");
                let status = ipc.call("ai_status", json!({})).unwrap();
                assert_eq!(status["message_count"], 0);
            },
        );
    }

    #[test]
    fn the_agent_log_tells_the_turn() {
        ai_test("the_agent_log_tells_the_turn", || {
            let ipc = Ipc::open();
            assert_eq!(ipc.call("ai_log", json!({})).unwrap()["text"], "");
            ipc.call("ai_send", json!({ "text": "Measure the wall" }))
                .unwrap();
            ipc.turn();
            let log = ipc.call("ai_log", json!({})).unwrap();
            assert!(log["path"].as_str().unwrap().ends_with("agent.log"));
            let text = log["text"].as_str().unwrap();
            for part in [
                "NEW CONVERSATION\n    provider: mock",
                "USER\n    Measure the wall",
                "REQUEST 1",
                "TOOL qiyas_execute (",
                "DONE",
            ] {
                assert!(text.contains(part), "the log lacks {part:?}:\n{text}");
            }
        });
    }

    #[test]
    fn conversations_survive_clear_resume_and_deletion() {
        ai_test("conversations_survive_clear_resume_and_deletion", || {
            let ipc = Ipc::open();
            let drawing = ipc.call("drawing_info", json!({})).unwrap()["path"].clone();
            ipc.call(
                "ai_send",
                json!({ "text": "Measure the wall", "pointed": ["Selection · 2"] }),
            )
            .unwrap();
            let (live_events, done) = ipc.turn();
            assert!(done["error"].is_null(), "{done}");
            let list = ipc
                .call("ai_conversations", json!({ "drawing": drawing }))
                .unwrap();
            assert_eq!(list.as_array().unwrap().len(), 1);
            assert_eq!(list[0]["title"], "Measure the wall");
            assert_eq!(list[0]["turns"], 1);
            let id = list[0]["id"].clone();
            // The reset must save before dropping the agent, even if the prior copy is gone.
            let saved_file = ipc
                ._dir
                .path()
                .join("config/conversations")
                .join(format!("{}.json", id.as_str().unwrap()));
            std::fs::remove_file(&saved_file).unwrap();
            ipc.call("ai_clear", json!({})).unwrap();
            assert_eq!(ipc.next().0, "ai:cleared");
            assert!(saved_file.is_file());
            let saved = ipc.call("ai_conversations", json!({})).unwrap();
            assert_eq!(saved[0]["id"], id);
            let resumed = ipc.call("ai_resume", json!({ "id": id })).unwrap();
            assert_eq!(resumed["drawing"], drawing);
            let transcript = resumed["transcript"].as_array().unwrap();
            assert_eq!(transcript[0]["text"], "Measure the wall");
            assert_eq!(transcript[0]["pointed"], json!(["Selection · 2"]));
            assert!(transcript
                .iter()
                .any(|entry| entry["event"] == *live_events.last().unwrap()));
            assert!(
                ipc.call("ai_status", json!({})).unwrap()["message_count"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            ipc.call("ai_delete_conversation", json!({ "id": id }))
                .unwrap();
            assert_eq!(ipc.next().0, "ai:cleared");
            assert_eq!(ipc.call("ai_conversations", json!({})).unwrap(), json!([]));
            ipc.call("ai_clear", json!({})).unwrap();
            assert_eq!(ipc.next().0, "ai:cleared");
            assert_eq!(
                ipc.call("ai_conversations", json!({})).unwrap(),
                json!([]),
                "a deleted active chat must not return on reset"
            );
        });
    }

    #[test]
    fn resume_is_refused_while_a_turn_is_reserved() {
        ai_test("resume_is_refused_while_a_turn_is_reserved", || {
            let ipc = Ipc::open();
            let state = ipc.webview.state::<AppState>();
            let (agent, generation) = ai_run(&state).reserve().unwrap();
            assert_eq!(
                ipc.call("ai_resume", json!({ "id": "missing" })),
                Err(json!("busy"))
            );
            ai_run(&state).release(agent, generation);
        });
    }

    #[test]
    fn an_untitled_conversation_follows_save_as_before_another_turn() {
        ai_test(
            "an_untitled_conversation_follows_save_as_before_another_turn",
            || {
                let ipc = Ipc::open();
                let untitled = ipc.call("new_drawing", json!({ "units": "m" })).unwrap();
                assert!(ipc.events.try_recv().is_err());
                ipc.call("ai_send", json!({ "text": "Hello" })).unwrap();
                ipc.turn();
                let list = ipc.call("ai_conversations", json!({})).unwrap();
                let id = list[0]["id"].clone();
                assert_eq!(list[0]["drawing"], untitled["path"]);
                assert_eq!(
                    ipc.call("ai_conversations", json!({ "drawing": untitled["path"] }))
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                let path = ipc._dir.path().join("named.dxf");
                ipc.call("save_drawing_as", json!({ "path": path }))
                    .unwrap();
                let list = ipc
                    .call("ai_conversations", json!({ "drawing": path }))
                    .unwrap();
                assert_eq!(list.as_array().unwrap().len(), 1);
                assert_eq!(list[0]["id"], id);
                ipc.call("ai_clear", json!({})).unwrap();
                assert_eq!(ipc.next().0, "ai:cleared");
                assert_eq!(
                    ipc.call("ai_resume", json!({ "id": id })).unwrap()["drawing"],
                    json!(path)
                );
                ipc.call("ai_delete_conversations", json!({})).unwrap();
                assert_eq!(ipc.next().0, "ai:cleared");
                assert_eq!(ipc.call("ai_conversations", json!({})).unwrap(), json!([]));
            },
        );
    }

    #[test]
    fn save_as_keeps_its_chats_out_of_the_next_untitled_drawing() {
        ai_test(
            "save_as_keeps_its_chats_out_of_the_next_untitled_drawing",
            || {
                let ipc = Ipc::open();
                let first = ipc.call("new_drawing", json!({"units":"m"})).unwrap();
                assert!(ipc.events.try_recv().is_err());
                ipc.call("ai_send", json!({"text":"Hello"})).unwrap();
                ipc.turn();
                let saved = ipc._dir.path().join("saved.dwg");
                ipc.call("save_drawing_as", json!({"path":saved})).unwrap();
                let second = ipc.call("new_drawing", json!({"units":"m"})).unwrap();
                assert_ne!(first["path"], second["path"]);
                assert_eq!(
                    ipc.call("ai_conversations", json!({"drawing":second["path"]}))
                        .unwrap(),
                    json!([])
                );
                assert_eq!(
                    ipc.call("ai_conversations", json!({"drawing":saved}))
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
            },
        );
    }

    #[test]
    fn recovered_untitled_history_and_inactive_chats_follow_save_as() {
        ai_test(
            "recovered_untitled_history_and_inactive_chats_follow_save_as",
            || {
                let ipc = Ipc::open();
                let untitled = ipc.call("new_drawing", json!({"units":"m"})).unwrap();
                assert!(ipc.events.try_recv().is_err());
                let state = ipc._app.state::<AppState>();
                // A copy of the founder's chat can exercise the same flow without reading his
                // live store. Normal tests use the canned provider and synthetic conversations.
                if let Some(path) = std::env::var_os("QIYAS_GOAL_L_CHAT_COPY") {
                    let mut chat: qiyas_ai::conversation::Conversation =
                        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                    chat.drawing = Some(untitled["path"].as_str().unwrap().into());
                    crate::conversations::Store::new(&state.config_dir)
                        .save(&chat)
                        .unwrap();
                }
                for _ in 0..2 {
                    ipc.call("ai_send", json!({"text":"Hello"})).unwrap();
                    ipc.turn();
                    ipc.call("ai_clear", json!({})).unwrap();
                    assert_eq!(ipc.next().0, "ai:cleared");
                }
                let before = ipc
                    .call("ai_conversations", json!({"drawing":untitled["path"]}))
                    .unwrap();
                assert!(before.as_array().unwrap().len() >= 2);
                ipc.call(
                    "run_command",
                    json!({"cmd":{"type":"draw","kind":"line","a":[0,0],"b":[5,0]}}),
                )
                .unwrap();
                let autosave = ipc.call("autosave_drawings", json!({})).unwrap();
                assert!(!autosave["written"].as_array().unwrap().is_empty());
                let source = Path::new(untitled["path"].as_str().unwrap());
                let auto = qiyas_core::autosave_path(source);
                let crash =
                    std::path::PathBuf::from(format!("{}.crash-20261024-120000", auto.display()));
                std::fs::copy(&auto, &crash).unwrap();
                let project = std::path::PathBuf::from(format!("{}.qiyas", auto.display()));
                if project.exists() {
                    std::fs::copy(&project, format!("{}.qiyas", crash.display())).unwrap();
                }
                let store = crate::conversations::Store::new(&state.config_dir);
                for path in [&auto, &crash] {
                    let mut chat = store.load(before[0]["id"].as_str().unwrap()).unwrap();
                    chat.id = uuid::Uuid::new_v4().to_string();
                    chat.drawing = Some(path.clone());
                    store.save(&chat).unwrap();
                }
                let recovered = ipc.call("recover_autosave", json!({"path":crash})).unwrap();
                let history = ipc
                    .call("ai_conversations", json!({"drawing":recovered["path"]}))
                    .unwrap();
                assert_eq!(
                    history.as_array().unwrap().len(),
                    before.as_array().unwrap().len() + 2
                );
                let saved = ipc._dir.path().join("villa.dwg");
                ipc.call("save_drawing_as", json!({"path":saved})).unwrap();
                let after = ipc
                    .call("ai_conversations", json!({"drawing":saved}))
                    .unwrap();
                assert_eq!(
                    after.as_array().unwrap().len(),
                    before.as_array().unwrap().len() + 2
                );
                let restarted = crate::conversations::Store::new(&state.config_dir);
                restarted.recover().unwrap();
                for entry in after.as_array().unwrap() {
                    let chat = restarted.load(entry["id"].as_str().unwrap()).unwrap();
                    assert_eq!(chat.drawing.as_deref(), Some(saved.as_path()));
                    assert!(!chat.transcript.is_empty());
                }
                if let Some(path) = std::env::var_os("QIYAS_GOAL_L_HISTORY_EVIDENCE") {
                    std::fs::write(path, serde_json::to_vec_pretty(&json!({"untitled":untitled,"before":before,"autosave":autosave,"recovered":recovered,"history":history,"save_as":after})).unwrap()).unwrap();
                }
            },
        );
    }

    /// Panics in its first turn (counted across the providers it makes), then answers "ok".
    struct PanicsFirst(Arc<AtomicUsize>);
    impl qiyas_ai::Provider for PanicsFirst {
        fn stream_turn<'a>(
            &'a self,
            _: qiyas_ai::TurnRequest,
            _: qiyas_ai::provider::Sink<'a>,
        ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, qiyas_ai::AiError>>
        {
            let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
            Box::pin(async move {
                assert!(!first, "the provider broke");
                Ok(qiyas_ai::TurnResult {
                    content: vec![qiyas_ai::ContentBlock::Text { text: "ok".into() }],
                    stop_reason: qiyas_ai::StopReason::EndTurn,
                    usage: Default::default(),
                })
            })
        }
    }

    #[test]
    fn a_panicking_turn_ends_with_an_error_and_frees_the_assistant() {
        ai_test(
            "a_panicking_turn_ends_with_an_error_and_frees_the_assistant",
            || {
                let ipc = Ipc::open();
                let calls = Arc::new(AtomicUsize::new(0));
                {
                    let state = ipc.webview.state::<AppState>();
                    let calls = calls.clone();
                    ai_run(&state).test_provider =
                        Some(Arc::new(move || Box::new(PanicsFirst(calls.clone()))));
                }
                ipc.call("ai_send", json!({ "text": "Measure" })).unwrap();
                let (_, done) = ipc.turn();
                assert_eq!(done, json!({ "error": "crashed" }));
                let status = ipc.call("ai_status", json!({})).unwrap();
                assert_eq!(status["busy"], false);
                assert_eq!(status["message_count"], 0, "the conversation was dropped");
                let saved = ipc.call("ai_conversations", json!({})).unwrap();
                assert_eq!(
                    saved.as_array().unwrap().len(),
                    1,
                    "the last checkpoint survives a panicking turn"
                );
                assert_eq!(saved[0]["title"], "Measure");

                ipc.call("ai_send", json!({ "text": "Again" })).unwrap();
                let (events, done) = ipc.turn();
                assert_eq!(done, json!({ "error": null }));
                assert_eq!(events.last().unwrap()["type"], "done");
                assert_eq!(calls.load(Ordering::SeqCst), 2);
            },
        );
    }

    #[test]
    fn ai_undo_takes_back_only_the_assistants_newest_change() {
        ai_test(
            "ai_undo_takes_back_only_the_assistants_newest_change",
            || {
                let ipc = Ipc::open();
                ipc.call("ai_send", json!({ "text": "Measure the wall" }))
                    .unwrap();
                ipc.turn();
                // The user measures after the assistant.
                let cmd = json!({ "type": "measure_distance", "points": [[0.0, 0.0], [1.0, 0.0]] });
                ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
                assert_eq!(ipc.call("ai_undo", json!({})), Ok(json!(false)));
                assert_eq!(
                    ipc.call("takeoff", json!({}))
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .len(),
                    2
                );
                assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
                assert_eq!(ipc.call("ai_undo", json!({})), Ok(json!(true)));
                assert_eq!(ipc.call("takeoff", json!({})), Ok(json!([])));
            },
        );
    }

    #[test]
    fn snap_follows_the_drafting_settings() {
        let ipc = Ipc::open();
        let at = json!({ "x": 4.03, "y": 2.97, "tol": 0.05 });
        let found = ipc.call("snap", at.clone()).unwrap();
        assert_eq!(found["kind"], "end");
        assert_eq!(found["point"], json!([4.0, 3.0]));

        let mut drafting = ipc.call("get_drafting", json!({})).unwrap();
        assert_eq!(drafting["osnap"], true);
        drafting["osnap"] = json!(false);
        ipc.call("set_drafting", json!({ "drafting": drafting }))
            .unwrap();
        assert_eq!(ipc.call("snap", at).unwrap(), Value::Null);
        assert_eq!(ipc.call("get_drafting", json!({})).unwrap()["osnap"], false);

        drafting["grid_spacing"] = json!(-1.0);
        assert!(ipc
            .call("set_drafting", json!({ "drafting": drafting }))
            .is_err());
    }

    #[test]
    fn boq_reads_the_sessions_bill_and_follows_undo() {
        let ipc = Ipc::open();
        assert_eq!(ipc.call("boq", json!({})), Ok(Value::Null));
        let item = json!({
            "code": "A.1", "description": "Walls", "unit": "m", "quantity": 12.0,
            "rate": null, "section": "Masonry", "tolerance": null, "row": 3
        });
        let bill = json!({
            "source": "boq.xlsx", "imported": "2026-10-08T00:00:00Z",
            "items": [item], "skipped_rows": 1
        });
        let cmd = json!({ "type": "set_boq", "boq": bill });
        ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
        assert_eq!(ipc.call("boq", json!({})), Ok(bill));
        assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
        assert_eq!(ipc.call("boq", json!({})), Ok(Value::Null));
    }

    #[test]
    fn compare_keeps_the_conflicts_and_exports_them() {
        let ipc = Ipc::open();
        let item = json!({
            "code": "A.1", "description": "Roof waterproofing", "unit": "m2", "quantity": 12.0,
            "rate": null, "section": "", "tolerance": null, "row": 3
        });
        let bill = json!({
            "source": "boq.xlsx", "imported": "2026-10-08T00:00:00Z",
            "items": [item], "skipped_rows": 0
        });
        let cmd = json!({ "type": "set_boq", "boq": bill });
        ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
        let found = ipc
            .call("compare", json!({ "toleranceDefault": null }))
            .unwrap();
        assert_eq!(found[0]["kind"], "not_in_drawings", "{found}");
        assert_eq!(ipc.call("conflicts", json!({})), Ok(found.clone()));

        let id = found[0]["id"].clone();
        let cmd =
            json!({ "type": "resolve_conflict", "id": id, "status": "ignored", "note": "Later" });
        ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
        assert_eq!(
            ipc.call("conflicts", json!({})).unwrap()[0]["status"],
            "ignored"
        );

        let path = ipc._dir.path().join("conflicts.xlsx");
        ipc.call("export_conflicts", json!({ "path": path }))
            .unwrap();
        assert!(path.exists());

        assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
        assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
        assert_eq!(ipc.call("conflicts", json!({})), Ok(json!([])));
    }

    #[test]
    fn a_cancelled_turn_says_so_and_ends() {
        ai_test("a_cancelled_turn_says_so_and_ends", || {
            let ipc = Ipc::open();
            ipc.call("ai_send", json!({ "text": "Measure the wall" }))
                .unwrap();
            ipc.call("ai_cancel", json!({})).unwrap();
            let (events, done) = ipc.turn();
            assert_eq!(done, json!({ "error": null }));
            assert_eq!(
                events.last(),
                Some(&json!({ "type": "stopped", "reason": "cancelled" }))
            );
            let status = ipc.call("ai_status", json!({})).unwrap();
            assert_eq!(status["busy"], false);
        });
    }

    /// Calls `mcp_status` until `done` holds for it, for up to 5 s.
    fn mcp_status_until(ipc: &Ipc, done: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let status = ipc.call("mcp_status", json!({})).unwrap();
            if done(&status) {
                return status;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("mcp_status never got there");
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[test]
    fn the_mcp_server_follows_the_settings() {
        let ipc = Ipc::open();
        let status = ipc.call("mcp_status", json!({})).unwrap();
        assert_eq!(
            status,
            json!({ "enabled": false, "listening": false, "addr": null, "error": null })
        );

        // On: it listens on the port.
        let port = free_port();
        let mut settings = ipc.call("get_settings", json!({})).unwrap();
        settings["mcp_enabled"] = json!(true);
        settings["mcp_port"] = json!(port);
        ipc.call("set_settings", json!({ "settings": settings }))
            .unwrap();
        let status = mcp_status_until(&ipc, |s| s["listening"] == true);
        assert_eq!(status["addr"], format!("127.0.0.1:{port}"));
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());

        // A port another program holds: the status says why.
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        settings["mcp_port"] = json!(taken.local_addr().unwrap().port());
        ipc.call("set_settings", json!({ "settings": settings }))
            .unwrap();
        let status = mcp_status_until(&ipc, |s| !s["error"].is_null());
        assert_eq!(status["listening"], false);
        assert!(status["error"].as_str().unwrap().contains("127.0.0.1"));

        // Off: the first port is free again.
        settings["mcp_enabled"] = json!(false);
        ipc.call("set_settings", json!({ "settings": settings }))
            .unwrap();
        let status = ipc.call("mcp_status", json!({})).unwrap();
        assert_eq!(
            (&status["enabled"], &status["listening"]),
            (&json!(false), &json!(false))
        );
        let mut freed = false;
        for _ in 0..100 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                freed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(freed, "the server kept port {port}");
    }

    #[test]
    fn a_pdf_page_attaches_as_an_underlay_and_its_picture_is_drawn() {
        if !qiyas_pdf::available() {
            eprintln!("skipped: needs pdfium (scripts/pdfium.sh)");
            return;
        }
        let ipc = Ipc::open();
        let pdf = qiyas_pdf::fixtures::write_wall(ipc._dir.path());
        assert_eq!(
            ipc.call("pdf_page_count", json!({ "path": pdf })),
            Ok(json!(1))
        );
        let thumbs = ipc.bytes(
            "pdf_thumbnails",
            json!({ "path": pdf, "first": 1, "count": 20, "px": 64 }),
        );
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        assert_eq!(
            [
                u32_at(&thumbs, 0),
                u32_at(&thumbs, 4),
                u32_at(&thumbs, 8),
                u32_at(&thumbs, 12)
            ],
            [1, 1, 64, 32]
        );
        assert_eq!(thumbs.len(), 16 + 64 * 32 * 4);

        let attach = json!({
            "path": pdf, "page": 1, "x": 0.0, "y": 0.0, "scale": 0.01, "mode": "underlay"
        });
        assert_eq!(ipc.call("is_dirty", json!({})), Ok(json!(false)));
        let handle = ipc.call("attach_pdf", attach).unwrap();
        assert!(handle.as_u64().unwrap() > 0);
        assert_eq!(ipc.call("is_dirty", json!({})), Ok(json!(true)));
        let batch = qiyas_render_batch(&ipc.bytes("render_batch", json!({})));
        assert_eq!(batch, vec![(0, true)]);

        // 2 units wide at 300 px per unit: a 1024 px picture (zoom rounded up to 512).
        let picture = ipc.bytes("underlay_image", json!({ "id": 0, "pxPerUnit": 300.0 }));
        let (w, h) = (u32_at(&picture, 0), u32_at(&picture, 4));
        assert_eq!((w, h), (1024, 512));
        assert_eq!(picture.len(), 8 + (w * h * 4) as usize);
        let (name, last) = std::iter::from_fn(|| ipc.events.try_recv().ok())
            .filter(|(n, _)| n == crate::pdf::PROGRESS)
            .last()
            .expect("progress was sent");
        assert_eq!((name.as_str(), last), (crate::pdf::PROGRESS, json!(100)));
        // The same zoom again comes from the cache.
        assert_eq!(
            ipc.bytes("underlay_image", json!({ "id": 0, "pxPerUnit": 290.0 })),
            picture
        );

        let calibrate = json!({ "x1": 0.5, "y1": 0.5, "x2": 1.5, "y2": 0.5, "distance": 3.0 });
        ipc.call("calibrate", calibrate).unwrap();
        let outside = json!({ "x1": -5.0, "y1": -5.0, "x2": 1.0, "y2": 0.0, "distance": 3.0 });
        assert_eq!(
            ipc.call("calibrate", outside),
            Err(json!("Pick the first point on a PDF underlay or an image"))
        );
        assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
        assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
        let batch = qiyas_render_batch(&ipc.bytes("render_batch", json!({})));
        assert!(batch.is_empty());
    }

    #[test]
    fn an_image_attaches_draws_calibrates_and_takes_its_display_settings() {
        let ipc = Ipc::open();
        let png = qiyas_core::fixtures::write_png(ipc._dir.path(), "pic.png");
        assert_eq!(
            ipc.call("image_size", json!({ "path": png })),
            Ok(json!([200, 100]))
        );
        let attach = json!({ "path": png, "x": 5.0, "y": 5.0, "width": 20.0 });
        let handle = ipc.call("attach_image", attach).unwrap().as_u64().unwrap();
        assert_eq!(ipc.call("is_dirty", json!({})), Ok(json!(true)));
        let batch = qiyas_render_batch(&ipc.bytes("render_batch", json!({})));
        assert_eq!(batch, vec![(0, true)]);

        // 20 units at 5 px per unit, rounded up to 8: 160 x 80.
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let picture = ipc.bytes("underlay_image", json!({ "id": 0, "pxPerUnit": 5.0 }));
        assert_eq!((u32_at(&picture, 0), u32_at(&picture, 4)), (160, 80));
        assert_eq!(picture.len(), 8 + 160 * 80 * 4);
        assert_eq!(&picture[8..12], &[255, 0, 0, 255]);

        assert_eq!(
            ipc.call("image_display", json!({ "handle": handle })),
            Ok(json!({ "brightness": 50, "contrast": 50, "fade": 0 }))
        );
        let cmd = json!({
            "type": "set_image_display", "handle": handle, "brightness": 60, "contrast": 45, "fade": 20
        });
        let changed = ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
        assert_eq!(changed["type"], "changed");
        assert!(changed["revision"].is_object());
        assert!(changed["handle_map"].is_object());
        assert_eq!(changed["advice"], json!([]));
        assert_eq!(
            ipc.call("image_display", json!({ "handle": handle })),
            Ok(json!({ "brightness": 60, "contrast": 45, "fade": 20 }))
        );
        assert_eq!(
            ipc.call("image_display", json!({ "handle": 1 })),
            Ok(json!(null))
        );

        // Calibrated by a point on it: 20 units become 10.
        let calibrate = json!({ "x1": 5.0, "y1": 5.0, "x2": 25.0, "y2": 5.0, "distance": 10.0 });
        ipc.call("calibrate", calibrate).unwrap();
        let picture = ipc.bytes("underlay_image", json!({ "id": 0, "pxPerUnit": 5.0 }));
        assert_eq!((u32_at(&picture, 0), u32_at(&picture, 4)), (80, 40));
        for _ in 0..3 {
            assert_eq!(ipc.call("undo", json!({})), Ok(json!(true)));
        }
        assert!(qiyas_render_batch(&ipc.bytes("render_batch", json!({}))).is_empty());
    }

    #[test]
    fn two_sessions_have_independent_takeoffs() {
        let ipc = Ipc::open();
        let first = ipc.call("drawing_info", json!({})).unwrap()["path"].clone();
        let second = qiyas_core::fixtures::write_sample(ipc._dir.path(), "dwg");
        let info = ipc.call("open_drawing", json!({ "path": second })).unwrap();
        assert_eq!(info["path"], json!(second));
        assert!(ipc.events.try_recv().is_err());
        let tabs = ipc.call("tabs", json!({})).unwrap();
        let list = tabs["tabs"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["path"], first);
        assert_eq!(list[1]["title"], "sample.dwg");
        assert_eq!(tabs["active"], list[1]["id"]);
        let (id1, id2) = (list[0]["id"].clone(), list[1]["id"].clone());
        {
            let state = ipc.webview.state::<AppState>();
            let workspaces = state.workspaces();
            assert_eq!(workspaces.len(), 2);
            assert!(!Arc::ptr_eq(&workspaces[0].session, &workspaces[1].session));
        }

        // Measure on the second drawing only.
        let cmd = json!({ "type": "measure_distance", "points": [[0.0, 0.0], [3.0, 0.0]] });
        ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
        let count = |ipc: &Ipc| {
            ipc.call("takeoff", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(count(&ipc), 1);

        let info = ipc.call("activate_tab", json!({ "id": id1 })).unwrap();
        assert_eq!(info["path"], first);
        assert!(
            ipc.events.try_recv().is_err(),
            "switching must not clear a conversation"
        );
        assert_eq!(ipc.call("drawing_info", json!({})).unwrap()["path"], first);
        assert_eq!(count(&ipc), 0, "the first drawing has its own take-off");
        assert_eq!(ipc.call("tabs", json!({})).unwrap()["active"], id1);
        {
            // The picture cache keeps only the drawing on screen's pictures.
            let state = ipc.webview.state::<AppState>();
            let epoch = state.session.lock().unwrap().as_ref().unwrap().epoch();
            assert_eq!(crate::pdf::pictures(&state).epoch(), epoch);
            // Back from parking, it draws and picks again: the rectangle's right edge.
            let session = state.session.lock().unwrap();
            let session = session.as_ref().unwrap();
            assert!(session.pick([4.0, 1.5], 0.05).is_some());
            assert!(session.is_drawn());
            assert_eq!(state.workspaces().len(), 2);
        }

        ipc.call("activate_tab", json!({ "id": id2 })).unwrap();
        assert_eq!(count(&ipc), 1);
        assert!(ipc.call("activate_tab", json!({ "id": 99 })).is_err());
    }

    #[test]
    fn the_same_file_twice_shows_its_tab() {
        let ipc = Ipc::open();
        let first = ipc.call("drawing_info", json!({})).unwrap()["path"].clone();
        let second = qiyas_core::fixtures::write_sample(ipc._dir.path(), "dwg");
        ipc.call("open_drawing", json!({ "path": second })).unwrap();
        let info = ipc.call("open_drawing", json!({ "path": first })).unwrap();
        assert_eq!(info["path"], first);
        let tabs = ipc.call("tabs", json!({})).unwrap();
        assert_eq!(
            tabs["tabs"].as_array().unwrap().len(),
            2,
            "not opened twice"
        );
        assert_eq!(tabs["active"], tabs["tabs"][0]["id"]);
    }

    #[test]
    fn a_dirty_tab_closes_only_when_forced() {
        let ipc = Ipc::open();
        let second = qiyas_core::fixtures::write_sample(ipc._dir.path(), "dwg");
        ipc.call("open_drawing", json!({ "path": second })).unwrap();
        let tabs = ipc.call("tabs", json!({})).unwrap();
        let (id1, id2) = (tabs["tabs"][0]["id"].clone(), tabs["tabs"][1]["id"].clone());
        let cmd =
            json!({ "type": "save_view", "name": "Hall", "bounds": [[0.0, 0.0], [4.0, 3.0]] });
        ipc.call("run_command", json!({ "cmd": cmd })).unwrap();
        assert_eq!(
            ipc.call("views", json!({})).unwrap(),
            json!([{ "name": "Hall", "bounds": [[0.0, 0.0], [4.0, 3.0]] }])
        );
        assert_eq!(
            ipc.call("tabs", json!({})).unwrap()["tabs"][1]["dirty"],
            true
        );

        // Parked or on screen, a dirty tab needs force.
        ipc.call("activate_tab", json!({ "id": id1 })).unwrap();
        assert_eq!(
            ipc.call("close_tab", json!({ "id": id2 })),
            Err(json!(crate::tabs::UNSAVED))
        );
        ipc.call("activate_tab", json!({ "id": id2 })).unwrap();
        assert_eq!(
            ipc.call("close_tab", json!({ "id": id2, "force": false })),
            Err(json!(crate::tabs::UNSAVED))
        );
        let left = ipc
            .call("close_tab", json!({ "id": id2, "force": true }))
            .unwrap();
        assert_eq!(left["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(left["active"], id1, "the tab left is shown");
        assert_eq!(ipc.call("takeoff", json!({})), Ok(json!([])));

        // A clean tab closes at once; then no drawing is open.
        let left = ipc.call("close_tab", json!({ "id": id1 })).unwrap();
        assert_eq!(left, json!({ "tabs": [], "active": null }));
        assert_eq!(
            ipc.call("drawing_info", json!({})),
            Err(json!("No drawing is open"))
        );
    }

    #[test]
    fn opening_and_saving_fill_the_recent_list() {
        let ipc = Ipc::open();
        let first = ipc.call("drawing_info", json!({})).unwrap()["path"].clone();
        let second = qiyas_core::fixtures::write_sample(ipc._dir.path(), "dwg");
        ipc.call("open_drawing", json!({ "path": second })).unwrap();
        ipc.call("open_drawing", json!({ "path": first })).unwrap();
        let list = ipc.call("recents", json!({})).unwrap();
        let paths: Vec<&Value> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["path"])
            .collect();
        assert_eq!(
            paths,
            vec![&first, &json!(second)],
            "most recent first, once each"
        );
        assert_eq!(list[0]["name"], "sample.dxf");
        assert_eq!(list[0]["folder"], json!(ipc._dir.path()));
        assert_eq!(list[0]["missing"], false);

        // The Settings dialog's older copy never drops them.
        let mut settings = ipc.call("get_settings", json!({})).unwrap();
        settings["recents"] = json!([]);
        ipc.call("set_settings", json!({ "settings": settings }))
            .unwrap();
        assert_eq!(
            ipc.call("recents", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let copy = ipc._dir.path().join("copy.dxf");
        ipc.call("save_drawing_as", json!({ "path": copy }))
            .unwrap();
        assert_eq!(
            ipc.call("recents", json!({})).unwrap()[0]["path"],
            json!(copy)
        );
        let tabs = ipc.call("tabs", json!({})).unwrap();
        assert_eq!(
            tabs["tabs"][0]["title"], "copy.dxf",
            "the tab follows Save As"
        );

        std::fs::remove_file(&second).unwrap();
        assert_eq!(ipc.call("recents", json!({})).unwrap()[2]["missing"], true);
        ipc.call("clear_recents", json!({})).unwrap();
        assert_eq!(ipc.call("recents", json!({})), Ok(json!([])));
    }

    struct GatedDrawingProvider {
        entered: Arc<AtomicUsize>,
        release: Arc<tokio::sync::Notify>,
        step: AtomicUsize,
        x: f64,
    }
    impl qiyas_ai::Provider for GatedDrawingProvider {
        fn stream_turn<'a>(
            &'a self,
            _: qiyas_ai::TurnRequest,
            sink: qiyas_ai::provider::Sink<'a>,
        ) -> qiyas_ai::provider::BoxFuture<'a, Result<qiyas_ai::TurnResult, qiyas_ai::AiError>>
        {
            let step = self.step.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if step == 0 {
                    self.entered.fetch_add(1, Ordering::SeqCst);
                    self.release.notified().await;
                    Ok(qiyas_ai::TurnResult {
                        content: vec![qiyas_ai::ContentBlock::ToolUse {
                            id: "draw".into(),
                            name: "qiyas_execute".into(),
                            input: json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[self.x,0],"b":[self.x+2.0,0]}}}),
                        }],
                        stop_reason: qiyas_ai::StopReason::ToolUse,
                        usage: Default::default(),
                    })
                } else {
                    let text = format!("Finished drawing at {}", self.x);
                    sink(qiyas_ai::provider::StreamEvent::TextDelta(text.clone()));
                    Ok(qiyas_ai::TurnResult {
                        content: vec![qiyas_ai::ContentBlock::Text { text }],
                        stop_reason: qiyas_ai::StopReason::EndTurn,
                        usage: Default::default(),
                    })
                }
            })
        }
    }

    #[test]
    fn workspace_turns_survive_switch_and_reload_with_independent_providers() {
        ai_test("workspace_turns", || {
            let ipc = Ipc::open();
            let first = ipc.call("tabs", json!({})).unwrap()["active"]
                .as_u64()
                .unwrap() as u32;
            ipc.call("new_drawing", json!({"units":"m"})).unwrap();
            let second = ipc.call("tabs", json!({})).unwrap()["active"]
                .as_u64()
                .unwrap() as u32;
            let entered = Arc::new(AtomicUsize::new(0));
            let release = Arc::new(tokio::sync::Notify::new());
            for (id, x, provider, model) in [
                (first, 101.0, "anthropic", "first-model"),
                (second, 202.0, "openai_compatible", "second-model"),
            ] {
                ipc.call(
                    "workspace_provider",
                    json!({"workspace":id,"provider":provider,"model":model,"effort":"low"}),
                )
                .unwrap();
                let state = ipc._app.state::<AppState>().scoped(Some(id)).unwrap();
                let entered = entered.clone();
                let release = release.clone();
                ai_run(&state).test_provider = Some(Arc::new(move || {
                    Box::new(GatedDrawingProvider {
                        entered: entered.clone(),
                        release: release.clone(),
                        step: AtomicUsize::new(0),
                        x,
                    })
                }));
                ipc.call("ai_send", json!({"workspace":id,"text":"Draw one line"}))
                    .unwrap();
            }
            let started = std::time::Instant::now();
            while entered.load(Ordering::SeqCst) < 2 {
                assert!(started.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(5));
            }
            ipc.call("activate_tab", json!({"id":first})).unwrap();
            for (id, model) in [(first, "first-model"), (second, "second-model")] {
                let attached = ipc
                    .call("workspace_attach", json!({"workspace":id}))
                    .unwrap();
                assert_eq!(attached["status"], "working");
                assert_eq!(attached["transcript"][0]["text"], "Draw one line");
                assert_eq!(attached["choice"]["model"], model);
                assert_eq!(
                    ipc.call("close_tab", json!({"id":id,"force":true})),
                    Err(json!(tabs::WORKING))
                );
            }
            // Same commands used by a new webview: they neither restart nor cancel the jobs.
            let reattached = ipc
                .call("workspace_attach", json!({"workspace":first}))
                .unwrap();
            assert_eq!(reattached["status"], "working");
            release.notify_waiters();
            let started = std::time::Instant::now();
            loop {
                let open = ipc.call("tabs", json!({})).unwrap();
                if open["tabs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|tab| tab["busy"] == false && tab["status"] == "done")
                {
                    break;
                }
                assert!(started.elapsed() < Duration::from_secs(20));
                std::thread::sleep(Duration::from_millis(10));
            }
            for (id, x) in [(first, 101.0), (second, 202.0)] {
                let state = ipc._app.state::<AppState>().scoped(Some(id)).unwrap();
                with_session(&state, |session| {
                    assert!(session.pick([x + 1.0, 0.0], 0.05).is_some());
                    assert!(session
                        .pick([if x == 101.0 { 203.0 } else { 102.0 }, 0.0], 0.05)
                        .is_none());
                    assert!(session.undo_if(Source::Ai));
                    assert!(session.pick([x + 1.0, 0.0], 0.05).is_none());
                    Ok(())
                })
                .unwrap();
            }
            let hidden = ipc
                .call("workspace_attach", json!({"workspace":second}))
                .unwrap();
            assert!(hidden["away"].as_str().unwrap().contains("202"));
        });
    }

    #[test]
    fn claude_workspaces_queue_and_keep_their_provider_choices() {
        ai_test("workspace_claude_queue", || {
            let ipc = Ipc::open();
            let first = ipc.call("tabs", json!({})).unwrap()["active"]
                .as_u64()
                .unwrap() as u32;
            ipc.call("new_drawing", json!({"units":"m"})).unwrap();
            let second = ipc.call("tabs", json!({})).unwrap()["active"]
                .as_u64()
                .unwrap() as u32;
            let entered = Arc::new(AtomicUsize::new(0));
            let release = Arc::new(tokio::sync::Notify::new());
            for id in [first, second] {
                ipc.call("workspace_provider", json!({"workspace":id,"provider":"claude_code","model":"sonnet","effort":"low"})).unwrap();
                let state = ipc._app.state::<AppState>().scoped(Some(id)).unwrap();
                let provider_entered = entered.clone();
                let provider_release = release.clone();
                ai_run(&state).test_provider = Some(Arc::new(move || {
                    Box::new(GatedDrawingProvider {
                        entered: provider_entered.clone(),
                        release: provider_release.clone(),
                        step: AtomicUsize::new(0),
                        x: 400.0 + f64::from(id),
                    })
                }));
                ipc.call("ai_send", json!({"workspace":id,"text":"Draw one line"}))
                    .unwrap();
                if id == first {
                    let until = std::time::Instant::now();
                    while entered.load(Ordering::SeqCst) == 0 {
                        assert!(until.elapsed() < Duration::from_secs(5));
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            let until = std::time::Instant::now();
            while entered.load(Ordering::SeqCst) == 0 {
                assert!(until.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(entered.load(Ordering::SeqCst), 1);
            let second_view = ipc
                .call("workspace_attach", json!({"workspace":second}))
                .unwrap();
            assert_eq!(second_view["status"], "queued");
            release.notify_one();
            let until = std::time::Instant::now();
            while entered.load(Ordering::SeqCst) < 2 {
                assert!(until.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                ipc.call("workspace_attach", json!({"workspace":second}))
                    .unwrap()["status"],
                "working"
            );
            release.notify_one();
            let until = std::time::Instant::now();
            while ipc.call("tabs", json!({})).unwrap()["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tab| tab["busy"] == true)
            {
                assert!(until.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }

    #[test]
    fn workspace_busy_tool_keeps_other_drawing_ipc_interactive() {
        // Fast by default; the combined acceptance run can request a real 60-second hold.
        let duration = Duration::from_millis(
            std::env::var("QIYAS_W_HOLD_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(100),
        );
        let ipc = Ipc::open();
        let first = ipc.call("tabs", json!({})).unwrap()["active"]
            .as_u64()
            .unwrap() as u32;
        ipc.call("new_drawing", json!({"units":"m"})).unwrap();
        let second = ipc.call("tabs", json!({})).unwrap()["active"]
            .as_u64()
            .unwrap() as u32;
        let committed = ipc.bytes("render_batch", json!({"workspace":first}));
        let busy = ipc._app.state::<AppState>().scoped(Some(first)).unwrap();
        let (ready, entered) = channel();
        let (release, released) = channel();
        let worker = std::thread::spawn(move || {
            let _session = busy.session.lock().unwrap();
            ready.send(()).unwrap();
            // Keep the lock until the final timing sample, including a slow last iteration.
            released
                .recv_timeout(duration + Duration::from_secs(30))
                .unwrap();
        });
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = std::time::Instant::now();
        let mut worst = Duration::ZERO;
        let mut checks = 0;
        while start.elapsed() < duration {
            let now = std::time::Instant::now();
            ipc.call("activate_tab", json!({"id":first})).unwrap();
            ipc.call("drawing_info", json!({"workspace":first}))
                .unwrap();
            assert_eq!(
                ipc.bytes("render_batch", json!({"workspace":first})),
                committed
            );
            assert_eq!(
                ipc.call("pick", json!({"workspace":first,"x":0,"y":0,"tol":1})),
                Err(json!("workspace_busy"))
            );
            ipc.call("activate_tab", json!({"id":second})).unwrap();
            ipc.call("drawing_info", json!({"workspace":second}))
                .unwrap();
            ipc.call("snap", json!({"workspace":second,"x":0,"y":0,"tol":1}))
                .unwrap();
            let session = ipc._app.state::<AppState>().scoped(Some(second)).unwrap();
            with_session(&session, |s| {
                s.run_input(
                    &json!({"type":"draw","kind":"line","a":[0,0],"b":[2,0]}),
                    Source::User,
                )?;
                assert!(s.pick([1.0, 0.0], 0.05).is_some());
                assert!(s.undo());
                Ok(())
            })
            .unwrap();
            worst = worst.max(now.elapsed());
            checks += 1;
            assert!(now.elapsed() < Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(10));
        }
        release.send(()).unwrap();
        worker.join().unwrap();
        let measured = json!({"duration_seconds":start.elapsed().as_secs_f64(),"checks":checks,"worst_ipc_group_ms":worst.as_secs_f64()*1000.0});
        if let Some(path) = std::env::var_os("QIYAS_W_TIMING") {
            std::fs::write(path, serde_json::to_vec_pretty(&measured).unwrap()).unwrap();
        }
        eprintln!("{measured}");
    }

    #[test]
    fn closed_workspace_ids_never_rebind_to_a_new_drawing() {
        let ipc = Ipc::open();
        let first = ipc.call("tabs", json!({})).unwrap()["active"]
            .as_u64()
            .unwrap() as u32;
        let original = ipc._app.state::<AppState>().scoped(Some(first)).unwrap();
        ipc.call("close_tab", json!({"id":first,"force":true}))
            .unwrap();
        assert!(original.closed.load(Ordering::Acquire));
        assert!(original.session.lock().unwrap().is_none());
        assert!(ipc
            .call("drawing_info", json!({"workspace":first}))
            .is_err());
        ipc.call("new_drawing", json!({"units":"m"})).unwrap();
        let second = ipc.call("tabs", json!({})).unwrap()["active"]
            .as_u64()
            .unwrap() as u32;
        assert_ne!(first, second);
        let replacement = ipc._app.state::<AppState>().scoped(Some(second)).unwrap();
        assert!(!Arc::ptr_eq(&original.session, &replacement.session));
        assert!(tabs::add(
            &original,
            Session::new_blank(&ipc._dir.path().join("late.dxf"), 6)
        )
        .is_err());
        assert_eq!(
            ipc.call("tabs", json!({})).unwrap()["tabs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    /// The image table of a batch (`Batch::to_bytes`, kind 11): (id, version below 2^53).
    fn qiyas_render_batch(bytes: &[u8]) -> Vec<(u32, bool)> {
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let k = u32_at(56) as usize;
        (0..k)
            .map(|i| {
                (
                    u32_at(60 + 12 * i),
                    u32_at(64 + 12 * i) as usize,
                    u32_at(68 + 12 * i) as usize,
                )
            })
            .filter(|(kind, ..)| *kind == 11)
            .flat_map(|(_, at, len)| {
                (at..at + len).step_by(16).map(move |e| {
                    let version = u64::from_le_bytes(bytes[e + 8..e + 16].try_into().unwrap());
                    (
                        u32::from_le_bytes(bytes[e..e + 4].try_into().unwrap()),
                        version < 1 << 53,
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod panic_recovery_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_panicking_desktop_edit_releases_the_session_and_keeps_working() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(AppState::new(dir.path().join("config")));
        *state.session.lock().unwrap() = Some(Session::new_blank(&dir.path().join("panic.dxf"), 6));
        let worker = state.clone();
        let error = std::thread::spawn(move || {
            with_session(&worker, |s| {
                qiyas_core::fixtures::panic_on_document_edit(2);
                s.run_input(
                    &json!({"type":"batch","commands":[
                        {"type":"draw","kind":"line","a":[0,0],"b":[1,0]},
                        {"type":"draw","kind":"line","a":[0,1],"b":[1,1]}
                    ]}),
                    Source::User,
                )
            })
            .unwrap_err()
        })
        .join()
        .expect("desktop command returned normally");
        assert!(error.contains("rolled back"), "{error}");
        {
            let guard = state.session.try_lock().expect("released and not poisoned");
            let s = guard.as_ref().unwrap();
            assert_eq!(s.drawing.doc.entities().count(), 0);
            assert!(s.history_steps().0.is_empty());
        }
        with_session(&state, |s| {
            s.run_input(
                &json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
                Source::User,
            )?;
            assert_eq!(s.drawing.doc.entities().count(), 1);
            assert!(s.undo());
            assert_eq!(s.drawing.doc.entities().count(), 0);
            assert!(s.redo());
            s.save_project()?;
            Ok(())
        })
        .unwrap();
        let error = with_session::<()>(&state, |_| panic!("injected host panic")).unwrap_err();
        assert!(error.contains("keep working"));
        assert!(
            state.session.try_lock().is_ok(),
            "host panic must not poison the session"
        );
    }
}
