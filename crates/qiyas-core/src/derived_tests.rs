use super::*;
use crate::Source;

fn state(s: &Session) -> Value {
    let entities: BTreeMap<_, _> = s
        .drawing
        .doc
        .entities()
        .map(|e| (e.common().handle.value(), json!(e)))
        .collect();
    let objects: BTreeMap<_, _> = s
        .drawing
        .doc
        .objects
        .iter()
        .map(|(h, o)| (h.value(), json!(o)))
        .collect();
    json!({"entities":entities,"objects":objects,"layers":s.drawing.doc.layers.iter().collect::<Vec<_>>(),
        "blocks":s.drawing.doc.block_records.iter().collect::<Vec<_>>(),"takeoff":s.takeoff})
}
fn equivalent(s: &mut Session, context: &str) {
    let before = state(s);
    let document = s.drawing.doc.clone();
    let takeoff = s.takeoff.clone();
    full(s).unwrap();
    let after = state(s);
    // Include generated geometry, exact handles, metadata, schedules, fields and quantities.
    // A skipped dependency is exposed by the independent full refresh changing this state.
    if before != after {
        panic!(
            "incremental differs from full derivation after {context}:\n{}",
            json_patch::diff(&before, &after)
        );
    }
    s.drawing.doc = document;
    s.takeoff = takeoff;
}
fn run(s: &mut Session, command: Value) -> Value {
    let outcome = s
        .run_input(&command, Source::User)
        .unwrap_or_else(|e| panic!("{command}: {e}"));
    let result = json!(outcome);
    equivalent(s, &command.to_string());
    result
}
#[test]
fn incremental_matches_full_for_walls_openings_regions_schedules_fields_and_history() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("derived.dxf"), unit);
        let k = 1.0 / s.drawing.units.to_metres();
        let walls = run(
            &mut s,
            json!({"type":"walls","runs":[
            {"points":[[0,0],[6.0*k,0]]},{"points":[[6.0*k,0],[6.0*k,4.0*k]]},
            {"points":[[6.0*k,4.0*k],[0,4.0*k]]},{"points":[[0,4.0*k],[0,0]]}
        ],"thickness":0.2*k,"openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"}]}),
        );
        let wall = walls["centrelines"][0].clone();
        let door = walls["inserts"][0].clone();
        run(
            &mut s,
            json!({"type":"label_spaces","query":{"seeds":[{"point":[3.0*k,2.0*k],"name":"Room"}]}}),
        );
        run(&mut s, json!({"type":"room_schedule","at":[8.0*k,4.0*k]}));
        run(&mut s, json!({"type":"opening_schedule","at":[8.0*k,0]}));
        run(
            &mut s,
            json!({"type":"object_schedule","at":[8.0*k,-4.0*k],"ids":[wall],"columns":["type","length","volume"]}),
        );
        run(
            &mut s,
            json!({"type":"field","at":[0,-2.0*k],"field":{"kind":"length","target":{"handle":wall}}}),
        );
        for i in 0..4 {
            run(
                &mut s,
                json!({"type":"new_layer","name":format!("Extra {i}")}),
            );
            assert!(
                s.last_change().added.is_empty()
                    && s.last_change().changed.is_empty()
                    && s.last_change().removed.is_empty()
            );
        }
        let text = run(
            &mut s,
            json!({"type":"text","at":[2.0*k,2.0*k],"value":"غرفة"}),
        )["handles"][0]
            .clone();
        run(
            &mut s,
            json!({"type":"edit_text","target":{"handle":text},"value":"Room note"}),
        );
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[door],"props":{"mark":"D1","distance":2.5}}),
        );
        run(
            &mut s,
            json!({"type":"edit_wall_opening","insert":door,"width":1.1*k}),
        );
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[wall],"props":{"wall_height":3.5}}),
        );
        run(
            &mut s,
            json!({"type":"rename_layer","from":"A-WALL","to":"Walls"}),
        );
        let before = state(&s);
        assert!(s.run_input(&json!({"type":"batch","commands":[{"type":"set_object_props","targets":[door],"props":{"mark":"bad"}},{"type":"delete_entities","handles":[999999]}]}),Source::User).is_err());
        assert_eq!(state(&s), before);
        equivalent(&mut s, "failed batch rollback");
        assert!(s.undo());
        equivalent(&mut s, "undo");
        assert!(s.redo());
        equivalent(&mut s, "redo");
        run(
            &mut s,
            json!({"type":"remove_wall_openings","inserts":[door]}),
        );
        let before = state(&s);
        assert!(s
            .run_input(
                &json!({"type":"delete_entities","handles":[wall]}),
                Source::User
            )
            .is_err());
        assert_eq!(state(&s), before);
        equivalent(&mut s, "refused partial wall erase");
        for ext in ["dxf", "dwg"] {
            let path = dir.path().join(format!("reopened.{ext}"));
            s.save_as_headless(&path).unwrap();
            let mut reopened = Session::open(&path).unwrap();
            equivalent(&mut reopened, "reopen");
            run(
                &mut reopened,
                json!({"type":"new_layer","name":"After reopening"}),
            );
            assert!(reopened.last_change().changed.is_empty());
        }
    }
}

#[test]
fn independent_objects_and_data_dependencies_match_full_derivation() {
    let mut s = Session::new_blank(std::path::Path::new("objects.dxf"), 6);
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"tables","name":"sizes","data":{"A":{"width":0.2}}}),
    );
    let definition = json!({"name":"Band","category":"Test","geometry":"band","join":"separate",
        "properties":{"width":{"kind":"number","unit":"length","default":0.2},"height":{"kind":"number","unit":"length","default":3}},
        "vertical":{"height":"height"},"plan":{"band_faces":"lookup('sizes','A','width')","hatch":true,"label_template":"{type}"},"quantities":{"volume":"geometry.net_area*height"}});
    run(
        &mut s,
        json!({"type":"define_object_type","definition":definition}),
    );
    let ids=run(&mut s,json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[5,0]]},{"type":"Band","points":[[0,10],[5,10]]}]}))["handles"].clone();
    run(
        &mut s,
        json!({"type":"object_schedule","at":[8,0],"ids":[ids[0]],"columns":["type","volume"]}),
    );
    let citation = json!({"source":"Synthetic test method","clause":"Volume"});
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[{"rule":"volume","code":"V","description_en":"Volume","description_ar":"حجم","unit":"m3","section":"Test","trade":"Test","citation":citation}]}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":{"version":1,"name":"test-volume","title":"Test volume","based_on":"Synthetic only","source_note":"Test fixture","rules":[{"id":"volume","element_type":"Band","trade":"Test","unit":"m3","terms":[{"label":"Volume","operation":"add","factors":["volume"],"citation":citation}],"citation":citation}]}}),
    );
    let untouched = s
        .drawing
        .doc
        .entities()
        .filter(|e| {
            crate::xdata::read_json::<crate::objects::ObjectPart>(
                e.common(),
                crate::objects::PART_APP,
            )
            .is_some_and(|part| Some(part.owner) == ids[1].as_u64())
        })
        .map(|e| s.drawing.doc.get_entity_arc(e.common().handle).unwrap())
        .collect::<Vec<_>>();
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[ids[0]],"props":{"height":4}}),
    );
    for e in untouched {
        assert!(
            Arc::ptr_eq(
                &e,
                &s.drawing.doc.get_entity_arc(e.common().handle).unwrap()
            ),
            "unrelated object was rewritten"
        );
    }
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"tables","name":"sizes","data":{"A":{"width":0.4}}}),
    );
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"levels","name":"Upper","data":{"elevation":4}}),
    );
}

#[test]
fn generated_label_sizes_do_not_depend_on_the_sheet_on_screen() {
    for units in [4, 5, 6, 1] {
        let mut s = Session::new_blank(std::path::Path::new("sheets.dxf"), units);
        run(
            &mut s,
            json!({"type":"define_object_type","definition":{"name":"Marker","category":"Test","geometry":"point","plan":{"label_template":"Marker"}}}),
        );
        run(
            &mut s,
            json!({"type":"place_objects","objects":[{"type":"Marker","points":[[0,0]]}]}),
        );
        run(
            &mut s,
            json!({"type":"new_layout","name":"Sheet","paper":"ISO A4","orientation":"landscape"}),
        );
        let before = state(&s)["entities"].clone();
        let block = s.sheet_block(Some("Sheet"));
        run(&mut s, json!({"type":"set_sheet","block":block}));
        assert_eq!(state(&s)["entities"], before);
        run(&mut s, json!({"type":"new_layer","name":"On paper"}));
        assert_eq!(state(&s)["entities"], before);
    }
}

#[test]
fn enclosure_deletion_invalidates_quantities_exactly_like_full_derivation() {
    for units in [4, 5, 6, 1] {
        let mut s = Session::new_blank(std::path::Path::new("enclosure.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let boundary = run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4.0*k,3.0*k]}),
        )["handles"][0]
            .clone();
        run(
            &mut s,
            json!({"type":"label_spaces","query":{"seeds":[{"point":[2.0*k,1.5*k],"name":"Room"}]}}),
        );
        run(&mut s, json!({"type":"room_schedule","at":[6.0*k,3.0*k]}));
        run(&mut s, json!({"type":"measure_room","point":[2.0*k,1.5*k]}));
        assert!(s.takeoff.items.iter().any(|row| row.quantity_valid));
        run(
            &mut s,
            json!({"type":"delete_entities","handles":[boundary]}),
        );
        assert!(s.takeoff.items.iter().any(|row| !row.quantity_valid));
        assert!(s.undo());
        equivalent(&mut s, "restored enclosure");
        assert!(s.redo());
        equivalent(&mut s, "removed enclosure again");
    }
}

#[test]
fn linked_fields_follow_only_the_changed_geometry_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fields.dxf");
        let mut s = Session::new_blank(&path, units);
        let k = 1.0 / s.drawing.units.to_metres();
        let a = run(
            &mut s,
            json!({"type":"draw","kind":"line","a":[0,0],"b":[4.0*k,0]}),
        )["handles"][0]
            .clone();
        let b = run(
            &mut s,
            json!({"type":"draw","kind":"line","a":[0,2.0*k],"b":[4.0*k,2.0*k]}),
        )["handles"][0]
            .clone();
        run(
            &mut s,
            json!({"type":"field","at":[0,-k],"field":{"kind":"length","target":{"handle":a}}}),
        );
        let other = run(
            &mut s,
            json!({"type":"field","at":[0,3.0*k],"field":{"kind":"length","target":{"handle":b}}}),
        )["handles"][0]
            .as_u64()
            .unwrap();
        let untouched = s
            .drawing
            .doc
            .get_entity_arc(opencadcodec::Handle::new(other))
            .unwrap();
        run(
            &mut s,
            json!({"type":"set_records","collection":"entities","updates":[{"key":a,"path":"/end/x","value":5.0*k}]}),
        );
        assert!(Arc::ptr_eq(
            &untouched,
            &s.drawing
                .doc
                .get_entity_arc(opencadcodec::Handle::new(other))
                .unwrap()
        ));
        run(&mut s, json!({"type":"delete_entities","handles":[a]}));
        assert!(s.undo());
        equivalent(&mut s, "restored field source");
        // Simulate a saved file whose geometry changed in another CAD program.
        let Some(EntityType::Line(line)) = s
            .drawing
            .doc
            .get_entity_mut(opencadcodec::Handle::new(b.as_u64().unwrap()))
        else {
            panic!("Line")
        };
        line.end.x = 6.0 * k;
        crate::fixtures::write_drawing(&s.drawing.doc, &path);
        let mut reopened = Session::open(&path).unwrap();
        equivalent(&mut reopened, "stale linked field on open");
    }
}

#[test]
fn reloading_an_external_boundary_refreshes_its_room_quantities() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("boundary.dxf");
        let mut s = Session::new_blank(&dir.path().join("host.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let write_boundary = |width| {
            let mut source = Session::new_blank(&path, units);
            run(
                &mut source,
                json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[width*k,3.0*k]}),
            );
            crate::fixtures::write_drawing(&source.drawing.doc, &path);
        };
        write_boundary(4.0);
        crate::fixtures::add_xref(
            &mut s.drawing.doc,
            "Boundary",
            "boundary.dxf",
            [0.0, 0.0],
            0.0,
        );
        s.reload_xrefs();
        run(
            &mut s,
            json!({"type":"label_spaces","query":{"seeds":[{"point":[k,k],"name":"Room"}]}}),
        );
        run(&mut s, json!({"type":"measure_room","point":[k,k]}));
        run(&mut s, json!({"type":"room_schedule","at":[8.0*k,3.0*k]}));
        assert!((s.takeoff.items[0].shown - 12.0).abs() < 1e-8);
        write_boundary(6.0);
        s.reload_xrefs();
        assert!((s.takeoff.items[0].shown - 18.0).abs() < 1e-8);
        equivalent(&mut s, "external boundary reload");
        run(&mut s, json!({"type":"new_layer","name":"After reload"}));
        assert!(s.last_change().changed.is_empty());
    }
}

#[test]
fn editing_a_wall_copy_preserves_source_part_metadata_and_matches_full() {
    for units in [4, 5, 6, 1] {
        let mut s = Session::new_blank(std::path::Path::new("copies.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[6.0*k,0]]}],"thickness":0.2*k,"openings":[{"run":0,"at":k,"width":0.9*k,"kind":"door"}]}),
        );
        let source = json!(s.wall_groups(None).unwrap()[0]);
        run(
            &mut s,
            json!({"type":"modify","kind":"copy","targets":source["handles"],"delta":[0,10.0*k],"count":2}),
        );
        let copied = json!(s.wall_groups(None).unwrap());
        let copy = copied
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["runs"][0]["points"][0][1].as_f64() == Some(10.0 * k))
            .unwrap();
        run(
            &mut s,
            json!({"type":"add_wall_openings","centreline":copy["runs"][0]["centreline"],"openings":[{"at":3.0*k,"width":k,"kind":"window"}]}),
        );
        let after = json!(s.wall_groups(None).unwrap());
        assert!(after.as_array().unwrap().contains(&source));
    }
}

#[test]
fn grouped_openings_match_full_derivation_and_undo_in_every_unit() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("grouped.dxf"), unit);
        s.apply_settings_method(Some(&crate::measurement::RuleSet::named("POMI (RICS)")))
            .unwrap();
        let k = 1.0 / s.drawing.units.to_metres();
        let walls = run(
            &mut s,
            json!({"type":"walls","thickness":0.2*k,"runs":[
                {"points":[[0,0],[20.0*k,0]]},{"points":[[20.0*k,0],[20.0*k,12.0*k]]},
                {"points":[[20.0*k,12.0*k],[0,12.0*k]]},{"points":[[0,12.0*k],[0,0]]},
                {"points":[[10.0*k,0],[10.0*k,12.0*k]]},{"points":[[0,6.0*k],[20.0*k,6.0*k]]}
            ]}),
        );
        let before = state(&s);
        let commands: Vec<_> = [0,1,4].into_iter().map(|i| json!({"type":"add_wall_openings",
            "centreline":walls["centrelines"][i],"openings":[{"at":3.0*k,"width":k,"kind":"door"}]
        })).collect();
        run(&mut s, json!({"type":"batch","commands":commands}));
        let after = state(&s);
        assert!(s.undo());
        assert_eq!(state(&s), before);
        assert!(s.redo());
        assert_eq!(state(&s), after);
        equivalent(&mut s, "grouped openings after redo");
    }
}
