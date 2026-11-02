//! Drawings the OS asks Qiyas to open. A double click on a `.dwg`, `.dxf` or `.qiyas` file (the
//! bundle's file associations) starts Qiyas with its path; when Qiyas is running already, the
//! new Qiyas hands its arguments to this one through `tauri-plugin-single-instance` and quits,
//! and this window comes to the front. macOS sends the files as an `Opened` event instead. The
//! paths wait in `OpenRequests` until the window takes them (`take_open_requests`, after
//! `OPEN_REQUESTED`) and opens each as File, Open does, so a newer autosave is offered first.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Url};

/// Tells the window that paths wait in `OpenRequests`.
pub const OPEN_REQUESTED: &str = "open:requested";

/// The drawings Qiyas opens itself.
const DRAWINGS: [&str; 2] = ["dwg", "dxf"];

/// Paths the OS asked to open, oldest first, until the window takes them.
#[derive(Default)]
pub struct OpenRequests(Mutex<Vec<String>>);

/// The drawings `args` (a launch's arguments after the program) name: existing `.dwg` and
/// `.dxf` files, relative ones under `cwd`, `file://` addresses as their files; a `.qiyas`
/// project gives the drawing it belongs to. Options (`-x`, `--x`), folders, other files and
/// missing ones are left out. Paths come tidied (`tidy`), so a drawing open already is found.
pub fn drawing_paths(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    args.iter()
        .filter(|a| !a.is_empty() && !a.starts_with('-'))
        .filter_map(|a| {
            // Only `file:` is read as an address: `C:\plan.dwg` parses as one too.
            if a.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
                Url::parse(a).ok()?.to_file_path().ok()
            } else {
                Some(cwd.join(a))
            }
        })
        .map(|path| tidy(&path))
        .filter_map(|path| {
            if has_extension(&path, &["qiyas"]) {
                drawing_of_project(&path)
            } else {
                (has_extension(&path, &DRAWINGS) && path.is_file()).then_some(path)
            }
        })
        .collect()
}

/// `path` with `.` dropped and each `..` taking off the folder before it, without reading the
/// disk: no links followed, and no `\\?\` on Windows as `fs::canonicalize` gives.
fn tidy(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out
}

/// The arguments that are text; one that isn't (a file name in an old code page) is left out
/// and logged, never a panic as `std::env::args` would.
fn text_args(args: impl IntoIterator<Item = OsString>) -> Vec<String> {
    args.into_iter()
        .filter_map(|a| {
            a.into_string()
                .map_err(|a| eprintln!("Not opened, its name is not text: {}", a.to_string_lossy()))
                .ok()
        })
        .collect()
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The `drawing` field of a `.qiyas` project; the rest is not read.
#[derive(Deserialize)]
struct ProjectDrawing {
    drawing: String,
}

/// The drawing whose project file is `project`: the one the project names, else the same name
/// with `.dwg` or `.dxf`. A drawing's project is always `<stem>.qiyas` beside it
/// (`qiyas_core::project_path`), so a name pointing elsewhere is not it.
fn drawing_of_project(project: &Path) -> Option<PathBuf> {
    let named = fs::read_to_string(project)
        .ok()
        .and_then(|text| serde_json::from_str::<ProjectDrawing>(&text).ok())
        .map(|p| project.with_file_name(p.drawing));
    named
        .into_iter()
        .chain(["dwg", "dxf", "DWG", "DXF"].map(|e| project.with_extension(e)))
        .find(|d| {
            qiyas_core::project_path(d) == project && has_extension(d, &DRAWINGS) && d.is_file()
        })
}

/// Queues `paths` for the window and tells it.
fn request<R: Runtime>(app: &AppHandle<R>, paths: Vec<PathBuf>) {
    let paths: Vec<String> = paths
        .into_iter()
        .filter_map(|p| p.into_os_string().into_string().ok())
        .collect();
    if paths.is_empty() {
        return;
    }
    let requests = app.state::<OpenRequests>();
    requests
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(paths);
    let _ = app.emit(OPEN_REQUESTED, ());
}

/// This launch's own files (a double click on a drawing while Qiyas was closed).
pub fn request_launch_files<R: Runtime>(app: &AppHandle<R>) {
    let args = text_args(std::env::args_os().skip(1));
    let cwd = std::env::current_dir().unwrap_or_default();
    request(app, drawing_paths(&args, &cwd));
}

/// The single-instance plugin, first of all: a second Qiyas hands its files to this one, which
/// comes to the front. A debug build listens under its own name on Linux, so `pnpm tauri dev`
/// still starts while an installed Qiyas runs.
pub fn single_instance<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri_plugin_single_instance::Builder::new().callback(|app, argv, cwd| {
        let args = argv.get(1..).unwrap_or_default();
        request(app, drawing_paths(args, Path::new(&cwd)));
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
    #[cfg(debug_assertions)]
    let builder = builder.dbus_id("com.qsmind.qiyas.dev");
    builder.build()
}

/// On macOS, files opened from Finder come as an `Opened` event, not as arguments.
pub fn os_opened<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("qiyas-open-files")
        .on_event(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = _event {
                let args: Vec<String> = urls.iter().map(Url::to_string).collect();
                request(_app, drawing_paths(&args, Path::new("/")));
            }
        })
        .build()
}

/// The paths waiting to open, oldest first; the queue is empty after.
#[tauri::command]
pub fn take_open_requests(requests: State<'_, OpenRequests>) -> Vec<String> {
    std::mem::take(&mut *requests.0.lock().unwrap_or_else(PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A folder with `plan.dwg`, `site.DXF`, `notes.txt` and a `sub/` folder named `sub.dwg`.
    fn folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in ["plan.dwg", "site.DXF", "notes.txt"] {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        fs::create_dir(dir.path().join("sub.dwg")).unwrap();
        dir
    }

    #[test]
    fn existing_drawings_are_kept_relative_ones_under_the_launch_folder() {
        let dir = folder();
        let d = dir.path();
        let absolute = d.join("plan.dwg").display().to_string();
        assert_eq!(
            drawing_paths(&args(&[&absolute, "site.DXF"]), d),
            [d.join("plan.dwg"), d.join("site.DXF")]
        );
    }

    #[test]
    fn options_folders_other_files_and_missing_ones_are_left_out() {
        let dir = folder();
        let d = dir.path();
        let found = drawing_paths(
            &args(&[
                "--flag",
                "-psn_0_12345",
                "sub.dwg",
                "notes.txt",
                "gone.dwg",
                "",
                "plan",
                "https://qsmind.app/plan.dwg",
                "file:///nowhere/plan.dwg",
                "plan.dwg",
            ]),
            d,
        );
        assert_eq!(found, [d.join("plan.dwg")]);
    }

    #[test]
    fn paths_are_tidied_so_an_open_drawing_is_found() {
        let dir = folder();
        let d = dir.path();
        assert_eq!(
            drawing_paths(&args(&["./plan.dwg", "sub.dwg/../plan.dwg"]), d),
            [d.join("plan.dwg"), d.join("plan.dwg")]
        );
        assert_eq!(tidy(Path::new("/a/./b/../../c.dwg")), Path::new("/c.dwg"));
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_text_is_left_out() {
        use std::os::unix::ffi::OsStringExt;
        let args = vec![
            OsString::from_vec(b"caf\xe9.dwg".to_vec()),
            OsString::from("plan.dwg"),
        ];
        assert_eq!(text_args(args), ["plan.dwg"]);
    }

    #[test]
    fn windows_leaves_dwg_and_dxf_to_autocad_and_the_others_only_offer_qiyas() {
        let names = |text: &str| -> Vec<(String, String)> {
            let conf: serde_json::Value = serde_json::from_str(text).unwrap();
            conf["bundle"]["fileAssociations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    let rank = a["rank"].as_str().unwrap_or("Default");
                    (a["name"].as_str().unwrap().to_string(), rank.to_string())
                })
                .collect()
        };
        let pair = |n: &str, r: &str| (n.to_string(), r.to_string());
        assert_eq!(
            names(include_str!("../tauri.conf.json")),
            [
                pair("Qiyas drawing (DWG)", "Alternate"),
                pair("Qiyas drawing (DXF)", "Alternate"),
                pair("Qiyas project", "Owner"),
            ]
        );
        // The Windows file replaces the list (a JSON merge patch replaces arrays): the NSIS and
        // MSI installers make every association they get the default.
        assert_eq!(
            names(include_str!("../tauri.windows.conf.json")),
            [pair("Qiyas project", "Default")]
        );
    }

    #[test]
    fn window_content_security_policies_are_restricted() {
        let base: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        for (name, text) in [
            ("tauri.conf.json", include_str!("../tauri.conf.json")),
            (
                "tauri.bundle.conf.json",
                include_str!("../tauri.bundle.conf.json"),
            ),
            (
                "tauri.windows.conf.json",
                include_str!("../tauri.windows.conf.json"),
            ),
            (
                "tauri.fast.conf.json",
                include_str!("../tauri.fast.conf.json"),
            ),
            (
                "tauri.updater.conf.json",
                include_str!("../tauri.updater.conf.json"),
            ),
        ] {
            let conf: serde_json::Value = serde_json::from_str(text).unwrap();
            let Some(security) = conf["app"].get("security") else {
                continue;
            };
            assert!(security.is_object(), "{name}: security must be an object");
            for key in ["csp", "devCsp"] {
                // Merge patches inherit omitted fields, but null disables a policy.
                let policy = security
                    .get(key)
                    .unwrap_or(&base["app"]["security"][key])
                    .as_str()
                    .unwrap_or_else(|| panic!("{name}: {key} must be a CSP string"));
                assert!(!policy.contains("'unsafe-eval'"), "{name}: {key}");
                let directives: Vec<Vec<&str>> = policy
                    .split(';')
                    .map(|directive| directive.split_whitespace().collect())
                    .filter(|directive: &Vec<&str>| !directive.is_empty())
                    .collect();
                for required in ["base-uri", "form-action", "object-src"] {
                    let matching: Vec<_> = directives
                        .iter()
                        .filter(|directive| directive[0] == required)
                        .collect();
                    assert_eq!(
                        matching,
                        [&vec![required, "'none'"]],
                        "{name}: {key} must restrict {required} to 'none'"
                    );
                }
                for directive in &directives {
                    if directive[0] == "script-src" {
                        assert_eq!(directive, &["script-src", "'self'"], "{name}: {key}");
                    }
                    for source in &directive[1..] {
                        if source.starts_with("http:")
                            || source.starts_with("https:")
                            || source.starts_with("ws:")
                            || source.starts_with("wss:")
                        {
                            assert!(
                                *source == "http://ipc.localhost"
                                    || (key == "devCsp"
                                        && matches!(
                                            *source,
                                            "http://localhost:1420" | "ws://localhost:1420"
                                        )),
                                "{name}: {key} has unexpected origin {source}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn window_ipc_permissions_match_handlers_and_frontend_calls() {
        use std::collections::BTreeSet;

        // Deliberately parse the literal lists, so adding a handler without declaring its
        // permission cannot silently leave it outside the app manifest.
        let handlers: BTreeSet<_> = include_str!("lib.rs")
            .split_once("tauri::generate_handler![")
            .unwrap()
            .1
            .split_once(']')
            .unwrap()
            .0
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.rsplit("::").next().unwrap())
            .collect();
        let manifest: BTreeSet<_> = include_str!("../build.rs")
            .split_once(".commands(&[")
            .unwrap()
            .1
            .split_once(']')
            .unwrap()
            .0
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.strip_prefix('"').unwrap().strip_suffix('"').unwrap())
            .collect();
        assert!(!handlers.is_empty());
        assert_eq!(
            handlers, manifest,
            "build.rs must declare every app handler"
        );

        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let allowed: BTreeSet<_> = capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|permission| permission.as_str().unwrap().strip_prefix("allow-"))
            .map(|command| command.replace('-', "_"))
            .collect();
        for command in &allowed {
            assert!(
                handlers.contains(command.as_str()),
                "capability allows an unregistered handler: {command}"
            );
        }

        fn window_calls(dir: &Path, calls: &mut BTreeSet<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    window_calls(&path, calls);
                    continue;
                }
                if !matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("ts" | "tsx")
                ) || path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .contains(".test.")
                {
                    continue;
                }
                let source = fs::read_to_string(&path).unwrap();
                // Both direct invoke("name") and typed invoke<Result>("name") calls,
                // including nested generic types and calls split across lines.
                for (offset, _) in source.match_indices("invoke") {
                    let before = &source[..offset];
                    if before.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
                        || before.trim_end().ends_with("function")
                    {
                        continue;
                    }
                    let mut tail = source[offset + "invoke".len()..].trim_start();
                    if tail.starts_with('<') {
                        let mut depth = 0;
                        let end = tail.char_indices().find_map(|(index, ch)| {
                            match ch {
                                '<' => depth += 1,
                                '>' => depth -= 1,
                                _ => {}
                            }
                            (depth == 0).then_some(index)
                        });
                        tail = tail[end.expect("unclosed invoke type argument") + 1..].trim_start();
                    }
                    let Some(arguments) = tail.strip_prefix('(') else {
                        continue;
                    };
                    let arguments = arguments.trim_start();
                    let quote = arguments.chars().next().unwrap();
                    assert!(
                        matches!(quote, '\'' | '"' | '`'),
                        "{}: invoke command must be a literal for the permission audit",
                        path.display()
                    );
                    let command = arguments[1..].split(quote).next().unwrap();
                    assert!(!command.contains("${"), "dynamic invoke command: {command}");
                    calls.insert(command.to_string());
                }
            }
        }

        let mut calls = BTreeSet::new();
        window_calls(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src"),
            &mut calls,
        );
        assert!(!calls.is_empty(), "frontend IPC audit found no calls");
        assert_eq!(
            calls, allowed,
            "allow exactly the commands invoked by the window (including typed wrappers)"
        );
    }

    #[test]
    fn default_capability_is_limited_to_the_main_window_and_known_permissions() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        assert_eq!(
            capability["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|permission| permission.as_str().unwrap())
                // App permissions are checked against handlers and frontend calls above.
                .filter(|permission| !permission.starts_with("allow-"))
                .collect::<Vec<_>>(),
            [
                "core:default",
                "dialog:default",
                "dialog:allow-open",
                "dialog:allow-save",
                "core:window:allow-destroy"
            ]
        );
    }

    #[test]
    fn file_addresses_are_read_as_their_files() {
        let dir = folder();
        let url = Url::from_file_path(dir.path().join("plan.dwg")).unwrap();
        assert_eq!(
            drawing_paths(&args(&[url.as_str()]), Path::new("/")),
            [dir.path().join("plan.dwg")]
        );
    }

    #[test]
    fn a_project_opens_the_drawing_it_belongs_to() {
        let dir = folder();
        let d = dir.path();
        // Named in the project.
        fs::write(
            d.join("plan.qiyas"),
            r#"{"version":2,"drawing":"plan.dwg"}"#,
        )
        .unwrap();
        // Unreadable as a project: the drawing of the same name.
        fs::write(d.join("survey.dxf"), b"x").unwrap();
        fs::write(d.join("survey.qiyas"), b"{").unwrap();
        // Its drawing is gone.
        fs::write(d.join("old.qiyas"), r#"{"drawing":"old.dwg"}"#).unwrap();
        // A name pointing out of the folder, or at a drawing whose project this is not.
        fs::write(d.join("other.qiyas"), r#"{"drawing":"../plan.dwg"}"#).unwrap();
        fs::write(d.join("x.qiyas"), r#"{"drawing":"plan.dwg"}"#).unwrap();
        assert_eq!(
            drawing_paths(
                &args(&[
                    "plan.qiyas",
                    "survey.qiyas",
                    "old.qiyas",
                    "other.qiyas",
                    "x.qiyas"
                ]),
                d
            ),
            [d.join("plan.dwg"), d.join("survey.dxf")]
        );
    }
}
