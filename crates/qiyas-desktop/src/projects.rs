//! Project commands have their own history so they work before a drawing is opened.
use crate::state::AppState;
use qiyas_core::{Command, Outcome, Session, Source};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};

#[derive(Default)]
pub struct Workspace {
    session: Option<Session>,
    pub active: Option<PathBuf>,
}
pub fn execute(state: &AppState, input: &Value, source: Source) -> Result<Value, String> {
    let command = qiyas_core::registry::parse_command(input.clone())?;
    let Command::Project(_) = command else {
        return Err("Expected a project command".into());
    };
    if let Command::Project(project) = &command {
        if let qiyas_core::projects::ProjectCommand::Run { path } = project.as_ref() {
            let path = qiyas_core::projects::project_path(path).map_err(|e| e.to_string())?;
            let set = qiyas_core::projects::load(&path).map_err(|e| e.to_string())?;
            let entries = qiyas_core::sheet_set_entries(&path, &set);
            let _tabs = state.tabs.lock().map_err(|e| e.to_string())?;
            let active = state.session.lock().map_err(|e| e.to_string())?;
            let parked = state.parked.lock().map_err(|e| e.to_string())?;
            for session in active.iter().chain(parked.iter().map(|tab| &tab.session)) {
                if session.is_dirty()
                    && entries.iter().any(|e| {
                        crate::settings::same_file(Path::new(&e.full), &session.drawing.path)
                    })
                {
                    return Err(format!(
                        "Save {} before running its project",
                        session.drawing.path.display()
                    ));
                }
            }
        }
    }
    let mut workspace = state.projects.lock().map_err(|e| e.to_string())?;
    if workspace.session.is_none() {
        workspace.session = Some(qiyas_boq::project::session()?);
    }
    let outcome = workspace
        .session
        .as_mut()
        .unwrap()
        .run_input(input, source)
        .map_err(|e| e.to_string())?;
    let mut result = json!(outcome);
    if let Outcome::Project { result: project } = &outcome.outcome {
        if let Some(path) = project["path"].as_str() {
            workspace.active = Some(PathBuf::from(path));
        }
    }
    result["result"]["active_project"] = json!(workspace.active);
    if input["kind"] == "run" {
        result["outputs"] = qiyas_boq::project::outputs(&outcome.outcome, None)?;
    }
    Ok(result)
}
#[tauri::command]
pub async fn project_command<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    command: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        execute(&app.state::<AppState>(), &command, Source::User)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn project_history(state: State<'_, AppState>, redo: bool) -> Result<bool, String> {
    let mut workspace = state.projects.lock().map_err(|e| e.to_string())?;
    Ok(workspace
        .session
        .as_mut()
        .is_some_and(|s| if redo { s.redo() } else { s.undo() }))
}
pub fn contains(workspace: &Mutex<Workspace>, drawing: &Path) -> bool {
    let Ok(workspace) = workspace.lock() else {
        return false;
    };
    let Some(path) = &workspace.active else {
        return false;
    };
    let Ok(set) = qiyas_core::projects::load(path) else {
        return false;
    };
    let Ok(drawing) = drawing.canonicalize() else {
        return false;
    };
    qiyas_core::sheet_set_entries(path, &set)
        .iter()
        .any(|entry| Path::new(&entry.full).canonicalize().ok().as_ref() == Some(&drawing))
}
