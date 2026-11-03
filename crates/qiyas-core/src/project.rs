//! The `.qiyas` project file: the take-off, the BOQ, the conflicts and the schedules, saved as
//! JSON next to the drawing.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Boq, Conflict, CoreError, Current, Drafting, Schedule, Takeoff, Ucs};

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    /// 2 since volumes, items and BOQ links; version-1 files load too (their measurements
    /// have no height or BOQ code).
    pub version: u32,
    /// The drawing's file name, without its folder.
    pub drawing: String,
    pub takeoff: Takeoff,
    /// The imported BOQ. Files saved before BOQ import have none, and load too.
    #[serde(default)]
    pub boq: Option<Boq>,
    /// The conflicts of the last comparison. Files saved before the comparison have none.
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
    /// The schedules read off the sheets at the last comparison.
    #[serde(default)]
    pub schedules: Vec<Schedule>,
    /// The drawing holds its Arabic text backwards (`Session::arabic_reversed`).
    #[serde(default)]
    pub arabic_reversed: bool,
    /// Snaps, ortho, polar, grid and display precision (`Session::drafting`).
    #[serde(default)]
    pub drafting: Drafting,
    /// The file the user chose for an xref, by xref name (`Session::xref_overrides`).
    #[serde(default)]
    pub xref_overrides: HashMap<String, String>,
    /// The layer, colour, linetype and lineweight new entities take (`Session::current`).
    #[serde(default)]
    pub current: Current,
    /// The layers the user had hidden when each layer state was saved, by state name: a layer
    /// state keeps the drawing's layer settings in the drawing, and Qiyas's hidden layers here.
    #[serde(default)]
    pub qiyas_hidden: BTreeMap<String, Vec<String>>,
    /// The user coordinate system (`Session::ucs`); `None` in files saved before it, which then
    /// take the drawing's own.
    #[serde(default)]
    pub ucs: Option<Ucs>,
}

/// The project file of `drawing`: same folder, same stem, extension `qiyas`.
pub fn project_path(drawing: &Path) -> PathBuf {
    drawing.with_extension("qiyas")
}

/// The file name of `path`, or "" when it has none.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads the project file at `path`.
pub(crate) fn load(path: &Path) -> Result<Project, CoreError> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| CoreError::Project(e.to_string()))
}

/// Writes `project` to `path` as pretty JSON, atomically (see `write_atomic`).
pub(crate) fn save(path: &Path, project: &Project) -> Result<(), CoreError> {
    let text =
        serde_json::to_string_pretty(project).map_err(|e| CoreError::Project(e.to_string()))?;
    Ok(write_atomic(path, text.as_bytes())?)
}

/// Writes `bytes` to `<path>.tmp`, synced to disk, then renames it over `path`, so a crash
/// mid-write never leaves a half-written file. A file already at `path` keeps its permissions.
/// On failure the temp file is removed.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, bytes, false)
}

/// Like `write_atomic`, but first copies a file already at `path` to `<path>.bak`. A copy, not a
/// rename, so the original stays in place if the rename fails.
pub(crate) fn write_with_backup(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, bytes, true)
}

fn replace(path: &Path, bytes: &[u8], backup: bool) -> std::io::Result<()> {
    let tmp = with_suffix(path, ".tmp");
    let written = write_synced(&tmp, bytes).and_then(|()| {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.is_file() {
                std::fs::set_permissions(&tmp, meta.permissions())?;
                if backup {
                    std::fs::copy(path, with_suffix(path, ".bak"))?;
                }
            }
        }
        std::fs::rename(&tmp, path)
    });
    if written.is_err() {
        // Best effort: the error that matters is the write or rename one.
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Copies the unreadable project file at `path` to `<path>.bak`; returns the backup's file name.
pub(crate) fn back_up(path: &Path) -> std::io::Result<String> {
    let bak = with_suffix(path, ".bak");
    std::fs::copy(path, &bak)?;
    Ok(file_name(&bak))
}

/// `path` with `suffix` added after its extension, e.g. `terminal.dwg` → `terminal.dwg.bak`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_save_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        // A non-empty folder where the project should go: the rename fails.
        let path = dir.path().join("terminal.qiyas");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), "").unwrap();
        let project = Project {
            version: 1,
            drawing: "terminal.dwg".to_string(),
            takeoff: Takeoff::default(),
            boq: None,
            conflicts: Vec::new(),
            schedules: Vec::new(),
            arabic_reversed: false,
            drafting: Drafting::default(),
            xref_overrides: HashMap::new(),
            current: Current::default(),
            qiyas_hidden: Default::default(),
            ucs: None,
        };
        assert!(save(&path, &project).is_err());
        assert!(!dir.path().join("terminal.qiyas.tmp").exists());
    }
}
