//! Filesystem boundary for window commands. Return resolved paths and use those for I/O.
use std::path::{Path, PathBuf};

use crate::state::{lock, AppState};

pub(crate) const DRAWINGS: &[&str] = &["dwg", "dxf"];
pub(crate) const WORKBOOKS: &[&str] = &["xlsx", "xlsm", "xls", "xlsb", "ods"];
pub(crate) const IMAGES: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp", "gif"];
pub(crate) const OUTPUTS: &[&str] = &[
    "pdf", "xlsx", "dwg", "dxf", "svg", "png", "jpg", "jpeg", "webp", "gif", "csv", "txt", "md",
    "json",
];

fn refused(path: &Path, why: impl std::fmt::Display) -> String {
    format!("Refused {}: {why}", path.display())
}

fn extension(path: &Path, allowed: &[&str]) -> Result<(), String> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| allowed.iter().any(|want| e.eq_ignore_ascii_case(want)))
    {
        Ok(())
    } else {
        Err(refused(
            path,
            format!(
                "choose a {} file",
                allowed
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        ))
    }
}

/// A missing or unreadable file is not a refusal: say plainly that it could not be read.
fn canonical(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|e| format!("{} could not be read: {e}", path.display()))
}

/// Native Open dialogs may select imports anywhere, but only the format the reader expects.
pub(crate) fn read(path: &Path, allowed: &[&str]) -> Result<PathBuf, String> {
    let resolved = canonical(path)?;
    extension(&resolved, allowed)?;
    if !resolved.is_file() {
        return Err(refused(path, "choose a regular file"));
    }
    Ok(resolved)
}

/// Resolve an existing file, or its existing parent for a new file. A dangling symlink is an
/// error, not a new file; otherwise a writer could follow it somewhere we never checked.
fn destination(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|_| refused(path, "the parent folder must exist and be accessible"))?;
    if !parent.is_dir() {
        return Err(refused(path, "the parent folder must exist"));
    }
    let name = path
        .file_name()
        .ok_or_else(|| refused(path, "choose a file name"))?;
    let candidate = parent.join(name);
    match candidate.symlink_metadata() {
        Ok(_) => {
            let resolved = canonical(&candidate)?;
            if !resolved.is_file() {
                return Err(refused(path, "choose a regular file"));
            }
            Ok(resolved)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(candidate),
        Err(e) => Err(refused(path, e)),
    }
}

fn protected_in_home(home: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<_> = [
        ".ssh",
        ".config",
        ".codex",
        ".claude",
        ".cargo",
        ".local/bin",
    ]
    .iter()
    .map(|name| home.join(name))
    .collect();
    if cfg!(target_os = "macos") {
        folders.push(home.join("Library"));
    }
    if cfg!(windows) {
        folders.push(home.join("AppData"));
    }
    folders
}

fn protected_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for name in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(name).filter(|s| !s.is_empty()) {
            folders.extend(protected_in_home(Path::new(&home)));
        }
    }
    for name in [
        "XDG_CONFIG_HOME",
        "CARGO_HOME",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
    ] {
        if let Some(path) = std::env::var_os(name).filter(|s| !s.is_empty()) {
            folders.push(path.into());
        }
    }
    #[cfg(windows)]
    for name in [
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "SYSTEMROOT",
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ] {
        if let Some(path) = std::env::var_os(name).filter(|s| !s.is_empty()) {
            folders.push(path.into());
        }
    }
    #[cfg(unix)]
    folders.extend(
        [
            "/etc", "/usr", "/bin", "/sbin", "/boot", "/System", "/Library",
        ]
        .map(PathBuf::from),
    );
    folders
}

fn plain_filename(path: &Path) -> Result<(), String> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name.starts_with('.') {
        return Err(refused(path, "writing dotfiles is not allowed"));
    }
    // ':' is an NTFS stream separator; trailing dots/spaces have Win32 aliases.
    #[cfg(windows)]
    if name.contains(':') || name.ends_with(['.', ' ']) {
        return Err(refused(
            path,
            "choose a plain file name without a stream or trailing dot or space",
        ));
    }
    Ok(())
}

fn write_with_protected(
    path: &Path,
    allowed: &[&str],
    protected: &[PathBuf],
) -> Result<PathBuf, String> {
    let resolved = destination(path)?;
    for candidate in [path, resolved.as_path()] {
        plain_filename(candidate)?;
        extension(candidate, allowed)?;
    }
    for folder in protected {
        // Windows keeps its ordinary temporary files under LocalAppData. That specific Temp
        // subtree is not a settings/executable folder, and is also where print spools live.
        #[cfg(windows)]
        if windows_temp_file(&resolved, folder) {
            continue;
        }
        // Check both spellings: a protected folder may itself be a symlink.
        if path.starts_with(folder)
            || resolved.starts_with(folder)
            || folder
                .canonicalize()
                .is_ok_and(|root| resolved.starts_with(root))
        {
            return Err(refused(
                path,
                "this is a protected settings or executable folder",
            ));
        }
    }
    Ok(resolved)
}

#[cfg(windows)]
fn windows_temp_file(path: &Path, protected: &Path) -> bool {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return false;
    };
    let local = PathBuf::from(local);
    let Ok(root) = protected.canonicalize() else {
        return false;
    };
    let broad_root = local.canonicalize().ok().as_ref() == Some(&root)
        || local.parent().and_then(|p| p.canonicalize().ok()).as_ref() == Some(&root);
    broad_root
        && local
            .join("Temp")
            .canonicalize()
            .is_ok_and(|temp| path.starts_with(temp))
}

/// Save dialogs can choose any existing parent outside protected folders.
pub(crate) fn write(path: &Path, allowed: &[&str]) -> Result<PathBuf, String> {
    write_with_protected(path, allowed, &protected_folders())
}

/// Take roots from Rust-owned sessions and projects, never from the window's payload.
pub(crate) fn output_roots(state: &AppState) -> Result<Vec<PathBuf>, String> {
    let mut roots = vec![qiyas_core::data_home::qiyas_dir()?];
    for workspace in state.conversations() {
        if !workspace.closed.load(std::sync::atomic::Ordering::Acquire) {
            roots.extend(lock(&workspace.output_folders).iter().cloned());
        }
    }
    let project = lock(&state.projects).active.clone();
    if let Some(path) = project {
        if let Some(folder) = path.parent() {
            roots.push(folder.to_path_buf());
        }
        roots.push(path.with_extension("output"));
        if let Ok(set) = qiyas_core::projects::load(&path) {
            for entry in qiyas_core::sheet_set_entries(&path, &set) {
                // Resolve the drawing, not just its parent: project entries may be symlinks.
                if let Ok(drawing) = read(Path::new(&entry.full), DRAWINGS) {
                    if let Some(folder) = drawing.parent() {
                        roots.push(folder.to_path_buf());
                    }
                }
            }
        }
    }
    Ok(roots)
}

pub(crate) fn output(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(refused(path, "use an absolute file path"));
    }
    let resolved = read(path, OUTPUTS)?;
    if !roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|root| resolved.starts_with(root))
    {
        return Err(refused(
            path,
            "the file is outside the open drawings, open project and Qiyas data folders",
        ));
    }
    Ok(resolved)
}

pub(crate) fn claude_executable(selected: Option<&str>) -> Result<Option<String>, String> {
    let Some(selected) = selected.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let path = Path::new(selected);
    let valid_name = |path: &Path| {
        path.file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| {
                name == "claude"
                    || cfg!(windows)
                        && (name.eq_ignore_ascii_case("claude.exe")
                            || name.eq_ignore_ascii_case("claude.cmd"))
            })
    };
    if !valid_name(path) {
        return Err(refused(
            path,
            "choose the claude executable (claude.exe or claude.cmd on Windows)",
        ));
    }
    if !path.is_absolute() {
        return Err(refused(
            path,
            "choose an absolute path to the claude executable",
        ));
    }
    let resolved = canonical(path)?;
    if !valid_name(&resolved) || !resolved.is_file() {
        return Err(refused(
            path,
            "the resolved file must be the claude executable",
        ));
    }
    Ok(Some(resolved.to_string_lossy().into_owned()))
}

/// Recovery also accepts a missing original drawing (untitled/crash recovery), or a named
/// autosave copy. Never let the suffix turn an unrelated file into a drawing.
pub(crate) fn recovery(path: &Path) -> Result<PathBuf, String> {
    let resolved = destination(path)?;
    for candidate in [path, resolved.as_path()] {
        plain_filename(candidate)?;
        let drawing =
            qiyas_core::autosave_copy_of(candidate).unwrap_or_else(|| candidate.to_path_buf());
        extension(&drawing, DRAWINGS)?;
    }
    Ok(resolved)
}

/// The command bus accepts paths inside JSON too. Keep its existing folder confinement and
/// relative-path anchors, adding the same format/protected-folder checks as dedicated IPC.
pub(crate) fn command(
    command: &qiyas_core::Command,
    session: &qiyas_core::Session,
) -> Result<(), String> {
    use qiyas_core::Command;
    let folder = session.drawing.path.parent().unwrap_or(Path::new("."));
    match command {
        Command::Batch { commands, .. } => {
            for command in commands {
                self::command(command, session)?;
            }
        }
        Command::WriteBlock { path, .. } => {
            write(&folder.join(path), &["dwg"])?;
        }
        Command::CompareRevisions {
            other_path: path, ..
        }
        | Command::CarryKnowledge {
            from_path: path, ..
        } => {
            read(&folder.join(path), DRAWINGS)?;
        }
        Command::RedefineBlock { from_path, .. } => {
            read(Path::new(from_path), DRAWINGS)?;
        }
        Command::InsertBlock {
            from_path: Some(path),
            ..
        }
        | Command::TranslateLayers {
            from_path: Some(path),
            ..
        } => {
            read(Path::new(path), DRAWINGS)?;
        }
        Command::ImportPdf { path, .. } | Command::AttachPdfUnderlay { path, .. } => {
            read(Path::new(path), &["pdf"])?;
        }
        Command::AttachImage { path, .. } => {
            read(Path::new(path), IMAGES)?;
        }
        Command::Recognition(command) => {
            use qiyas_core::recognition::RecognitionCommand;
            match command.as_ref() {
                RecognitionCommand::Pdf { path, .. } | RecognitionCommand::PdfText { path, .. } => {
                    read(Path::new(path), &["pdf"])?;
                }
                RecognitionCommand::Vision { spec } => {
                    read(Path::new(&spec.path), &["pdf"])?;
                }
                _ => {}
            }
        }
        Command::Project(command) => project(command)?,
        // The deliverable's actual (possibly generated) path is checked in `outcome`, before
        // the workbook/PDF writer runs. Core also confines it to its source/export folder.
        _ => {}
    }
    Ok(())
}

pub(crate) fn outcome(outcome: &qiyas_core::Outcome) -> Result<(), String> {
    if let qiyas_core::Outcome::Deliverable { export } = outcome {
        let ext = match export.format {
            qiyas_core::deliverables::Format::Excel => "xlsx",
            qiyas_core::deliverables::Format::Pdf => "pdf",
        };
        write(&export.path, &[ext])?;
        if let Some(client) = &export.report.layout.client {
            read(Path::new(&client.path), &["xlsx"])?;
        }
    }
    Ok(())
}

/// Project runs generate these two fixed names; they do not use a native Save dialog.
pub(crate) fn project_output_folder(qss: &Path) -> Result<PathBuf, String> {
    let qss = write(qss, &["qss"])?;
    let folder = qss.with_extension("output");
    std::fs::create_dir_all(&folder).map_err(|e| refused(&folder, e))?;
    let folder = canonical(&folder)?;
    write(&folder.join("project.xlsx"), &["xlsx"])?;
    write(&folder.join("morning-report.md"), &["md"])?;
    Ok(folder)
}

pub(crate) fn project(command: &qiyas_core::projects::ProjectCommand) -> Result<(), String> {
    use qiyas_core::projects::{project_path, ProjectCommand};
    let path = match command {
        ProjectCommand::List => return Ok(()),
        ProjectCommand::Create { name } => {
            let path = project_path(&format!("{name}.qss")).map_err(|e| e.to_string())?;
            write(&path, &["qss"])?;
            return Ok(());
        }
        ProjectCommand::Open { path }
        | ProjectCommand::AddDrawing { path, .. }
        | ProjectCommand::RemoveDrawing { path, .. }
        | ProjectCommand::SetMethod { path, .. }
        | ProjectCommand::SetChecks { path, .. }
        | ProjectCommand::SetRegisterRules { path, .. }
        | ProjectCommand::SetReports { path, .. }
        | ProjectCommand::Run { path } => path,
    };
    let path = project_path(path).map_err(|e| e.to_string())?;
    if command.is_read() {
        read(&path, &["qss"])?;
    } else {
        write(&path, &["qss"])?;
    }
    if let ProjectCommand::AddDrawing { drawing, .. } = command {
        read(Path::new(drawing), DRAWINGS)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{commands, output_files, state::AppState};
    use qiyas_core::Session;
    use tauri::{
        test::{mock_builder, mock_context, noop_assets, MockRuntime},
        Manager,
    };

    // A tower/podium workspace, with real files and a real open session. No OS viewer runs.
    pub(crate) fn tower_app(root: &Path) -> tauri::App<MockRuntime> {
        let drawing = root.join("tower-podium.dxf");
        let mut session = Session::new_blank(&drawing, 6);
        session
            .run_input(
                &serde_json::json!({
                    "type":"draw", "kind":"line", "a":[0,0], "b":[180,0]
                }),
                qiyas_core::Source::User,
            )
            .unwrap();
        session.save_as(&drawing).unwrap();
        let state = AppState::new(root.join("settings"));
        crate::tabs::add(&state, session).unwrap();
        mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    pub(crate) fn confirm_output(app: &tauri::App<MockRuntime>, path: &Path) {
        let state = app.state::<AppState>().scoped(None).unwrap();
        let provider = || Box::new(qiyas_ai::anthropic::Anthropic::new(String::new()));
        let mut saved =
            qiyas_ai::Agent::new(provider(), state.session.clone(), Default::default()).snapshot();
        saved.written_files.insert(path.canonicalize().unwrap());
        let agent =
            qiyas_ai::Agent::resume(saved, provider(), state.session.clone(), Default::default());
        lock(&state.ai).resume(agent);
    }

    #[test]
    fn export_table_refuses_dotfiles_and_writes_a_tower_workbook() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("x");
        std::fs::create_dir(&folder).unwrap();
        let export = |name: &str| {
            tauri::async_runtime::block_on(commands::export_table(
                folder.join(name).to_string_lossy().into_owned(),
                vec!["Tower podium".into()],
                vec![vec!["180".into()]],
            ))
        };
        assert!(export(".bashrc").unwrap_err().contains("Refused"));
        assert!(!folder.join(".bashrc").exists());
        export("tower-boq.xlsx").unwrap();
        assert!(folder.join("tower-boq.xlsx").is_file());
    }

    #[test]
    fn file_view_bytes_is_confined_to_open_drawing_folders() {
        let dir = tempfile::tempdir().unwrap();
        let tower = dir.path().join("tower");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&tower).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let app = tower_app(&tower);
        let inside = tower.join("quantities.json");
        let outside = elsewhere.join("auth.json");
        std::fs::write(&inside, b"{\"podium\":180}").unwrap();
        std::fs::write(&outside, b"{\"secret\":true}").unwrap();
        let read = |path: &Path| {
            tauri::async_runtime::block_on(output_files::file_view_bytes(
                path.to_string_lossy().into_owned(),
                app.state(),
            ))
        };
        assert!(read(&outside).is_err());
        assert!(read(&inside).is_err());
        confirm_output(&app, &inside);
        assert!(read(&inside).is_ok());
        assert!(read(&tower.join("../elsewhere/auth.json")).is_err());
        #[cfg(unix)]
        {
            let link = tower.join("linked.json");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(read(&link).is_err());
        }
    }

    #[test]
    fn macro_workbooks_are_not_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let app = tower_app(dir.path());
        for extension in ["xlsm", "xls", "xlsb", "XLSM"] {
            let file = dir.path().join(format!("tower-boq.{extension}"));
            std::fs::write(&file, b"workbook").unwrap();
            let path = file.to_string_lossy().into_owned();
            assert!(tauri::async_runtime::block_on(output_files::file_view_info(
                path.clone(),
                app.state()
            ))
            .is_err());
            assert!(
                tauri::async_runtime::block_on(output_files::open_output_file(
                    path.clone(),
                    app.state()
                ))
                .is_err()
            );
            assert!(
                tauri::async_runtime::block_on(output_files::reveal_output_file(path, app.state()))
                    .is_err()
            );
        }
    }

    #[test]
    fn claude_status_refuses_other_executables_before_running_them() {
        let error = tauri::async_runtime::block_on(commands::claude_code_status(Some(
            "/usr/bin/env".into(),
        )))
        .err()
        .unwrap();
        assert!(error.contains("claude"), "{error}");
    }

    #[test]
    fn block_write_policy_refuses_protected_folders() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        for folder in [
            ".ssh",
            ".config",
            ".codex",
            ".claude",
            ".cargo",
            ".local/bin",
        ] {
            let protected = home.join(folder);
            std::fs::create_dir_all(&protected).unwrap();
            let error = write_with_protected(
                &protected.join("x.dwg"),
                DRAWINGS,
                &protected_in_home(&home),
            )
            .unwrap_err();
            assert!(error.contains("protected"), "{error}");
            assert!(!protected.join("x.dwg").exists());
        }
    }

    #[test]
    fn writes_require_existing_parents_correct_extensions_and_plain_filenames() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["missing/tower.xlsx", "tower.xlsm", ".tower.xlsx"] {
            assert!(write(&dir.path().join(name), &["xlsx"]).is_err(), "{name}");
        }
        assert!(write(&dir.path().join("tower.XLSX"), &["xlsx"]).is_ok());
        #[cfg(unix)]
        {
            let target = dir.path().join(".bashrc");
            std::fs::write(&target, b"keep").unwrap();
            let link = dir.path().join("tower.xlsx");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(write(&link, &["xlsx"]).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"keep");
            let home = dir.path().join("home");
            let protected = home.join(".config");
            std::fs::create_dir_all(&protected).unwrap();
            let alias = dir.path().join("alias");
            std::os::unix::fs::symlink(&protected, &alias).unwrap();
            assert!(write_with_protected(
                &alias.join("tower.dwg"),
                DRAWINGS,
                &protected_in_home(&home)
            )
            .is_err());
        }
    }

    #[test]
    fn output_roots_include_other_tabs_export_folders_and_project_drawings() {
        let dir = tempfile::tempdir().unwrap();
        let tower = dir.path().join("tower");
        let podium = dir.path().join("podium");
        let exports = dir.path().join("exports");
        for folder in [&tower, &podium, &exports] {
            std::fs::create_dir(folder).unwrap();
        }
        let app = tower_app(&tower);
        let state = app.state::<AppState>();
        let mut untitled = Session::new_blank(&tower.join("untitled.dwg"), 6);
        untitled.mark_untitled();
        untitled.set_export_directory(exports.clone());
        crate::tabs::add(&state, untitled).unwrap();
        let podium_drawing = podium.join("podium.dxf");
        Session::new_blank(&podium_drawing, 6)
            .save_as(&podium_drawing)
            .unwrap();
        let project = dir.path().join("tower-project.qss");
        let set: qiyas_core::SheetSet = serde_json::from_value(serde_json::json!({
            "name":"Tower and podium", "project":{},
            "drawings":[{"path":"podium/podium.dxf", "layouts":[]}]
        }))
        .unwrap();
        set.save(&project).unwrap();
        lock(&state.projects).active = Some(project);
        for folder in [&tower, &podium, &exports] {
            let path = folder.join("quantities.json");
            std::fs::write(&path, b"{}").unwrap();
            assert!(output(&path, &output_roots(&state).unwrap()).is_ok());
        }
    }

    #[test]
    fn recovery_paths_cannot_disguise_unrelated_files_as_autosaves() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["auth.json", "auth.json.sv$.123", ".tower.dwg"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"keep").unwrap();
            assert!(recovery(&path).is_err(), "{name}");
        }
        for name in ["tower.dwg", "podium.dxf.sv$.123", "tower.dwg.sv$.crash-123"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"drawing").unwrap();
            assert!(recovery(&path).is_ok(), "{name}");
        }
        assert!(recovery(&dir.path().join("unsaved-tower.dwg")).is_ok());
    }

    #[test]
    fn an_open_tower_output_remains_readable_while_its_session_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let app = tower_app(dir.path());
        let state = app.state::<AppState>();
        let bound = state.scoped(None).unwrap();
        let _busy = bound.session.lock().unwrap();
        let path = dir.path().join("podium.json");
        std::fs::write(&path, b"{}").unwrap();
        assert!(output(&path, &output_roots(&state).unwrap()).is_ok());
    }

    #[test]
    fn write_block_command_refuses_a_protected_destination_before_accessing_the_block() {
        let dir = tempfile::tempdir().unwrap();
        let app = tower_app(dir.path());
        // Use an existing OS settings folder without creating or touching anything there.
        let folder = protected_folders()
            .into_iter()
            .find(|p| p.is_dir())
            .unwrap();
        let path = folder.join(format!("qiyas-test-{}.dwg", uuid::Uuid::new_v4()));
        let error = tauri::async_runtime::block_on(crate::blocks::write_block(
            None,
            "missing-block".into(),
            path.to_string_lossy().into_owned(),
            app.state(),
        ))
        .unwrap_err();
        assert!(error.contains("protected"), "{error}");
        assert!(!path.exists());
    }

    #[test]
    fn nested_command_paths_get_the_same_write_and_read_checks() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::new_blank(&dir.path().join("tower.dxf"), 6);
        let block = |path: &str| qiyas_core::Command::WriteBlock {
            name: "podium".into(),
            path: path.into(),
        };
        assert!(command(&block("podium.dwg"), &session).is_ok());
        assert!(command(&block(".podium.dwg"), &session).is_err());
        let json = dir.path().join("auth.json");
        std::fs::write(&json, b"{}").unwrap();
        let read = qiyas_core::Command::CompareRevisions {
            other_path: json.to_string_lossy().into_owned(),
            tolerance: None,
        };
        assert!(command(&read, &session).is_err());
    }

    #[test]
    fn project_outputs_check_the_generated_workbook_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let qss = dir.path().join("tower.qss");
        std::fs::write(&qss, b"{}").unwrap();
        let folder = project_output_folder(&qss).unwrap();
        assert_eq!(
            folder,
            dir.path().canonicalize().unwrap().join("tower.output")
        );
        #[cfg(unix)]
        {
            let dotfile = dir.path().join(".bashrc");
            std::fs::write(&dotfile, b"keep").unwrap();
            std::os::unix::fs::symlink(&dotfile, folder.join("project.xlsx")).unwrap();
            assert!(project_output_folder(&qss).is_err());
            assert_eq!(std::fs::read(dotfile).unwrap(), b"keep");
        }
    }

    #[test]
    fn a_project_entry_for_a_non_drawing_does_not_grant_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let tower = dir.path().join("tower");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&tower).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        let app = tower_app(&tower);
        let auth = elsewhere.join("auth.json");
        std::fs::write(&auth, b"{}").unwrap();
        // Real project location, with an isolated name; no process-wide HOME override.
        let project = qiyas_core::data_home::qiyas_dir()
            .unwrap()
            .join("projects")
            .join(format!("path-test-{}.qss", uuid::Uuid::new_v4()));
        let set: qiyas_core::SheetSet = serde_json::from_value(serde_json::json!({
            "name":"Tower", "project":{},
            "drawings":[{"path":auth.to_string_lossy(), "layouts":[]}]
        }))
        .unwrap();
        set.save(&project).unwrap();
        lock(&app.state::<AppState>().projects).active = Some(project.clone());
        let roots = output_roots(&app.state::<AppState>()).unwrap();
        std::fs::remove_file(&project).unwrap();
        assert!(output(&auth, &roots).is_err());
    }
}
