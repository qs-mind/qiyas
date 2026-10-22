//! Run script (SCRIPT, the ribbon's Manage tab): a `.scr` file in the grammar of
//! `qiyas_core::script` (read and checked whole first, with its folder rule: paths from the
//! script's folder, inside it or the folder of the drawing on screen) run on the drawing on
//! screen, each step as the window does it: OPEN opens a tab (or shows the one open already),
//! SAVE and SAVEAS save as the app does, each drawing command is its own undo step, QUIT ends the
//! script (never the app), DELAY waits, ECHO writes its text in the log. A drawing the script
//! changed has its take-off saved beside it before OPEN takes another one on screen, as the
//! window saves it after every command (the window saves the one on screen after the run).
//! Refused while a block is edited in place. OPEN leaves a drawing with an autosave to offer, or
//! open in another Qiyas, to the window (its recovery question). DELAY waits at most 10 s, and
//! Stop (`stop_script`) ends the run between steps or during a DELAY; each step is announced as
//! `script:line` for the window's "Running script" state. The run keeps to the tab on screen
//! when it starts (moved only by OPEN) and stops before a line when another tab has come on
//! screen.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use qiyas_core::script::{self, Action, Context};
use qiyas_core::{Command, Current, Kind, LayerProp, Outcome, Reported, Session, Source};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::commands::{new_conversation, remember_recent, with_session};
use crate::settings::same_file;
use crate::state::AppState;
use crate::tabs;

/// One script line as run: its number (from 1), its text, and what it did or why it failed.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ScriptLine {
    pub line_no: usize,
    pub text: String,
    pub ok: bool,
    pub message: String,
}

/// What a run did: the lines run, the failing one last (a script that doesn't read is its bad
/// line alone, and nothing ran); whether a drawing, its take-off or the tabs may have changed;
/// and how long it took.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ScriptResult {
    /// False when the script didn't read: its bad line is the only one, and nothing ran.
    pub ran: bool,
    pub lines: Vec<ScriptLine>,
    pub changed: bool,
    pub took_ms: u64,
}

/// The longest DELAY a script waits in the window.
pub const DELAY_CAP_MS: u64 = 10_000;

/// Sent with the line number before each step runs.
pub const SCRIPT_LINE: &str = "script:line";

/// The running script's Stop: set by `stop_script`, read between steps and during a DELAY.
#[derive(Default)]
pub struct ScriptControl {
    pub stop: AtomicBool,
}

/// Stops the running script before its next step (or during its DELAY).
#[tauri::command]
pub async fn stop_script(control: State<'_, ScriptControl>) -> Result<(), String> {
    control.stop.store(true, Ordering::Relaxed);
    Ok(())
}

/// DELAY in the window: `ms`, at most `cap`, in short slices so Stop ends it (an error then).
fn wait(ms: u64, cap: u64, stop: &AtomicBool) -> Result<String, String> {
    let goal = Duration::from_millis(ms.min(cap));
    let start = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(format!("stopped after {} ms", start.elapsed().as_millis()));
        }
        let left = goal.saturating_sub(start.elapsed());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
    Ok(if ms > cap {
        format!(
            "waited {cap} ms (DELAY is at most {} s in the window)",
            cap / 1000
        )
    } else {
        format!("waited {ms} ms")
    })
}

/// Runs the script at `path` on the drawing on screen; fails only when the script can't be read
/// or a block is edited in place.
#[tauri::command]
pub async fn run_script<R: Runtime>(
    path: String,
    app: AppHandle<R>,
) -> Result<ScriptResult, String> {
    // DELAY sleeps and steps read and write files: off the async runtime's threads.
    tauri::async_runtime::spawn_blocking(move || run_file(&app, Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

fn run_file<R: Runtime>(app: &AppHandle<R>, path: &Path) -> Result<ScriptResult, String> {
    let started = Instant::now();
    let state = app.state::<AppState>();
    let control = app.state::<ScriptControl>();
    control.stop.store(false, Ordering::Relaxed);
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("The script {} could not be read: {e}", path.display()))?;
    let drawing = {
        let session = state.session.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(name) = session.as_ref().and_then(Session::block_edit_name) {
            return Err(format!(
                "Block {name} is being edited in place: save or discard the edit first (REFCLOSE), then run the script"
            ));
        }
        session.as_ref().map(|s| s.drawing.path.clone())
    };
    let source: Vec<&str> = text
        .strip_prefix('\u{feff}')
        .unwrap_or(&text)
        .lines()
        .collect();
    let line = |line_no: usize, ok: bool, message: String| ScriptLine {
        line_no,
        text: source
            .get(line_no.wrapping_sub(1))
            .map_or("", |t| t.trim())
            .to_string(),
        ok,
        message,
    };
    let cx = Context {
        script: path,
        drawing: drawing.as_deref(),
        out_dir: None,
    };
    let mut lines = Vec::new();
    let mut changed = false;
    let mut ran = true;
    match script::parse(&text, &cx) {
        Err(e) => {
            ran = false;
            lines.push(line(e.line, false, e.message));
        }
        Ok(script) => {
            // The drawing on screen has changes of the script's its project file lacks.
            let mut unsaved = false;
            // The drawing on screen is one the failsafe reader repaired on open: the window asks
            // before saving over it, a script can't, so SAVE is refused (SAVEAS elsewhere is not).
            let mut repaired = on_screen_repaired(&state);
            // The tab the script works on: the one on screen at the start, then the one each
            // OPEN shows. A tab switch, a closed tab or a file opened from outside meanwhile
            // would make every later line edit and save another drawing: the run stops.
            let mut pinned = on_screen_tab(&state);
            for step in &script.steps {
                if control.stop.load(Ordering::Relaxed) {
                    lines.push(line(
                        step.line,
                        false,
                        "stopped before this line".to_string(),
                    ));
                    break;
                }
                if on_screen_tab(&state) != pinned {
                    lines.push(line(
                        step.line,
                        false,
                        "the drawing on screen changed: stopped before this line".to_string(),
                    ));
                    break;
                }
                let _ = app.emit(SCRIPT_LINE, step.line);
                if let Action::Quit = step.action {
                    lines.push(line(step.line, true, "quit".to_string()));
                    break;
                }
                if repaired && saves_over_drawing(&state, &step.action) {
                    let why =
                        "the drawing was repaired when it opened: SAVE would write what Qiyas \
                               could read over it; use SAVEAS to a new path, or Save in the \
                               window, which asks first";
                    lines.push(line(step.line, false, why.to_string()));
                    break;
                }
                if matches!(step.action, Action::Open(_)) && unsaved {
                    if let Err(e) = with_session(&state, |s| s.save_project().map(|_| ())) {
                        let why = format!("the take-off could not be saved first: {e}");
                        lines.push(line(step.line, false, why));
                        break;
                    }
                }
                match run_step(app, &state, step, &control.stop) {
                    Ok((message, change)) => {
                        changed |= change;
                        unsaved = match step.action {
                            Action::Open(_) | Action::Save | Action::SaveAs(_) => false,
                            _ => unsaved || change,
                        };
                        if let Action::Open(_) = step.action {
                            pinned = on_screen_tab(&state);
                        }
                        repaired = match step.action {
                            Action::Open(_) => on_screen_repaired(&state),
                            Action::SaveAs(_) => false,
                            _ => repaired,
                        };
                        lines.push(line(step.line, true, message));
                    }
                    Err(message) => {
                        lines.push(line(step.line, false, message));
                        break;
                    }
                }
            }
        }
    }
    Ok(ScriptResult {
        ran,
        lines,
        changed,
        took_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// The id of the tab on screen (none when no drawing is open).
fn on_screen_tab(state: &AppState) -> Option<u32> {
    state
        .tabs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .active
}

/// How many records the failsafe reader dropped or repaired reading `session`'s drawing.
pub(crate) fn read_problems(session: &Session) -> usize {
    session.drawing.report.dropped.iter().map(|d| d.count).sum()
}

/// Whether the drawing on screen was repaired by the failsafe reader when it opened.
fn on_screen_repaired(state: &AppState) -> bool {
    with_session(state, |s| Ok(read_problems(s) > 0)).unwrap_or(false)
}

/// SAVE, or SAVEAS onto the drawing's own file.
fn saves_over_drawing(state: &AppState, action: &Action) -> bool {
    match action {
        Action::Save => true,
        Action::SaveAs(path) => {
            with_session(state, |s| Ok(same_file(&s.drawing.path, path))).unwrap_or(false)
        }
        _ => false,
    }
}

/// Runs one step (not QUIT) as the window would: what it did, in a line, and whether it may have
/// changed a drawing, its take-off or the tabs.
fn run_step<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    step: &script::Step,
    stop: &AtomicBool,
) -> Result<(String, bool), String> {
    Ok(match &step.action {
        Action::Quit => ("quit".to_string(), false),
        Action::Delay(ms) => (wait(*ms, DELAY_CAP_MS, stop)?, false),
        Action::Echo(text) => (text.clone(), false),
        Action::Zoom => ("zoom: nothing to do in a script".to_string(), false),
        Action::Open(path) => (open(app, state, path, false)?, true),
        Action::SaveAs(path) => {
            with_session(state, |s| s.save_as(path))?;
            remember_recent(state, &path.display().to_string());
            (format!("saved {}", path.display()), true)
        }
        Action::Save => {
            let path = with_session(state, |s| {
                s.save()?;
                Ok(s.drawing.path.clone())
            })?;
            (format!("saved {}", path.display()), true)
        }
        Action::CurrentLayer(name) => {
            with_session(state, |s| {
                let current = Current {
                    layer: name.clone(),
                    ..s.current.clone()
                };
                s.run(Command::SetCurrent { current })
            })?;
            (format!("current layer {name}"), true)
        }
        Action::Export(path) => {
            // As the Export button: the take-off copied out, so writing doesn't hold the lock.
            let (items, totals, title, report) = with_session(state, |s| {
                let title = match s.drawing.path.file_name() {
                    Some(name) => format!("{} take-off", name.to_string_lossy()),
                    None => "Take-off".to_string(),
                };
                Ok((
                    s.takeoff.items.clone(),
                    s.takeoff.totals(),
                    title,
                    s.measurement_report()?,
                ))
            })?;
            qiyas_boq::export_takeoff_with_report(path, &items, &totals, &title, &report)
                .map_err(|e| e.to_string())?;
            (format!("exported {}", path.display()), false)
        }
        Action::ImportBoq(path) => {
            let boq = qiyas_boq::import_boq(path).map_err(|e| e.to_string())?;
            let items = boq.items.len();
            with_session(state, |s| s.run(Command::SetBoq { boq }))?;
            (
                format!("imported {items} BOQ items from {}", path.display()),
                true,
            )
        }
        Action::Compare => {
            let conflicts = with_session(state, |s| {
                qiyas_takeoff::compare_session(s, qiyas_takeoff::DEFAULT_TOLERANCE, Source::User)
            })?;
            (format!("{} conflicts", conflicts.len()), true)
        }
        Action::Command(cmd) => {
            let _turn = if crate::commands::command_changes_sheet(cmd) {
                Some(crate::commands::window_sheet_guard(state)?)
            } else {
                None
            };
            let input = step.input.clone().unwrap_or_else(|| serde_json::json!(cmd));
            let outcome = with_session(state, |s| s.run_input(&input, Source::User))?;
            let change = !matches!(
                &outcome.outcome,
                Outcome::Nothing | Outcome::Info { .. } | Outcome::Table { .. }
            );
            (describe(cmd, outcome), change)
        }
    })
}

/// OPEN: the drawing's tab is shown when it is open already; else it opens in a new tab, as the
/// Open button does (the assistant starts a new conversation unless `keep_conversation`, the
/// assistant's own open; the file goes first in recents). A drawing whose autosave the user must
/// decide about first is refused (see below); so is it for the assistant (`drawing_host`).
pub(crate) fn open<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    path: &Path,
    keep_conversation: bool,
) -> Result<String, String> {
    let shown = path.display().to_string();
    if let Some(id) = tabs::find_path(state, path) {
        tabs::activate(app, state, id, keep_conversation)?;
        remember_recent(state, &shown);
        return Ok(format!("showed {shown}, open already"));
    }
    // The window asks about these before opening; a script can't, and a SAVE after it would
    // delete the autosave or write over the other Qiyas's drawing. Older copies alone (crash
    // copies, ended instances' autosaves) are kept by a save: the script may open the drawing.
    if let Some(found) = qiyas_core::recoverable(path) {
        let why = if let Some(lock) = &found.locked_elsewhere {
            Some(format!(
                "it is open in another Qiyas (PID {} on {})",
                lock.pid, lock.host
            ))
        } else if found.crashed {
            Some("Qiyas closed unexpectedly with it open".to_string())
        } else if found.autosave.is_some() {
            Some(match (found.saved_at, found.drawing_at) {
                (Some(saved), Some(drawn)) if saved < drawn => {
                    "it has an autosave, older than the drawing".to_string()
                }
                _ => "it has an autosave newer than it".to_string(),
            })
        } else {
            None
        };
        if let Some(why) = why {
            return Err(format!(
                "{shown}: {why}; open it from the ribbon first to decide about its autosave"
            ));
        }
    }
    let session = Session::open(path).map_err(|e| e.to_string())?;
    let problems = read_problems(&session);
    if !keep_conversation {
        new_conversation(app, state);
    }
    tabs::add(state, session)?;
    remember_recent(state, &shown);
    Ok(match problems {
        0 => format!("opened {shown}"),
        1 => format!("opened {shown} (1 problem reading it)"),
        n => format!("opened {shown} ({n} problems reading it)"),
    })
}

/// What `cmd` did, in a line, as `qiyas-cli run` says it (`qiyas_core::script` keeps its own
/// private).
fn describe(cmd: &Command, outcome: Reported) -> String {
    let notes = outcome.input_notes();
    let message = match outcome.outcome {
        Outcome::Measured { measurement: m } => {
            let shown = match m.kind {
                Kind::Count => format!("{:.0}", m.shown),
                _ => format!("{:.3}", m.shown),
            };
            format!("{} = {shown} {}", m.description, m.unit)
        }
        Outcome::Changed => match cmd {
            Command::SetLayerHidden { name, hidden } => {
                format!("layer {name} {}", if *hidden { "off" } else { "on" })
            }
            Command::SetLayer {
                name,
                prop: LayerProp::Frozen,
                value,
            } => format!(
                "layer {name} {}",
                if value.as_bool() == Some(true) {
                    "frozen"
                } else {
                    "thawed"
                }
            ),
            _ => "done".to_string(),
        },
        Outcome::Added { handles } => format!("added {} entities", handles.len()),
        Outcome::TitleBlock {
            handles,
            empty_fields,
        } => format!(
            "title block: {} entities; empty fields: {}",
            handles.len(),
            empty_fields.join(", ")
        ),
        Outcome::Batch { outcomes, added } => format!(
            "ran {} commands as one step, added {} entities",
            outcomes.len(),
            added.len()
        ),
        Outcome::Nothing => "nothing to do".to_string(),
        other => format!("{other:?}"),
    };
    if notes.is_empty() {
        message
    } else {
        format!("{message}; {notes}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::webview::InvokeRequest;
    use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

    use crate::state::AppState;

    /// The app on the mock runtime with the sample drawing (`sample.dxf`) open in a tab.
    struct Ipc {
        dir: tempfile::TempDir,
        app: App<MockRuntime>,
        webview: WebviewWindow<MockRuntime>,
    }

    impl Ipc {
        fn open() -> Ipc {
            let dir = tempfile::tempdir().unwrap();
            let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
            let app = mock_builder()
                .invoke_handler(tauri::generate_handler![
                    crate::commands::open_drawing,
                    crate::commands::drawing_info,
                    crate::commands::recents,
                    crate::commands::run_command,
                    crate::commands::takeoff,
                    crate::commands::undo,
                    crate::commands::pick,
                    crate::tabs::tabs,
                    crate::tabs::activate_tab,
                    crate::tabs::close_tab,
                    super::run_script,
                    super::stop_script
                ])
                .build(mock_context(noop_assets()))
                .unwrap();
            app.manage(AppState::new(dir.path().join("config")));
            app.manage(super::ScriptControl::default());
            let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .unwrap();
            let ipc = Ipc { dir, app, webview };
            ipc.call("open_drawing", json!({ "path": path })).unwrap();
            ipc
        }

        fn call(&self, cmd: &str, body: Value) -> Result<Value, Value> {
            let request = InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            get_ipc_response(&self.webview, request).map(|b| b.deserialize().unwrap())
        }

        /// Writes `text` as the script `name` beside the drawing and runs it.
        fn run(&self, name: &str, text: &str) -> Result<Value, Value> {
            let script = self.path(name);
            std::fs::write(&script, text).unwrap();
            self.call("run_script", json!({ "path": script }))
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn takeoff_len(&self) -> usize {
            self.call("takeoff", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        }
    }

    /// The lines of a result as (line, text, ok, message).
    fn lines(result: &Value) -> Vec<(u64, String, bool, String)> {
        result["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["line_no"].as_u64().unwrap(),
                    l["text"].as_str().unwrap().to_string(),
                    l["ok"].as_bool().unwrap(),
                    l["message"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn line(no: u64, text: &str, ok: bool, message: &str) -> (u64, String, bool, String) {
        (no, text.to_string(), ok, message.to_string())
    }

    #[test]
    fn a_script_runs_on_the_drawing_on_screen_one_undo_step_per_command() {
        let ipc = Ipc::open();
        let result = ipc
            .run(
                "count.scr",
                "; one undo step per command\n\
                 {\"type\":\"draw\",\"kind\":\"line\",\"a\":[0,0],\"b\":[0,5]}\n\
                 COUNT DOOR\n\
                 DI 0,0\n\
                 3,4\n\
                 LAYOFF A-WALL\n\
                 ECHO hello there\n\
                 QUIT\n\
                 COUNT DOOR\n",
            )
            .unwrap();
        assert_eq!(
            lines(&result),
            [
                line(
                    2,
                    "{\"type\":\"draw\",\"kind\":\"line\",\"a\":[0,0],\"b\":[0,5]}",
                    true,
                    "added 1 entities"
                ),
                line(3, "COUNT DOOR", true, "DOOR count = 3 pcs"),
                line(4, "DI 0,0", true, "Distance = 5.000 m"),
                line(6, "LAYOFF A-WALL", true, "layer A-WALL off"),
                line(7, "ECHO hello there", true, "hello there"),
                line(8, "QUIT", true, "quit"),
            ]
        );
        assert_eq!(result["changed"], json!(true));
        assert!(result["took_ms"].is_u64());

        // QUIT ended the script, not the app: the drawing is still on screen with what it did.
        let info = ipc.call("drawing_info", json!({})).unwrap();
        assert!(info["path"].as_str().unwrap().ends_with("sample.dxf"));
        assert_eq!(info["hidden_layers"], json!(["A-WALL"]));
        assert_eq!(ipc.takeoff_len(), 2);
        // The line, the count and the distance are one undo step each (hiding a layer is none).
        assert_eq!(ipc.call("undo", json!({})).unwrap(), json!(true));
        assert_eq!(ipc.takeoff_len(), 1);
        assert_eq!(ipc.call("undo", json!({})).unwrap(), json!(true));
        assert_eq!(ipc.takeoff_len(), 0);
        assert_eq!(ipc.call("undo", json!({})).unwrap(), json!(true));
        assert_eq!(ipc.call("undo", json!({})).unwrap(), json!(false));
    }

    #[test]
    fn open_shows_a_tab_and_saves_go_as_the_app_does() {
        let ipc = Ipc::open();
        let dir = ipc.dir.path().to_path_buf();
        std::fs::create_dir(dir.join("sub")).unwrap();
        qiyas_core::fixtures::write_sample(&dir.join("sub"), "dwg");
        let shown = |name: &str| dir.join(name).canonicalize().unwrap().display().to_string();

        // Before OPEN takes another drawing on screen, the take-off of the one the script changed
        // is saved beside it, as the window does after every command.
        assert!(!dir.join("sample.qiyas").exists());
        let result = ipc
            .run("open.scr", "COUNT DOOR\nOPEN sub/sample.dwg\n")
            .unwrap();
        assert_eq!(
            lines(&result),
            [
                line(1, "COUNT DOOR", true, "DOOR count = 3 pcs"),
                line(
                    2,
                    "OPEN sub/sample.dwg",
                    true,
                    &format!("opened {}", shown("sub/sample.dwg"))
                ),
            ]
        );
        assert!(std::fs::read_to_string(dir.join("sample.qiyas"))
            .unwrap()
            .contains("DOOR"));

        let result = ipc
            .run(
                "files.scr",
                "COUNT DOOR\nSAVEAS sub/copy dxf\nOPEN sample.dxf\nSAVE\nDELAY 5\n",
            )
            .unwrap();
        assert_eq!(
            lines(&result),
            [
                line(1, "COUNT DOOR", true, "DOOR count = 3 pcs"),
                line(
                    2,
                    "SAVEAS sub/copy dxf",
                    true,
                    &format!("saved {}", shown("sub/copy.dxf"))
                ),
                line(
                    3,
                    "OPEN sample.dxf",
                    true,
                    &format!("showed {}, open already", shown("sample.dxf"))
                ),
                line(4, "SAVE", true, &format!("saved {}", shown("sample.dxf"))),
                line(5, "DELAY 5", true, "waited 5 ms"),
            ]
        );
        assert_eq!(result["changed"], json!(true));

        // Two tabs: the drawing the first script started on, on screen again, and the one its
        // OPEN made, now the copy.
        let tabs = ipc.call("tabs", json!({})).unwrap();
        let titles: Vec<&str> = tabs["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["sample.dxf", "copy.dxf"]);
        assert_eq!(tabs["active"], tabs["tabs"][0]["id"]);
        assert_eq!(ipc.takeoff_len(), 1);
        // Save As wrote the copy and its project; Save kept the original as .bak.
        assert!(dir.join("sub/copy.dxf").exists());
        assert!(dir.join("sub/copy.qiyas").exists());
        assert!(dir.join("sample.dxf.bak").exists());
        // Both went first in the recent list, as the app's Open and Save As do.
        let recents = ipc.call("recents", json!({})).unwrap();
        let recent: Vec<&str> = recents
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect();
        assert_eq!(&recent[..2], ["sample.dxf", "copy.dxf"]);
    }

    #[test]
    fn a_bad_line_stops_the_script_and_says_why() {
        let ipc = Ipc::open();
        // A line that doesn't read: nothing runs.
        let result = ipc.run("bad.scr", "COUNT DOOR\n\nFOO bar\n").unwrap();
        assert_eq!(
            lines(&result),
            [line(3, "FOO bar", false, "unknown command FOO")]
        );
        assert_eq!(result["changed"], json!(false));
        assert_eq!(result["ran"], json!(false), "nothing ran");
        assert_eq!(ipc.takeoff_len(), 0);

        // A path outside the script's and the drawing's folders: refused before anything runs.
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("x.xlsx").display().to_string();
        let result = ipc
            .run("out.scr", &format!("COUNT DOOR\nEXPORT {outside}\n"))
            .unwrap();
        let got = lines(&result);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].0, got[0].2), (2, false));
        assert!(got[0].3.contains("outside the script's folder"), "{got:?}");
        assert!(!Path::new(&outside).exists());
        assert_eq!(ipc.takeoff_len(), 0);

        // A step that fails: the steps before it stay done, those after it never run.
        let result = ipc
            .run("fails.scr", "COUNT DOOR\nMEASURE FFFF\nCOUNT DOOR\n")
            .unwrap();
        let got = lines(&result);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0], line(1, "COUNT DOOR", true, "DOOR count = 3 pcs"));
        assert_eq!((got[1].0, got[1].2), (2, false));
        assert!(!got[1].3.is_empty());
        assert_eq!(result["changed"], json!(true));
        assert_eq!(ipc.takeoff_len(), 1);

        // A script that isn't there.
        let missing = ipc.path("none.scr").display().to_string();
        let error = ipc
            .call("run_script", json!({ "path": missing }))
            .unwrap_err();
        assert!(
            error.as_str().unwrap().contains("could not be read"),
            "{error}"
        );
    }

    #[test]
    fn a_drawing_repaired_on_open_is_never_saved_over() {
        let ipc = Ipc::open();
        // As if the failsafe reader had dropped records reading it.
        {
            let state = ipc.app.state::<AppState>();
            let mut session = state.session.lock().unwrap();
            session
                .as_mut()
                .unwrap()
                .drawing
                .report
                .dropped
                .push(qiyas_core::Dropped {
                    kind: "LINE".to_string(),
                    count: 2,
                });
        }
        let dir = ipc.dir.path().to_path_buf();
        for (name, text) in [
            ("save.scr", "COUNT DOOR\nSAVE\n"),
            ("over.scr", "SAVEAS sample.dxf\n"),
        ] {
            let result = ipc.run(name, text).unwrap();
            let got = lines(&result);
            let last = got.last().unwrap();
            assert!(!last.2, "{got:?}");
            assert!(last.3.contains("repaired"), "{got:?}");
        }
        assert!(!dir.join("sample.dxf.bak").exists());

        // Saved as a new file, it is what Qiyas wrote: SAVE goes over that one.
        let result = ipc.run("new.scr", "SAVEAS fixed.dxf\nSAVE\n").unwrap();
        assert!(
            result["lines"]
                .as_array()
                .unwrap()
                .iter()
                .all(|l| l["ok"] == json!(true)),
            "{result}"
        );
        assert!(dir.join("fixed.dxf.bak").exists());
    }

    #[test]
    fn open_leaves_a_drawing_with_an_autosave_or_open_elsewhere_to_the_window() {
        let ipc = Ipc::open();
        let dir = ipc.dir.path().to_path_buf();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let other = qiyas_core::fixtures::write_sample(&dir.join("sub"), "dwg");
        // An autosave newer than the drawing: the window asks about it first; a script can't.
        let autosave = qiyas_core::autosave_path(&other);
        std::fs::write(&autosave, b"autosave").unwrap();
        let later = std::fs::metadata(&other).unwrap().modified().unwrap()
            + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&autosave)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let result = ipc.run("open.scr", "OPEN sub/sample.dwg\nSAVE\n").unwrap();
        let got = lines(&result);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].0, got[0].2), (1, false));
        assert!(
            got[0].3.contains("open it from the ribbon first"),
            "{got:?}"
        );
        assert!(autosave.exists(), "SAVE never ran, so the autosave stays");
        assert_eq!(
            ipc.call("tabs", json!({})).unwrap()["tabs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Open in another Qiyas (another computer's lock): refused too.
        std::fs::remove_file(&autosave).unwrap();
        std::fs::write(
            qiyas_core::lock_path(&other),
            r#"{"pid": 7, "host": "elsewhere", "started_at": 1}"#,
        )
        .unwrap();
        let result = ipc.run("open.scr", "OPEN sub/sample.dwg\nSAVE\n").unwrap();
        let got = lines(&result);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].3.contains("another Qiyas"), "{got:?}");
        assert!(!dir.join("sub/sample.dwg.bak").exists());
    }

    #[test]
    fn delay_is_capped_in_the_window_and_stop_ends_it() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};
        // Capped: a long DELAY waits the cap and says so.
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let said = super::wait(5_000, 50, &stop).unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(said.contains("at most"), "{said}");
        assert_eq!(super::wait(20, 50, &stop).unwrap(), "waited 20 ms");
        assert_eq!(super::DELAY_CAP_MS, 10_000);

        // Stop during a DELAY: the run ends on that line, the lines after it never run.
        let ipc = Ipc::open();
        let script = ipc.path("long.scr");
        std::fs::write(&script, "COUNT DOOR\nDELAY 9000\nCOUNT DOOR\n").unwrap();
        let app = ipc.app.handle().clone();
        let running = std::thread::spawn(move || super::run_file(&app, &script));
        std::thread::sleep(Duration::from_millis(300));
        let start = Instant::now();
        ipc.call("stop_script", json!({})).unwrap();
        let result = running.join().unwrap().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        let got: Vec<_> = result.lines.iter().map(|l| (l.line_no, l.ok)).collect();
        assert_eq!(got, [(1, true), (2, false)]);
        assert!(
            result.lines[1].message.contains("stopped"),
            "{:?}",
            result.lines
        );
        assert_eq!(ipc.takeoff_len(), 1);
        assert!(ipc
            .app
            .state::<super::ScriptControl>()
            .stop
            .load(Ordering::Relaxed));

        // The next run starts afresh.
        let result = ipc.run("again.scr", "COUNT DOOR\n").unwrap();
        assert_eq!(result["lines"][0]["ok"], json!(true));
    }

    #[test]
    fn open_takes_a_drawing_with_only_older_copies_and_names_an_older_autosave() {
        let ipc = Ipc::open();
        let dir = ipc.dir.path().to_path_buf();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let other = qiyas_core::fixtures::write_sample(&dir.join("sub"), "dwg");
        // A crash copy, offered once already: a save keeps it, so the script may open it.
        let mut copy = other.clone().into_os_string();
        copy.push(".sv$.crash-20261001-101010");
        std::fs::write(&copy, b"crash copy").unwrap();
        assert!(qiyas_core::recoverable(&other).is_some());
        let result = ipc.run("open.scr", "OPEN sub/sample.dwg\n").unwrap();
        assert_eq!(result["lines"][0]["ok"], json!(true), "{result}");
        assert!(Path::new(&copy).exists());

        // An autosave older than the drawing: refused, and said so.
        let tabs = ipc.call("tabs", json!({})).unwrap();
        let first = tabs["tabs"][0]["id"].clone();
        let second = tabs["tabs"][1]["id"].clone();
        ipc.call("activate_tab", json!({ "id": first })).unwrap();
        ipc.call("close_tab", json!({ "id": second })).unwrap();
        let autosave = qiyas_core::autosave_path(&other);
        std::fs::write(&autosave, b"autosave").unwrap();
        let earlier = std::fs::metadata(&other).unwrap().modified().unwrap()
            - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&autosave)
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        let result = ipc.run("open.scr", "OPEN sub/sample.dwg\n").unwrap();
        let got = lines(&result);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(!got[0].2);
        assert!(
            got[0]
                .3
                .contains("it has an autosave, older than the drawing"),
            "{got:?}"
        );
    }

    #[test]
    fn a_script_stops_when_another_tab_comes_on_screen() {
        use std::time::Duration;
        let ipc = Ipc::open();
        let dir = ipc.dir.path().to_path_buf();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let other = qiyas_core::fixtures::write_sample(&dir.join("sub"), "dxf");
        ipc.call("open_drawing", json!({ "path": other })).unwrap();
        let tabs = ipc.call("tabs", json!({})).unwrap();
        let first = tabs["tabs"][0]["id"].clone();
        let second = tabs["tabs"][1]["id"].clone();
        assert_eq!(tabs["active"], second);

        // The script starts on the second drawing; the first comes on screen during its DELAY.
        let script = ipc.path("switch.scr");
        std::fs::write(&script, "DELAY 600\nLAYOFF A-WALL\nSAVE\n").unwrap();
        let app = ipc.app.handle().clone();
        let running = std::thread::spawn(move || super::run_file(&app, &script));
        std::thread::sleep(Duration::from_millis(200));
        ipc.call("activate_tab", json!({ "id": first })).unwrap();
        let result = running.join().unwrap().unwrap();
        let got: Vec<_> = result
            .lines
            .iter()
            .map(|l| (l.line_no, l.ok, l.message.clone()))
            .collect();
        assert_eq!(
            got,
            [
                (1, true, "waited 600 ms".to_string()),
                (
                    2,
                    false,
                    "the drawing on screen changed: stopped before this line".to_string()
                ),
            ]
        );
        // Neither drawing was changed or saved.
        let info = ipc.call("drawing_info", json!({})).unwrap();
        assert_eq!(info["hidden_layers"], json!([]));
        assert!(!dir.join("sample.dxf.bak").exists());
        assert!(!dir.join("sub/sample.dxf.bak").exists());

        // OPEN moves the script to the tab it shows: the lines after it run there.
        let result = ipc
            .run("open.scr", "OPEN sub/sample.dxf\nLAYOFF A-WALL\n")
            .unwrap();
        assert!(
            result["lines"]
                .as_array()
                .unwrap()
                .iter()
                .all(|l| l["ok"] == json!(true)),
            "{result}"
        );
        let tabs = ipc.call("tabs", json!({})).unwrap();
        assert_eq!(tabs["active"], second);
        let info = ipc.call("drawing_info", json!({})).unwrap();
        assert_eq!(info["hidden_layers"], json!(["A-WALL"]));
    }

    #[test]
    fn a_script_waits_for_the_block_edit_in_place_to_end() {
        let ipc = Ipc::open();
        // The first DOOR's leaf runs from (1,1) to (1.9,1).
        let picked = ipc
            .call("pick", json!({ "x": 1.5, "y": 1.0, "tol": 0.05 }))
            .unwrap();
        let target = json!({ "handle": picked["top"], "chain": [] });
        ipc.call(
            "run_command",
            json!({ "cmd": { "type": "edit_block_in_place", "target": target } }),
        )
        .unwrap();
        let error = ipc.run("count.scr", "COUNT DOOR\n").unwrap_err();
        assert!(
            error.as_str().unwrap().contains("being edited in place"),
            "{error}"
        );
        assert_eq!(ipc.takeoff_len(), 0);

        ipc.call(
            "run_command",
            json!({ "cmd": { "type": "discard_block_edits" } }),
        )
        .unwrap();
        let result = ipc.run("count.scr", "COUNT DOOR\n").unwrap();
        assert_eq!(result["lines"][0]["ok"], json!(true));
    }

    #[test]
    fn command_ipc_reports_all_missing_fields_and_keeps_unknown_field_warnings() {
        let ipc = Ipc::open();
        let error = ipc
            .call(
                "run_command",
                json!({"cmd": {"type": "draw", "kind": "line"}}),
            )
            .unwrap_err();
        let message = error.as_str().unwrap();
        assert!(message.contains("/a"), "{message}");
        assert!(message.contains("/b"), "{message}");
        let outcome = ipc
            .call(
                "run_command",
                json!({"cmd": {
                    "type": "draw", "kind": "line", "a": [0,0], "b": [10,0],
                    "unexpected": true
                }}),
            )
            .unwrap();
        assert_eq!(outcome["type"], "added");
        assert_eq!(outcome["warnings"][0]["code"], "unknown_field");
        assert_eq!(outcome["warnings"][0]["pointer"], "/unexpected");
    }

    #[test]
    fn script_logs_keep_defaults_and_unknown_fields_from_raw_input() {
        let ipc = Ipc::open();
        let result = ipc.run(
            "input-notes.scr",
            "{\"type\":\"text\",\"at\":[0,0],\"value\":\"Room\",\"typo\":true}\nTEXT 2,0 Door\n",
        ).unwrap();
        let lines = result["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 2);
        for line in lines {
            assert_eq!(line["ok"], true, "{line}");
            let text = line["message"].as_str().unwrap();
            assert!(text.contains("/height="), "{text}");
            assert!(text.contains("/rotation=0"), "{text}");
        }
        assert!(lines[0]["message"].as_str().unwrap().contains("/typo"));
    }
}
