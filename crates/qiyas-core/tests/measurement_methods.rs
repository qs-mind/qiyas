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

#[test]
fn the_publishing_body_is_not_part_of_a_method_name() {
    use qiyas_core::measurement::{canonical_method_name, same_method_name};
    for (name, bare) in [
        ("POMI (RICS)", "POMI"),
        ("NRM2 (rics)", "NRM2"),
        ("NRM2 [RICS]", "NRM2"),
        ("SMM7", "SMM7"),
        ("Client's method / ARM4", "Client's method / ARM4"),
        ("طريقة العميل (RICS)", "طريقة العميل"),
        ("(RICS)", "(RICS)"),
        ("RICS NRM2 4.1", "NRM2 4.1"),
        ("NRM2 - rics", "NRM2 -"),
        ("بند 3 من RICS POMI", "بند 3 من POMI"),
        ("Metrics sheet", "Metrics sheet"),
    ] {
        assert_eq!(canonical_method_name(name), bare, "{name}");
    }
    assert!(same_method_name("POMI (RICS)", "POMI"));
    assert!(!same_method_name("POMI", "NRM2 (RICS)"));
    assert_eq!(RuleSet::named("POMI (RICS)").title, "POMI");
}

/// A drawing saved before this ruling holds "POMI (RICS)" with its rules and a cached result that
/// says so. It must still match the bare "POMI" Settings now offer (the rules stay), and what is
/// shown and exported must not carry the body.
#[test]
fn an_old_drawing_with_the_long_name_keeps_its_rules_and_exports_the_bare_name() {
    use qiyas_core::knowledge::{put, Kind};
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("old.dxf"), 6);
    fixture::fixture(&mut session);
    fixture::apply(&mut session, fixture::method("POMI", false));
    // Written as an older version wrote it: the long name in the selection and in the cached result.
    let selection = serde_json::to_string(&session.measurement_selection().unwrap())
        .unwrap()
        .replace("POMI", "POMI (RICS)");
    put(
        &mut session.drawing.doc,
        Kind::Measurement,
        "selection",
        &serde_json::from_str(&selection).unwrap(),
    )
    .unwrap();
    let result = serde_json::to_string(&session.remeasure_report().unwrap())
        .unwrap()
        .replace("\"POMI\"", "\"POMI (RICS)\"");
    put(
        &mut session.drawing.doc,
        Kind::Measurement,
        "result",
        &serde_json::from_str(&result).unwrap(),
    )
    .unwrap();
    assert!(result.contains("POMI (RICS)"));
    assert_eq!(
        session
            .measurement_selection()
            .unwrap()
            .settings
            .unwrap()
            .name,
        "POMI (RICS)"
    );

    // Settings now say "POMI": the same method, so the authored rules stay and nothing changes.
    let history = session.history_steps();
    session
        .apply_settings_method(Some(&RuleSet::named("POMI")))
        .unwrap();
    assert_eq!(session.history_steps(), history);
    assert!(!session
        .measurement_selection()
        .unwrap()
        .settings
        .unwrap()
        .rules
        .is_empty());

    // What is shown: the report and every deliverable say "POMI".
    let report = session.measurement_report().unwrap();
    assert!(!report.items.is_empty());
    assert!(!serde_json::to_string(&report).unwrap().contains("RICS"));
    assert!(serde_json::to_string(&report).unwrap().contains("POMI"));
    for language in [
        qiyas_core::deliverables::Language::En,
        qiyas_core::deliverables::Language::Ar,
    ] {
        for kind in [
            qiyas_core::deliverables::Kind::Boq,
            qiyas_core::deliverables::Kind::Dimensions,
        ] {
            let table = qiyas_core::deliverables::drawing(&session, kind, language, None)
                .unwrap()
                .table()
                .unwrap();
            let rendered = serde_json::to_string(&table).unwrap();
            assert!(rendered.contains("POMI"));
            assert!(!rendered.contains("RICS"), "{rendered}");
        }
    }
}

/// A drawing whose name-only selection was saved as "POMI (RICS)" opens under Settings' "POMI"
/// without becoming a changed drawing: no undo step, nothing to save.
#[test]
fn an_old_name_only_selection_opens_unchanged() {
    use qiyas_core::knowledge::{put, Kind};
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("old.dxf"), 6);
    session
        .apply_settings_method(Some(&RuleSet::named("POMI")))
        .unwrap();
    let selection = serde_json::to_string(&session.measurement_selection().unwrap())
        .unwrap()
        .replace("POMI", "POMI (RICS)");
    put(
        &mut session.drawing.doc,
        Kind::Measurement,
        "selection",
        &serde_json::from_str(&selection).unwrap(),
    )
    .unwrap();
    let history = session.history_steps();
    let revision = session.document_revision();
    session
        .apply_settings_method(Some(&RuleSet::named("POMI")))
        .unwrap();
    assert_eq!(session.history_steps(), history);
    assert_eq!(session.document_revision(), revision);
}
