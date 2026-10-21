//! Schedule recipes share formatting, grouping, fields and override behavior across types.

use opencadcodec::{EntityType, Handle};
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, command: Value) -> Outcome {
    session.run_input(&command, Source::User).unwrap().outcome
}

fn add(session: &mut Session, command: Value) -> Vec<u64> {
    match run(session, command) {
        Outcome::Added { handles } => handles,
        outcome => panic!("Expected handles, got {outcome:?}"),
    }
}

fn cells(session: &Session, handle: u64) -> Vec<Vec<String>> {
    let EntityType::Table(table) = session.drawing.doc.get_entity(Handle::new(handle)).unwrap()
    else {
        panic!("Expected table")
    };
    (0..table.rows.len())
        .map(|r| {
            (0..table.columns.len())
                .map(|c| table.cell_text(r, c).unwrap_or("").to_owned())
                .collect()
        })
        .collect()
}

fn setup() -> (tempfile::TempDir, Session, Vec<u64>) {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("schedule.dxf"), 6);
    run(
        &mut session,
        json!({"type":"define_object_type","definition":{
            "name":"Vessel", "category":"Storage", "geometry":"point",
            "properties":{
                "code":{"kind":"text","default":"A"},
                "capacity":{"kind":"number","unit":"volume","default":1.2345}
            },
            "quantities":{"volume":"capacity", "pieces":"count"}
        }}),
    );
    let ids = add(
        &mut session,
        json!({"type":"place_objects","objects":[
            {"type":"Vessel","points":[[0,0]]},
            {"type":"Vessel","points":[[1,0]]},
            {"type":"Vessel","points":[[2,0]],"props":{"code":"B","capacity":2}}
        ]}),
    );
    (dir, session, ids)
}

#[test]
fn generic_headers_templates_precision_totals_layer_and_grouping_follow_edits_and_round_trip() {
    let (dir, mut session, ids) = setup();
    let before = session.history_steps().0.len();
    let table = add(
        &mut session,
        json!({"type":"object_schedule","at":[0,5],
            "columns":["label","volume","pieces"], "headers":["Container","Capacity (m3)","Count"],
            "group_by":["code"], "templates":{"label":"{code} / {capacity:6t} m3"},
            "decimals":{"volume":2,"pieces":0}, "totals":{"label":"Total","volume":"","pieces":""},
            "default_layer":"SCHEDULES"
        }),
    )[0];
    assert_eq!(session.history_steps().0.len(), before + 1);
    let initial = cells(&session, table);
    assert_eq!(
        initial,
        vec![
            vec!["Container", "Capacity (m3)", "Count"],
            vec!["A / 1.2345 m3", "2.47", "2"],
            vec!["B / 2 m3", "2.00", "1"],
            vec!["Total", "4.47", "3"]
        ]
    );
    assert_eq!(
        session
            .drawing
            .doc
            .get_entity(Handle::new(table))
            .unwrap()
            .common()
            .layer,
        "SCHEDULES"
    );
    assert_eq!(session.current.layer, "0");
    let grouped = add(
        &mut session,
        json!({"type":"object_schedule", "at":[10,5],
            "columns":["capacity","pieces"], "group_by":["capacity"],
            "decimals":{"capacity":2,"pieces":0}
        }),
    )[0];
    assert_eq!(cells(&session, grouped)[1], ["1.23", "2"]);
    assert_eq!(cells(&session, grouped)[2], ["2.00", "1"]);
    run(
        &mut session,
        json!({"type":"set_object_props","targets":[ids[2]],"props":{"capacity":3}}),
    );
    assert_eq!(cells(&session, table)[3], ["Total", "5.47", "3"]);
    assert!(session.undo());
    assert_eq!(cells(&session, table), initial);
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("saved.{extension}"));
        session.save_as_headless(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        assert_eq!(cells(&reopened, table), initial);
        run(
            &mut reopened,
            json!({"type":"set_object_props","targets":[ids[2]],"props":{"capacity":3}}),
        );
        assert_eq!(cells(&reopened, table)[3], ["Total", "5.47", "3"]);
    }
}

#[test]
fn explicit_ids_preserve_manual_overrides_until_the_selected_dependency_changes() {
    let (_dir, mut session, ids) = setup();
    let table = add(
        &mut session,
        json!({"type":"object_schedule","at":[0,5],
            "ids":[ids[0],ids[0]], "columns":["code","volume"], "decimals":{"volume":2},
            "totals":{"code":"Total","volume":""}, "preserve_overrides":true
        }),
    )[0];
    assert_eq!(cells(&session, table)[1], ["A", "1.23"]);
    run(
        &mut session,
        json!({"type":"edit_cell","target":{"handle":table},"row":1,"col":1,"text":"99.00"}),
    );
    assert_eq!(cells(&session, table)[1][1], "99.00");
    run(
        &mut session,
        json!({"type":"set_object_props","targets":[ids[1]],"props":{"capacity":5}}),
    );
    assert_eq!(cells(&session, table)[1][1], "99.00");
    assert_eq!(cells(&session, table)[2][1], "1.23");
    run(
        &mut session,
        json!({"type":"set_object_props","targets":[ids[0]],"props":{"capacity":4}}),
    );
    assert_eq!(cells(&session, table)[1][1], "4.00");
    assert_eq!(cells(&session, table)[2][1], "4.00");
    assert!(session.undo());
    assert_eq!(cells(&session, table)[1][1], "99.00");
    run(
        &mut session,
        json!({"type":"delete_entities","targets":[{"handle":ids[0]}]}),
    );
    assert_eq!(cells(&session, table)[1][1], "####");
    assert_eq!(cells(&session, table)[2][1], "####");
}

#[test]
fn legacy_room_tables_are_adopted_on_open_without_erasing_manual_cells_or_moving_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("old-room.dxf"), 6);
    run(
        &mut session,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4,3]}),
    );
    run(
        &mut session,
        json!({"type":"label_spaces","query":{"seeds":[{"name":"Room A","point":[1,1]}]}}),
    );
    let table = add(
        &mut session,
        json!({"type":"room_schedule","at":[11,12],"decimals":4}),
    )[0];
    run(
        &mut session,
        json!({"type":"edit_cell","target":{"handle":table},"row":1,"col":1,"text":"99.0000"}),
    );
    for app in ["QIYAS_OBJECT_SCHEDULE", "QIYAS_OBJECT_SCHEDULE_STATE"] {
        let app_handle = qiyas_core::xdata::app_handle(&session.drawing.doc, app);
        qiyas_core::xdata::write_text(
            session
                .drawing
                .doc
                .get_entity_mut(Handle::new(table))
                .unwrap()
                .common_mut(),
            app,
            app_handle,
            None,
        );
    }
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("legacy.{extension}"));
        session.save_as_headless(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        assert_eq!(cells(&reopened, table)[1][1], "99.0000");
        let entity = reopened.drawing.doc.get_entity(Handle::new(table)).unwrap();
        let spec: Value =
            qiyas_core::xdata::read_json(entity.common(), "QIYAS_OBJECT_SCHEDULE").unwrap();
        assert_eq!(spec["at"], json!([11.0, 12.0]));
        assert_eq!(spec["decimals"]["net_floor_area"], 4);
        run(&mut reopened, json!({"type":"update_fields"}));
        assert_eq!(cells(&reopened, table)[1][1], "99.0000");
        assert_eq!(cells(&reopened, table)[2][1], "12.0000");
        let report = reopened.check_drawing(&qiyas_core::check::CheckScope {
            packs: vec!["construction".into()],
            rules: vec!["room_area_mismatch".into()],
            ..Default::default()
        });
        assert_eq!(report.counts.get("room_area_mismatch"), Some(&1));
    }
}
