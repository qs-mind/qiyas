//! The old authoring surface and the common typed records must describe the same geometry.
use qiyas_core::knowledge::{self, Kind};
use qiyas_core::objects::ObjectFilter;
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Value {
    serde_json::to_value(s.run_input(&command, Source::User).unwrap().outcome).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn check_wall_quantities(s: &Session, result: &Value) {
    let walls = s
        .objects(&ObjectFilter {
            category: Some("wall".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(walls.len(), 2);
    for (quantity, field) in [
        ("run_length", "centreline_length"),
        ("gross_plan_area", "gross_area"),
        ("net_plan_area", "net_area"),
        ("net_volume", "volume"),
    ] {
        near(
            walls.iter().map(|o| o.quantities[quantity]).sum(),
            result[field].as_f64().unwrap(),
        );
    }
    let openings = s
        .objects(&ObjectFilter {
            category: Some("opening".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(openings.len(), 2);
    near(openings.iter().map(|o| o.quantities["pieces"]).sum(), 2.0);
    near(
        openings.iter().map(|o| o.quantities["opening_area"]).sum(),
        0.9 * 2.1 + 1.2 * 1.2,
    );
}

#[test]
fn mixed_joined_walls_and_openings_keep_exact_quantities_in_objects_round_trip_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        for ext in ["dwg", "dxf"] {
            let path = dir.path().join(format!("migration-{units}.{ext}"));
            let mut s = Session::new_blank(&path, units);
            let walls = run(
                &mut s,
                json!({
                    "type":"walls", "thickness":0.25/factor,
                    "runs":[
                        {"points":[[0,0],[10.0/factor,0]], "height":3},
                        {"points":[[5.0/factor,0],[5.0/factor,4.0/factor]], "thickness":0.15/factor,"height":2.5}
                    ],
                    "openings":[
                        {"run":0,"at":2.0/factor,"width":0.9/factor,"kind":"door","sill":0,"head":2.1},
                        {"run":1,"at":2.0/factor,"width":1.2/factor,"kind":"window","sill":0.9,"head":2.1}
                    ]
                }),
            );
            check_wall_quantities(&s, &walls);
            assert_eq!(s.history_steps().0.len(), 1);
            assert!(s.undo());
            assert!(s.objects(&ObjectFilter::default()).unwrap().is_empty());
            assert!(knowledge::list(&s.drawing.doc, Kind::Types)
                .unwrap()
                .is_empty());
            assert!(s.redo());
            check_wall_quantities(&s, &walls);
            s.save_as(&path).unwrap();
            let mut s = Session::open(&path).unwrap();
            check_wall_quantities(&s, &walls);
            let driver = s.wall_groups(None).unwrap()[0].runs[0].centreline;
            run(
                &mut s,
                json!({"type":"set_object_props","targets":[driver],"props":{"wall_height":4}}),
            );
            let changed = s.wall_groups(None).unwrap();
            near(changed[0].runs[0].height, 4.0);
            near(
                s.objects(&ObjectFilter {
                    category: Some("wall".into()),
                    ..Default::default()
                })
                .unwrap()
                .iter()
                .map(|o| o.quantities["net_volume"])
                .sum(),
                walls["volume"].as_f64().unwrap() + 2.5,
            );
            assert_eq!(s.history_steps().0.len(), 1);
            assert!(s.undo());
            check_wall_quantities(&s, &walls);
            assert!(s.redo());
            s.save_as(&path).unwrap();
            let reopened = Session::open(&path).unwrap();
            near(reopened.wall_groups(None).unwrap()[0].runs[0].height, 4.0);
        }
    }
}

#[test]
fn retained_room_and_its_legacy_schedule_match_the_object_measurement_after_edit_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("room.dwg");
    let mut s = Session::new_blank(&path, 6);
    let shape = run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4,3]}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    let room = run(
        &mut s,
        json!({"type":"label_spaces","query":{"seeds":[{"name":"Room A","point":[1,1]}]},"layer":"A-ROOM-LABEL"}),
    );
    let boundary = room["report"]["rooms"][0]["boundary_handle"]
        .as_u64()
        .unwrap();
    run(&mut s, json!({"type":"room_schedule","at":[8,8]}));
    let objects = s
        .objects(&ObjectFilter {
            category: Some("room".into()),
            ..Default::default()
        })
        .unwrap();
    near(objects[0].quantities["net_floor_area"], 12.0);
    run(
        &mut s,
        json!({"type":"modify","kind":"scale","targets":[{"handle":shape}],"base":[0,0],"factor":2}),
    );
    let objects = s
        .objects(&ObjectFilter {
            category: Some("room".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(objects[0].instance.id, boundary);
    near(objects[0].quantities["net_floor_area"], 48.0);
    s.save_as(&path).unwrap();
    let s = Session::open(&path).unwrap();
    let objects = s
        .objects(&ObjectFilter {
            category: Some("room".into()),
            ..Default::default()
        })
        .unwrap();
    near(objects[0].quantities["net_floor_area"], 48.0);
    assert_eq!(objects[0].instance.props["name"], "Room A");
}

#[test]
fn opening_a_pre_object_drawing_converts_without_redrawing_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.dxf");
    let mut s = Session::new_blank(&path, 6);
    run(
        &mut s,
        json!({"type":"walls","thickness":0.2,"runs":[{"points":[[0,0],[4,0]]}]}),
    );
    for entity in s.drawing.doc.entities_mut() {
        entity
            .common_mut()
            .extended_data
            .remove_record("QIYAS_OBJECT");
        entity
            .common_mut()
            .extended_data
            .remove_record("QIYAS_OBJECT_PART");
    }
    for entry in knowledge::list(&s.drawing.doc, Kind::Types).unwrap() {
        knowledge::remove(&mut s.drawing.doc, Kind::Types, &entry.name).unwrap();
    }
    s.save_as(&path).unwrap();
    let handles: Vec<_> = s
        .drawing
        .doc
        .entities()
        .map(|e| e.common().handle.value())
        .collect();
    let s = Session::open(&path).unwrap();
    assert_eq!(
        s.drawing
            .doc
            .entities()
            .map(|e| e.common().handle.value())
            .collect::<Vec<_>>(),
        handles
    );
    let objects = s.objects(&ObjectFilter::default()).unwrap();
    assert_eq!(objects.len(), 1);
    near(objects[0].quantities["net_volume"], 2.4);
    assert!(s.history_steps().0.is_empty());
}
