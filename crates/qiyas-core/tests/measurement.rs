//! Invented test methods exercise the engine; no measurement standard is shipped.
use crate::measurement_fixture as fixture;
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

#[test]
fn digest_preserves_note_scope_neighbours_and_wide_schedule_columns_and_checks_units() {
    let mut s = Session::new_blank(std::path::Path::new("unused.dxf"), 6);
    let qiyas_core::Outcome::Added { handles } = run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[3,0]}),
    ) else {
        panic!()
    };
    let line = handles[0];
    let text = format!("{}; factor 2 m", "Evidence ".repeat(100));
    let qiyas_core::Outcome::Added { handles } = run(
        &mut s,
        json!({"type":"text","at":[0,0.5],"height":0.1,"value":text}),
    ) else {
        panic!()
    };
    let note = handles[0];
    run(
        &mut s,
        json!({"type":"table","at":[10,10],"rows":2,"cols":6,"col_width":2,"row_height":1,"data":[["A","B","C","D","E","F"],["a","b","c","d","e","f"]]}),
    );
    let mut offset = 0;
    let mut notes = Vec::new();
    let mut columns = Vec::new();
    loop {
        let page = s.drawing_digest(offset).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= qiyas_core::digest::MAX_BYTES);
        for entry in page["entries"].as_array().unwrap() {
            if entry["handle"] == note && entry["section"] == "text_clusters" {
                notes.push(entry["text"].as_str().unwrap().to_owned());
                assert_eq!(entry["closest"][0]["handle"], line);
                assert_eq!(entry["closest"][0]["layer"], "0");
            }
            if entry["section"] == "schedule_columns" {
                columns.push(entry["heading"].clone());
            }
        }
        let Some(next) = page["next_offset"].as_u64() else {
            break;
        };
        assert!(next as usize > offset);
        offset = next as usize;
    }
    assert_eq!(notes.concat(), text);
    assert_eq!(
        columns,
        [
            json!("A"),
            json!("B"),
            json!("C"),
            json!("D"),
            json!("E"),
            json!("F")
        ]
    );
    let citation = json!({"source":"drawing","clause":format!("note {note}")});
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"Observed","geometry":"path","category":"Test"}}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":{"name":"test","title":"test","based_on":"drawing","source_note":"Test","rules":[{"id":"scaled","element_type":"Observed","kind":"area","basis":"rule:sample-note","unit":"m2","terms":[{"label":"Scaled","operation":"add","factors":["geometry.area","2"],"citation":citation}],"citation":citation}]}}),
    );
    let check = s.takeoff_check(0, &[]).unwrap();
    assert_eq!(check["quantity_basis_warnings"], 1);
    assert_eq!(check["passed"], false);
    assert!(check["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["section"] == "quantity_basis_warnings" && e["handle"] == note));
    let selection = s.measurement_selection().unwrap();
    let rule = &selection.effective().unwrap().rules[0];
    assert_eq!(rule.kind.as_deref(), Some("area"));
    assert_eq!(rule.basis.as_deref(), Some("rule:sample-note"));
    let mut invalid = selection.effective().unwrap().clone();
    invalid.rules[0].basis = Some("rule:".into());
    assert!(invalid.validate().is_err());
    invalid.rules[0].basis = Some("geometric".into());
    invalid.rules[0].kind = Some("unknown".into());
    assert!(invalid.validate().is_err());
}
