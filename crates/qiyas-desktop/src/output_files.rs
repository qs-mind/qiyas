//! User-initiated viewing of generated PDFs and workbooks. No URLs or shell command strings.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn output_path(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("An output needs an absolute file path".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !path.is_file()
        || !["pdf", "xlsx"]
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext))
    {
        return Err("Only PDF and workbook output files can be opened".into());
    }
    Ok(path)
}

fn viewer(path: &Path, reveal: bool) -> Command {
    #[cfg(target_os = "linux")]
    {
        let mut command = Command::new("xdg-open");
        command.arg(if reveal {
            path.parent().unwrap_or(path)
        } else {
            path
        });
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        if reveal {
            command.arg("-R");
        }
        command.arg(path);
        command
    }
    #[cfg(target_os = "windows")]
    {
        if reveal {
            let mut command = Command::new("explorer.exe");
            command.arg(path.parent().unwrap_or(path));
            command
        } else {
            let mut command = Command::new("rundll32.exe");
            command.arg("url.dll,FileProtocolHandler").arg(path);
            command
        }
    }
}

async fn show(path: String, reveal: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = output_path(&path)?;
        let status = viewer(&path, reveal)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("The system viewer could not open this output".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_output_file(path: String) -> Result<(), String> {
    show(path, false).await
}

#[tauri::command]
pub async fn reveal_output_file(path: String) -> Result<(), String> {
    show(path, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_output_files_reach_the_viewer() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["sheet.PDF", "bill.xlsx"] {
            let file = dir.path().join(name);
            std::fs::write(&file, b"test").unwrap();
            assert_eq!(
                output_path(file.to_str().unwrap()).unwrap(),
                file.canonicalize().unwrap()
            );
        }
        let other = dir.path().join("run.sh");
        std::fs::write(&other, b"test").unwrap();
        assert!(output_path(other.to_str().unwrap()).is_err());
        assert!(output_path(dir.path().to_str().unwrap()).is_err());
        assert!(output_path(dir.path().join("missing.pdf").to_str().unwrap()).is_err());
        assert!(output_path("samples/plan.pdf").is_err());
        assert!(output_path("https://example.test/plan.pdf").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn viewer_treats_paths_as_one_argument_and_reveals_the_parent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sheet with spaces.pdf");
        let open = viewer(&path, false);
        assert_eq!(open.get_program(), "xdg-open");
        assert_eq!(open.get_args().collect::<Vec<_>>(), vec![path.as_os_str()]);
        let reveal = viewer(&path, true);
        assert_eq!(
            reveal.get_args().collect::<Vec<_>>(),
            vec![dir.path().as_os_str()]
        );
    }
}
