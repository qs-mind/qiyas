//! IPC for the block palette, the Design Center, WBLOCK through the Save dialog and the user's
//! tool palette, and another drawing's layers for the layer translator. Inserting, editing and
//! attributes go through `run_command`.

use std::path::Path;
use std::sync::PoisonError;

use qiyas_core::{BlockInfo, LayerInfo};
use tauri::ipc::Response;
use tauri::State;

use crate::commands::with_session;
use crate::settings::{self, PaletteItem};
use crate::state::AppState;

/// Margin around a thumbnail's lines, as a share of their size.
const MARGIN: f64 = 0.08;

/// The drawing's named blocks with their attribute definitions, sorted.
#[tauri::command]
pub async fn blocks(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<BlockInfo>, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.blocks()))
}

/// A PNG of the block `name`, `px` pixels wide (its lines on white, as the assistant's pictures).
#[tauri::command]
pub async fn block_thumbnail(
    workspace: Option<u32>,

    name: String,
    px: u32,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let state = state.scoped(workspace)?;
    let batch = with_session(&state, |s| s.block_batch(&name))?;
    let [lo, hi] = batch.bounds;
    // A block of one point or nothing still gets a square to show.
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
    let m = size * MARGIN;
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let half = size / 2.0 + m;
    let bounds = [[cx - half, cy - half], [cx + half, cy + half]];
    Ok(Response::new(qiyas_ai::render_png(&batch, &[], bounds, px)))
}

/// The named blocks of the drawing at `path`, opened read-only, with their attribute
/// definitions (the Design Center).
#[tauri::command]
pub async fn drawing_blocks(path: String) -> Result<Vec<BlockInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        qiyas_core::drawing_blocks(Path::new(&path)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The layers of the drawing at `path`, opened read-only (the layer translator's standard).
#[tauri::command]
pub async fn drawing_layers(path: String) -> Result<Vec<LayerInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        qiyas_core::drawing_layers(Path::new(&path)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Writes the block `name` to `path`, a .dwg file the user chose in the Save dialog (WBLOCK).
#[tauri::command]
pub async fn write_block(
    workspace: Option<u32>,

    name: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| s.write_block(&name, Path::new(&path)))
}

/// Keeps the user's tool palette (blocks and commands) in the settings file.
#[tauri::command]
pub async fn set_tool_palette(
    items: Vec<PaletteItem>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    settings::validate_palette(&items)?;
    let mut cached = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut next = cached.clone();
    next.tool_palette = items;
    settings::save(&state.config_dir, &next)
        .map_err(|e| format!("The settings could not be saved: {e}"))?;
    *cached = next;
    Ok(())
}
