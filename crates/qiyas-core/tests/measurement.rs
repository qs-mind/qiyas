//! Invented test methods exercise the engine; no measurement standard is shipped.
#[path = "support/measurement_fixture.rs"]
mod fixture;
use fixture::*;
use qiyas_core::{Session, Source};
use serde_json::json;

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
#[test]
fn methods_change_units_deductions_and_boundary_policy_in_one_undo_step_in_all_units() {
    for units in [1, 4, 5, 6] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("test.dxf"), units);
        let (host, children) = fixture(&mut s);
        apply(&mut s, method("area-test", false));
        let before = s.measurement_report().unwrap();
        near(before.items[0].quantity, 32.68); // 3 × 4.2 × 2.8 - 2.4 - 0.2.
        assert_eq!(before.items[0].unit, "m2");
        assert!(!before.items[0].lines[1].included); // Interior 0.4 is retained.
        assert!(before.items[0].lines[3].included); // Boundary 0.2 is deducted.
        let steps = s.history_steps().0.len();
        run(
            &mut s,
            json!({"type":"measurement","kind":"set_method","scope":"settings","method":method("linear-test",true)}),
        );
        let after = s.measurement_report().unwrap();
        assert_eq!(s.history_steps().0.len(), steps + 1);
        assert_eq!(after.items[0].unit, "m");
        near(after.items[0].quantity, 11.0); // 12.6 - 0.4 - 1.2, boundary left in.
        assert!(after.items[0].lines[1].included); // Equality at the threshold.
        assert!(!after.items[0].lines[3].included);
        assert!(s.undo());
        assert_eq!(s.measurement_report().unwrap(), before);
        assert!(s.redo());
        assert_eq!(s.measurement_report().unwrap(), after);
        for line in &after.items[0].lines {
            assert_eq!(line.method, "linear-test");
            assert!(!line.citations.is_empty());
            assert!(!line.sources.is_empty());
            for source in &line.sources {
                assert!(source.handle == host || children.contains(&source.handle));
                assert_eq!(source.sheet, "*Model_Space");
                assert!(source.grid.is_some());
            }
            let value = qiyas_param::expr::Expr::parse(&line.expression)
                .unwrap()
                .eval_with(&|_: &str| None)
                .unwrap()
                .as_number()
                .unwrap();
            near(value, line.value);
        }
    }
}
#[test]
fn live_boq_follows_geometry_properties_deletions_and_dwg_dxf_round_trip() {
    for ext in ["dwg", "dxf"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("method.{ext}"));
        let mut s = Session::new_blank(&path, 6);
        let (host, children) = fixture(&mut s);
        apply(&mut s, method("area-test", false));
        let before = s.measurement_report().unwrap();
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[host],"props":{"copies":2,"width":0.25}}),
        );
        let edited = s.measurement_report().unwrap();
        near(edited.items[0].quantity, 20.92);
        assert_eq!(edited.items[0].description_en, "Surface 0.25 wide");
        assert_eq!(edited.items[0].description_ar, "سطح بعرض 0.25");
        assert!(s.undo());
        assert_eq!(s.measurement_report().unwrap(), before);
        assert!(s.redo());
        run(
            &mut s,
            json!({"type":"modify","kind":"scale","targets":[{"handle":host}],"base":[0,0],"factor":2}),
        );
        near(s.measurement_report().unwrap().items[0].quantity, 44.44);
        assert!(s.undo());
        run(
            &mut s,
            json!({"type":"delete_entities","targets":[{"handle":children[1]}]}),
        );
        near(s.measurement_report().unwrap().items[0].quantity, 23.32);
        assert!(s.undo());
        assert_eq!(s.measurement_report().unwrap(), edited);
        s.save_as(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        assert_eq!(reopened.measurement_report().unwrap(), edited);
        run(
            &mut reopened,
            json!({"type":"set_object_props","targets":[host],"props":{"copies":3}}),
        );
        near(
            reopened.measurement_report().unwrap().items[0].quantity,
            32.68,
        );
    }
}
#[test]
fn project_override_wins_and_invalid_methods_roll_back_without_losing_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("test.dxf"), 6);
    fixture(&mut s);
    apply(&mut s, method("settings", false));
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":method("project",true)}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":method("new-default",false)}),
    );
    assert_eq!(
        s.measurement_report().unwrap().quantities[0].method,
        "project"
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":null}),
    );
    assert_eq!(
        s.measurement_report().unwrap().quantities[0].method,
        "new-default"
    );
    let before = s.measurement_report().unwrap();
    let history = s.history_steps();
    for broken in ["citation", "expression", "bands", "negative"] {
        let mut m = method("broken", true);
        match broken {
            "citation" => m["rules"][0]["citation"]["clause"] = json!(""),
            "expression" => m["rules"][0]["bands"][0]["terms"][0]["factors"] = json!(["missing"]),
            "bands" => {
                let b = m["rules"][0]["bands"][0].clone();
                m["rules"][0]["bands"].as_array_mut().unwrap().push(b);
            }
            _ => m["rules"][0]["bands"][0]["terms"][0]["factors"] = json!(["-1"]),
        }
        assert!(s
            .run_input(
                &json!({"type":"measurement","kind":"set_method","scope":"settings","method":m}),
                Source::User
            )
            .is_err());
        assert_eq!(s.measurement_report().unwrap(), before);
        assert_eq!(s.history_steps(), history);
    }
    let mut m = mappings();
    m[0]["unit"] = json!("kg");
    assert!(s
        .run_input(
            &json!({"type":"measurement","kind":"set_mappings","mappings":m}),
            Source::User
        )
        .is_err());
    assert_eq!(s.measurement_report().unwrap(), before);
}
