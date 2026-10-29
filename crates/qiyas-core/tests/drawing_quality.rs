//! Goal G regression fixtures: rendered ink, physical placement, coverage and live tags.
use qiyas_core::check::CheckScope;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) -> Value {
    serde_json::to_value(s.run_input(&v, Source::User).unwrap().outcome).unwrap()
}
fn add(s: &mut Session, v: Value) -> u64 {
    run(s, v)["handles"][0].as_u64().unwrap()
}
fn blank(units: i16) -> Session {
    Session::new_blank(&std::env::temp_dir().join("goal-g.dxf"), units)
}
fn has(s: &Session, rule: &str, h: u64) -> bool {
    s.check_drawing(&CheckScope {
        rules: vec![rule.into()],
        ..Default::default()
    })
    .findings
    .iter()
    .any(|f| f.handles.contains(&h))
}
#[test]
fn tables_dimensions_grid_and_text_edges_are_checked_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let mut s = blank(units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(&mut s, json!({"type":"annotation_scale","name":"1:100"}));
        add(
            &mut s,
            json!({"type":"draw","kind":"circle","centre":[k,k],"radius":0.3*k}),
        );
        add(
            &mut s,
            json!({"type":"text","at":[0.9*k,0.9*k],"height":0.2*k,"value":"A"}),
        );
        let table = add(
            &mut s,
            json!({"type":"table","at":[0.7*k,1.3*k],"rows":2,"cols":2,"row_height":0.3*k,"col_width":0.5*k,"data":[["Door","Count"],["D1","1"]]}),
        );
        let dim = add(
            &mut s,
            json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[2.0*k,0],"line_point":[k,k]}),
        );
        assert!(has(&s, "annotation_over_grid", table), "unit {units}");
        assert!(has(&s, "annotation_over_grid", dim), "unit {units}");
        run(
            &mut s,
            json!({"type":"auto_place","targets":[{"handle":table}],"side":"right"}),
        );
        assert!(!has(&s, "annotation_over_grid", table));
        let before = s.entity_info(dim, &[]).unwrap().length;
        run(
            &mut s,
            json!({"type":"auto_place","targets":[{"handle":dim}]}),
        );
        assert!(!has(&s, "annotation_over_grid", dim));
        assert_eq!(s.entity_info(dim, &[]).unwrap().length, before);
        assert!(s.undo());
        assert!(has(&s, "annotation_over_grid", dim));
        let text = add(
            &mut s,
            json!({"type":"text","at":[5.0*k,0],"height":0.25*k,"value":"Wide label"}),
        );
        // The first tenth was outside the old 20–80% collision test.
        add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[5.02*k,-k],"b":[5.02*k,k]}),
        );
        assert!(has(&s, "text_over_lines", text));
    }
}
#[test]
fn room_labels_fit_or_wrap_and_unknown_rules_are_errors() {
    for units in [4, 5, 6, 1] {
        for language in ["en", "ar"] {
            let mut s = blank(units);
            let k = 1.0 / s.drawing.units.to_metres();
            run(&mut s, json!({"type":"annotation_scale","name":"1:100"}));
            add(
                &mut s,
                json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[2.0*k,2.0*k]}),
            );
            let r = run(
                &mut s,
                json!({"type":"label_spaces","query":{"seeds":[{"name":"غرفة","point":[1.8*k,k]}]},"height":0.25,"language":language}),
            );
            let h = r["report"]["rooms"][0]["label_handle"].as_u64().unwrap();
            assert!(!has(&s, "label_outside_room", h), "{r}");
            assert!(!has(&s, "text_over_lines", h));
            let info = s.entity_info(h, &[]).unwrap();
            assert!(info.text.unwrap().contains(if language == "ar" {
                "المساحة"
            } else {
                "Area"
            }));
            let outcome = run(&mut s, json!({"type":"room_schedule"}));
            let table = outcome["handles"][0].as_u64().unwrap();
            assert!(!has(&s, "annotation_overlap", table));
            run(
                &mut s,
                json!({"type":"modify","kind":"move","targets":[{"handle":h}],"delta":[3.0*k,0]}),
            );
            assert!(has(&s, "label_outside_room", h));
            assert!(s
                .run_input(
                    &json!({"type":"check_drawing","scope":{"rules":["outside_room"]}}),
                    Source::User
                )
                .is_err());
        }
    }
}
#[test]
fn oversized_room_labels_shrink_inside_without_losing_readability() {
    for units in [4, 5, 6, 1] {
        for language in ["en", "ar"] {
            let mut s = blank(units);
            let k = 1.0 / s.drawing.units.to_metres();
            run(&mut s, json!({"type":"annotation_scale","name":"1:100"}));
            add(
                &mut s,
                json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4.0*k,4.0*k]}),
            );
            let result = run(
                &mut s,
                json!({"type":"label_spaces","height":4.0,
                "language":language,"query":{"seeds":[{"point":[2.0*k,2.0*k]}]}}),
            );
            let room = &result["report"]["rooms"][0];
            assert!(
                matches!(
                    room["placement"].as_str(),
                    Some("shrunk" | "shrunk_wrapped")
                ),
                "{result}"
            );
            let h = room["label_handle"].as_u64().unwrap();
            let entity = s
                .drawing
                .doc
                .get_entity(opencadcodec::Handle::new(h))
                .unwrap();
            let height = match entity {
                opencadcodec::EntityType::Text(t) => t.height,
                opencadcodec::EntityType::MText(t) => t.height,
                _ => panic!("text label"),
            };
            assert!(height < 4.0 * k);
            assert!(height >= 0.25 * k - 1e-8);
            assert!(!has(&s, "label_outside_room", h));
            assert!(!has(&s, "text_over_lines", h));
        }
    }
}

#[test]
fn tiny_rooms_get_linked_outside_labels_and_the_whole_batch_undoes() {
    for units in [4, 5, 6, 1] {
        for language in ["en", "ar"] {
            let mut s = blank(units);
            let k = 1.0 / s.drawing.units.to_metres();
            run(&mut s, json!({"type":"annotation_scale","name":"1:100"}));
            add(
                &mut s,
                json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[0.4*k,0.4*k]}),
            );
            let before = s.drawing.entities_in("*Model_Space").count();
            let result = run(
                &mut s,
                json!({"type":"batch","commands":[
                    {"type":"text","at":[8.0*k,8.0*k],"value":"Note"},
                    {"type":"label_spaces","height":0.5,"language":language,
                        "query":{"seeds":[{"name":"Room","point":[0.2*k,0.2*k]}]}}
                ]}),
            );
            let room = &result["outcomes"][1]["report"]["rooms"][0];
            assert_eq!(room["placement"], "shrunk_outside", "{result}");
            let h = room["label_handle"].as_u64().unwrap();
            let handle = opencadcodec::Handle::new(h);
            let label = s.drawing.doc.get_entity(handle).unwrap();
            let leader = label
                .common()
                .reactors
                .iter()
                .find_map(|id| match s.drawing.doc.get_entity(*id) {
                    Some(opencadcodec::EntityType::Leader(l)) if l.annotation_handle == handle => {
                        Some(*id)
                    }
                    _ => None,
                })
                .expect("outside label has a linked leader");
            let height = match label {
                opencadcodec::EntityType::Text(t) => t.height,
                opencadcodec::EntityType::MText(t) => t.height,
                _ => panic!("text label"),
            };
            assert!((height - 0.25 * k).abs() < 1e-8);
            assert!(!has(&s, "label_outside_room", h));
            assert!(!has(&s, "text_over_lines", h));
            assert!(!has(&s, "text_height_vs_scale", h));
            let after = s.drawing.entities_in("*Model_Space").count();
            assert!(s.undo());
            assert_eq!(s.drawing.entities_in("*Model_Space").count(), before);
            assert!(s.drawing.doc.get_entity(handle).is_none());
            assert!(s.drawing.doc.get_entity(leader).is_none());
            assert!(s.redo());
            assert_eq!(s.drawing.entities_in("*Model_Space").count(), after);
            // The same outside label is still a live room label accepted by schedules.
            let schedule = run(
                &mut s,
                json!({"type":"room_schedule","labels":[{"handle":h}]}),
            );
            assert!(schedule["handles"].as_array().is_some());
            run(
                &mut s,
                json!({"type":"delete_entities","targets":[{"handle":leader.value()}]}),
            );
            assert!(
                has(&s, "label_outside_room", h),
                "an unlinked outside label still needs review"
            );
        }
    }
}

#[test]
fn checks_inspect_text_after_the_former_two_thousand_limit() {
    let mut s = blank(6);
    for i in 0..2002 {
        let mut t = opencadcodec::Text::with_value(
            format!("Note {i}"),
            opencadcodec::Vector3::new(i as f64 * 5.0, 0.0, 0.0),
        );
        t.height = 0.1;
        s.drawing
            .doc
            .add_entity(opencadcodec::EntityType::Text(t))
            .unwrap();
    }
    let h = add(
        &mut s,
        json!({"type":"text","at":[10005,0],"height":0.1,"value":"Collision"}),
    );
    let report = s.check_drawing(&CheckScope {
        rules: vec!["text_overlaps_text".into()],
        ..Default::default()
    });
    assert!(report.complete);
    assert!(report.findings.iter().any(|f| f.handles.contains(&h)));
}
#[test]
fn nonplotted_helpers_do_not_cross_text_and_blocks_do() {
    let mut s = blank(6);
    run(&mut s, json!({"type":"new_layer","name":"Helpers"}));
    run(
        &mut s,
        json!({"type":"set_layer","name":"Helpers","prop":"plot","value":false}),
    );
    let helper = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0.01,-1],"b":[0.01,1]}),
    );
    run(
        &mut s,
        json!({"type":"set_property","target":{"handle":helper},"prop":"layer","value":"Helpers"}),
    );
    let h = add(
        &mut s,
        json!({"type":"text","at":[0,0],"height":0.2,"value":"Room"}),
    );
    assert!(!has(&s, "text_over_lines", h));
    let l = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0.02,-1],"b":[0.02,1]}),
    );
    run(
        &mut s,
        json!({"type":"create_block","name":"Symbol","base":[0,0],"targets":[{"handle":l}],"convert_to_insert":true}),
    );
    assert!(has(&s, "text_over_lines", h));
}
#[test]
fn free_space_is_read_only_and_has_a_registry_contract() {
    let mut s = blank(6);
    let before = s.history_steps();
    let r = s
        .run_input(
            &json!({"type":"free_space","size":[2,1],"near":[0,0]}),
            Source::User,
        )
        .unwrap();
    assert!(matches!(r.outcome, Outcome::FreeSpace { .. }));
    assert_eq!(s.history_steps(), before);
    for name in ["free_space", "auto_place", "object_tags"] {
        assert!(qiyas_core::registry::registry().get(name).is_some());
    }
}

#[test]
fn opening_tags_follow_properties_position_undo_and_reopen_in_every_unit() {
    for units in [4, 5, 6, 1] {
        for extension in ["dxf", "dwg"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("tags.{extension}"));
            let mut s = Session::new_blank(&path, units);
            let k = 1.0 / s.drawing.units.to_metres();
            run(&mut s, json!({"type":"annotation_scale","name":"1:50"}));
            run(
                &mut s,
                json!({"type":"walls","runs":[{"points":[[0,0],[8.0*k,0]]}],"thickness":0.2*k,"openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"},{"run":0,"at":5.0*k,"width":1.2*k,"kind":"window"}]}),
            );
            run(&mut s, json!({"type":"opening_schedule"}));
            let group = s.wall_groups(None).unwrap().remove(0);
            let openings = &group.runs[0].openings;
            let door = openings[0].insert;
            let window = openings[1].insert;
            let tags = run(
                &mut s,
                json!({"type":"object_tags","targets":[{"handle":door},{"handle":window}],"property":"mark"}),
            );
            let h = tags["handles"][0].as_u64().unwrap();
            let old = s.entity_info(h, &[]).unwrap();
            assert_eq!(old.text.as_deref(), Some("D01"));
            // The door hinges at the wall face on the swing side.
            assert!(
                (s.entity_info(door, &[]).unwrap().position.unwrap()[1] - 0.1 * k).abs() < 1e-6 * k
            );
            run(
                &mut s,
                json!({"type":"edit_wall_opening","insert":door,"at":3.0*k,"width":k}),
            );
            let now = s.entity_info(h, &[]).unwrap();
            assert_eq!(
                old.text, now.text,
                "a size edit preserves the numbered mark"
            );
            assert_ne!(old.position, now.position);
            assert!(!has(&s, "object_tag_mismatch", h));
            s.undo();
            assert_eq!(s.entity_info(h, &[]).unwrap().text, old.text);
            s.redo();
            s.save_as_headless(&path).unwrap();
            let reopened = Session::open(&path).unwrap();
            assert_eq!(reopened.entity_info(h, &[]).unwrap().text, now.text);
            assert!(!has(&reopened, "object_tag_mismatch", h));
        }
    }
}

#[test]
fn text_inside_symbols_tables_and_hatches_is_checked() {
    let mut s = blank(6);
    let table = add(
        &mut s,
        json!({"type":"table","at":[0,4],"rows":2,"cols":1,"row_height":0.1,"col_width":0.3,"data":[["Long heading"],["Long value"]]}),
    );
    // Authored tables grow to fit their contents. Imported cells may retain a cramped row.
    assert!(!has(&s, "text_over_lines", table));
    if let Some(opencadcodec::EntityType::Table(t)) = s
        .drawing
        .doc
        .get_entity_mut(opencadcodec::Handle::new(table))
    {
        t.rows[0].height = 0.01;
        t.block_name.clear();
        t.block_record_handle = None;
    }
    assert!(has(&s, "text_over_lines", table) || has(&s, "text_overlaps_text", table));
    let outline = add(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[10,0],"b":[12,2]}),
    );
    add(
        &mut s,
        json!({"type":"draw","kind":"hatch","boundary_handles":[outline],"pattern":"solid"}),
    );
    let label = add(
        &mut s,
        json!({"type":"text","at":[10.5,0.8],"height":0.25,"value":"Inside fill"}),
    );
    assert!(has(&s, "text_over_lines", label));
}

#[test]
fn dashed_boundaries_remain_closed_in_blocks_and_arrays_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let mut s = blank(units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(&mut s, json!({"type":"new_layer","name":"Outline"}));
        run(
            &mut s,
            json!({"type":"set_layer","name":"Outline","prop":"linetype","value":"HIDDEN"}),
        );
        run(
            &mut s,
            json!({"type":"set_current","current":{"layer":"Outline"}}),
        );
        let rect = add(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4.0*k,3.0*k]}),
        );
        let block = run(
            &mut s,
            json!({"type":"create_block","name":"Dashed enclosure","base":[0,0],"targets":[{"handle":rect}],"convert_to_insert":true}),
        );
        let h = block["added"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_u64)
            .find(|h| {
                matches!(
                    s.drawing.doc.get_entity(opencadcodec::Handle::new(*h)),
                    Some(opencadcodec::EntityType::Insert(_))
                )
            })
            .unwrap();
        // Native arrays arrive in imported drawings, not through raw entity editing.
        let Some(opencadcodec::EntityType::Insert(array)) =
            s.drawing.doc.get_entity_mut(opencadcodec::Handle::new(h))
        else {
            unreachable!()
        };
        array.column_count = 2;
        array.column_spacing = 6.0 * k;
        let dir = tempfile::tempdir().unwrap();
        let path = qiyas_core::fixtures::write_drawing(
            &s.drawing.doc,
            &dir.path().join("dashed-array.dxf"),
        );
        let s = Session::open(&path).unwrap();
        for x in [2.0, 8.0] {
            let qiyas_core::Boundary::Ring { ring, .. } = s.boundary_at([x * k, k], 20.0 * k)
            else {
                panic!("Dashed array in unit {units}")
            };
            assert!((qiyas_geom::walls::ring_area(&ring).abs() / k.powi(2) - 12.0).abs() < 1e-6);
        }
    }
}

#[test]
fn opening_symbols_number_in_requested_order_and_share_schedule_marks() {
    use opencadcodec::{EntityType, Handle};
    for units in [1, 4, 5, 6] {
        let mut s = blank(units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(&mut s, json!({"type":"annotation_scale","name":"1:50"}));
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[12.0*k,0]]}],"thickness":0.2*k,
            "openings":[{"run":0,"at":8.0*k,"width":k,"kind":"door"},
                        {"run":0,"at":2.0*k,"width":k,"kind":"door"},
                        {"run":0,"at":5.0*k,"width":k,"kind":"window"}]}),
        );
        let openings = s.wall_groups(None).unwrap()[0].runs[0].openings.clone();
        let right = openings[0].insert;
        let left = openings[1].insert;
        let window = openings[2].insert;
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[right],"props":{"mark":"D07"}}),
        );
        let schedule = add(
            &mut s,
            json!({"type":"opening_schedule","at":[15.0*k,10.0*k]}),
        );
        let before = s.history_steps().0.len();
        let request = json!({"type":"object_tags","category":"opening","property":"mark","order":"left_to_right"});
        let result = run(&mut s, request.clone());
        let handles: Vec<_> = result["handles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        assert_eq!(s.history_steps().0.len(), before + 1);
        let marks: Vec<_> = handles
            .iter()
            .map(|h| s.entity_info(*h, &[]).unwrap().text.unwrap())
            .collect();
        assert_eq!(marks, ["D01", "W01", "D07"]);
        for (h, mark) in [(left, "D01"), (window, "W01"), (right, "D07")] {
            assert_eq!(
                s.object_quantity(h).unwrap().unwrap().instance.props["mark"],
                mark
            );
        }
        let Some(EntityType::Table(table)) = s.drawing.doc.get_entity(Handle::new(schedule)) else {
            panic!("schedule")
        };
        for mark in &marks {
            assert!(
                (0..table.rows.len()).any(|row| table.cell_text(row, 0) == Some(mark.as_str())),
                "missing {mark}"
            );
        }
        let parts: Vec<_> = s
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .filter(|e| {
                matches!(e, EntityType::Circle(_))
                    || matches!(e, EntityType::LwPolyline(p) if p.vertices.len() == 6)
            })
            .collect();
        assert_eq!(
            parts
                .iter()
                .filter(|e| matches!(e, EntityType::Circle(_)))
                .count(),
            2
        );
        assert_eq!(
            parts
                .iter()
                .filter(|e| matches!(e, EntityType::LwPolyline(_)))
                .count(),
            1
        );
        for h in &handles {
            assert!(!has(&s, "text_overlaps_text", *h));
            assert!(!has(&s, "text_over_lines", *h));
        }
        assert!(s.undo());
        assert!(handles
            .iter()
            .all(|h| s.drawing.doc.get_entity(Handle::new(*h)).is_none()));
        assert!(s.redo());
        assert_eq!(run(&mut s, request)["handles"], result["handles"]);
        run(
            &mut s,
            json!({"type":"object_tags","targets":[{"handle":window}],"property":"mark","shapes":{"window":"circle"}}),
        );
        assert_eq!(
            s.drawing
                .doc
                .entities_in_block("*Model_Space")
                .filter(|e| matches!(e, EntityType::Circle(_)))
                .count(),
            3
        );
    }
}

#[test]
fn tag_outlines_and_revision_annotations_never_become_room_enclosures() {
    let mut s = blank(6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[10,10]}),
    );
    run(&mut s, json!({"type":"annotation_scale","name":"1:50"}));
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"Fixture","category":"fixture","geometry":"point","properties":{"mark":{"kind":"text","default":"F01"}}}}),
    );
    let source = add(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Fixture","points":[[5,5]]}]}),
    );
    let tag = add(
        &mut s,
        json!({"type":"object_tags","targets":[{"handle":source}],"property":"mark","shapes":{"fixture":"circle"}}),
    );
    let at = s.entity_info(tag, &[]).unwrap().position.unwrap();
    run(
        &mut s,
        json!({"type":"modify","kind":"move","targets":[{"handle":tag}],"delta":[5.0-at[0],5.0-at[1]]}),
    );
    run(
        &mut s,
        json!({"type":"comment","points":[[4,4],[6,4],[6,6],[4,6]],"text":"Review / مراجعة"}),
    );
    let measured = run(&mut s, json!({"type":"measure_room","point":[5,5]}));
    assert!((measured["measurement"]["shown"].as_f64().unwrap() - 100.0).abs() < 1e-6);
}
