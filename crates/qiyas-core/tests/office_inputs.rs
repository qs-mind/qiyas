//! Small regressions for accepted command forms in a multi-floor office drawing.
use opencadcodec::{Color, EntityType, Handle};
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new_blank(std::path::Path::new("office-towers.dxf"), 6)
}

fn run(s: &mut Session, input: Value) -> qiyas_core::Reported {
    s.run_input(&input, Source::Ai).unwrap()
}

fn line(x: f64) -> Value {
    json!({"type":"draw","kind":"line","a":[x,0],"b":[x,30]})
}

fn define_block(s: &mut Session) {
    run(
        s,
        json!({"type":"define_block","name":"Desk_Service","base":[0,0],"entities":[
            {"type":"Circle","data":{"center":{"x":0,"y":0,"z":0},"radius":0.1}}
        ]}),
    );
}

#[test]
fn insert_records_accept_data_paths_and_explain_missing_fields() {
    let mut s = session();
    define_block(&mut s);
    run(
        &mut s,
        json!({"type":"insert_block","name":"Desk_Service","at":[1.94,4.85]}),
    );
    let query = json!({"collection":"entities","type":"Insert","fields":[
        "key","block_name","bounds","data/insert_point","/insert_point","layer"
    ],"detail":"summary"});
    let page = s
        .records(&serde_json::from_value(query.clone()).unwrap())
        .unwrap();
    assert_eq!(page.records.len(), 1);
    let fields = &page.records[0]["fields"];
    assert_eq!(fields["data/insert_point"], fields["/insert_point"]);
    assert_eq!(fields["data/insert_point"]["x"], 1.94);
    assert_eq!(fields["block_name"], "Desk_Service");
    let mut missing = query;
    missing["fields"] = json!(["data/no_such_point"]);
    let page = s
        .records(&serde_json::from_value(missing).unwrap())
        .unwrap();
    assert!(page
        .warnings
        .iter()
        .any(|w| w.contains("data/...") && w.contains("/...")));
    let bad = json!({"collection":"entities","fields":["wrong/insert_point"]});
    let error = s
        .records(&serde_json::from_value(bad).unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("data/common/layer") && error.contains("/common/layer"));
}

#[test]
fn nested_batch_context_and_generate_share_one_undo_step() {
    let mut s = session();
    define_block(&mut s);
    let before = s.current.clone();
    let history = s.history_steps().0.len();
    let report = run(
        &mut s,
        json!({"type":"batch","layer":"A-BASE","color":"ACI 4","commands":[
            {"type":"batch","layer":"A-POWER","color":"ACI 2","commands":[
                {"type":"generate","loops":[{"var":"r","count":2},{"var":"c","count":3}],
                 "commands":[{"type":"insert_block","name":"Desk_Service","at":["=1.94+c*1.9","=4.85+r*2.5"]}]},
                {"type":"insert_block","name":"Desk_Service","at":[10.85,6.8]}
            ]},
            {"type":"batch","layer":"A-PLANT","commands":[line(60.0)]},
            line(80.0)
        ]}),
    );
    let Outcome::Batch { outcomes, added } = report.outcome else {
        panic!("batch")
    };
    assert_eq!(added.len(), 9);
    let Outcome::Batch { added: inner, .. } = &outcomes[0] else {
        panic!("inner batch")
    };
    assert_eq!(inner.len(), 7);
    for handle in inner {
        let entity = s.drawing.doc.get_entity(Handle::new(*handle)).unwrap();
        assert_eq!(entity.common().layer, "A-POWER");
        assert_eq!(entity.common().color, Color::Index(2));
    }
    for (handle, layer) in added[7..].iter().zip(["A-PLANT", "A-BASE"]) {
        let entity = s.drawing.doc.get_entity(Handle::new(*handle)).unwrap();
        assert_eq!(entity.common().layer, layer);
        assert_eq!(entity.common().color, Color::Index(4));
    }
    assert!(report
        .defaults_used
        .iter()
        .any(|d| d.field.starts_with("/commands/0/commands/1/")));
    assert_eq!(s.current, before);
    assert_eq!(s.history_steps().0.len(), history + 1);
    assert!(s.undo());
    assert!(added
        .iter()
        .all(|h| s.drawing.doc.get_entity(Handle::new(*h)).is_none()));
    assert!(!s.drawing.doc.layers.contains("A-POWER"));
    assert!(s.redo());
    assert!(added
        .iter()
        .all(|h| s.drawing.doc.get_entity(Handle::new(*h)).is_some()));
    assert_eq!(s.current, before);
}

#[test]
fn nested_batch_failure_rolls_back_outer_edits_and_rejects_unsafe_steps() {
    for failing in [
        json!({"type":"insert_block","name":"missing","at":[0,0]}),
        json!({"type":"write_block","name":"missing","path":"must-not-write.dwg"}),
    ] {
        let mut s = session();
        let before = s.current.clone();
        let revision = s.document_revision();
        let error = s.run_input(&json!({"type":"batch","commands":[line(0.0),
            {"type":"batch","layer":"A-POWER","color":"ACI 2","commands":[line(30.0),failing]}
        ]}), Source::Ai).unwrap_err().to_string();
        assert!(error.contains("Batch step 1"), "{error}");
        assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
        assert!(!s.drawing.doc.layers.contains("A-POWER"));
        assert_eq!(s.current, before);
        assert_eq!(s.document_revision(), revision);
        assert!(!s.can_undo());
    }
}

#[test]
fn name_only_set_layer_selects_bylayer_and_reports_each_default() {
    let mut s = session();
    run(
        &mut s,
        json!({"type":"set_layer","name":"A-POWER","color":"ACI 3"}),
    );
    run(
        &mut s,
        json!({"type":"set_current","current":{"color":"ACI 2"}}),
    );
    let before = s.current.clone();
    let names = ["A-POWER", "A-PLANT", "A-CLEAR", "A-FINISH"];
    let commands: Vec<_> = names
        .iter()
        .enumerate()
        .flat_map(|(i, name)| {
            [
                json!({"type":"set_layer","name":name}),
                line(i as f64 * 30.0),
            ]
        })
        .collect();
    let report = run(&mut s, json!({"type":"batch","commands":commands}));
    let Outcome::Batch { added, .. } = report.outcome else {
        panic!("batch")
    };
    assert_eq!(added.len(), 4);
    for (i, (handle, name)) in added.iter().zip(names).enumerate() {
        let entity = s.drawing.doc.get_entity(Handle::new(*handle)).unwrap();
        assert_eq!(entity.common().layer, name);
        assert_eq!(entity.common().color, Color::ByLayer);
        assert!(report.defaults_used.iter().any(|d| d.field
            == format!("/commands/{}/current/color", i * 2)
            && d.value == "ByLayer"));
    }
    assert_eq!(
        s.drawing.doc.layers.get("A-POWER").unwrap().color,
        Color::Index(3)
    );
    assert!(s.undo());
    assert_eq!(s.current, before);
    assert!(!s.drawing.doc.layers.contains("A-FINISH"));
}

#[test]
fn unknown_rules_warn_without_widening_the_requested_checks() {
    let mut s = session();
    run(&mut s, line(0.0));
    run(&mut s, line(0.0));
    let report = run(
        &mut s,
        json!({"type":"check_drawing","scope":{
            "rules":["duplicate","overlap","outside_boundary"]
        }}),
    );
    let warning = report
        .warnings
        .iter()
        .find(|w| w.code == "unknown_rule")
        .unwrap();
    assert_eq!(warning.pointer, "/scope/rules/2");
    assert!(
        warning.message.contains("nearest valid choice") && warning.message.contains("job_limit")
    );
    let Outcome::Checked { report } = report.outcome else {
        panic!("checked")
    };
    assert_eq!(report.checked_rules, ["duplicate", "overlap"]);
    assert!(report.findings.iter().any(|f| f.rule == "duplicate"));
    let report = run(
        &mut s,
        json!({"type":"check_drawing","scope":{"rules":["outside_boundary"]}}),
    );
    let Outcome::Checked { report } = report.outcome else {
        panic!("checked")
    };
    assert!(report.checked_rules.is_empty());
    assert!(report.findings.is_empty());
    assert!(s
        .run_input(
            &json!({"type":"check_drawing","scope":{"rules":[7]}}),
            Source::Ai
        )
        .is_err());
}

#[test]
fn opening_schedule_height_alias_sets_text_height_and_survives_refresh() {
    let mut s = session();
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[100,0]]}],"thickness":0.2,
        "openings":[{"run":0,"at":3,"width":0.9,"kind":"door"}]}),
    );
    for field in ["height", "text_height"] {
        let report = run(
            &mut s,
            json!({"type":"batch","commands":[
                {"type":"opening_schedule","at":[20,10],field:0.14}
            ]}),
        );
        let Outcome::Batch { added, .. } = report.outcome else {
            panic!("batch")
        };
        let handle = added
            .into_iter()
            .find(|h| {
                matches!(
                    s.drawing.doc.get_entity(Handle::new(*h)),
                    Some(EntityType::Table(_))
                )
            })
            .unwrap();
        run(&mut s, json!({"type":"update_fields"}));
        let Some(EntityType::Table(table)) = s.drawing.doc.get_entity(Handle::new(handle)) else {
            panic!("table")
        };
        assert!((table.rows[0].cells[0].style.as_ref().unwrap().text_height - 0.14).abs() < 1e-10);
    }
    assert!(s
        .run_input(&json!({"type":"opening_schedule","height":0}), Source::Ai)
        .is_err());
}

fn design(value: Value) -> Value {
    json!({"name":"net_room_areas","value":value,"unit":"m2",
        "reason":"Measured closed boundaries between finished wall faces",
        "source":"mizan_design","citation":"Measured floor boundaries","set_by":"mizan","status":"answered"})
}

#[test]
fn structured_decisions_roundtrip_and_only_mizan_design_can_default_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("office-towers.dxf");
    let mut s = Session::new_blank(&path, 6);
    let value = json!({"meeting":20.900625,"studio":69.875,"total":90.775625});
    run(
        &mut s,
        json!({"type":"batch","commands":[{"type":"decision","kind":"set","entries":[design(value.clone())]}]}),
    );
    assert_eq!(s.decisions().unwrap()[0].value, value);
    s.save_as_headless(&path).unwrap();
    let reopened = Session::open_embedded(&path).unwrap();
    assert_eq!(reopened.decisions().unwrap()[0].value, value);
    let mut entry = design(json!([20.900625, 69.875]));
    entry.as_object_mut().unwrap().remove("reason");
    let report = run(
        &mut s,
        json!({"type":"batch","commands":[{
            "type":"decision","kind":"set","description":"Design decision: measured office floor areas","entries":[entry.clone()]
        }]}),
    );
    assert!(report
        .defaults_used
        .iter()
        .any(|d| d.field == "/commands/0/entries/0/reason"
            && d.value == "Design decision: measured office floor areas"));
    assert_eq!(s.decisions().unwrap()[0].value, entry["value"]);
    for (source, setter) in [
        ("engineer", "engineer"),
        ("drawing", "mizan"),
        ("mizan_design", "engineer"),
    ] {
        let mut invalid = entry.clone();
        invalid["source"] = json!(source);
        invalid["set_by"] = json!(setter);
        assert!(s.run_input(&json!({"type":"decision","kind":"set","description":"Measured areas","entries":invalid}), Source::Ai).is_err());
    }
    for value in [
        Value::Null,
        json!({}),
        json!([]),
        json!(""),
        json!({"missing":null}),
    ] {
        assert!(s
            .run_input(
                &json!({"type":"decision","kind":"set","entries":design(value)}),
                Source::Ai
            )
            .is_err());
    }
    assert!(s
        .run_input(
            &json!({"type":"decision","kind":"set","entries":entry}),
            Source::Ai
        )
        .is_err());
}
