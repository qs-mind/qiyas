//! PDF pages for Qiyas: how many there are and how big, a page as pixels (underlays, thumbnails),
//! and a page's vectors and text as drawing entities (import).
//!
//! Pages are numbered from 1, as a PDF reader shows them. Sizes are in PDF points (1/72 inch) on
//! the page as it is shown: its crop box, turned by its `/Rotate`, origin at the bottom left.
//!
//! # pdfium
//!
//! Everything goes through pdfium, a shared library loaded at run time (`scripts/pdfium.sh`
//! downloads it). When `QIYAS_PDFIUM` is set, only there (the library file or its folder);
//! otherwise, in order: in the folder given to `set_library_dir` (the desktop app's bundled
//! resources), in `resources/pdfium/` next to the running program, and in debug builds in
//! `crates/qiyas-desktop/resources/pdfium/`. When it is not found, every function returns
//! `PdfError::NotInstalled`; nothing panics. A failed load is tried again on the next call, so
//! running the script while the app is open is enough.
//!
//! pdfium is not thread-safe. It is bound once, lazily, on a dedicated worker thread that lives
//! as long as the program; every call is sent to that thread as a job over a channel and its
//! answer comes back over another. pdfium's objects never leave that thread, and one call never
//! overlaps another. (Chosen over pdfium-render's `thread_safe` mutex: the mutex would make each
//! FFI call safe, but documents and pages would still be shared across threads, and a long render
//! would still block every other caller.)

mod calibration;
mod pages;
mod raster;
mod vectors;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread;

use pdfium_render::prelude::{PdfDocument, PdfPageIndex, Pdfium, PdfiumError};

pub use calibration::{refine_raster_span, RasterSpan};
pub use pages::{page_count, page_rotation, page_size, page_text, thumbnails};
pub use raster::{render_page, MAX_SIDE};
pub use vectors::{extract_vectors, FILL_LAYER_PREFIX, LAYER_PREFIX, TEXT_LAYER};

/// What went wrong with a PDF.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PdfError {
    /// The pdfium library was not found or could not be loaded.
    #[error("PDF support is not installed: run scripts/pdfium.sh")]
    NotInstalled,
    #[error("This PDF can't be opened: {0}")]
    Unreadable(String),
    #[error("Page {page} is not in this PDF (it has {count} pages)")]
    NoSuchPage { page: u32, count: u32 },
    /// pdfium failed on a page it opened.
    #[error("The PDF page could not be read: {0}")]
    Page(String),
}

pub type Result<T> = std::result::Result<T, PdfError>;

type Job = Box<dyn FnOnce(&Pdfium) + Send>;

/// The worker's queue, once pdfium is bound.
static WORKER: OnceLock<Sender<Job>> = OnceLock::new();
/// Held while the worker starts, so two first calls don't start two.
static STARTING: Mutex<()> = Mutex::new(());
/// Set by the desktop app: the folder its bundle keeps pdfium in.
static LIBRARY_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Looks for pdfium in `dir` (before the places it looks by default). Takes effect on the next
/// call when pdfium is not loaded yet.
pub fn set_library_dir(dir: impl Into<PathBuf>) {
    *LIBRARY_DIR.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.into());
}

/// True when pdfium is (or can now be) loaded.
pub fn available() -> bool {
    worker().is_ok()
}

/// The pdfium file name on this platform, e.g. `libpdfium.so`.
fn library_name() -> String {
    format!(
        "{}pdfium{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

/// Where pdfium may be, in the order looked at (see the crate docs).
fn candidates() -> Vec<PathBuf> {
    let name = library_name();
    let mut out = Vec::new();
    if let Some(env) = std::env::var_os("QIYAS_PDFIUM") {
        let env = PathBuf::from(env);
        return vec![env.join(&name), env];
    }
    if let Some(dir) = LIBRARY_DIR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        out.push(dir.join(&name));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        out.push(exe_dir.join("resources").join("pdfium").join(&name));
    }
    // The repository's copy (`scripts/pdfium.sh`): debug builds and a release `qiyas-cli` run
    // from `target/`; an installed app has it beside the program and never this path.
    out.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../qiyas-desktop/resources/pdfium")
            .join(&name),
    );
    out
}

/// The worker's queue, starting the worker (and binding pdfium on it) on first use.
fn worker() -> Result<&'static Sender<Job>> {
    if let Some(worker) = WORKER.get() {
        return Ok(worker);
    }
    let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(worker) = WORKER.get() {
        return Ok(worker);
    }
    let library = candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or(PdfError::NotInstalled)?;
    let (jobs, queue) = channel::<Job>();
    let (ready, bound) = channel::<bool>();
    thread::Builder::new()
        .name("pdfium".to_string())
        .spawn(move || {
            let pdfium = match Pdfium::bind_to_library(&library) {
                Ok(bindings) => Pdfium::new(bindings),
                Err(e) => {
                    eprintln!("pdfium at {} could not be loaded: {e}", library.display());
                    let _ = ready.send(false);
                    return;
                }
            };
            let _ = ready.send(true);
            // A job that panics answers nothing (its caller sees `NotInstalled`'s sibling below);
            // the worker carries on with the next.
            for job in queue {
                let _ = catch_unwind(AssertUnwindSafe(|| job(&pdfium)));
            }
        })
        .map_err(|_| PdfError::NotInstalled)?;
    if !bound.recv().unwrap_or(false) {
        return Err(PdfError::NotInstalled);
    }
    Ok(WORKER.get_or_init(|| jobs))
}

/// Runs `f` on the pdfium thread and waits for its answer.
pub(crate) fn run<R: Send + 'static>(
    f: impl FnOnce(&Pdfium) -> Result<R> + Send + 'static,
) -> Result<R> {
    let (answer, answered) = channel();
    let job: Job = Box::new(move |pdfium| {
        let _ = answer.send(f(pdfium));
    });
    worker()?.send(job).map_err(|_| PdfError::NotInstalled)?;
    answered
        .recv()
        .unwrap_or_else(|_| Err(PdfError::Page("pdfium stopped on this page".to_string())))
}

/// Opens `path` (read lazily: only the pages asked for are read).
pub(crate) fn open<'a>(pdfium: &'a Pdfium, path: &Path) -> Result<PdfDocument<'a>> {
    pdfium.load_pdf_from_file(path, None).map_err(|e| match e {
        PdfiumError::IoError(e) => PdfError::Unreadable(e.to_string()),
        e => PdfError::Unreadable(format!("{e:?}")),
    })
}

/// The 0-based index of page `page` (from 1), or `NoSuchPage`.
pub(crate) fn page_index(doc: &PdfDocument, page: u32) -> Result<PdfPageIndex> {
    let count = doc.pages().len().max(0) as u32;
    if page == 0 || page > count {
        return Err(PdfError::NoSuchPage { page, count });
    }
    Ok((page - 1) as PdfPageIndex)
}

pub(crate) fn page_error(e: PdfiumError) -> PdfError {
    PdfError::Page(format!("{e:?}"))
}

#[cfg(feature = "fixtures")]
pub mod fixtures;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_the_library_every_call_says_pdf_support_is_not_installed() {
        // Only this test in this binary starts pdfium, so it is not loaded yet.
        std::env::set_var("QIYAS_PDFIUM", "/no/such/folder");
        assert!(!available());
        let err = page_count(Path::new("plan.pdf")).unwrap_err();
        assert_eq!(err, PdfError::NotInstalled);
        assert_eq!(
            err.to_string(),
            "PDF support is not installed: run scripts/pdfium.sh"
        );
        assert!(matches!(
            render_page(Path::new("plan.pdf"), 1, 72.0, |_| {}),
            Err(PdfError::NotInstalled)
        ));
        std::env::remove_var("QIYAS_PDFIUM");
    }
}
