//! IPC for PDFs and raster images: page count and size, thumbnails, attaching a page (underlay or
//! import) or an image, calibrating either, an image's brightness, contrast and fade, and the
//! pictures the window draws them with. pdfium runs on its own thread
//! (`qiyas_pdf`); reading a PDF here never holds the session lock: a page is read first, then the
//! session is locked briefly to add it. Without pdfium every command answers "PDF support is not installed: run
//! scripts/pdfium.sh".

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use qiyas_core::{AttachMode, CoreError, ImageDisplay, PreparedPdf};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::commands::with_session;
use crate::state::AppState;

/// Event with the percent drawn of the underlay picture being rendered.
pub const PROGRESS: &str = "pdf:progress";
/// Pictures kept, most recently used last.
const KEPT: usize = 4;
/// Thumbnails asked for at once, at most.
const MAX_THUMBNAILS: u32 = 40;

/// The last `KEPT` underlay pictures used, by drawing, image id, version and resolution, so
/// zooming back and forth does not draw a page again; the least recently used goes first. Only
/// pictures of the drawing on screen (`epoch`, its session's) are kept: image ids are per
/// drawing, and a picture still being drawn when another tab came up must not land under them.
#[derive(Default)]
pub struct PictureCache {
    entries: VecDeque<(PictureKey, Arc<Vec<u8>>)>,
    epoch: u64,
}

impl PictureCache {
    /// The picture under `key`, now the most recently used.
    fn get(&mut self, key: PictureKey) -> Option<Arc<Vec<u8>>> {
        let at = self.entries.iter().position(|(k, _)| *k == key)?;
        let entry = self.entries.remove(at)?;
        let bytes = entry.1.clone();
        self.entries.push_back(entry);
        Some(bytes)
    }

    /// The session with `epoch` is on screen now (none: 0); another one's pictures go.
    pub fn show(&mut self, epoch: u64) {
        if epoch != self.epoch {
            self.entries.clear();
            self.epoch = epoch;
        }
    }

    /// Keeps `bytes` under `key`, letting go of the least recently used beyond `KEPT`; a picture
    /// of a drawing no longer on screen is dropped.
    fn put(&mut self, key: PictureKey, bytes: Arc<Vec<u8>>) {
        if key.0 != self.epoch {
            return;
        }
        self.entries.retain(|(k, _)| *k != key);
        while self.entries.len() >= KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back((key, bytes));
    }

    #[cfg(test)]
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}

/// Session epoch, image id, version, and the resolution (`UnderlayImage::resolution`).
type PictureKey = (u64, u32, u64, u64);

/// Where the bundled pdfium is: `resources/pdfium/` in the app's resources (next to the program
/// on Windows), or the crate's own `resources/pdfium/` in a debug build.
pub fn use_bundled_pdfium<R: Runtime>(app: &AppHandle<R>) {
    let dir = if cfg!(debug_assertions) {
        Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/pdfium"))
    } else {
        app.path()
            .resource_dir()
            .ok()
            .map(|r| r.join("resources/pdfium"))
    };
    if let Some(dir) = dir {
        qiyas_pdf::set_library_dir(dir);
    }
}

/// Runs blocking PDF work off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "Reading the PDF stopped unexpectedly".to_string())?
}

/// How many pages the PDF at `path` has.
#[tauri::command]
pub async fn pdf_page_count(path: String) -> Result<u32, String> {
    blocking(move || qiyas_pdf::page_count(Path::new(&path)).map_err(|e| e.to_string())).await
}

/// Width and height in points of page `page` (from 1), as shown.
#[tauri::command]
pub async fn pdf_page_size(path: String, page: u32) -> Result<(f64, f64), String> {
    blocking(move || qiyas_pdf::page_size(Path::new(&path), page).map_err(|e| e.to_string())).await
}

/// Pages `first` to `first + count - 1` (from 1, cut at the last page, 40 at most) drawn to fit
/// `px` x `px`. Little-endian: `u32` n, then per page `u32` page number, `u32` width, `u32`
/// height and width x height x 4 RGBA bytes, rows from the top.
#[tauri::command]
pub async fn pdf_thumbnails(
    path: String,
    first: u32,
    count: u32,
    px: u32,
) -> Result<Response, String> {
    blocking(move || {
        let path = PathBuf::from(path);
        let pages = qiyas_pdf::page_count(&path).map_err(|e| e.to_string())?;
        let first = first.max(1);
        let last = (first + count.min(MAX_THUMBNAILS)).min(pages + 1);
        let numbers: Vec<u32> = (first..last).collect();
        let images = qiyas_pdf::thumbnails(&path, &numbers, px).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        out.extend((images.len() as u32).to_le_bytes());
        for (page, image) in numbers.iter().zip(&images) {
            for v in [*page, image.width(), image.height()] {
                out.extend(v.to_le_bytes());
            }
            out.extend(image.as_raw());
        }
        Ok(Response::new(out))
    })
    .await
}

/// Attaches page `page` of the PDF at `path` to the sheet on screen at (`x`, `y`), `scale`
/// drawing units per point: `mode` "underlay" (returns the underlay's handle) or "import"
/// (returns how many entities were added). One undo step.
// Keep the existing IPC fields compatible while adding optional workspace addressing.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn attach_pdf(
    workspace: Option<u32>,

    path: String,
    page: u32,
    x: f64,
    y: f64,
    scale: f64,
    mode: AttachMode,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let state = state.scoped(workspace)?;
    // The slow part, reading the page, without the session.
    let prepared = blocking(move || {
        PreparedPdf::read(Path::new(&path), page, scale, mode).map_err(|e| e.to_string())
    })
    .await?;
    let handles = with_session(&state, |s| s.apply_pdf(prepared, [x, y]))?;
    Ok(match mode {
        AttachMode::Underlay => handles.first().copied().unwrap_or(0),
        AttachMode::Import => handles.len() as u64,
    })
}

/// True when the drawing itself has changes not saved yet (PDF imports, underlays, calibration).
#[tauri::command]
pub async fn is_dirty(workspace: Option<u32>, state: State<'_, AppState>) -> Result<bool, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.is_dirty()))
}

/// Rescales an underlay or image about the first point so that the two points are `distance`
/// drawing units apart. `handle` left out: the underlay or image under the first point. One undo
/// step.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn calibrate(
    workspace: Option<u32>,
    handle: Option<u64>,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    distance: f64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| {
        let handle = match handle {
            Some(h) => h,
            None => s.underlay_at([x1, y1]).ok_or_else(|| {
                CoreError::InvalidCommand(
                    "Pick the first point on a PDF underlay or an image".to_string(),
                )
            })?,
        };
        s.calibrate(handle, [x1, y1], [x2, y2], distance)
    })
}

/// The picture of image `id` (from the batch's image table: a PDF underlay or a raster image) for
/// a zoom of `px_per_unit` screen pixels per drawing unit: `u32` width, `u32` height, then width
/// x height x 4 RGBA bytes, rows from the top. Sends `pdf:progress` (0 to 100) while pdfium draws
/// a PDF page.
#[tauri::command]
pub async fn underlay_image<R: Runtime>(
    workspace: Option<u32>,

    id: u32,
    px_per_unit: f64,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let state = state.scoped(workspace)?;
    let (picture, epoch) = with_session(&state, |s| {
        Ok((s.underlay_image(id, px_per_unit)?, s.epoch()))
    })?;
    let key = (epoch, id, picture.version, picture.resolution());
    if let Some(bytes) = pictures(&state).get(key) {
        return Ok(Response::new(bytes.as_ref().clone()));
    }
    let emitter = app.clone();
    let workspace = state.id;
    let bytes = blocking(move || {
        let image = qiyas_core::draw_picture(&picture, |p| {
            let _ = emitter.emit(
                PROGRESS,
                serde_json::json!({"workspace":workspace,"progress":p}),
            );
        })
        .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(8 + image.as_raw().len());
        out.extend(image.width().to_le_bytes());
        out.extend(image.height().to_le_bytes());
        out.extend(image.as_raw());
        Ok(out)
    })
    .await?;
    let bytes = Arc::new(bytes);
    pictures(&state).put(key, bytes.clone());
    Ok(Response::new(bytes.as_ref().clone()))
}

/// Width and height in pixels of the raster image at `path`, as shown (EXIF orientation applied).
#[tauri::command]
pub async fn image_size(path: String) -> Result<(u32, u32), String> {
    blocking(move || qiyas_core::image_size(Path::new(&path)).map_err(|e| e.to_string())).await
}

/// Attaches the raster image at `path` to the sheet on screen, bottom-left corner at (`x`, `y`),
/// `width` drawing units wide. Its handle. One undo step.
#[tauri::command]
pub async fn attach_image(
    workspace: Option<u32>,

    path: String,
    x: f64,
    y: f64,
    width: f64,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| s.attach_image(Path::new(&path), [x, y], width))
}

/// The brightness, contrast and fade of the raster image `handle`; null when it is not one.
#[tauri::command]
pub async fn image_display(
    workspace: Option<u32>,

    handle: u64,
    state: State<'_, AppState>,
) -> Result<Option<ImageDisplay>, String> {
    let state = state.scoped(workspace)?;
    with_session(&state, |s| Ok(s.image_display(handle)))
}

pub(crate) fn pictures(state: &AppState) -> std::sync::MutexGuard<'_, PictureCache> {
    state
        .pictures
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_keeps_the_four_most_recently_used_pictures() {
        let mut cache = PictureCache::default();
        cache.show(7);
        let key = |id| (7, id, 1, 0);
        for id in 0..4 {
            cache.put(key(id), Arc::new(vec![id as u8]));
        }
        // Using picture 0 makes picture 1 the oldest.
        assert_eq!(cache.get(key(0)).as_deref(), Some(&vec![0]));
        cache.put(key(4), Arc::new(vec![4]));
        assert!(cache.get(key(1)).is_none());
        for id in [0, 2, 3, 4] {
            assert!(cache.get(key(id)).is_some(), "{id}");
        }
        assert_eq!(cache.entries.len(), KEPT);
        // Putting a key again replaces it.
        cache.put(key(4), Arc::new(vec![9]));
        assert_eq!(cache.get(key(4)).as_deref(), Some(&vec![9]));
        assert_eq!(cache.entries.len(), KEPT);
    }

    #[test]
    fn a_picture_of_a_drawing_no_longer_shown_is_not_kept() {
        let mut cache = PictureCache::default();
        cache.show(1);
        cache.put((1, 0, 1, 0), Arc::new(vec![1]));
        // Another tab comes up while drawing 1's next picture is still being drawn.
        cache.show(2);
        assert!(
            cache.get((1, 0, 1, 0)).is_none(),
            "drawing 1's pictures went"
        );
        cache.put((1, 0, 1, 64), Arc::new(vec![1]));
        assert!(
            cache.get((1, 0, 1, 64)).is_none(),
            "a late picture of drawing 1 is dropped"
        );
        assert!(cache.entries.is_empty());
        cache.put((2, 0, 1, 0), Arc::new(vec![2]));
        assert_eq!(cache.get((2, 0, 1, 0)).as_deref(), Some(&vec![2]));
    }
}
