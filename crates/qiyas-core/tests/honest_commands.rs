use opencadcodec::{EntityType, Handle};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Value {
    let result = s
        .run_input(&command, Source::User)
        .unwrap_or_else(|e| panic!("{command}: {e}"));
    serde_json::to_value(result.outcome).unwrap()
}

fn fixture(units: i16) -> (tempfile::TempDir, Session, Value, f64) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("honest.dxf"), units);
    let k = 1.0 / s.drawing.units.to_metres();
    let walls = run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[8.0*k,0]]}],"thickness":0.2*k,
        "openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"}]}),
    );
    (dir, s, walls, k)
}

fn cell_texts(s: &Session, handle: u64) -> Vec<String> {
    let EntityType::Table(table) = s.drawing.doc.get_entity(Handle::new(handle)).unwrap() else {
        panic!("table");
    };
    (0..table.rows.len())
        .flat_map(|r| {
            (0..table.columns.len()).map(move |c| table.cell_text(r, c).unwrap_or("").to_string())
        })
        .collect()
}

#[test]
fn unchanged_generated_graphics_keep_their_handles() {
    let (_dir, mut s, walls, _) = fixture(6);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    let door = walls["inserts"][0].as_u64().unwrap();
    let parts = |s: &Session| {
        s.drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let part: Value =
                    qiyas_core::xdata::read_json(entity.common(), "QIYAS_OBJECT_PART")?;
                let mut shape = serde_json::to_value(entity).unwrap();
                if let Some(map) = shape.as_object_mut() {
                    map.remove("common");
                    for value in map.values_mut() {
                        if let Some(map) = value.as_object_mut() {
                            map.remove("common");
                        }
                    }
                }
                Some((entity.common().handle.value(), part["role"].clone(), shape))
            })
            .collect::<Vec<_>>()
    };
    for command in [
        json!({"type":"add_wall_openings","centreline":wall,"openings":[{"at":5,"width":1,"kind":"window"}]}),
        json!({"type":"set_object_props","targets":[door],"props":{"distance":2.5}}),
        json!({"type":"edit_wall_opening","insert":door,"width":1}),
    ] {
        let before = parts(&s);
        run(&mut s, command);
        let after = parts(&s);
        let mut unchanged = 0;
        for (handle, role, shape) in &before {
            if let Some((current, _, _)) =
                after.iter().find(|(_, r, data)| r == role && data == shape)
            {
                assert_eq!(current, handle, "unchanged {role} changed handle");
                unchanged += 1;
            }
        }
        assert!(unchanged > 0, "the edit must leave some graphics unchanged");
    }
}

#[test]
fn rotating_a_wall_with_its_door_preserves_the_requested_transform() {
    for units in [4, 5, 6, 1] {
        let (_dir, mut s, walls, _) = fixture(units);
        let wall = walls["centrelines"][0].as_u64().unwrap();
        let door = walls["inserts"][0].as_u64().unwrap();
        for angle in [-90.0_f64, 180.0, 90.0] {
            let before = s.entity_info(door, &[]).unwrap().position.unwrap();
            run(
                &mut s,
                json!({"type":"modify","kind":"rotate","targets":[wall,door],"base":[0,0],"angle":angle}),
            );
            let after = s.entity_info(door, &[]).unwrap().position.unwrap();
            let (sin, cos) = angle.to_radians().sin_cos();
            assert!((after[0] - (before[0] * cos - before[1] * sin)).abs() < 1e-7);
            assert!((after[1] - (before[0] * sin + before[1] * cos)).abs() < 1e-7);
        }
    }
}

#[test]
fn source_edits_survive_schedules_and_rebuilds_in_all_units() {
    for units in [4, 5, 6, 1] {
        let (dir, mut s, walls, k) = fixture(units);
        let wall = walls["centrelines"][0].as_u64().unwrap();
        let door = walls["inserts"][0].as_u64().unwrap();
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[door.to_string()],"props":{"mark":"D1"}}),
        );
        let schedule = run(&mut s, json!({"type":"opening_schedule","at":[9.0*k,0]}))["handles"][0]
            .as_u64()
            .unwrap();
        assert!(cell_texts(&s, schedule).contains(&"D1".into()));
        run(
            &mut s,
            json!({"type":"modify","kind":"move","targets":[format!("0x{door:X}")],"delta":[0.5*k,0]}),
        );
        assert!(
            (s.wall_groups(None).unwrap()[0].runs[0].openings[0]
                .opening
                .at
                - 2.5 * k)
                .abs()
                < 1e-7
        );
        run(
            &mut s,
            json!({"type":"set_property","target":{"handle":wall},"prop":"color","value":"ACI 1"}),
        );
        let selected = run(&mut s, json!({"type":"object_schedule","at":[9.0*k,-k],"columns":["type","count"],"ids":[wall]}))["handles"][0].as_u64().unwrap();
        let rebuilt = run(
            &mut s,
            json!({"type":"add_wall_openings","centreline":wall,"openings":[{"at":5.0*k,"width":k,"kind":"window"}]}),
        );
        assert_eq!(rebuilt["centrelines"][0], wall);
        assert_eq!(rebuilt["inserts"][0], door);
        assert_eq!(
            s.drawing
                .doc
                .get_entity(Handle::new(wall))
                .unwrap()
                .common()
                .color,
            opencadcodec::Color::Index(1)
        );
        assert!(cell_texts(&s, schedule).contains(&"D1".into()));
        assert!(!cell_texts(&s, selected).iter().any(|s| s.contains("####")));
        let before = s.history_steps().0.len();
        let unchanged = run(&mut s, json!({"type":"edit_wall_opening","insert":door}));
        for field in ["centreline_length", "gross_area", "net_area", "volume"] {
            assert!(
                (unchanged[field].as_f64().unwrap() - rebuilt[field].as_f64().unwrap()).abs()
                    < 1e-7,
                "{field}"
            );
        }
        assert_eq!(s.history_steps().0.len(), before);
        assert!(s.last_change().is_empty());
        run(
            &mut s,
            json!({"type":"edit_wall_opening","insert":door,"mark":"D2"}),
        );
        assert!(cell_texts(&s, schedule).contains(&"D2".into()));
        for extension in ["dwg", "dxf"] {
            let path = dir.path().join(format!("custom-id.{extension}"));
            s.save_as_headless(&path).unwrap();
            let mut reopened = Session::open(&path).unwrap();
            assert!(cell_texts(&reopened, schedule).contains(&"D2".into()));
            let objects = reopened
                .objects(&qiyas_core::objects::ObjectFilter::default())
                .unwrap();
            assert_eq!(
                objects
                    .iter()
                    .find(|o| o.instance.id == door)
                    .unwrap()
                    .instance
                    .props["mark"],
                "D2"
            );
            let history = reopened.history_steps();
            run(
                &mut reopened,
                json!({"type":"edit_wall_opening","insert":door}),
            );
            assert_eq!(reopened.history_steps(), history);
            assert!(reopened.last_change().is_empty());
        }
    }
}

#[test]
fn partial_override_is_an_error_and_rolls_back_every_requested_field() {
    let (_dir, mut s, walls, _) = fixture(4);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    let before = s.drawing.doc.get_entity(Handle::new(wall)).unwrap().clone();
    let history = s.history_steps().0.len();
    // A hidden centreline is controlled by its recipe. The valid colour change must not
    // hide the fact that the same request's visibility edit was reversed.
    let error = s
        .run_input(
            &json!({"type":"set_records","collection":"entities","updates":[
                {"key":wall,"path":"/common/color","value":{"Index":2}},
                {"key":wall,"path":"/common/invisible","value":true}
            ]}),
            Source::User,
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("overridden") && error.contains("object geometry"),
        "{error}"
    );
    assert_eq!(s.drawing.doc.get_entity(Handle::new(wall)), Some(&before));
    assert_eq!(s.history_steps().0.len(), history);
    assert!(s.last_change().is_empty());
}

#[test]
fn raw_field_and_live_schedule_edits_are_refused_without_history() {
    let (_dir, mut s, walls, _) = fixture(6);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    let field = run(
        &mut s,
        json!({"type":"field","at":[0,2],"field":{"kind":"length","target":{"handle":wall}}}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    let table = run(&mut s, json!({"type":"opening_schedule","at":[0,3]}))["handles"][0]
        .as_u64()
        .unwrap();
    for (handle, path, reason) in [
        (field, "/value", "edit_text"),
        (table, "/rows/1/cells/0/contents/0/value/text", "edit_cell"),
    ] {
        let before = s
            .drawing
            .doc
            .get_entity(Handle::new(handle))
            .unwrap()
            .clone();
        let history = s.history_steps();
        let error = s.run_input(&json!({"type":"set_records","collection":"entities","updates":[{"key":handle,"path":path,"value":"D9"}]}), Source::User).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
        assert_eq!(s.drawing.doc.get_entity(Handle::new(handle)), Some(&before));
        assert_eq!(s.history_steps(), history);
        assert!(s.last_change().is_empty());
    }
}

#[test]
fn no_op_source_edit_makes_no_history_and_keeps_redo() {
    let (_dir, mut s, walls, _) = fixture(4);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,5000],"b":[1000,5000]}),
    );
    assert!(s.undo());
    let before = s.history_steps();
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[wall],"props":{"width":0.2}}),
    );
    assert_eq!(s.history_steps(), before);
    assert!(s.last_change().is_empty());
    assert!(s.redo());
}

#[test]
fn unchanged_table_cell_keeps_its_display_handles_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("table.dxf"), 6);
    let table = run(
        &mut s,
        json!({"type":"table","at":[0,0],"rows":1,"cols":1,"data":[["Door"]]}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    let before: Vec<_> = s.drawing.doc.entities().cloned().collect();
    let history = s.history_steps();
    run(
        &mut s,
        json!({"type":"edit_cell","target":{"handle":table},"row":0,"col":0,"text":"Door"}),
    );
    assert_eq!(s.history_steps(), history);
    assert!(s.last_change().is_empty());
    assert_eq!(
        s.drawing.doc.entities().cloned().collect::<Vec<_>>(),
        before
    );
}

#[test]
fn unchanged_takeoff_properties_keep_history_and_redo() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("takeoff.dxf"), 6);
    run(
        &mut s,
        json!({"type":"add_measurement","kind":"length","points":[[0,0],[1,0]],"description":"Measured length"}),
    );
    let id = s.takeoff.items[0].id.clone();
    run(&mut s, json!({"type":"set_note","id":id,"text":"Reviewed"}));
    assert!(s.undo());
    let before = s.history_steps();
    for command in [
        json!({"type":"set_note","id":id,"text":""}),
        json!({"type":"set_description","id":id,"text":"Measured length"}),
        json!({"type":"link_boq","id":id,"code":null}),
    ] {
        assert_eq!(run(&mut s, command)["type"], "nothing");
        assert_eq!(s.history_steps(), before);
    }
    assert!(s.redo());
    assert_eq!(s.takeoff.items[0].note, "Reviewed");
}

#[test]
fn generated_edits_and_unknown_fields_are_refused_before_history_changes() {
    let (_dir, mut s, walls, _) = fixture(4);
    let door = walls["inserts"][0].as_u64().unwrap();
    let outline = walls["outlines"][0].as_u64().unwrap();
    let schedule = run(&mut s, json!({"type":"opening_schedule","at":[9000,0]}))["handles"][0]
        .as_u64()
        .unwrap();
    for command in [
        json!({"type":"set_property","target":{"handle":outline},"prop":"layer","value":"0"}),
        json!({"type":"set_records","collection":"entities","updates":[{"key":outline,"path":"/common/color","value":{"Index":2}}]}),
        json!({"type":"modify","kind":"copy","targets":[{"handle":door}],"delta":[2000,0]}),
        json!({"type":"modify","kind":"move","targets":[{"handle":door}],"delta":[0,100]}),
        json!({"type":"edit_cell","target":{"handle":schedule},"row":1,"col":0,"text":"D9"}),
        json!({"type":"insert_row","target":{"handle":schedule},"at":1}),
        json!({"type":"edit_wall_opening","insert":door,"unknown":1}),
    ] {
        let before = s.history_steps();
        assert!(s.run_input(&command, Source::User).is_err(), "{command}");
        assert_eq!(s.history_steps(), before);
        assert!(s.last_change().is_empty());
    }
}

#[test]
fn rename_and_role_layer_properties_survive_opening_edits() {
    let (_dir, mut s, walls, _) = fixture(4);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    run(
        &mut s,
        json!({"type":"rename_layer","from":"A-WALL","to":"Walls"}),
    );
    assert!(!s.drawing.doc.layers.contains("A-WALL"));
    assert!(
        s.drawing
            .doc
            .entities()
            .filter(|e| e.common().layer == "Walls")
            .count()
            > 0
    );
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[wall],"props":{"hatch_layer":"Pattern","doors_layer":"Doors"}}),
    );
    assert!(s
        .drawing
        .doc
        .entities()
        .any(|e| matches!(e, EntityType::Hatch(_)) && e.common().layer == "Pattern"));
    let door = walls["inserts"][0].as_u64().unwrap();
    assert_eq!(
        s.drawing
            .doc
            .get_entity(Handle::new(door))
            .unwrap()
            .common()
            .layer,
        "Doors"
    );
    run(
        &mut s,
        json!({"type":"edit_wall_opening","insert":door,"width":1000}),
    );
    assert!(!s.drawing.doc.layers.contains("A-WALL"));
    assert!(s
        .drawing
        .doc
        .entities()
        .any(|e| matches!(e, EntityType::Hatch(_)) && e.common().layer == "Pattern"));
}

#[test]
fn literal_wall_handles_remain_valid_in_native_batches() {
    let (_dir, mut s, walls, k) = fixture(4);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    let door = walls["inserts"][0].as_u64().unwrap();
    run(
        &mut s,
        json!({"type":"batch","commands":[
            {"type":"add_wall_openings","centreline":wall,"openings":[{"at":4.0*k,"width":0.8*k,"kind":"window"}]},
            {"type":"add_wall_openings","centreline":wall,"openings":[{"at":6.0*k,"width":0.8*k,"kind":"window"}]},
            {"type":"edit_wall_opening","insert":door,"mark":"D1"}
        ]}),
    );
    assert_eq!(s.wall_groups(None).unwrap()[0].runs[0].openings.len(), 3);
    assert!(s.drawing.doc.get_entity(Handle::new(door)).is_some());
}

#[test]
fn handle_strings_are_normalized_without_coercing_other_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("handles.dxf"), 4);
    for value in [json!(673), json!("673"), json!("0x2A1")] {
        let error = s.run_input(&json!({"type":"set_property","target":{"handle":value},"prop":"color","value":"ACI 1"}), Source::User).unwrap_err().to_string();
        assert!(error.contains("No entity with handle 673"), "{error}");
        assert!(!error.contains("2A1"), "{error}");
    }
    assert!(
        !s.validate_input(&json!({"type":"draw","kind":"circle","centre":[0,0],"radius":"3"}))
            .valid
    );
    assert!(
        !s.validate_input(
            &json!({"type":"set_property","target":{"handle":"2A1"},"prop":"color","value":"ACI 1"})
        )
        .valid
    );
}

#[test]
fn older_schedule_handle_templates_also_display_decimal_handles() {
    let (_dir, mut s, walls, _) = fixture(6);
    let wall = walls["centrelines"][0].as_u64().unwrap();
    let table = run(
        &mut s,
        json!({"type":"object_schedule","at":[10,0],"columns":["reference"],
        "templates":{"reference":"{id_hex}"},"ids":[wall]}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    assert!(cell_texts(&s, table).contains(&wall.to_string()));
}

#[test]
fn recipe_layer_references_rename_without_changing_other_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("layers.dxf"), 6);
    run(&mut s, json!({"type":"new_layer","name":"text"}));
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{
            "name":"Survey area","category":"Survey","geometry":"region",
            "properties":{"face":{"kind":"text","default":"text"},"pattern":{"kind":"choice","default":"text","choices":["text","Other patterns"]},"name":{"kind":"text","default":"text"}},
            "plan":{"outline":true,"hatch":true,"layer_properties":{"outline":"face","hatch":"pattern"}},"quantities":{"area_value":"area"}
        }}),
    );
    let driver = run(
        &mut s,
        json!({"type":"place_objects","objects":[{
            "type":"Survey area","points":[[0,0],[4,0],[4,3],[0,3]],"props":{}
        }]}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        json!({"type":"rename_layer","from":"text","to":"Survey outlines"}),
    );
    let object = s
        .objects(&qiyas_core::objects::ObjectFilter::default())
        .unwrap()
        .into_iter()
        .find(|o| o.instance.id == driver)
        .unwrap();
    assert_eq!(object.instance.props["face"], "Survey outlines");
    assert_eq!(object.instance.props["pattern"], "Survey outlines");
    assert_eq!(object.instance.props["name"], "text");
    assert!(!s.drawing.doc.layers.contains("text"));
    assert!(s
        .drawing
        .doc
        .entities()
        .any(|e| e.common().layer == "Survey outlines"));
    let placed = run(&mut s, json!({"type":"place_objects","objects":[{
        "type":"Survey area","points":[[5,0],[9,0],[9,3],[5,3]],"layers":{"outline":"Other outlines"}
    }]}))["handles"][0].as_u64().unwrap();
    let objects = s
        .objects(&qiyas_core::objects::ObjectFilter::default())
        .unwrap();
    assert_eq!(
        objects
            .iter()
            .find(|o| o.instance.id == placed)
            .unwrap()
            .instance
            .props["face"],
        "Other outlines"
    );
    assert!(s
        .drawing
        .doc
        .entities()
        .any(|e| e.common().layer == "Other outlines"));
}

#[test]
fn a_layer_edit_on_any_joined_run_updates_the_shared_graphics() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("joined.dxf"), 6);
    let walls = run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[4,0]]},{"points":[[4,0],[4,3]]}],"thickness":0.2}),
    );
    let runs = walls["centrelines"].as_array().unwrap();
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[runs[1]],"props":{"hatch_layer":"Shared pattern"}}),
    );
    assert!(s
        .drawing
        .doc
        .entities()
        .filter(|e| matches!(e, EntityType::Hatch(_)))
        .all(|e| e.common().layer == "Shared pattern"));
    let records = s
        .objects(&qiyas_core::objects::ObjectFilter::default())
        .unwrap();
    assert!(records
        .iter()
        .all(|r| r.instance.props["hatch_layer"] == "Shared pattern"));
}
