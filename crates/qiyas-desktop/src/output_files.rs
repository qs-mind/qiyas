//! Read-only previews and system viewers for output files. No URLs or shell command strings.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use tauri::ipc::Response;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileViewKind {
    Pdf,
    Workbook,
    Image,
    Text,
}

#[derive(Debug, Serialize)]
pub struct FileViewInfo {
    pub kind: FileViewKind,
    pub name: String,
    pub size_bytes: u64,
    pub pages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<qiyas_plot::report::PdfTrace>,
}

fn output_kind(path: &Path) -> Result<FileViewKind, String> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match extension.to_ascii_lowercase().as_str() {
        "pdf" => Ok(FileViewKind::Pdf),
        "xlsx" | "xlsm" | "xls" => Ok(FileViewKind::Workbook),
        "png" | "jpg" | "jpeg" | "webp" | "gif" => Ok(FileViewKind::Image),
        "csv" | "txt" | "md" | "json" => Ok(FileViewKind::Text),
        _ => Err("This file type cannot be viewed. لا يمكن عرض هذا النوع من الملفات.".into()),
    }
}

fn output_path(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("Use an absolute file path. استخدم مسار ملف مطلقًا.".into());
    }
    let path = path.canonicalize().map_err(|_| {
        "The file could not be found or accessed. تعذر العثور على الملف أو الوصول إليه.".to_string()
    })?;
    if !path.is_file() {
        return Err("Choose a regular file. اختر ملفًا عاديًا.".into());
    }
    output_kind(&path)?;
    Ok(path)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| {
            "Reading the file stopped unexpectedly. توقفت قراءة الملف بشكل غير متوقع.".to_string()
        })?
}

/// The output's type, name, byte size and PDF page count.
#[tauri::command]
pub async fn file_view_info(path: String) -> Result<FileViewInfo, String> {
    blocking(move || {
        let path = output_path(&path)?;
        let kind = output_kind(&path)?;
        let size_bytes = path
            .metadata()
            .map_err(|_| {
                "The file details could not be read. تعذرت قراءة تفاصيل الملف.".to_string()
            })?
            .len();
        let pages = if kind == FileViewKind::Pdf {
            Some(qiyas_pdf::page_count(&path).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(FileViewInfo {
            kind,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            size_bytes,
            pages,
            trace: if kind == FileViewKind::Pdf {
                qiyas_plot::report::read_trace(&path)
            } else {
                None
            },
        })
    })
    .await
}

/// Page numbers start at 1. Bytes match `underlay_image`: width, height, then top-down RGBA.
#[tauri::command]
pub async fn file_view_pdf_page(
    path: String,
    page: u32,
    width_px: u32,
) -> Result<Response, String> {
    blocking(move || {
        let path = output_path(&path)?;
        if output_kind(&path)? != FileViewKind::Pdf {
            return Err("Choose a PDF file. اختر ملف PDF.".into());
        }
        let (width_pt, _) = qiyas_pdf::page_size(&path, page).map_err(|e| e.to_string())?;
        if !width_pt.is_finite() || width_pt <= 0.0 {
            return Err("The PDF page width is invalid. عرض صفحة PDF غير صالح.".into());
        }
        let dpi = f64::from(width_px.clamp(200, 2400)) * 72.0 / width_pt;
        let image = qiyas_pdf::render_page(&path, page, dpi, |_| {}).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(8 + image.as_raw().len());
        out.extend(image.width().to_le_bytes());
        out.extend(image.height().to_le_bytes());
        out.extend(image.as_raw());
        Ok(Response::new(out))
    })
    .await
}

const IMAGE_LIMIT: u64 = 25 * 1024 * 1024;
const TEXT_LIMIT: u64 = 2 * 1024 * 1024;

/// Every workbook sheet, with at most 5000 rows and 80 columns per sheet.
#[tauri::command]
pub async fn file_view_workbook(path: String) -> Result<serde_json::Value, String> {
    blocking(move || {
        let path = output_path(&path)?;
        if output_kind(&path)? != FileViewKind::Workbook {
            return Err("Choose an Excel workbook. اختر مصنف Excel.".into());
        }
        qiyas_boq::preview::preview_workbook(&path, 5000, 80)
            .map(|preview| preview.into_json())
            .map_err(|e| format!("The workbook could not be read. تعذرت قراءة المصنف. {e}"))
    })
    .await
}

fn read_view_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let (limit, too_large) = match output_kind(path)? {
        FileViewKind::Image => (
            IMAGE_LIMIT,
            "The image exceeds the 25 MB preview limit. تتجاوز الصورة حد العرض البالغ 25 ميغابايت.",
        ),
        FileViewKind::Text => (
            TEXT_LIMIT,
            "The text exceeds the 2 MB preview limit. يتجاوز النص حد العرض البالغ 2 ميغابايت.",
        ),
        _ => return Err("Only image and text files can be read this way. يمكن قراءة الصور والملفات النصية فقط بهذه الطريقة.".into()),
    };
    let read_error = || "The file could not be read. تعذرت قراءة الملف.".to_string();
    let file = std::fs::File::open(path).map_err(|_| read_error())?;
    if file.metadata().map_err(|_| read_error())?.len() > limit {
        return Err(too_large.into());
    }
    // Bound the read even if the file grows after its size was checked.
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| read_error())?;
    if bytes.len() as u64 > limit {
        return Err(too_large.into());
    }
    Ok(bytes)
}

/// Raw image or text bytes, within the preview size limit.
#[tauri::command]
pub async fn file_view_bytes(path: String) -> Result<Response, String> {
    blocking(move || {
        let path = output_path(&path)?;
        read_view_bytes(&path).map(Response::new)
    })
    .await
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
    fn all_preview_extensions_are_allowed_in_any_case() {
        let dir = tempfile::tempdir().unwrap();
        for (extension, kind) in [
            ("pdf", FileViewKind::Pdf),
            ("xlsx", FileViewKind::Workbook),
            ("xlsm", FileViewKind::Workbook),
            ("xls", FileViewKind::Workbook),
            ("csv", FileViewKind::Text),
            ("txt", FileViewKind::Text),
            ("md", FileViewKind::Text),
            ("json", FileViewKind::Text),
            ("png", FileViewKind::Image),
            ("jpg", FileViewKind::Image),
            ("jpeg", FileViewKind::Image),
            ("webp", FileViewKind::Image),
            ("gif", FileViewKind::Image),
        ] {
            for extension in [extension.to_string(), extension.to_ascii_uppercase()] {
                let file = dir.path().join(format!("output.{extension}"));
                std::fs::write(&file, b"test").unwrap();
                let path = output_path(file.to_str().unwrap()).unwrap();
                assert_eq!(path, file.canonicalize().unwrap());
                assert_eq!(output_kind(&path).unwrap(), kind);
            }
        }
        let directory = dir.path().join("directory.pdf");
        std::fs::create_dir(&directory).unwrap();
        assert!(output_path(directory.to_str().unwrap()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn path_validation_checks_the_canonical_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"text").unwrap();
        let link = dir.path().join("link.pdf");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            output_path(link.to_str().unwrap()).unwrap(),
            target.canonicalize().unwrap()
        );
        let other = dir.path().join("other.sh");
        std::fs::write(&other, b"test").unwrap();
        let disguised = dir.path().join("disguised.txt");
        std::os::unix::fs::symlink(&other, &disguised).unwrap();
        assert!(output_path(disguised.to_str().unwrap()).is_err());
    }

    #[test]
    fn byte_reader_enforces_kind_and_size_limits() {
        let dir = tempfile::tempdir().unwrap();
        for extension in ["pdf", "xlsx", "xlsm", "xls"] {
            let path = dir.path().join(format!("output.{extension}"));
            std::fs::write(&path, b"test").unwrap();
            assert!(read_view_bytes(&path).is_err());
        }
        for (extension, limit) in [("png", IMAGE_LIMIT), ("txt", TEXT_LIMIT)] {
            let path = dir.path().join(format!("output.{extension}"));
            let file = std::fs::File::create(&path).unwrap();
            file.set_len(limit).unwrap();
            assert_eq!(read_view_bytes(&path).unwrap().len() as u64, limit);
            file.set_len(limit + 1).unwrap();
            assert!(read_view_bytes(&path).is_err());
        }
    }

    #[test]
    fn preview_commands_return_snake_case_info_and_raw_text() {
        use tauri::ipc::{InvokeResponseBody, IpcResponse};

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("الكمية.CSV");
        let content = "البند,الكمية\nجدار,2".as_bytes();
        std::fs::write(&file, content).unwrap();
        let path = file.to_str().unwrap().to_string();
        let info = tauri::async_runtime::block_on(file_view_info(path.clone())).unwrap();
        assert_eq!(
            serde_json::to_value(info).unwrap(),
            serde_json::json!({
                "kind": "text", "name": "الكمية.CSV", "size_bytes": content.len(), "pages": null,
            })
        );
        let response = tauri::async_runtime::block_on(file_view_bytes(path.clone())).unwrap();
        let InvokeResponseBody::Raw(bytes) = response.body().unwrap() else {
            panic!("expected raw bytes");
        };
        assert_eq!(bytes, content);
        assert!(tauri::async_runtime::block_on(file_view_workbook(path.clone())).is_err());
        assert!(tauri::async_runtime::block_on(file_view_pdf_page(path, 1, 800)).is_err());
    }

    #[test]
    fn pdf_preview_uses_the_underlay_byte_layout_and_clamps_width() {
        use tauri::ipc::{InvokeResponseBody, IpcResponse};

        if !qiyas_pdf::available() {
            eprintln!("skipped: needs pdfium (scripts/pdfium.sh)");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = qiyas_pdf::fixtures::write_wall(dir.path());
        let path = path.to_str().unwrap().to_string();
        let info = tauri::async_runtime::block_on(file_view_info(path.clone())).unwrap();
        assert_eq!(info.pages, Some(1));
        assert_eq!(info.kind, FileViewKind::Pdf);
        for (requested, width) in [(1, 200u32), (400, 400), (3000, 2400)] {
            let response =
                tauri::async_runtime::block_on(file_view_pdf_page(path.clone(), 1, requested))
                    .unwrap();
            let InvokeResponseBody::Raw(bytes) = response.body().unwrap() else {
                panic!("expected raw RGBA bytes");
            };
            let actual_width = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
            let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            assert_eq!(actual_width, width);
            assert_eq!(height, width / 2);
            assert_eq!(bytes.len(), 8 + width as usize * height as usize * 4);
            let image = qiyas_pdf::render_page(
                Path::new(&path),
                1,
                f64::from(width) * 72.0 / qiyas_pdf::fixtures::PAGE_PT.0,
                |_| {},
            )
            .unwrap();
            assert_eq!(&bytes[8..], image.as_raw().as_slice());
        }
        for page in [0, 2] {
            assert!(
                tauri::async_runtime::block_on(file_view_pdf_page(path.clone(), page, 400))
                    .is_err()
            );
        }
    }

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

#[cfg(test)]
mod trace_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pdf_file_info_returns_the_validated_trace_map() {
        if !qiyas_pdf::available() {
            eprintln!("skipped: needs pdfium (scripts/pdfium.sh)");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.pdf");
        let report: qiyas_core::deliverables::Report = serde_json::from_value(json!({
            "drawing": "/plans/source.dxf", "kind":"boq", "language":"en",
            "layout": qiyas_core::deliverables::default_layout(qiyas_core::deliverables::Kind::Boq),
            "warnings": [],
            "rows": [{"code":"W","section":"Walls","description":"Wall","unit":"m2","quantity":12.5,
                "method":"Measured","citations":[],"target":{"item":"generated:W"}}],
        }))
        .unwrap();
        qiyas_plot::report::write(&path, &report).unwrap();
        let info =
            tauri::async_runtime::block_on(file_view_info(path.to_string_lossy().into_owned()))
                .unwrap();
        let trace = info.trace.unwrap();
        assert_eq!(trace.drawing.as_deref(), Some("/plans/source.dxf"));
        let targets: Vec<_> = trace.pages.iter().flat_map(|page| &page.regions).collect();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].target.item, "generated:W");
        assert!(targets[0].target.line.is_none());
    }
}
