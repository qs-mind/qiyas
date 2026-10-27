//! Optional local rules supplement any method name selected by the QS.

use std::fs;
use std::path::{Path, PathBuf};

use qiyas_ai::MeasurementMethod;
pub use qiyas_core::measurement::load_method_at as load_at;
use qiyas_core::measurement::RuleSet;
use qiyas_core::{CoreError, Session, Source};
use serde::Serialize;

#[cfg(test)]
use qiyas_core::data_home::home_path;
pub use qiyas_core::data_home::qiyas_dir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeasurementMethodInfo {
    pub name: String,
    pub title: String,
    pub based_on: String,
    pub source_note: String,
    pub rules_loaded: bool,
}

fn read(path: &Path) -> Result<MeasurementMethodInfo, String> {
    let text = fs::read(path).map_err(|e| e.to_string())?;
    let rules: RuleSet = serde_json::from_slice(&text).map_err(|e| e.to_string())?;
    rules.validate().map_err(|e| e.to_string())?;
    if rules.version != 1 {
        return Err(format!(
            "Unsupported measurement rule set version {}",
            rules.version
        ));
    }
    if rules.name.trim().is_empty() || rules.title.trim().is_empty() {
        return Err("A measurement rule set needs a name and title".into());
    }
    let rules = rules.bare();
    Ok(MeasurementMethodInfo {
        name: rules.name,
        title: rules.title,
        based_on: rules.based_on,
        source_note: rules.source_note,
        rules_loaded: !rules.rules.is_empty(),
    })
}

pub fn list_at(home: &Path) -> Result<Vec<MeasurementMethodInfo>, String> {
    let dir = home.join("methods");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    paths.sort();
    let mut methods = Vec::new();
    for path in paths {
        if path.extension().is_none_or(|e| e != "json") || !path.is_file() {
            continue;
        }
        match read(&path) {
            Ok(method) => {
                if !methods
                    .iter()
                    .any(|m: &MeasurementMethodInfo| m.name == method.name)
                {
                    methods.push(method);
                }
            }
            Err(e) => eprintln!(
                "Measurement method {} could not be read: {e}",
                path.display()
            ),
        }
    }
    methods.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.name.cmp(&b.name)));
    Ok(methods)
}

#[tauri::command]
pub fn measurement_methods() -> Result<Vec<MeasurementMethodInfo>, String> {
    list_at(&qiyas_dir()?)
}

pub fn selected(name: Option<&str>) -> MeasurementMethod {
    let loaded = name.is_some_and(|name| {
        measurement_methods().is_ok_and(|methods| {
            methods
                .iter()
                .any(|m| qiyas_core::measurement::same_method_name(&m.name, name) && m.rules_loaded)
        })
    });
    MeasurementMethod {
        name: name.map(qiyas_core::measurement::canonical_method_name),
        rules_loaded: loaded,
    }
}

pub fn apply(s: &mut Session, method: Option<&RuleSet>) -> Result<(), CoreError> {
    s.apply_settings_method(method)
}

/// Nested transactions preserve every drawing and its history if validation or saving fails.
pub fn apply_all<T>(
    sessions: &mut [&mut Session],
    method: Option<&RuleSet>,
    finish: impl FnOnce() -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let Some((first, rest)) = sessions.split_first_mut() else {
        return finish();
    };
    first.transaction(Source::User, "measurement.set_method", |s| {
        apply(s, method)?;
        apply_all(rest, method, finish)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn test_rules() -> Value {
        json!([{"id":"test", "element_type":"Run", "unit":"m", "terms":[{"label":"Length", "operation":"add", "factors":["geometry.length"], "citation":{"source":"Test", "clause":"A"}}], "citation":{"source":"Test", "clause":"A"}}])
    }
    fn fixture(rules: Value) -> Value {
        json!({"version":1,"name":"local-test","title":"Local test","based_on":"",
            "source_note":"Test fixture only","rules":rules})
    }

    #[test]
    fn validates_rules_and_lists_json() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("methods");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("local.json"), fixture(test_rules()).to_string()).unwrap();
        fs::write(dir.join("ignored.txt"), fixture(json!([])).to_string()).unwrap();
        fs::write(dir.join("broken.json"), "{").unwrap();
        let list = list_at(home.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "local-test");
        assert!(list[0].rules_loaded);
        fs::write(dir.join("local.json"), fixture(json!([])).to_string()).unwrap();
        assert!(!list_at(home.path()).unwrap()[0].rules_loaded);
        for value in [
            json!({"name":"local-test"}),
            {
                let mut v = fixture(json!([]));
                v["version"] = json!(2);
                v
            },
            {
                let mut v = fixture(json!([]));
                v["title"] = json!("");
                v
            },
            fixture(json!({"not":"an array"})),
            fixture(json!([{"future_rule":true}])),
        ] {
            fs::write(dir.join("local.json"), value.to_string()).unwrap();
            assert!(read(&dir.join("local.json")).is_err());
        }
        let mut original = fixture(json!([]));
        original.as_object_mut().unwrap().remove("version");
        fs::write(dir.join("local.json"), original.to_string()).unwrap();
        assert!(read(&dir.join("local.json")).is_ok());
    }

    mod measurement_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../qiyas-core/tests/support/measurement_fixture.rs"
        ));
    }
    #[test]
    fn local_method_files_rederive_open_drawings_and_rollback_all_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("methods")).unwrap();
        for (name, linear) in [("first", false), ("second", true)] {
            fs::write(
                dir.path().join(format!("methods/{name}.json")),
                measurement_fixture::method(name, linear).to_string(),
            )
            .unwrap();
        }
        let mut a = Session::new_blank(&dir.path().join("a.dxf"), 6);
        let mut b = Session::new_blank(&dir.path().join("b.dxf"), 4);
        for session in [&mut a, &mut b] {
            measurement_fixture::fixture(session);
            measurement_fixture::apply(session, measurement_fixture::method("first", false));
        }
        let first = load_at(dir.path(), Some("first")).unwrap().unwrap();
        let second = load_at(dir.path(), Some("second")).unwrap().unwrap();
        let steps = a.history_steps().0.len();
        apply_all(&mut [&mut a, &mut b], Some(&second), || Ok(())).unwrap();
        assert_eq!(a.history_steps().0.len(), steps + 1);
        for session in [&mut a, &mut b] {
            assert_eq!(session.measurement_report().unwrap().items[0].unit, "m");
        }
        let before = a.measurement_report().unwrap();
        let history = a.history_steps();
        let result: Result<(), CoreError> = apply_all(&mut [&mut a, &mut b], Some(&first), || {
            Err(CoreError::InvalidCommand("save failed".into()))
        });
        assert!(result.is_err());
        assert_eq!(a.measurement_report().unwrap(), before);
        assert_eq!(a.history_steps(), history);
        assert_eq!(b.measurement_report().unwrap().items[0].unit, "m");
        assert!(a.undo());
        assert_eq!(a.measurement_report().unwrap().items[0].unit, "m2");
        assert_eq!(
            load_at(dir.path(), Some("absent")).unwrap(),
            Some(RuleSet::named("absent"))
        );
    }

    #[test]
    fn development_home_is_inside_the_repository() {
        if cfg!(debug_assertions) {
            assert!(home_path(None).unwrap().ends_with(".local/qiyas-home"));
        }
    }

    #[test]
    fn qiyas_home_override_lists_methods_in_an_isolated_process() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "methods::tests::home_override_child"])
            .env("QIYAS_HOME", home.path())
            .env("QIYAS_METHODS_TEST_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
        assert!(home.path().join("methods").is_dir());
        assert!(home.path().join("projects").is_dir());
    }

    #[test]
    fn home_override_child() {
        if std::env::var_os("QIYAS_METHODS_TEST_CHILD").is_none() {
            return;
        }
        let home = qiyas_dir().unwrap();
        assert_eq!(home, PathBuf::from(std::env::var_os("QIYAS_HOME").unwrap()));
        fs::write(
            home.join("methods/local.json"),
            fixture(test_rules()).to_string(),
        )
        .unwrap();
        assert_eq!(measurement_methods().unwrap()[0].name, "local-test");
        assert!(selected(Some("local-test")).rules_loaded);
        assert_eq!(
            selected(Some("missing")),
            MeasurementMethod {
                name: Some("missing".into()),
                rules_loaded: false
            }
        );
        assert_eq!(selected(None), MeasurementMethod::default());
    }
}
