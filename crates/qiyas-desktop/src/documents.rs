//! Composer attachments live in the Qiyas data home, separate from the drawing and settings.
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Attachment {
    pub name: String,
    pub path: String,
    pub pages: u32,
}
fn store(root: &Path, name: String, bytes: Vec<u8>) -> Result<Attachment, String> {
    if bytes.len() > 20 * 1024 * 1024
        || !bytes.starts_with(b"%PDF-")
        || name.len() > 255
        || !name.to_lowercase().ends_with(".pdf")
    {
        return Err("Attach a PDF of at most 20 MiB".into());
    }
    let folder = root.join("attachments");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!("{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    match qiyas_pdf::page_count(&path) {
        Ok(pages) => Ok(Attachment {
            name,
            path: path.to_string_lossy().into_owned(),
            pages,
        }),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e.to_string())
        }
    }
}
#[tauri::command]
pub async fn ai_attach_pdf(name: String, bytes: Vec<u8>) -> Result<Attachment, String> {
    tauri::async_runtime::spawn_blocking(move || store(&crate::methods::qiyas_dir()?, name, bytes))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_non_pdf_before_creating_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(store(dir.path(), "notes.pdf".into(), b"not a pdf".to_vec()).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn pdf_name_is_a_label_not_a_filesystem_path() {
        if !qiyas_pdf::available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let pdf = qiyas_pdf::fixtures::write_wall(dir.path());
        let attachment = store(
            dir.path(),
            "../../drawing.pdf".into(),
            std::fs::read(pdf).unwrap(),
        )
        .unwrap();
        assert!(Path::new(&attachment.path).starts_with(dir.path().join("attachments")));
        assert_eq!(attachment.pages, 1);
    }
}
