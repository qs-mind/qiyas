//! Name selection and persistence, with invented rules only.
use qiyas_core::measurement::{load_method_at, RuleSet};
use qiyas_core::Session;
use serde_json::json;
use std::fs;

#[path = "support/measurement_fixture.rs"]
mod fixture;

#[test]
fn names_need_no_file_and_files_are_resolved_by_declared_name() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_method_at(dir.path(), None).unwrap(), None);
    for name in ["POMI (RICS)", "Client's method / ARM4", "طريقة العميل"] {
        assert_eq!(
            load_method_at(dir.path(), Some(name)).unwrap(),
            Some(RuleSet::named(name))
        );
    }
    assert!(load_method_at(dir.path(), Some(" ")).is_err());
    let methods = dir.path().join("methods");
    fs::create_dir(&methods).unwrap();
    let rules = fixture::method("client method", false);
    fs::write(methods.join("unrelated-filename.json"), rules.to_string()).unwrap();
    fs::write(methods.join("broken.json"), "{").unwrap();
    let loaded = load_method_at(dir.path(), Some("client method"))
        .unwrap()
        .unwrap();
    assert!(!loaded.rules.is_empty());
    assert_eq!(loaded.name, "client method");
    fs::write(methods.join("duplicate.json"), rules.to_string()).unwrap();
    assert!(load_method_at(dir.path(), Some("client method"))
        .unwrap_err()
        .contains("More than one"));
    fs::remove_file(methods.join("duplicate.json")).unwrap();
    fs::write(
        methods.join("unrelated-filename.json"),
        json!({"name":"client method"}).to_string(),
    )
    .unwrap();
    assert!(load_method_at(dir.path(), Some("client method")).is_err());
}

#[test]
fn name_only_reopen_reuses_authored_rules_and_project_override_in_all_units() {
    let dir = tempfile::tempdir().unwrap();
    for units in [4, 5, 6, 1] {
        for extension in ["dxf", "dwg"] {
            let path = dir.path().join(format!("drawing-{units}.{extension}"));
            let mut session = Session::new_blank(&path, units);
            fixture::fixture(&mut session);
            fixture::apply(&mut session, fixture::method("client method", false));
            let before = session.measurement_report().unwrap();
            for language in [
                qiyas_core::deliverables::Language::En,
                qiyas_core::deliverables::Language::Ar,
            ] {
                for kind in [
                    qiyas_core::deliverables::Kind::Boq,
                    qiyas_core::deliverables::Kind::Dimensions,
                ] {
                    let report =
                        qiyas_core::deliverables::drawing(&session, kind, language, None).unwrap();
                    let rendered = serde_json::to_string(&report.table().unwrap()).unwrap();
                    assert!(rendered.contains("client method"));
                    assert!(rendered.contains("Invented test method; not a published standard /"));
                }
            }
            let history = session.history_steps();
            session
                .apply_settings_method(Some(&RuleSet::named("client method")))
                .unwrap();
            assert_eq!(session.measurement_report().unwrap(), before);
            assert_eq!(session.history_steps(), history);
            session.save_as_headless(&path).unwrap();
            let mut reopened = Session::open_standalone(&path).unwrap();
            reopened
                .apply_settings_method(Some(&RuleSet::named("client method")))
                .unwrap();
            assert_eq!(reopened.measurement_report().unwrap().items, before.items);
            assert!(!reopened
                .measurement_selection()
                .unwrap()
                .settings
                .unwrap()
                .rules
                .is_empty());
            fixture::run(
                &mut reopened,
                json!({"type":"measurement","kind":"set_method","scope":"project","method":fixture::method("project method", true)}),
            );
            reopened
                .apply_settings_method(Some(&RuleSet::named("another choice")))
                .unwrap();
            assert_eq!(
                reopened
                    .measurement_selection()
                    .unwrap()
                    .effective()
                    .unwrap()
                    .name,
                "project method"
            );
            reopened.apply_settings_method(None).unwrap();
            assert!(reopened.measurement_selection().unwrap().settings.is_none());
            assert_eq!(
                reopened
                    .measurement_selection()
                    .unwrap()
                    .effective()
                    .unwrap()
                    .name,
                "project method"
            );
        }
    }
}
