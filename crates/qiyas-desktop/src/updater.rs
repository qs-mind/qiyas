//! Updates through `tauri-plugin-updater`: a static feed (`latest.json` on the company site, at
//! `plugins.updater.endpoints` in `tauri.conf.json`; `updater/latest.sample.json` shows its
//! shape) lists a signed bundle per platform. The About box checks when asked; release builds
//! also check at launch, at most once a day (`last_update_check` in `settings.json`). Nothing
//! downloads or installs without a click, and an install that would lose unsaved drawing changes
//! is refused. While the public key or the feed address in `tauri.conf.json` is still the
//! placeholder, every check is refused with `NOT_CONFIGURED`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use qiyas_ai::prompt::Lang;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio_util::sync::CancellationToken;

use crate::settings;
use crate::state::AppState;
use crate::tabs;

/// Marks the public key and the feed address in `tauri.conf.json` until the founder sets them.
pub const PLACEHOLDER: &str = "REPLACE-ME";

/// Refusal of a check: this build has no real public key or feed address.
pub const NOT_CONFIGURED: &str = "updates_not_configured";
/// Refusal of an install: a drawing has unsaved changes, and the install restarts Qiyas.
pub const UNSAVED: &str = "unsaved_drawings";
/// Refusal of an install: no check found an update since Qiyas started.
pub const NO_UPDATE: &str = "no_update";
/// Refusal of an install: another one is running (the About box and the launch dialog).
pub const BUSY: &str = "update_busy";
/// The download was stopped (the About box closed during it).
pub const CANCELLED: &str = "update_cancelled";
/// The update is installed, but a drawing got unsaved changes meanwhile (a deb or rpm install
/// waits for the password), so Qiyas doesn't restart: it uses the update when next started.
pub const RESTART_LATER: &str = "installed_restart_later";

/// Sent during the download: `Progress`.
pub const PROGRESS: &str = "update:progress";
/// Sent when the download is done and checked, as the install starts: it can't be stopped now.
pub const INSTALLING: &str = "update:installing";

/// The launch check runs again after this many seconds.
const DAY: u64 = 24 * 60 * 60;
/// How long the launch check waits, so it doesn't compete with the window opening.
const LAUNCH_DELAY: Duration = Duration::from_secs(15);
/// How long the request to the feed may take, whole.
const TIMEOUT: Duration = Duration::from_secs(60);
/// How long the feed or the bundle may send nothing (connecting, or between two reads) before
/// it fails: a big bundle on a slow line still finishes, a stalled one does not hang.
const STALL: Duration = Duration::from_secs(30);
/// Progress is sent after at least this many more bytes.
const PROGRESS_STEP: u64 = 256 * 1024;

/// The update the last check found, kept for Install, and the install running now.
#[derive(Default)]
pub struct Pending {
    update: Mutex<Option<Update>>,
    /// An install is running: another is refused with `BUSY`.
    busy: AtomicBool,
    /// Stops the running install's download; gone once the download is done.
    cancel: Mutex<Option<CancellationToken>>,
}

/// How much of the bundle has come; `total` when the server says.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// What a check found.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Checked {
    UpToDate,
    Available {
        version: String,
        notes: Option<String>,
    },
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `updater` (the `plugins.updater` section of the app's config) is filled in: a public
/// key and at least one feed address, none of them the placeholder.
pub fn configured(updater: Option<&Value>) -> bool {
    let Some(updater) = updater else {
        return false;
    };
    let real = |s: &str| !s.trim().is_empty() && !s.contains(PLACEHOLDER);
    let key = updater.get("pubkey").and_then(Value::as_str);
    let endpoints = updater.get("endpoints").and_then(Value::as_array);
    // The plugin refuses plain http in release builds at start-up: refuse it here first.
    let feed = |s: &str| real(s) && s.starts_with("https://");
    key.is_some_and(real)
        && endpoints.is_some_and(|list| {
            !list.is_empty() && list.iter().all(|e| e.as_str().is_some_and(feed))
        })
}

/// Whether the launch check is due at `now` (seconds since 1970) after the last one at `last`
/// (0: never): a day or more since, or the clock went back.
pub fn due(last: u64, now: u64) -> bool {
    last == 0 || now < last || now - last >= DAY
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Keeps the time of this check in `settings.json`; a failed write is only logged.
fn stamp<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let mut settings = lock(&state.settings);
    settings.last_update_check = now();
    if let Err(e) = settings::save(&state.config_dir, &settings) {
        eprintln!("The time of the update check could not be saved: {e}");
    }
}

/// Asks the feed for an update, after refusing a build whose updater is not set up.
async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Update>, String> {
    if !configured(app.config().plugins.0.get("updater")) {
        return Err(NOT_CONFIGURED.into());
    }
    stamp(app);
    let handle = app.clone();
    app.updater_builder()
        .timeout(TIMEOUT)
        .configure_client(|client| client.connect_timeout(STALL).read_timeout(STALL))
        // Windows: the installer is started and Qiyas exits at once, without the Exit event.
        .on_before_exit(move || crate::files::release_locks(&handle))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

/// Refuses while a drawing has unsaved changes: the install restarts Qiyas.
fn ensure_saved<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<AppState>();
    if tabs::snapshot(&state).tabs.iter().any(|t| t.dirty) {
        return Err(UNSAVED.into());
    }
    Ok(())
}

/// Downloads the update the last check found (with `PROGRESS`; `cancel_update` stops it),
/// checks its signature, installs it and restarts Qiyas. One at a time (`BUSY`). Unsaved drawing
/// changes stop it before the download and again before the install; changes made while it
/// installs (a deb or rpm install waits for the password) keep Qiyas running (`RESTART_LATER`).
/// On Windows the installer closes Qiyas (its drawings' lock files go first) and starts it again
/// itself.
async fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let pending = app.state::<Pending>();
    if pending.busy.swap(true, Ordering::SeqCst) {
        return Err(BUSY.into());
    }
    let result = install_once(app, &pending).await;
    *lock(&pending.cancel) = None;
    pending.busy.store(false, Ordering::SeqCst);
    result
}

async fn install_once<R: Runtime>(app: &AppHandle<R>, pending: &Pending) -> Result<(), String> {
    ensure_saved(app)?;
    let update = lock(&pending.update).clone().ok_or(NO_UPDATE)?;
    let stop = CancellationToken::new();
    *lock(&pending.cancel) = Some(stop.clone());
    let sender = app.clone();
    let (mut downloaded, mut sent) = (0u64, 0u64);
    let download = update.download(
        move |chunk, total| {
            downloaded += chunk as u64;
            if downloaded - sent >= PROGRESS_STEP || total == Some(downloaded) {
                sent = downloaded;
                let _ = sender.emit(PROGRESS, Progress { downloaded, total });
            }
        },
        || {},
    );
    let bytes = tokio::select! {
        bytes = download => bytes.map_err(|e| e.to_string())?,
        _ = stop.cancelled() => return Err(CANCELLED.into()),
    };
    *lock(&pending.cancel) = None;
    ensure_saved(app)?;
    let _ = app.emit(INSTALLING, ());
    update.install(bytes).map_err(|e| e.to_string())?;
    restart_unless_unsaved(app)
}

/// Restarts Qiyas on the installed update, unless a drawing got unsaved changes while it
/// installed: then `RESTART_LATER`, and the update starts with Qiyas next time.
fn restart_unless_unsaved<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    ensure_saved(app).map_err(|_| RESTART_LATER.to_string())?;
    app.restart()
}

/// "Check for updates" in the About box.
#[tauri::command]
pub async fn check_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, Pending>,
) -> Result<Checked, String> {
    let update = check(&app).await?;
    let checked = match &update {
        None => Checked::UpToDate,
        Some(u) => Checked::Available {
            version: u.version.clone(),
            notes: u.body.clone(),
        },
    };
    *lock(&pending.update) = update;
    Ok(checked)
}

/// "Install and restart" in the About box: only returns on failure or `RESTART_LATER`.
#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    install(&app).await
}

/// Stops the download of the install running now (the About box closed); nothing once the
/// install itself has started.
#[tauri::command]
pub fn cancel_update(pending: State<'_, Pending>) {
    if let Some(stop) = lock(&pending.cancel).take() {
        stop.cancel();
    }
}

/// The launch check's dialog, in the window's language (the About box's words are in
/// `src/i18n`).
struct Words {
    title: &'static str,
    body: &'static str,
    install: &'static str,
    later: &'static str,
    unsaved: &'static str,
    restart_later: &'static str,
    failed: &'static str,
}

fn words(lang: Lang) -> Words {
    match lang {
        Lang::En => Words {
            title: "Update available: {version}",
            body: "Qiyas {version} is ready to install. This is {current}.",
            install: "Install and restart",
            later: "Later",
            unsaved: "Save your drawings first: the update restarts Qiyas. Then install it from \
                      About, Check for updates.",
            restart_later: "Installed. Restart Qiyas when you are ready: save your drawings \
                            first.",
            failed: "The update could not be installed: {error}",
        },
        Lang::Ar => Words {
            title: "تحديث متاح: {version}",
            body: "قياس {version} جاهز للتثبيت. النسخة الحالية {current}.",
            install: "التثبيت وإعادة التشغيل",
            later: "لاحقًا",
            unsaved:
                "احفظ رسوماتك أولًا: التحديث يعيد تشغيل قياس. ثم ثبّته من «حول»، «البحث عن تحديثات».",
            restart_later: "تم التثبيت. أعد تشغيل قياس عندما تكون جاهزًا، بعد حفظ رسوماتك.",
            failed: "تعذّر تثبيت التحديث: {error}",
        },
    }
}

/// Release builds: 15 seconds after launch, when the last check is a day old or more, asks the
/// feed; an update found is offered in a dialog (Install and restart, or Later). A refusal or a
/// failure is only logged: the About box says why when asked.
pub fn check_at_launch<R: Runtime>(app: &AppHandle<R>) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(LAUNCH_DELAY).await;
        let last = lock(&app.state::<AppState>().settings).last_update_check;
        if !due(last, now()) {
            return;
        }
        match check(&app).await {
            Ok(Some(update)) => offer(&app, update),
            Ok(None) => {}
            Err(e) => eprintln!("Update check at launch: {e}"),
        }
    });
}

/// Shows the launch check's dialog for `update`; Install installs it, as the About box does.
fn offer<R: Runtime>(app: &AppHandle<R>, update: Update) {
    let w = words(lock(&app.state::<AppState>().ai).language());
    let fill = |s: &str| {
        s.replace("{version}", &update.version)
            .replace("{current}", &update.current_version)
    };
    let mut body = fill(w.body);
    if let Some(notes) = update.body.as_deref().filter(|n| !n.trim().is_empty()) {
        body.push_str("\n\n");
        body.push_str(notes);
    }
    let title = fill(w.title);
    *lock(&app.state::<Pending>().update) = Some(update);
    let handle = app.clone();
    app.dialog()
        .message(body)
        .title(title)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            w.install.into(),
            w.later.into(),
        ))
        .show(move |install_it| {
            if !install_it {
                return;
            }
            tauri::async_runtime::spawn(async move {
                if let Err(e) = install(&handle).await {
                    let (text, kind) = match e.as_str() {
                        // The About box's install is running, and says how it went.
                        BUSY => return,
                        UNSAVED => (w.unsaved.to_string(), MessageDialogKind::Warning),
                        RESTART_LATER => (w.restart_later.to_string(), MessageDialogKind::Info),
                        _ => (w.failed.replace("{error}", &e), MessageDialogKind::Error),
                    };
                    handle.dialog().message(text).kind(kind).show(|_| {});
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tauri_plugin_updater::{RemoteRelease, RemoteReleaseInner};

    use super::*;

    const SAMPLE: &str = include_str!("../updater/latest.sample.json");

    #[test]
    fn the_sample_feed_parses_with_version_notes_date_and_a_signed_bundle_per_platform() {
        let release: RemoteRelease = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(release.version.to_string(), "0.2.0");
        assert!(release.notes.as_deref().unwrap().starts_with("Plotting"));
        assert_eq!(release.pub_date.unwrap().year(), 2026);
        let RemoteReleaseInner::Static { platforms } = &release.data else {
            panic!("a static feed lists its platforms");
        };
        let mut targets: Vec<&str> = platforms.keys().map(String::as_str).collect();
        targets.sort();
        assert_eq!(
            targets,
            [
                "darwin-aarch64-app",
                "darwin-x86_64-app",
                "linux-x86_64-appimage",
                "linux-x86_64-deb",
                "linux-x86_64-rpm",
                "windows-x86_64-msi",
                "windows-x86_64-nsis",
            ]
        );
        for target in targets {
            let url = release.download_url(target).unwrap();
            assert_eq!(url.scheme(), "https", "{target}");
            assert!(!release.signature(target).unwrap().is_empty(), "{target}");
        }
        // A platform the feed doesn't list has no bundle.
        assert!(release.download_url("linux-aarch64").is_err());
    }

    #[test]
    fn a_v_before_the_version_is_read_as_the_version() {
        let mut feed: Value = serde_json::from_str(SAMPLE).unwrap();
        feed["version"] = json!("v0.2.0");
        let release: RemoteRelease = serde_json::from_value(feed).unwrap();
        assert_eq!(release.version.to_string(), "0.2.0");
    }

    #[test]
    fn malformed_feeds_are_refused() {
        let sample: Value = serde_json::from_str(SAMPLE).unwrap();
        let broken = |f: &dyn Fn(&mut Value)| {
            let mut feed = sample.clone();
            f(&mut feed);
            serde_json::from_value::<RemoteRelease>(feed)
        };
        assert!(serde_json::from_str::<RemoteRelease>("<html>Not found</html>").is_err());
        assert!(serde_json::from_str::<RemoteRelease>(&SAMPLE[..SAMPLE.len() / 2]).is_err());
        assert!(broken(&|f| {
            f.as_object_mut().unwrap().remove("version");
        })
        .is_err());
        assert!(broken(&|f| f["version"] = json!("next")).is_err());
        assert!(broken(&|f| f["pub_date"] = json!("2 November")).is_err());
        assert!(broken(&|f| {
            f["platforms"]["linux-x86_64-appimage"]
                .as_object_mut()
                .unwrap()
                .remove("signature");
        })
        .is_err());
        assert!(
            broken(&|f| f["platforms"]["windows-x86_64-nsis"]["url"] = json!("setup.exe")).is_err()
        );
        // Without platforms, a feed must give one url and signature (the server form).
        assert!(broken(&|f| {
            f.as_object_mut().unwrap().remove("platforms");
        })
        .is_err());
    }

    fn updater(pubkey: &str, endpoints: &[&str]) -> Value {
        json!({ "pubkey": pubkey, "endpoints": endpoints })
    }

    #[test]
    fn a_placeholder_key_or_feed_address_refuses_every_check() {
        let feed = "https://qsmind.app/qiyas/latest.json";
        let key = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDEyMzQK";
        assert!(configured(Some(&updater(key, &[feed]))));
        assert!(!configured(None), "no updater section");
        assert!(!configured(Some(&updater(
            "REPLACE-ME: the public key",
            &[feed]
        ))));
        assert!(!configured(Some(&updater(
            key,
            &["https://REPLACE-ME.invalid/latest.json"]
        ))));
        assert!(!configured(Some(&updater(
            key,
            &[feed, "https://REPLACE-ME.invalid/latest.json"]
        ))));
        assert!(!configured(Some(&updater(" ", &[feed]))), "an empty key");
        assert!(!configured(Some(&updater(key, &[]))), "no feed address");
        assert!(
            !configured(Some(&updater(
                key,
                &["http://qsmind.app/qiyas/latest.json"]
            ))),
            "plain http"
        );
        assert!(!configured(Some(&json!({ "endpoints": [feed] }))));
    }

    #[test]
    fn the_shipped_config_is_valid_for_the_plugin_and_refused_while_a_placeholder_is_in_it() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let section = &config["plugins"]["updater"];
        // The plugin reads the same section at startup: a bad one would stop Qiyas starting.
        serde_json::from_value::<tauri_plugin_updater::Config>(section.clone()).unwrap();
        let placeholder = section.to_string().contains(PLACEHOLDER);
        assert_eq!(configured(Some(section)), !placeholder);
    }

    #[test]
    fn the_launch_check_runs_once_a_day() {
        let now = 1_800_000_000;
        assert!(due(0, now), "never checked");
        assert!(!due(now - 60, now), "a minute ago");
        assert!(!due(now - DAY + 1, now), "just under a day ago");
        assert!(due(now - DAY, now), "a day ago");
        assert!(due(now - 30 * DAY, now), "a month ago");
        assert!(due(now + 3600, now), "the clock went back");
    }

    #[test]
    fn the_launch_dialog_speaks_english_and_arabic() {
        for lang in [Lang::En, Lang::Ar] {
            let w = words(lang);
            assert!(w.title.contains("{version}"));
            assert!(w.body.contains("{version}") && w.body.contains("{current}"));
            assert!(w.failed.contains("{error}"));
            assert!(!w.install.is_empty() && !w.later.is_empty() && !w.unsaved.is_empty());
            assert!(!w.restart_later.is_empty());
        }
        assert!(words(Lang::Ar).body.contains("قياس"));
    }

    mod ipc {
        use tauri::ipc::{CallbackFn, InvokeBody};
        use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};
        use tauri::webview::InvokeRequest;
        use tauri::{App, WebviewWindow, WebviewWindowBuilder};

        use super::*;

        struct Ipc {
            _dir: tempfile::TempDir,
            app: App<MockRuntime>,
            webview: WebviewWindow<MockRuntime>,
        }

        impl Ipc {
            /// The app on the mock runtime, whose config has no updater section.
            fn new() -> Ipc {
                let dir = tempfile::tempdir().unwrap();
                let app = mock_builder()
                    .invoke_handler(tauri::generate_handler![
                        check_update,
                        install_update,
                        cancel_update,
                        crate::commands::get_settings,
                        crate::commands::set_settings
                    ])
                    .build(mock_context(noop_assets()))
                    .unwrap();
                app.manage(AppState::new(dir.path().join("config")));
                app.manage(Pending::default());
                let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
                    .build()
                    .unwrap();
                Ipc {
                    _dir: dir,
                    app,
                    webview,
                }
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

            fn settings(&self) -> Value {
                self.call("get_settings", json!({})).unwrap()
            }
        }

        #[test]
        fn a_build_without_a_real_key_refuses_to_check_and_keeps_no_time() {
            let ipc = Ipc::new();
            assert_eq!(
                ipc.call("check_update", json!({})),
                Err(json!(NOT_CONFIGURED))
            );
            assert_eq!(ipc.settings()["last_update_check"], 0);
        }

        #[test]
        fn install_without_an_update_found_is_refused() {
            let ipc = Ipc::new();
            assert_eq!(ipc.call("install_update", json!({})), Err(json!(NO_UPDATE)));
            // Nothing to stop: no harm.
            assert_eq!(ipc.call("cancel_update", json!({})), Ok(Value::Null));
            // Not left busy by the refusal.
            assert_eq!(ipc.call("install_update", json!({})), Err(json!(NO_UPDATE)));
        }

        #[test]
        fn a_second_install_while_one_runs_is_refused() {
            let ipc = Ipc::new();
            ipc.app
                .state::<Pending>()
                .busy
                .store(true, Ordering::SeqCst);
            assert_eq!(ipc.call("install_update", json!({})), Err(json!(BUSY)));
        }

        /// The sample drawing open in a tab, with a line drawn and not saved.
        fn unsaved_drawing(ipc: &Ipc) {
            let mut session = qiyas_core::Session::open(&qiyas_core::fixtures::write_sample(
                ipc._dir.path(),
                "dxf",
            ))
            .unwrap();
            session
                .run(qiyas_core::Command::Draw(qiyas_core::DrawSpec::Line {
                    a: [0.0, 0.0],
                    b: [1.0, 1.0],
                }))
                .unwrap();
            assert!(session.is_dirty());
            crate::tabs::add(&ipc.app.state::<AppState>(), session).unwrap();
        }

        #[test]
        fn unsaved_changes_stop_the_install_and_the_restart_after_it() {
            let ipc = Ipc::new();
            unsaved_drawing(&ipc);
            assert_eq!(ipc.call("install_update", json!({})), Err(json!(UNSAVED)));
            // Changes made while a deb or rpm install waited for the password: installed, no
            // restart.
            assert_eq!(
                restart_unless_unsaved(ipc.app.handle()),
                Err(RESTART_LATER.to_string())
            );
        }

        #[test]
        fn the_settings_dialog_keeps_the_time_of_the_last_check() {
            let ipc = Ipc::new();
            {
                let state = ipc.app.state::<AppState>();
                lock(&state.settings).last_update_check = 1_800_000_000;
            }
            // The dialog's copy is older, or leaves the field out.
            let mut from_dialog = ipc.settings();
            from_dialog["last_update_check"] = json!(0);
            ipc.call("set_settings", json!({ "settings": from_dialog }))
                .unwrap();
            assert_eq!(ipc.settings()["last_update_check"], 1_800_000_000);
            from_dialog
                .as_object_mut()
                .unwrap()
                .remove("last_update_check");
            ipc.call("set_settings", json!({ "settings": from_dialog }))
                .unwrap();
            assert_eq!(ipc.settings()["last_update_check"], 1_800_000_000);
        }
    }
}
