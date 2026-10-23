//! Settings on disk and the API key in the keychain, without Tauri. The keychain is the
//! `keyring_core` mock store, so no test touches the real one.

use std::fs;
use std::path::Path;
use std::sync::Once;

use qiyas_desktop_lib::secrets;
use qiyas_desktop_lib::settings::{self, Provider, Settings};

fn mock_keychain() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap()));
}

fn legacy_key_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn read(dir: &Path) -> String {
    fs::read_to_string(dir.join("settings.json")).unwrap()
}

#[test]
fn defaults_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let s = settings::load(dir.path());
    assert_eq!(s.provider, Provider::Anthropic);
    assert_eq!(s.profile().model, "claude-opus-5-5");
    assert_eq!(s.profile().base_url, None);
    assert_eq!(s.effort, "high");
    assert_eq!(s.autonomy, qiyas_ai::Autonomy::Autonomous);
    assert_eq!(s.measurement_method, None);
    assert!(!s.mcp_enabled);
    assert_eq!(s.mcp_port, 48123);
    assert_eq!(s.mcp_token.len(), 32);
    assert!(s.mcp_token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!((s.language.as_str(), s.theme.as_str()), ("en", "system"));
    // The token is made once: the next start reads the same one.
    assert_eq!(settings::load(dir.path()).mcp_token, s.mcp_token);
}

#[test]
fn round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings {
        provider: Provider::OpenaiCompatible,
        providers: settings::ProviderProfiles {
            openai_compatible: settings::ProviderProfile {
                model: "my-model".into(),
                base_url: Some("http://localhost:11434/v1".into()),
            },
            ..Default::default()
        },
        effort: "max".into(),
        mcp_enabled: true,
        mcp_port: 50000,
        language: "ar".into(),
        theme: "dark".into(),
        aliases: [("LO".to_string(), "LAYOFF".to_string())].into(),
        ..Settings::default()
    };
    settings::save(dir.path(), &s).unwrap();
    assert!(read(dir.path()).contains("\"openai_compatible\""));
    assert_eq!(settings::load(dir.path()), s);
    // Saving leaves no temporary file behind.
    assert!(!dir.path().join("settings.json.tmp").exists());
}

#[test]
fn approval_modes_and_measurement_method_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    for autonomy in [
        qiyas_ai::Autonomy::PerJob,
        qiyas_ai::Autonomy::PerPhase,
        qiyas_ai::Autonomy::Autonomous,
    ] {
        let s = Settings {
            autonomy,
            measurement_method: Some("local-test".into()),
            ..Settings::default()
        };
        assert_eq!(settings::validate(&s), Ok(()));
        settings::save(dir.path(), &s).unwrap();
        assert_eq!(settings::load(dir.path()), s);
    }
    assert!(serde_json::from_str::<Settings>(r#"{"autonomy":"unknown"}"#).is_err());
    let s = Settings {
        measurement_method: Some(" ".into()),
        ..Settings::default()
    };
    assert!(settings::validate(&s).is_err());
}

#[test]
fn flat_settings_migrate_to_only_the_selected_provider() {
    for (provider, model, address) in [
        ("anthropic", "claude-sonnet-5-5", None),
        (
            "openai_compatible",
            "google:gemini@3.8-flash",
            Some("https://example.test/v1"),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let original = serde_json::json!({
            "provider": provider, "model": model, "base_url": address,
            "effort": "max", "language": "ar", "mcp_token": "a".repeat(32)
        });
        fs::write(dir.path().join("settings.json"), original.to_string()).unwrap();
        let mut s = settings::load(dir.path());
        assert_eq!(s.profile().model, model);
        assert_eq!(s.profile().base_url.as_deref(), address);
        assert_eq!(s.effort, "max");
        assert_eq!(s.language, "ar");
        assert_eq!(s.mcp_token, "a".repeat(32));
        let written: serde_json::Value = serde_json::from_str(&read(dir.path())).unwrap();
        assert!(written.get("model").is_none());
        assert!(written.get("base_url").is_none());
        assert_eq!(written["providers"][provider]["model"], model);
        assert_eq!(settings::load(dir.path()), s);
        s.provider = if provider == "anthropic" {
            Provider::OpenaiCompatible
        } else {
            Provider::Anthropic
        };
        assert_eq!(
            s.profile().model,
            if provider == "anthropic" {
                ""
            } else {
                "claude-opus-5-5"
            }
        );
        assert!(s.profile().base_url.is_none());
    }
}

#[test]
fn a_new_compatible_profile_can_be_saved_before_choosing_a_model() {
    let s = Settings {
        provider: Provider::OpenaiCompatible,
        ..Settings::default()
    };
    assert!(s.profile().model.is_empty());
    assert!(settings::validate(&s).is_ok());
}

#[test]
fn existing_profiles_win_over_leftover_flat_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = serde_json::to_value(Settings::default()).unwrap();
    value["model"] = "old-model".into();
    value["base_url"] = "https://old.example.test".into();
    fs::write(dir.path().join("settings.json"), value.to_string()).unwrap();
    let s = settings::load(dir.path());
    assert_eq!(s.profile().model, "claude-opus-5-5");
    assert!(s.profile().base_url.is_none());
}

#[test]
fn missing_fields_take_defaults() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("settings.json"), r#"{"effort":"low"}"#).unwrap();
    let s = settings::load(dir.path());
    assert_eq!(s.effort, "low");
    assert_eq!(s.profile().model, "claude-opus-5-5");
    assert_eq!(s.mcp_token.len(), 32);
    assert!(s.aliases.is_empty());
    // The new token was written back.
    assert!(read(dir.path()).contains(&s.mcp_token));
}

#[test]
fn corrupt_file_is_backed_up_and_defaults_used() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("settings.json"), "{ not json").unwrap();
    let s = settings::load(dir.path());
    assert_eq!(s.profile().model, "claude-opus-5-5");
    assert_eq!(
        fs::read_to_string(dir.path().join("settings.json.bak")).unwrap(),
        "{ not json"
    );
    assert_eq!(settings::load(dir.path()), s);
}

#[test]
fn validation_rejects_bad_values() {
    let good = Settings::default();
    assert!(settings::validate(&good).is_ok());
    let port = Settings {
        mcp_port: 80,
        ..good.clone()
    };
    assert!(settings::validate(&port).unwrap_err().contains("1024"));
    let effort = Settings {
        effort: "ultra".into(),
        ..good.clone()
    };
    assert!(settings::validate(&effort).unwrap_err().contains("ultra"));
    let alias = Settings {
        aliases: [("L O".to_string(), "LAYOFF".to_string())].into(),
        ..good.clone()
    };
    assert!(settings::validate(&alias).unwrap_err().contains("L O"));
    let token = Settings {
        mcp_token: "short".into(),
        ..good
    };
    assert!(settings::validate(&token).is_err());
    // An unknown provider never gets past the IPC layer.
    assert!(serde_json::from_str::<Provider>("\"gemini\"").is_err());
}

#[test]
fn api_key_set_read_delete() {
    let _guard = legacy_key_lock();
    mock_keychain();
    assert_eq!(secrets::get_api_key("openai_compatible").unwrap(), None);
    secrets::set_api_key("openai_compatible", "  sk-test-123  ").unwrap();
    assert_eq!(
        secrets::get_api_key("openai_compatible")
            .unwrap()
            .as_deref(),
        Some("sk-test-123")
    );
    let mut saved = Settings::default();
    saved.providers.openai_compatible.base_url = Some("https://migration.example.test/v1".into());
    secrets::migrate(&saved).unwrap();
    assert_eq!(
        secrets::get_at("openai_compatible", "https://migration.example.test/v1")
            .unwrap()
            .as_deref(),
        Some("sk-test-123")
    );
    assert!(
        secrets::get_at("openai_compatible", "https://different.example.test/v1")
            .unwrap()
            .is_none()
    );
    secrets::delete_api_key("openai_compatible").unwrap();
    assert_eq!(secrets::get_api_key("openai_compatible").unwrap(), None);
    // Deleting twice is fine.
    secrets::delete_api_key("openai_compatible").unwrap();
    assert!(secrets::set_api_key("openai_compatible", "   ").is_err());
    assert!(secrets::set_api_key("gemini", "key").is_err());
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn api_key_never_written_to_disk() {
    let _guard = legacy_key_lock();
    mock_keychain();
    let dir = tempfile::tempdir().unwrap();
    let key = "sk-ant-never-on-disk-5f3c9a";
    secrets::set_api_key("anthropic", key).unwrap();
    let s = settings::load(dir.path());
    settings::save(dir.path(), &s).unwrap();
    assert_eq!(
        secrets::get_api_key("anthropic").unwrap().as_deref(),
        Some(key)
    );

    let mut files = Vec::new();
    files_under(dir.path(), &mut files);
    assert!(!files.is_empty());
    for f in files {
        let bytes = fs::read(&f).unwrap();
        assert!(
            !bytes.windows(key.len()).any(|w| w == key.as_bytes()),
            "the key is in {}",
            f.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn only_the_user_can_read_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    // A leftover temporary file from a crash, readable by all.
    fs::create_dir_all(dir.path()).unwrap();
    fs::write(dir.path().join("settings.json.tmp"), "{}").unwrap();
    fs::set_permissions(
        dir.path().join("settings.json.tmp"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    settings::save(dir.path(), &Settings::default()).unwrap();
    let mode = fs::metadata(dir.path().join("settings.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn load_makes_old_files_private() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let bak = dir.path().join("settings.json.bak");
    settings::save(dir.path(), &Settings::default()).unwrap();
    fs::write(&bak, "old").unwrap();
    for p in [&file, &bak] {
        fs::set_permissions(p, fs::Permissions::from_mode(0o644)).unwrap();
    }
    let before = read(dir.path());
    settings::load(dir.path());
    assert_eq!(read(dir.path()), before, "nothing else changes");
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&bak), 0o600);
}

#[test]
fn recents_are_ordered_deduplicated_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = settings::load(dir.path());
    assert!(s.recents.is_empty());
    for i in 0..12 {
        s.push_recent(&format!("/plans/{i}.dwg"));
    }
    assert_eq!(s.recents.len(), settings::MAX_RECENTS);
    assert_eq!(s.recents[0], "/plans/11.dwg");
    assert_eq!(s.recents[9], "/plans/2.dwg");
    // Opening one again moves it to the top, once.
    s.push_recent("/plans/5.dwg");
    assert_eq!(s.recents[0], "/plans/5.dwg");
    assert_eq!(s.recents.iter().filter(|p| *p == "/plans/5.dwg").count(), 1);
    assert_eq!(s.recents.len(), settings::MAX_RECENTS);
    settings::save(dir.path(), &s).unwrap();
    assert_eq!(settings::load(dir.path()).recents, s.recents);

    // Another spelling of the same file is the same recent drawing.
    let plan = dir.path().join("plan.dwg");
    fs::write(&plan, b"x").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    let other = dir.path().join("sub").join("..").join("plan.dwg");
    s.push_recent(&plan.display().to_string());
    s.push_recent(&other.display().to_string());
    assert_eq!(s.recents[0], other.display().to_string());
    assert_ne!(s.recents[1], plan.display().to_string(), "listed once");
    assert_eq!(s.recents.len(), settings::MAX_RECENTS);
}

#[test]
fn autosave_runs_every_five_minutes_by_default_and_zero_turns_it_off() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(settings::load(dir.path()).autosave_minutes, 5);
    let off = Settings {
        autosave_minutes: 0,
        ..Settings::default()
    };
    assert!(settings::validate(&off).is_ok());
    let long = Settings {
        autosave_minutes: 121,
        ..Settings::default()
    };
    assert!(settings::validate(&long).unwrap_err().contains("120"));
}

#[test]
fn credentials_are_scoped_and_draft_addresses_are_rejected_without_network() {
    mock_keychain();
    let first = "https://scope.example.test/v1";
    let second = "https://other.example.test/v1";
    let mut saved = Settings::default();
    saved.providers.openai_compatible.base_url = Some(first.into());
    secrets::set_at("openai_compatible", first, "mock-scope-only").unwrap();
    assert_eq!(
        secrets::discovery_address(&saved, "openai_compatible", Some(first)).unwrap(),
        first
    );
    for draft in [
        second,
        "https://",
        "https://half",
        "http://example.test/v1",
        "https://scope.example.test/v2",
    ] {
        assert!(secrets::discovery_address(&saved, "openai_compatible", Some(draft)).is_err());
    }
    assert!(secrets::get_at("openai_compatible", second)
        .unwrap()
        .is_none());
    assert!(secrets::get_at("anthropic", first).unwrap().is_none());
    assert!(
        secrets::get_at("openai_compatible", "https://scope.example.test/v2")
            .unwrap()
            .is_none()
    );
    secrets::delete_at("openai_compatible", first).unwrap();
}

#[tokio::test]
async fn credentials_follow_the_saved_address_and_never_a_draft() {
    mock_keychain();
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let first = format!("http://{}/v1", listener.local_addr().unwrap());
    let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    other.set_nonblocking(true).unwrap();
    let second = format!("http://{}/v1", other.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut buf = [0; 4096];
        let mut bytes = Vec::new();
        while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "Request ended before its headers");
            bytes.extend_from_slice(&buf[..n]);
        }
        let request = String::from_utf8_lossy(&bytes).to_lowercase();
        assert!(request.contains("authorization: bearer mock-goal-a"));
        let body = r#"{"data":[{"id":"test-model"}]}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let mut saved = Settings::default();
    saved.providers.openai_compatible.base_url = Some(first.clone());
    secrets::set_at("openai_compatible", &first, "mock-goal-a").unwrap();
    let accepted = secrets::discovery_address(&saved, "openai_compatible", Some(&first)).unwrap();
    let key = secrets::get_at("openai_compatible", &accepted).unwrap();
    let list = qiyas_ai::models::list_models(
        qiyas_ai::models::ModelSource::OpenAiCompatible {
            base_url: &accepted,
        },
        key.as_deref(),
    )
    .await
    .unwrap();
    assert_eq!(list[0].id, "test-model");
    server.join().unwrap();
    for draft in [&second, "https://", "http://example.test/v1"] {
        assert!(secrets::discovery_address(&saved, "openai_compatible", Some(draft)).is_err());
    }
    assert!(other.accept().is_err());
    assert!(secrets::get_at("openai_compatible", &second)
        .unwrap()
        .is_none());
    assert!(secrets::get_at("anthropic", &first).unwrap().is_none());
    assert!(
        secrets::get_at("openai_compatible", &format!("{first}/different"))
            .unwrap()
            .is_none()
    );
    secrets::delete_at("openai_compatible", &first).unwrap();
}
