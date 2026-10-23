//! Room labels, schedules and take-off read one persisted region, including islands.
use opencadcodec::{EntityType, Handle};
use qiyas_core::check::CheckScope;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Outcome {
    s.run_input(&command, Source::User).unwrap().outcome
}

fn add(s: &mut Session, command: Value) -> u64 {
    let Outcome::Added { handles } = run(s, command) else {
        panic!("added entity")
    };
    handles[0]
}

fn setup(units: i16) -> (tempfile::TempDir, Session, u64, u64, u64) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("rooms.dwg"), units);
    let u = 1.0 / s.drawing.units.to_metres();
    let outline = add(
        &mut s,
        json!({"type":"draw", "kind":"rectangle", "a":[0,0], "b":[4.0*u,3.0*u]}),
    );
    let before = s.history_steps().0.len();
    let Outcome::Rooms { report } = run(
        &mut s,
        json!({"type":"label_spaces", "query":{"seeds":[{"name":"Room A","point":[u,u]}]}, "layer":"A-ROOM-LABEL"}),
    ) else {
        panic!("rooms")
    };
    assert_eq!(s.history_steps().0.len(), before + 1);
    let room = &report.rooms[0];
    assert_eq!(room.net_area_m2, 12.0);
    (
        dir,
        s,
        outline,
        room.label_handle.unwrap(),
        room.boundary_handle.unwrap(),
    )
}

fn text(s: &Session, h: u64) -> String {
    s.entity_info(h, &[]).unwrap().text.unwrap()
}

fn cell(s: &Session, h: u64, row: usize) -> String {
    let EntityType::Table(t) = s.drawing.doc.get_entity(Handle::new(h)).unwrap() else {
        panic!("table")
    };
    t.cell_text(row, 1).unwrap().to_string()
}

#[test]
fn room_label_schedule_and_takeoff_follow_edits_and_undo_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let (_dir, mut s, outline, label, boundary) = setup(units);
        let u = 1.0 / s.drawing.units.to_metres();
        assert_eq!(text(&s, label), "Area 12.00 m²");
        let before = s.history_steps().0.len();
        let schedule = add(&mut s, json!({"type":"room_schedule", "at":[8.0*u,8.0*u]}));
        assert_eq!(s.history_steps().0.len(), before + 1);
        assert_eq!(cell(&s, schedule, 1), "12.00");
        assert_eq!(cell(&s, schedule, 2), "12.00");
        let Outcome::Measured {
            measurement: by_label,
        } = run(
            &mut s,
            json!({"type":"measure_room","label":{"handle":label}}),
        )
        else {
            panic!("measured")
        };
        let Outcome::Measured {
            measurement: by_seed,
        } = run(&mut s, json!({"type":"measure_room","point":[u,u]}))
        else {
            panic!("measured")
        };
        assert_eq!(by_label.handles, [boundary]);
        assert_eq!(by_seed.handles, [boundary]);
        assert!((by_label.shown - 12.0).abs() < 1e-8);
        run(
            &mut s,
            json!({"type":"measure_entity", "handle":boundary, "chain":[]}),
        );
        let before = s.history_steps().0.len();
        run(
            &mut s,
            json!({"type":"modify","kind":"scale","targets":[{"handle":outline}],"base":[0,0],"factor":2}),
        );
        assert_eq!(s.history_steps().0.len(), before + 1);
        assert_eq!(text(&s, label), "Area 48.00 m²");
        assert_eq!(cell(&s, schedule, 1), "48.00");
        assert_eq!(cell(&s, schedule, 2), "48.00");
        assert!(s
            .takeoff
            .items
            .iter()
            .all(|m| (m.shown - 48.0).abs() < 1e-8));
        assert!(s.undo());
        assert_eq!(text(&s, label), "Area 12.00 m²");
        assert_eq!(cell(&s, schedule, 2), "12.00");
        assert!(s
            .takeoff
            .items
            .iter()
            .all(|m| (m.shown - 12.0).abs() < 1e-8));
        assert!(s.redo());
        assert_eq!(text(&s, label), "Area 48.00 m²");
    }
}

#[test]
fn room_fields_and_islands_survive_dwg_and_dxf_without_the_project_file() {
    for ext in ["dwg", "dxf"] {
        let (dir, mut s, outline, label, boundary) = setup(6);
        add(
            &mut s,
            json!({"type":"draw", "kind":"rectangle", "a":[2,1], "b":[3,2]}),
        );
        assert_eq!(text(&s, label), "Area 11.00 m²");
        let schedule = add(&mut s, json!({"type":"room_schedule", "at":[8,8]}));
        let path = dir.path().join(format!("roundtrip.{ext}"));
        s.save_as(&path).unwrap();
        let project = path.with_extension("qiyas");
        if project.exists() {
            std::fs::remove_file(project).unwrap();
        }
        let mut reopened = Session::open(&path).unwrap();
        assert!(reopened
            .drawing
            .doc
            .get_entity(Handle::new(boundary))
            .is_some());
        let Outcome::Measured { measurement } = run(
            &mut reopened,
            json!({"type":"measure_room", "label":{"handle":label}}),
        ) else {
            panic!("measured")
        };
        assert!((measurement.shown - 11.0).abs() < 1e-8);
        run(
            &mut reopened,
            json!({"type":"modify", "kind":"scale", "targets":[{"handle":outline}], "base":[0,0], "factor":2}),
        );
        assert_eq!(text(&reopened, label), "Area 47.00 m²");
        assert_eq!(cell(&reopened, schedule, 2), "47.00");
        add(
            &mut reopened,
            json!({"type":"draw", "kind":"rectangle", "a":[5,1], "b":[6,2]}),
        );
        assert_eq!(text(&reopened, label), "Area 46.00 m²");
        assert_eq!(cell(&reopened, schedule, 2), "46.00");
        reopened.save_as(&path).unwrap();
        let project = path.with_extension("qiyas");
        if project.exists() {
            std::fs::remove_file(project).unwrap();
        }
        let mut twice = Session::open(&path).unwrap();
        assert_eq!(
            twice.entity_info(boundary, &[]).unwrap().area,
            Some(46.0),
            "{ext}: the retained geometry and islands must be correct before any refresh"
        );
        let Outcome::Measured { measurement } = run(
            &mut twice,
            json!({"type":"measure_room", "label":{"handle":label}}),
        ) else {
            panic!("measured after a second read")
        };
        assert!(
            (measurement.shown - 46.0).abs() < 1e-8,
            "{ext}: expected 46 m2 after reloading both islands, got {}",
            measurement.shown
        );
        assert_eq!(cell(&twice, schedule, 2), "46.00");
    }
}

#[test]
fn construction_checks_stale_label_schedule_and_takeoff_quantities() {
    let (_dir, mut s, _outline, label, _) = setup(6);
    let schedule = add(&mut s, json!({"type":"room_schedule", "at":[8,8]}));
    run(
        &mut s,
        json!({"type":"measure_room", "label":{"handle":label}}),
    );
    run(
        &mut s,
        json!({"type":"edit_text", "target":{"handle":label}, "value":"Area 13.00 m²"}),
    );
    run(
        &mut s,
        json!({"type":"edit_cell", "target":{"handle":schedule}, "row":1,"col":1,"text":"14.00"}),
    );
    s.takeoff.items[0].shown = 15.0;
    let report = s.check_drawing(&CheckScope {
        packs: vec!["construction".into()],
        rules: vec!["room_area_mismatch".into()],
        ..Default::default()
    });
    assert_eq!(
        report.counts.get("room_area_mismatch"),
        Some(&3),
        "{:?}",
        report.findings
    );
    let general = s.check_drawing(&CheckScope {
        packs: vec!["general".into()],
        rules: vec!["room_area_mismatch".into()],
        ..Default::default()
    });
    assert!(general.findings.is_empty());
}

#[test]
fn opening_the_enclosure_invalidates_the_room_instead_of_measuring_its_old_boundary() {
    let (_dir, mut s, outline, label, _) = setup(6);
    run(
        &mut s,
        json!({"type":"measure_room", "label":{"handle":label}}),
    );
    run(
        &mut s,
        json!({"type":"delete_entities", "targets":[{"handle":outline}]}),
    );
    assert_eq!(text(&s, label), "Area #### m²");
    assert!(s.takeoff.items[0].note.contains("unavailable"));
    assert!(!s.takeoff.items[0].available());
    assert!(s.takeoff.totals().is_empty());
    qiyas_core::knowledge::put(
        &mut s.drawing.doc,
        qiyas_core::knowledge::Kind::Revisions,
        "result",
        &json!({"items":[{"code":"ROOM","description_en":"Room","description_ar":"Room","unit":"m2","a_quantity":12.0,"b_quantity":12.0,"delta":0.0,"elements":[]}],"comparison":{"sheets":[],"tolerance":0.001},"from_path":"a.dxf","to_path":"b.dxf","clouds":[],"warnings":[]}),
    ).unwrap();
    let variation = s.variation().unwrap().unwrap();
    assert!(variation.items.is_empty());
    assert!(variation
        .warnings
        .iter()
        .any(|warning| warning.contains("unavailable")));
    let record = serde_json::to_value(&s.takeoff.items[0]).unwrap();
    assert!(record["shown"].is_null());
    assert!(record["value"].is_null());
    let restored: qiyas_core::Measurement = serde_json::from_value(record).unwrap();
    assert!(!restored.available());
    let path = s.drawing.path.clone();
    s.save_as_headless(&path).unwrap();
    let reopened = Session::open(&path).unwrap();
    assert!(!reopened.takeoff.items[0].available());
    assert!(reopened.takeoff.totals().is_empty());
    for extension in ["dwg", "dxf"] {
        let path = path.with_extension(extension);
        s.save_as_headless(&path).unwrap();
        let project = path.with_extension("qiyas");
        if project.exists() {
            std::fs::remove_file(project).unwrap();
        }
        let standalone = Session::open(&path).unwrap();
        assert!(!standalone.takeoff.items[0].available());
        assert!(standalone.takeoff.totals().is_empty());
    }
    assert!(s
        .run_input(
            &json!({"type":"measure_room", "label":{"handle":label}}),
            Source::User
        )
        .is_err());
    let report = s.check_drawing(&CheckScope {
        rules: vec!["room_area_mismatch".into()],
        packs: vec!["construction".into()],
        ..Default::default()
    });
    assert_eq!(report.counts.get("room_area_mismatch"), Some(&2));
    assert!(s.undo());
    assert_eq!(text(&s, label), "Area 12.00 m²");
}

#[test]
fn room_schedule_totals_several_rooms_without_typed_quantities() {
    let (_dir, mut s, _, _, _) = setup(6);
    add(
        &mut s,
        json!({"type":"draw", "kind":"rectangle", "a":[5,0],"b":[7,3]}),
    );
    run(
        &mut s,
        json!({"type":"label_spaces", "query":{"seeds":[{"name":"Room B", "point":[6,1]}]}, "layer":"A-ROOM-LABEL"}),
    );
    let table = add(&mut s, json!({"type":"room_schedule", "at":[10,8]}));
    assert_eq!(cell(&s, table, 1), "12.00");
    assert_eq!(cell(&s, table, 2), "6.00");
    assert_eq!(cell(&s, table, 3), "18.00");
}

#[test]
fn joined_wall_edits_retrace_the_room_and_update_its_schedule() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("walls.dwg"), 6);
    run(
        &mut s,
        json!({"type":"walls", "thickness":0.2, "runs":[{"points":[[0,0],[6,0],[6,4],[0,4]], "closed":true}]}),
    );
    let Outcome::Rooms { report } = run(
        &mut s,
        json!({"type":"label_spaces", "query":{"seeds":[{"name":"Left", "point":[1,1]}]}, "layer":"A-ROOM-LABEL"}),
    ) else {
        panic!("room")
    };
    let label = report.rooms[0].label_handle.unwrap();
    assert_eq!(text(&s, label), "Area 22.04 m²");
    let schedule = add(&mut s, json!({"type":"room_schedule", "at":[10,8]}));
    run(
        &mut s,
        json!({"type":"measure_room", "label":{"handle":label}}),
    );
    let before = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"walls", "thickness":0.2, "runs":[{"points":[[3,0],[3,4]]}]}),
    );
    assert_eq!(s.history_steps().0.len(), before + 1);
    assert_eq!(text(&s, label), "Area 10.64 m²");
    assert_eq!(cell(&s, schedule, 2), "10.64");
    assert!((s.takeoff.items[0].shown - 10.64).abs() < 1e-8);
    assert!(s.undo());
    assert_eq!(text(&s, label), "Area 22.04 m²");
    assert_eq!(cell(&s, schedule, 2), "22.04");
}
