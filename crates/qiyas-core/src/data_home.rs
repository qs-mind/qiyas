//! The data home shared by the window and headless project commands.
use std::path::{Path, PathBuf};

pub fn home_path(override_path: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = override_path.filter(|p| !p.as_os_str().is_empty()) {
        return Ok(path);
    }
    if cfg!(debug_assertions) {
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".local/qiyas-home"));
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|home| PathBuf::from(home).join(".qiyas"))
        .ok_or_else(|| "The Qiyas data home could not be found; set QIYAS_HOME".into())
}
pub fn qiyas_dir() -> Result<PathBuf, String> {
    let dir = home_path(std::env::var_os("QIYAS_HOME").map(PathBuf::from))?;
    for child in ["methods", "projects"] {
        std::fs::create_dir_all(dir.join(child)).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}
/// Canonical directories prevent a sibling or symlink from gaining project path privileges.
pub fn is_project_path(path: &Path) -> bool {
    let Ok(home) = home_path(std::env::var_os("QIYAS_HOME").map(PathBuf::from)) else {
        return false;
    };
    let Ok(projects) = home.join("projects").canonicalize() else {
        return false;
    };
    let candidate = if path.symlink_metadata().is_ok() {
        path.canonicalize().ok()
    } else {
        path.parent()
            .and_then(|p| p.canonicalize().ok())
            .zip(path.file_name())
            .map(|(p, n)| p.join(n))
    };
    candidate.is_some_and(|p| {
        p.parent() == Some(projects.as_path()) && p.extension().is_some_and(|e| e == "qss")
    })
}
