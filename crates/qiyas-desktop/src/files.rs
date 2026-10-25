//! Files beyond open and save: the system clipboard (`arboard`), autosave and recovery of
//! drawings, Recover (a damaged file opened with a report of what was dropped), and sheet sets.
//!
//! Crash recovery: a watch thread (`start_watch`) keeps a lock file beside every open drawing
//! (`Locks`, `qiyas_core::take_lock`; the lock files are read and written with none of the app's
//! locks held, and a lock another Qiyas has is looked at again every 10 s) and autosaves a
//! drawing after every 20 changes; quitting removes the locks (`release_locks`), a crash leaves
//! them. The panic hook
//! (`install_panic_hook`) autosaves every drawing with changes, best effort, within 5 s.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use qiyas_core::{
    autosave_all, recoverable, release_lock, sheet_set_entries, sheet_set_entry as entry_for,
    take_lock, AutosaveRun, CoreError, Drawing, DrawingInfo, ReadReport, Recoverable, Session,
    SheetSet, SheetSetDrawing, SheetSetEntry,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::commands::{new_conversation, remember_recent, with_session};
use crate::state::AppState;
use crate::tabs;

fn lock<T>(m: &std::sync::Mutex<T>) -> MutexGuard<'_, T> {
    // A panic in an earlier command leaves the data as it was; keep using it.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `f` on the app's system clipboard, opening it on first use.
fn with_clipboard<R>(
    state: &AppState,
    f: impl FnOnce(&mut arboard::Clipboard) -> Result<R, arboard::Error>,
) -> Result<R, arboard::Error> {
    let mut held = lock(&state.clipboard);
    if held.is_none() {
        *held = Some(arboard::Clipboard::new()?);
    }
    f(held.as_mut().expect("just opened"))
}

/// The text on the system clipboard.
pub(crate) fn clipboard_text(state: &AppState) -> Result<String, String> {
    with_clipboard(state, |c| c.get_text())
        .map_err(|e| format!("The clipboard could not be read: {e}"))
}

/// The clipboard's text for a paste into a text field, or None when it holds none. Pastes go
/// through here, not the web view: WebKitGTK 2.54 on Wayland hangs and then crashes reading
/// the clipboard (`src/shell/safePaste.ts`).
#[tauri::command(async)]
pub fn clipboard_read_text(state: State<'_, AppState>) -> Result<Option<String>, String> {
    match with_clipboard(&state, |c| c.get_text()) {
        Ok(text) => Ok(Some(text)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(format!("The clipboard could not be read: {e}")),
    }
}

/// The clipboard's picture for a paste: width and height (u32, little-endian), then its RGBA
/// pixels; empty when it holds none.
#[tauri::command(async)]
pub fn clipboard_read_image(state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    match with_clipboard(&state, |c| c.get_image()) {
        Ok(img) => {
            let mut out = Vec::with_capacity(8 + img.bytes.len());
            out.extend_from_slice(&(img.width as u32).to_le_bytes());
            out.extend_from_slice(&(img.height as u32).to_le_bytes());
            out.extend_from_slice(&img.bytes);
            Ok(tauri::ipc::Response::new(out))
        }
        Err(arboard::Error::ContentNotAvailable | arboard::Error::ConversionFailure) => {
            Ok(tauri::ipc::Response::new(Vec::new()))
        }
        Err(e) => Err(format!("The clipboard could not be read: {e}")),
    }
}

/// Puts `text` on the system clipboard; the app keeps it there while it runs.
pub(crate) fn set_clipboard_text(state: &AppState, text: &str) -> Result<(), String> {
    with_clipboard(state, |c| c.set_text(text.to_string()))
        .map_err(|e| format!("The clipboard could not be written: {e}"))
}

/// Runs `f` on every open drawing, the one on screen first, under the lock order of `tabs`:
/// tabs, session, parked.
fn with_open<T>(state: &AppState, f: impl FnOnce(Vec<&Session>) -> T) -> T {
    let _list = lock(&state.tabs);
    let session = lock(&state.session);
    let parked = lock(&state.parked);
    f(session
        .iter()
        .chain(parked.iter().map(|p| &p.session))
        .collect())
}

/// Autosaves every open drawing with unsaved changes, the one on screen and those parked in
/// other tabs (see `Session::autosave`).
#[tauri::command]
pub async fn autosave_drawings(state: State<'_, AppState>) -> Result<AutosaveRun, String> {
    Ok(with_open(&state, |open| autosave_all(open)))
}

/// The lock files this app holds beside its open drawings (`qiyas_core::take_lock`), kept in
/// step with the tabs by the watch (`tick`) and removed when Qiyas quits (`release_locks`).
#[derive(Default)]
pub struct Locks(Mutex<HeldLocks>);

#[derive(Default)]
struct HeldLocks {
    held: BTreeSet<PathBuf>,
    /// Open drawings not locked (another Qiyas has them, or no file yet), with when that was
    /// last tried: tried again after `RETRY_LOCK`, so another process's liveness is looked at
    /// no more often.
    refused: BTreeMap<PathBuf, Instant>,
    /// Qiyas is quitting: no lock is taken any more.
    released: bool,
}

/// How long a drawing that could not be locked waits before the next try.
const RETRY_LOCK: Duration = Duration::from_secs(10);

impl Locks {
    /// Locks every drawing in `open` not held yet (files only: a recovered drawing not saved yet
    /// has none), unless another Qiyas has it (tried again after `RETRY_LOCK`); unlocks the held
    /// ones no longer open (closed, or saved under another name). `now` is the beat's time.
    fn sync(&self, open: &[PathBuf], now: Instant) {
        let mut locks = lock(&self.0);
        if locks.released {
            return;
        }
        let open: BTreeSet<&PathBuf> = open.iter().collect();
        let closed: Vec<PathBuf> = locks
            .held
            .iter()
            .filter(|p| !open.contains(p))
            .cloned()
            .collect();
        for path in closed {
            let _ = release_lock(&path);
            locks.held.remove(&path);
        }
        locks.refused.retain(|p, _| open.contains(p));
        for path in open {
            let waiting = locks
                .refused
                .get(path)
                .is_some_and(|tried| now.saturating_duration_since(*tried) < RETRY_LOCK);
            if locks.held.contains(path) || waiting {
                continue;
            }
            if path.is_file() && take_lock(path).unwrap_or(false) {
                locks.refused.remove(path);
                locks.held.insert(path.clone());
            } else {
                locks.refused.insert(path.clone(), now);
            }
        }
    }

    /// Removes every lock this app holds; none is taken after (Qiyas quits).
    fn release_all(&self) {
        let mut locks = lock(&self.0);
        locks.released = true;
        for path in std::mem::take(&mut locks.held) {
            let _ = release_lock(&path);
        }
    }
}

/// How often the watch looks at the open drawings.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// One beat of the watch: the lock files follow the open drawings (their paths taken first, the
/// files read and written with none of the app's locks held), and a drawing with 20 changes
/// since it was last saved or autosaved is autosaved (`Session::autosave_due`).
fn tick(state: &AppState, locks: &Locks) -> AutosaveRun {
    let paths: Vec<PathBuf> = with_open(state, |open| {
        open.iter().map(|s| s.drawing.path.clone()).collect()
    });
    locks.sync(&paths, Instant::now());
    with_open(state, |open| {
        autosave_all(open.into_iter().filter(|s| s.autosave_due()))
    })
}

/// Starts the watch: `tick` every second while Qiyas runs.
pub(crate) fn start_watch<R: Runtime>(app: AppHandle<R>) {
    let watch = move || loop {
        std::thread::sleep(WATCH_EVERY);
        let (Some(state), Some(locks)) = (app.try_state::<AppState>(), app.try_state::<Locks>())
        else {
            continue;
        };
        // A panic in one beat (a writer bug) must not stop the next ones.
        if let Ok(run) = catch_unwind(AssertUnwindSafe(|| tick(&state, &locks))) {
            for failed in run.failed {
                eprintln!("Autosave failed: {failed}");
            }
        }
    };
    if let Err(e) = std::thread::Builder::new()
        .name("qiyas-watch".into())
        .spawn(watch)
    {
        eprintln!("The autosave watch could not start: {e}");
    }
}

/// Qiyas quits: the lock files of its drawings go (a crash leaves them behind).
pub(crate) fn release_locks<R: Runtime>(app: &AppHandle<R>) {
    if let Some(locks) = app.try_state::<Locks>() {
        locks.release_all();
    }
}

/// How long the panic hook waits for its autosaves.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// A panic flush is running; a panic meanwhile (in it, or elsewhere) starts no other.
static FLUSHING: AtomicBool = AtomicBool::new(false);

/// Ends a panic flush when dropped: when it finishes, panics, or never starts.
struct Flushing;

impl Drop for Flushing {
    fn drop(&mut self) {
        FLUSHING.store(false, Ordering::SeqCst);
    }
}

/// After the usual panic report, autosaves every drawing with unsaved changes it can get at
/// once (`flush_dirty`), waiting at most `FLUSH_TIMEOUT`. Never panics itself.
pub(crate) fn install_panic_hook<R: Runtime>(app: AppHandle<R>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if FLUSHING.swap(true, Ordering::SeqCst) {
            return;
        }
        let flushing = Flushing;
        let app = app.clone();
        let flushed = flush_within(FLUSH_TIMEOUT, move || {
            let _flushing = flushing;
            app.try_state::<AppState>().map(|state| {
                if let Ok(run) = state.ai.try_lock() {
                    run.save_if_available();
                }
                flush_dirty(&state)
            })
        });
        let mut err = std::io::stderr();
        let _ = match flushed {
            Some(Some(f)) => writeln!(
                err,
                "Autosaved after the error: {} written, {} failed {:?}, {} held by another thread",
                f.run.written.len(),
                f.run.failed.len(),
                f.run.failed,
                f.busy
            ),
            Some(None) => Ok(()),
            None => writeln!(err, "The autosave after the error did not finish"),
        };
    }));
}

/// Runs `f` on its own thread and waits for it at most `limit`: its result, or `None` when it
/// panicked, took longer (it is left to finish on its own) or no thread could start.
fn flush_within<T: Send + 'static>(
    limit: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("qiyas-flush".into())
        .spawn(move || {
            if let Ok(done) = catch_unwind(AssertUnwindSafe(f)) {
                let _ = tx.send(done);
            }
        })
        .ok()?;
    rx.recv_timeout(limit).ok()
}

/// What a panic flush did: the autosaves, and how many of the two places drawings live (the one
/// on screen, the parked ones) another thread held, so were left (the command that panicked
/// holds its drawing).
#[derive(Debug)]
struct Flushed {
    run: AutosaveRun,
    busy: usize,
}

/// `lock`, never waiting: `None` when another thread holds it.
fn try_lock<T>(m: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match m.try_lock() {
        Ok(held) => Some(held),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// Autosaves every open drawing with unsaved changes that no other thread holds.
fn flush_dirty(state: &AppState) -> Flushed {
    let session = try_lock(&state.session);
    let parked = try_lock(&state.parked);
    let busy = usize::from(session.is_none()) + usize::from(parked.is_none());
    let on_screen = session.as_deref().and_then(Option::as_ref);
    let others = parked.as_deref().into_iter().flatten().map(|p| &p.session);
    Flushed {
        run: autosave_all(on_screen.into_iter().chain(others)),
        busy,
    }
}

/// The autosaves of `paths` newer than their drawings (the recent drawings at launch, or the
/// drawing about to open).
#[tauri::command]
pub async fn recoverable_drawings(paths: Vec<String>) -> Result<Vec<Recoverable>, String> {
    Ok(paths
        .iter()
        .filter_map(|p| recoverable(Path::new(p)))
        .collect())
}

/// The untitled drawings (`tabs::untitled_path`) not open here whose autosave is still in the
/// app's folder: Qiyas closed before their first Save As. The window asks about them at launch
/// with the recent drawings.
#[tauri::command]
pub async fn untitled_autosaves(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let dir = state.config_dir.join("untitled");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let open: Vec<String> = tabs::snapshot(&state)
        .tabs
        .into_iter()
        .map(|t| t.path)
        .collect();
    let mut found: Vec<String> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sv$")))
        .filter_map(|p| {
            let drawing = p.with_extension("");
            let shown = drawing.display().to_string();
            let untitled = drawing
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("Untitled-"));
            (untitled && !open.contains(&shown)).then_some(shown)
        })
        .collect();
    found.sort();
    Ok(found)
}

/// Opens the autosave of the drawing `path` (or, for an older copy's path, that copy) in a new
/// tab as a new drawing with unsaved changes beside it (`Session::open_autosave`); the window
/// then offers Save As. An untitled drawing's autosave stays untitled.
#[tauri::command]
pub async fn recover_autosave<R: Runtime>(
    path: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<DrawingInfo, String> {
    let mut session = Session::open_autosave(Path::new(&path)).map_err(|e| e.to_string())?;
    if Path::new(&path).starts_with(state.config_dir.join("untitled")) {
        session.mark_untitled();
        tabs::set_untitled_export_folder(&app, &mut session)?;
    }
    new_conversation(&app, &state);
    let recovered = session.drawing.path.clone();
    tabs::add(&state, session)?;
    {
        let _run = crate::commands::ai_run(&state);
        let store = crate::conversations::Store::new(&state.config_dir);
        let source = qiyas_core::autosave_copy_of(Path::new(&path))
            .unwrap_or_else(|| Path::new(&path).to_path_buf());
        store.follow_drawing(&source, &recovered)?;
        store.follow_drawing(&qiyas_core::autosave_path(&source), &recovered)?;
        store.follow_drawing(Path::new(&path), &recovered)?;
    }
    with_session(&state, |s| Ok(s.info()))
}

/// Deletes the autosave of the drawing `path` (see `discard`).
#[tauri::command]
pub async fn discard_autosave(path: String, state: State<'_, AppState>) -> Result<(), String> {
    discard(&state, Path::new(&path)).map_err(|e| e.to_string())
}

/// Don't Save on a drawing open here deletes its own autosave (never a crash copy); the
/// recovery dialog's Discard, on a drawing not open here, deletes the autosave it offered; an
/// older copy's path (`older_copies`) deletes that copy. A drawing's `<file>.sv$` is kept, with an
/// error, while another Qiyas has the drawing open.
fn discard(state: &AppState, drawing: &Path) -> Result<(), CoreError> {
    if qiyas_core::autosave_copy_of(drawing).is_some() || tabs::find_path(state, drawing).is_some()
    {
        qiyas_core::discard_autosave(drawing)
    } else {
        qiyas_core::discard_recoverable(drawing)
    }
}

/// A drawing Recover opened, and what its reader dropped.
#[derive(Serialize)]
pub struct Recovered {
    pub info: DrawingInfo,
    pub report: ReadReport,
}

/// RECOVER: opens the (damaged) drawing at `path` in a new tab through the failsafe reader and
/// says what it read and dropped. A drawing open already is refused: close it first, so it is
/// read again from the file.
#[tauri::command]
pub async fn recover_drawing<R: Runtime>(
    path: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Recovered, String> {
    if tabs::find_path(&state, Path::new(&path)).is_some() {
        return Err("This drawing is open: close its tab first to recover it from the file".into());
    }
    let session = Session::open(Path::new(&path)).map_err(|e| e.to_string())?;
    let report = session.read_report();
    new_conversation(&app, &state);
    tabs::add(&state, session)?;
    remember_recent(&state, &path);
    with_session(&state, |s| {
        Ok(Recovered {
            info: s.info(),
            report,
        })
    })
}

/// A sheet set and its drawings as the window lists them.
#[derive(Serialize)]
pub struct SheetSetView {
    pub set: SheetSet,
    pub entries: Vec<SheetSetEntry>,
}

fn view(qss: &Path, set: SheetSet) -> SheetSetView {
    SheetSetView {
        entries: sheet_set_entries(qss, &set),
        set,
    }
}

/// Reads the sheet set at `path` (a `.qss` file).
#[tauri::command]
pub async fn sheet_set_open(path: String) -> Result<SheetSetView, String> {
    let qss = Path::new(&path);
    let set = SheetSet::load(qss).map_err(|e| e.to_string())?;
    Ok(view(qss, set))
}

/// Writes `set` to `path` (a `.qss` file; nothing else is written).
#[tauri::command]
pub async fn sheet_set_save(path: String, set: SheetSet) -> Result<SheetSetView, String> {
    let qss = Path::new(&path);
    set.save(qss).map_err(|e| e.to_string())?;
    Ok(view(qss, set))
}

/// The layout names of `s`, in tab order.
fn layouts(s: &Session) -> Vec<String> {
    s.drawing
        .sheets()
        .into_iter()
        .filter(|sheet| !sheet.is_model)
        .map(|sheet| sheet.name)
        .collect()
}

/// The layouts of the drawing at `path`: its open tab's, else read from the file.
fn layouts_of(state: &AppState, path: &Path) -> Result<Vec<String>, String> {
    let _list = lock(&state.tabs);
    if let Some(s) = lock(&state.session).as_ref() {
        if crate::settings::same_file(&s.drawing.path, path) {
            return Ok(layouts(s));
        }
    }
    if let Some(p) = lock(&state.parked)
        .iter()
        .find(|p| crate::settings::same_file(&p.session.drawing.path, path))
    {
        return Ok(layouts(&p.session));
    }
    let drawing = Drawing::open(path).map_err(|e| e.to_string())?;
    Ok(drawing
        .sheets()
        .into_iter()
        .filter(|sheet| !sheet.is_model)
        .map(|sheet| sheet.name)
        .collect())
}

/// The sheet set drawing for the file `drawing` with all its layouts, its path relative to the
/// sheet set `qss`; refused when it is outside the set's folder.
#[tauri::command]
pub async fn sheet_set_entry(
    qss: String,
    drawing: String,
    state: State<'_, AppState>,
) -> Result<SheetSetDrawing, String> {
    let layouts = layouts_of(&state, Path::new(&drawing))?;
    entry_for(Path::new(&qss), Path::new(&drawing), layouts).map_err(|e| e.to_string())
}

/// A new sheet set `name` at `qss` (not written yet) holding every open drawing in its folder
/// with all their layouts; `skipped` names the open drawings outside it.
#[derive(Serialize)]
pub struct FromTabs {
    pub set: SheetSet,
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn sheet_set_from_tabs(
    qss: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<FromTabs, String> {
    let qss = Path::new(&qss);
    let mut set = SheetSet {
        project: None,
        name,
        drawings: Vec::new(),
    };
    let mut skipped = Vec::new();
    let mut add = |s: &Session| match entry_for(qss, &s.drawing.path, layouts(s)) {
        Ok(d) => set.drawings.push(d),
        Err(_) => skipped.push(s.drawing.path.display().to_string()),
    };
    let list = lock(&state.tabs);
    let session = lock(&state.session);
    let parked = lock(&state.parked);
    for id in &list.order {
        if list.active == Some(*id) {
            if let Some(s) = session.as_ref() {
                add(s);
            }
        } else if let Some(p) = parked.iter().find(|p| p.id == *id) {
            add(&p.session);
        }
    }
    Ok(FromTabs { set, skipped })
}

/// The paper-space viewports of the sheet `sheet` (the one on screen when left out) and how
/// the window draws model space inside each (`Session::paper_views_of`).
#[tauri::command]
pub async fn paper_views(
    sheet: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<qiyas_core::PaperView>, String> {
    with_session(&state, |s| {
        Ok(match sheet {
            Some(sheet) => s.paper_views_of(&sheet),
            None => s.paper_views(),
        })
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use qiyas_core::fixtures::{sample_doc, write_drawing};
    use qiyas_core::{
        autosave_path, lock_path, read_lock, AutosaveRun, Command, DrawSpec, Session,
        AUTOSAVE_EVERY,
    };

    use super::*;

    fn line(s: &mut Session, y: f64) {
        s.run(Command::Draw(DrawSpec::Line {
            a: [100.0, y],
            b: [101.0, y],
        }))
        .unwrap();
    }

    /// The app's state with `a.dwg` and `b.dwg` open in two tabs, `b` on screen.
    fn two_open(dir: &Path) -> (AppState, PathBuf, PathBuf) {
        let state = AppState::new(dir.join("config"));
        let a = write_drawing(&sample_doc(), &dir.join("a.dwg"));
        let b = write_drawing(&sample_doc(), &dir.join("b.dwg"));
        tabs::add(&state, Session::open(&a).unwrap()).unwrap();
        tabs::add(&state, Session::open(&b).unwrap()).unwrap();
        (state, a, b)
    }

    #[test]
    fn tick_locks_open_drawings_autosaves_when_due_and_unlocks_closed_ones() {
        let dir = tempfile::tempdir().unwrap();
        let (state, a, b) = two_open(dir.path());
        let locks = Locks::default();
        assert_eq!(tick(&state, &locks), AutosaveRun::default());
        for p in [&a, &b] {
            assert_eq!(read_lock(p).unwrap().pid, std::process::id());
        }

        // 19 changes: not yet; the 20th: the next beat autosaves it, once.
        for i in 0..AUTOSAVE_EVERY - 1 {
            line(lock(&state.session).as_mut().unwrap(), i as f64);
        }
        assert!(tick(&state, &locks).written.is_empty());
        line(lock(&state.session).as_mut().unwrap(), 99.0);
        assert_eq!(
            tick(&state, &locks).written,
            [autosave_path(&b).display().to_string()]
        );
        assert!(tick(&state, &locks).written.is_empty());
        assert!(!autosave_path(&a).exists());

        // b's tab closed: its lock goes on the next beat; a's stays.
        *lock(&state.session) = None;
        tick(&state, &locks);
        assert!(!lock_path(&b).exists());
        assert!(lock_path(&a).exists());
    }

    #[test]
    fn release_locks_removes_every_held_lock_and_takes_none_after() {
        let dir = tempfile::tempdir().unwrap();
        let (state, a, b) = two_open(dir.path());
        let locks = Locks::default();
        tick(&state, &locks);
        assert!(lock_path(&a).exists() && lock_path(&b).exists());
        locks.release_all();
        assert!(!lock_path(&a).exists() && !lock_path(&b).exists());
        // A beat of the watch while Qiyas quits takes none again.
        tick(&state, &locks);
        assert!(!lock_path(&a).exists() && !lock_path(&b).exists());
    }

    #[test]
    fn flush_within_gives_the_result_or_returns_at_the_timeout() {
        assert_eq!(flush_within(Duration::from_secs(5), || 7), Some(7));

        // A flush stuck on a lock: back after the limit, the flush left to finish on its own.
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let stuck = Arc::clone(&gate);
        let start = Instant::now();
        let done = flush_within(Duration::from_millis(200), move || {
            let _wait = stuck.lock();
            1
        });
        let took = start.elapsed();
        assert_eq!(done, None);
        assert!(
            took >= Duration::from_millis(200) && took < Duration::from_secs(10),
            "{took:?}"
        );
        drop(held);

        // A flush that panics is no result, and no panic here.
        let failed = flush_within(Duration::from_secs(5), || -> u8 { panic!("flush failed") });
        assert_eq!(failed, None);
    }

    #[test]
    fn flush_dirty_autosaves_what_it_can_and_skips_a_held_drawing() {
        let dir = tempfile::tempdir().unwrap();
        let (state, a, b) = two_open(dir.path());
        line(lock(&state.session).as_mut().unwrap(), 1.0);
        line(&mut lock(&state.parked)[0].session, 1.0);

        // The drawing on screen is held (by the command that panicked): skipped, not waited for.
        let held = lock(&state.session);
        let flushed = flush_dirty(&state);
        drop(held);
        assert_eq!(flushed.busy, 1);
        assert_eq!(
            flushed.run.written,
            [autosave_path(&a).display().to_string()]
        );
        assert!(!autosave_path(&b).exists());

        // A panic while holding it poisons the lock; its drawing is autosaved all the same.
        let session = Arc::clone(&state.session);
        let _ = std::thread::spawn(move || {
            let _held = session.lock().unwrap();
            panic!("a command failed");
        })
        .join();
        assert!(state.session.is_poisoned());
        let flushed = flush_dirty(&state);
        assert_eq!(flushed.busy, 0);
        assert_eq!(flushed.run.written.len(), 2);
        assert!(autosave_path(&b).exists());
    }
    #[test]
    fn a_refused_lock_is_tried_again_after_ten_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_drawing(&sample_doc(), &dir.path().join("a.dwg"));
        std::fs::write(
            lock_path(&a),
            r#"{"pid": 7, "host": "elsewhere", "started_at": 1}"#,
        )
        .unwrap();
        let locks = Locks::default();
        let start = Instant::now();
        locks.sync(std::slice::from_ref(&a), start);
        assert_eq!(read_lock(&a).unwrap().host, "elsewhere");
        // The other computer closes it; the beats in between leave it alone.
        std::fs::remove_file(lock_path(&a)).unwrap();
        locks.sync(std::slice::from_ref(&a), start + Duration::from_secs(1));
        assert!(!lock_path(&a).exists());
        locks.sync(std::slice::from_ref(&a), start + Duration::from_secs(11));
        assert_eq!(read_lock(&a).unwrap().pid, std::process::id());
    }

    /// The crash copies beside `drawing`.
    fn crash_copies(drawing: &Path) -> usize {
        let prefix = format!(
            "{}.sv$.crash-",
            drawing.file_name().unwrap().to_string_lossy()
        );
        std::fs::read_dir(drawing.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&prefix) && !n.ends_with(".qiyas"))
            .count()
    }

    /// `drawing` crashed with unsaved changes in an earlier Qiyas that had this pid, and was
    /// opened again: its autosave is kept aside as a crash copy.
    fn leave_crash_copy(drawing: &Path) {
        let mut s = Session::open(drawing).unwrap();
        line(&mut s, 5.0);
        s.autosave().unwrap().unwrap();
        assert!(take_lock(drawing).unwrap());
        let mut earlier = read_lock(drawing).unwrap();
        earlier.started_at = 1;
        std::fs::write(lock_path(drawing), serde_json::to_vec(&earlier).unwrap()).unwrap();
        assert!(take_lock(drawing).unwrap());
        release_lock(drawing).unwrap();
        assert_eq!(crash_copies(drawing), 1);
    }

    #[test]
    fn discard_deletes_an_open_drawings_own_autosave_or_what_the_dialog_offered() {
        let dir = tempfile::tempdir().unwrap();
        let (state, a, _b) = two_open(dir.path());
        let c = write_drawing(&sample_doc(), &dir.path().join("c.dwg"));
        leave_crash_copy(&c);
        leave_crash_copy(&a);
        {
            let mut parked = lock(&state.parked);
            line(&mut parked[0].session, 1.0);
            parked[0].session.autosave().unwrap().unwrap();
        }

        // Don't Save on `a`, open here: its own autosave goes, the crash copy stays.
        discard(&state, &a).unwrap();
        assert!(!autosave_path(&a).exists());
        assert_eq!(crash_copies(&a), 1);
        // The recovery dialog's Discard on `c`, not open: no autosave offered, the crash copy
        // stays; Discard of the older copy by its path deletes it.
        discard(&state, &c).unwrap();
        assert_eq!(crash_copies(&c), 1);
        let older = recoverable(&c).unwrap().older_copies[0].path.clone();
        discard(&state, Path::new(&older)).unwrap();
        assert_eq!(crash_copies(&c), 0);
    }
}
