//! Clinic regressions: use the public wire boundary and verify the drawing after each edit.
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};
#[path = "support/measurement_fixture.rs"]
mod measurement_fixture;

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn run(s: &mut Session, input: Value) -> Outcome {
    s.run_input(&input, Source::User)
        .unwrap_or_else(|error| panic!("{input}: {error}"))
        .outcome
}

fn add(s: &mut Session, input: Value) -> u64 {
    let Outcome::Added { handles } = run(s, input) else {
        panic!("expected additions")
    };
    handles[0]
}

fn room(s: &mut Session, point: [f64; 2]) -> qiyas_core::rooms::RoomsReport {
    let Outcome::Rooms { report } = run(
        s,
        json!({"type":"rooms","query":{"seeds":[{"point":point}]}}),
    ) else {
        panic!("rooms")
    };
    *report
}

#[test]
fn u6_untitled_export_uses_the_host_folder_then_follows_save_as() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join("config/untitled");
    let documents = dir.path().join("Documents");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::create_dir_all(&documents).unwrap();
    let mut s = Session::new_blank(&hidden.join("Untitled.dwg"), 6);
    s.set_export_directory(documents.clone());
    let command = json!({"type":"deliverable","kind":"export","report":"boq","format":"pdf","path":"Quantities.pdf"});
    let Outcome::Deliverable { export } = run(&mut s, command.clone()) else {
        panic!()
    };
    assert_eq!(export.path, documents.join("Quantities.pdf"));
    assert!(!hidden.join("Quantities.pdf").exists());
    let saved = dir.path().join("Saved.dxf");
    s.save_as_headless(&saved).unwrap();
    let Outcome::Deliverable { export } = run(&mut s, command) else {
        panic!()
    };
    assert_eq!(export.path, dir.path().join("Quantities.pdf"));
}

#[test]
fn u7_new_drawing_labels_default_to_english_even_with_arabic_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled.dwg"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    let Outcome::Rooms { report } = run(
        &mut s,
        json!({"type":"label_spaces","query":{"seeds":[{"name":"غرفة","point":[2,2]}]}}),
    ) else {
        panic!()
    };
    let text = s
        .entity_info(report.rooms[0].label_handle.unwrap(), &[])
        .unwrap()
        .text
        .unwrap();
    assert!(text.starts_with("Area "), "{text}");
    let table = add(&mut s, json!({"type":"room_schedule","at":[10,10]}));
    let cells = s.entity_info(table, &[]).unwrap().table.unwrap().cells;
    assert_eq!(cells[0], ["Room", "Area (m²)"]);
}

#[test]
fn u7_supplied_arabic_room_names_can_match_or_be_explicitly_english() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("supplied.dxf");
    let mut s = Session::new_blank(&path, 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    run(&mut s, json!({"type":"new_layer","name":"Names"}));
    run(
        &mut s,
        json!({"type":"set_current","current":{"layer":"Names"}}),
    );
    add(
        &mut s,
        json!({"type":"text","at":[2,2],"height":0.2,"value":"غرفة"}),
    );
    s.save_as_headless(&path).unwrap();
    let mut s = Session::open(&path).unwrap();
    for language in [None, Some("en")] {
        let mut command = json!({"type":"label_spaces","query":{"text_layer":"Names"}});
        if let Some(language) = language {
            command["language"] = json!(language);
        }
        let Outcome::Rooms { report } = run(&mut s, command) else {
            panic!()
        };
        let text = s
            .entity_info(report.rooms[0].label_handle.unwrap(), &[])
            .unwrap()
            .text
            .unwrap();
        assert!(
            text.starts_with(if language.is_some() {
                "Area "
            } else {
                "المساحة "
            }),
            "{text}"
        );
        assert!(s.undo());
    }
}

#[test]
fn u8_correct_arabic_english_and_mixed_area_labels_do_not_mismatch() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("room.dxf"), unit);
        let u = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5.0*u,2.5*u]}),
        );
        run(
            &mut s,
            json!({"type":"label_spaces","language":"ar","query":{"seeds":[{"point":[u,u]}]}}),
        );
        for (i, value) in [
            "Area 12.50 m²",
            "المساحة ١٢٫٥٠ م²",
            "Area ۱۲٫۵۰ m²",
            "المساحة 12.50 م²",
            "Area ١2٫5٠ m²",
        ]
        .iter()
        .enumerate()
        {
            add(
                &mut s,
                json!({"type":"text","value":value,"at":[u,(1.5+i as f64*0.1)*u],"height":0.05*u}),
            );
        }
        let Outcome::Checked { report } = run(
            &mut s,
            json!({"type":"check_drawing","scope":{"rules":["room_area_mismatch"]}}),
        ) else {
            panic!()
        };
        assert!(report.findings.is_empty(), "{unit}: {:?}", report.findings);
        let bad = add(
            &mut s,
            json!({"type":"text","value":"المساحة ١٣٫٥٠ م²","at":[2.0*u,u],"height":0.05*u}),
        );
        let Outcome::Checked { report } = run(
            &mut s,
            json!({"type":"check_drawing","scope":{"rules":["room_area_mismatch"]}}),
        ) else {
            panic!()
        };
        assert!(
            report.findings.iter().any(|f| f.handles.contains(&bad)),
            "incorrect Arabic area must still be reported"
        );
    }
}

#[test]
fn u9_fixture_blocks_do_not_deduct_floor_area_but_explicit_islands_do() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("room.dxf"), unit);
        let u = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5.0*u,4.0*u]}),
        );
        near(room(&mut s, [u, u]).rooms[0].net_area_m2, 20.0);
        run(
            &mut s,
            json!({"type":"define_block","name":"Fixture","base":[0,0],"entities":[{"type":"circle","data":{"center":{"x":0,"y":0,"z":0},"radius":0.3*u}}]}),
        );
        run(
            &mut s,
            json!({"type":"insert_block","name":"Fixture","at":[3.0*u,2.0*u]}),
        );
        let report = room(&mut s, [u, u]);
        near(report.rooms[0].net_area_m2, 20.0);
        assert!(report.rooms[0].islands.is_empty());
        run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[2.0*u,2.0*u],"b":[2.5*u,3.0*u]}),
        );
        let report = room(&mut s, [u, u]);
        near(report.rooms[0].net_area_m2, 19.5);
        assert_eq!(report.rooms[0].islands.len(), 1);
        near(report.rooms[0].islands[0].area_m2, 0.5);
        assert!(s.undo());
        near(room(&mut s, [u, u]).rooms[0].net_area_m2, 20.0);
    }
}

#[test]
fn u9_imported_block_enclosures_work_and_voids_are_an_explicit_type_property() {
    for nested in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("block-plan.dxf"), 6);
        run(
            &mut s,
            json!({"type":"define_block","name":"Plan","base":[0,0],"entities":[
                {"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":5,"y":0,"z":0}}},
                {"type":"Line","data":{"start":{"x":5,"y":0,"z":0},"end":{"x":5,"y":4,"z":0}}},
                {"type":"Line","data":{"start":{"x":5,"y":4,"z":0},"end":{"x":0,"y":4,"z":0}}},
                {"type":"Line","data":{"start":{"x":0,"y":4,"z":0},"end":{"x":0,"y":0,"z":0}}}
            ]}),
        );
        run(
            &mut s,
            json!({"type":"insert_block","name":"Plan","at":[0,0]}),
        );
        run(
            &mut s,
            json!({"type":"define_block","name":"Symbol","base":[0,0],"entities":[{"type":"Circle","data":{"radius":0.3}}]}),
        );
        let symbol = add(
            &mut s,
            json!({"type":"insert_block","name":"Symbol","at":[3,2]}),
        );
        let symbol = if nested {
            let Outcome::Edited { added, .. } = run(
                &mut s,
                json!({"type":"create_block","name":"Nested","base":[0,0],"targets":[{"handle":symbol}],"convert_to_insert":true}),
            ) else {
                panic!()
            };
            added[0]
        } else {
            symbol
        };
        assert_eq!(room(&mut s, [1.0, 1.0]).rooms[0].net_area_m2, 20.0);
        run(
            &mut s,
            json!({"type":"define_object_type","definition":{"name":"Opening in floor","category":"user-defined","geometry":"point","room_void":true}}),
        );
        run(
            &mut s,
            json!({"type":"place_objects","objects":[{"type":"Opening in floor","driver":symbol}]}),
        );
        let result = room(&mut s, [1.0, 1.0]);
        assert_eq!(result.rooms[0].islands.len(), 1);
        // Room reports round areas to six decimal places.
        let expected = ((20.0 - std::f64::consts::PI * 0.3f64.powi(2)) * 1e6).round() / 1e6;
        near(result.rooms[0].net_area_m2, expected);
        assert!(s.undo());
        assert_eq!(room(&mut s, [1.0, 1.0]).rooms[0].net_area_m2, 20.0);
    }
}

#[test]
fn u11_discrepancy_check_has_a_clear_name_and_legacy_alias() {
    assert!(qiyas_core::registry::registry()
        .get("discrepancy_check")
        .is_some());
    let old: qiyas_core::Command = serde_json::from_value(json!({"type":"check"})).unwrap();
    assert_eq!(
        serde_json::to_value(old).unwrap()["type"],
        "discrepancy_check"
    );
}

#[test]
fn u12_reusing_a_named_layer_keeps_properties_and_undo_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("layers.dxf"), 6);
    run(&mut s, json!({"type":"new_layer","name":"Notes"}));
    run(
        &mut s,
        json!({"type":"set_layer","name":"Notes","prop":"color","value":"ACI 3"}),
    );
    let history = s.history_steps().0.len();
    let reply = s
        .run_input(&json!({"type":"new_layer","name":"notes"}), Source::Ai)
        .unwrap();
    assert_eq!(s.history_steps().0.len(), history);
    assert_eq!(
        s.drawing.doc.layers.get("Notes").unwrap().color,
        opencadcodec::Color::Index(3)
    );
    assert!(serde_json::to_string(&reply)
        .unwrap()
        .contains("already exists"));
}

#[test]
fn u12_named_creates_reuse_matching_data_and_refuse_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("names.dxf"), 6);
    for (same, conflict) in [
        (
            json!({"type":"new_layout","name":"Issued","paper":"ISO A3"}),
            json!({"type":"new_layout","name":"Issued","paper":"ISO A4"}),
        ),
        (
            json!({"type":"define_object_type","definition":{"name":"Surveyed","category":"review","geometry":"point"}}),
            json!({"type":"define_object_type","definition":{"name":"Surveyed","category":"changed","geometry":"point"}}),
        ),
        (
            json!({"type":"define_block","name":"Symbol","base":[0,0],"entities":[{"type":"Circle","data":{"radius":1}}]}),
            json!({"type":"define_block","name":"Symbol","base":[0,0],"entities":[{"type":"Circle","data":{"radius":2}}]}),
        ),
    ] {
        run(&mut s, same.clone());
        let history = s.history_steps();
        let before = serde_json::to_value(
            s.records(&serde_json::from_value(json!({"collection":"entities"})).unwrap())
                .unwrap(),
        )
        .unwrap();
        let reply = s.run_input(&same, Source::User).unwrap();
        assert!(reply.input_notes().contains("already exists"));
        assert_eq!(s.history_steps(), history);
        assert!(s.run_input(&conflict, Source::User).is_err());
        assert_eq!(s.history_steps(), history);
        assert_eq!(
            serde_json::to_value(
                s.records(&serde_json::from_value(json!({"collection":"entities"})).unwrap())
                    .unwrap()
            )
            .unwrap(),
            before
        );
    }
}

#[test]
fn u15_edit_an_explicit_sheet_without_switching_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("sheets.dxf"), 6);
    let model = add(
        &mut s,
        json!({"type":"text","at":[0,0],"value":"Model text"}),
    );
    run(
        &mut s,
        json!({"type":"new_layout","name":"Sheet A","paper":"ISO A3"}),
    );
    let paper_sheet = s
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.name == "Sheet A")
        .unwrap()
        .block;
    run(&mut s, json!({"type":"set_sheet","block":paper_sheet}));
    let sheet = s.sheet.clone();
    let paper = add(
        &mut s,
        json!({"type":"text","at":[10,10],"value":"Sheet text"}),
    );
    let err = s
        .run_input(
            &json!({"type":"edit_text","target":{"handle":model},"value":"Changed"}),
            Source::Ai,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("Model") && err.contains("sheet"), "{err}");
    run(
        &mut s,
        json!({"type":"edit_text","target":{"handle":model},"value":"Changed model","sheet":"Model"}),
    );
    assert_eq!(s.sheet, sheet);
    assert_eq!(
        s.entity_info(model, &[]).unwrap().text.as_deref(),
        Some("Changed model")
    );
    run(&mut s, json!({"type":"set_sheet","block":"*Model_Space"}));
    run(
        &mut s,
        json!({"type":"edit_text","target":{"handle":paper},"value":"Changed sheet","sheet":"Sheet A"}),
    );
    assert_eq!(s.sheet, "*Model_Space");
    assert_eq!(
        s.entity_info(paper, &[]).unwrap().text.as_deref(),
        Some("Changed sheet")
    );
    assert!(s.undo());
    assert_eq!(
        s.entity_info(paper, &[]).unwrap().text.as_deref(),
        Some("Sheet text")
    );
}

#[test]
fn u14_radial_dimension_text_follows_the_picked_point_outside_the_circle() {
    for unit in [4, 5, 6, 1] {
        for kind in ["radius", "diameter"] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Session::new_blank(&dir.path().join("dimensions.dxf"), unit);
            let u = 1.0 / s.drawing.units.to_metres();
            let circle = add(
                &mut s,
                json!({"type":"draw","kind":"circle","centre":[0,0],"radius":u}),
            );
            let dimension = add(
                &mut s,
                json!({"type":"dimension","kind":kind,"target":{"handle":circle},"point":[3.0*u,4.0*u]}),
            );
            let opencadcodec::EntityType::Dimension(d) = s
                .drawing
                .doc
                .get_entity(opencadcodec::Handle::new(dimension))
                .unwrap()
            else {
                panic!()
            };
            let at = d.base().text_middle_point;
            assert!(
                (at.x / u - 3.0).abs() < 1e-8 && (at.y / u - 4.0).abs() < 1e-8,
                "{unit} {kind}: {at:?}"
            );
            let geometry = qiyas_geom::dimension_entities(&s.drawing.doc, d);
            assert!(
                geometry.entities.iter().any(|part| match part {
                    opencadcodec::EntityType::Line(line) => line.end.x.hypot(line.end.y) > 2.0 * u,
                    _ => false,
                }),
                "leader must reach towards the outside text"
            );
            assert!(s.undo());
            assert!(s
                .drawing
                .doc
                .get_entity(opencadcodec::Handle::new(dimension))
                .is_none());
        }
    }
}

#[test]
fn u3_u4_client_references_are_plain_and_citations_are_unique() {
    use qiyas_core::deliverables::{self, Kind, Language, Report};
    let mut report = Report {
        kind: Kind::Variation,
        language: Language::En,
        layout: deliverables::default_layout(Kind::Variation),
        warnings: vec![],
        rows: vec![
            json!({"drawing":"/internal/drawing.dxf","code":"A","description":"Wall","unit":"m2","a_quantity":1,"b_quantity":2,"delta":1,"elements":[{"sheet":"*Model_Space","kind":"added","b_handles":[987654321]}]}),
        ],
    };
    let visible = serde_json::to_string(
        &report
            .table()
            .unwrap()
            .iter()
            .map(|r| &r.cells)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(
        !visible.contains("987654321") && !visible.contains("*Model_Space"),
        "{visible}"
    );
    report.kind = Kind::Boq;
    report.layout = deliverables::default_layout(Kind::Boq);
    report.rows = vec![
        json!({"code":"A","section":"Walls","description":"Wall","unit":"m2","quantity":2,"method":"POMI","citations":[{"source":"POMI (RICS)","clause":"Section D"},{"source":"POMI (RICS)","clause":"Section D"}]}),
    ];
    let table = report.table().unwrap();
    let row = table.iter().find(|r| !r.heading).unwrap();
    assert_eq!(row.cells.last().unwrap(), "POMI (RICS) / Section D");
    assert!(row.cells.contains(&json!("m²")), "{row:?}");
}

#[test]
fn u5_narrow_room_names_wrap_inside_and_area_labels_avoid_ink() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("labels.dxf"), unit);
        let u = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[1.6*u,5.0*u]}),
        );
        let name = add(
            &mut s,
            json!({"type":"text","at":[0.25*u,3.5*u],"height":0.25*u,"value":"Consulting room 4"}),
        );
        let Outcome::Checked { report } = run(
            &mut s,
            json!({"type":"check_drawing","scope":{"rules":["text_over_lines"]}}),
        ) else {
            panic!()
        };
        assert!(report.findings.iter().any(|f| f.handles.contains(&name)));
        add(
            &mut s,
            json!({"type":"draw","kind":"arc","centre":[0,0],"radius":1.2*u,"start":0,"end":90}),
        );
        run(
            &mut s,
            json!({"type":"auto_place","targets":[{"handle":name}]}),
        );
        let at = s.entity_info(name, &[]).unwrap().position.unwrap();
        assert!(
            at[0] > 0.0 && at[0] < 1.6 * u && at[1] > 0.0 && at[1] < 5.0 * u,
            "name escaped the room: {at:?}"
        );
        assert!(s
            .entity_info(name, &[])
            .unwrap()
            .text
            .unwrap()
            .contains("Consulting"));
        let Outcome::Rooms { report } = run(
            &mut s,
            json!({"type":"label_spaces","height":0.25,"query":{"seeds":[{"point":[0.8*u,3.0*u]}]}}),
        ) else {
            panic!()
        };
        let label = report.rooms[0].label_handle.unwrap();
        let Outcome::Checked { report } = run(
            &mut s,
            json!({"type":"check_drawing","scope":{"rules":["text_over_lines","text_overlaps_text"]}}),
        ) else {
            panic!()
        };
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.handles.contains(&name) || f.handles.contains(&label)),
            "{unit}: {:?}",
            report.findings
        );
    }
}

#[test]
fn u10_view_only_regions_exclude_measurements_and_survive_reopen_and_undo() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("detail.dxf");
        let mut s = Session::new_blank(&path, unit);
        let u = 1.0 / s.drawing.units.to_metres();
        let region = add(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5.0*u,4.0*u]}),
        );
        let line = add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[u,u],"b":[3.0*u,u]}),
        );
        let measure = json!({"type":"measure_entity","handle":line,"chain":[]});
        run(&mut s, measure.clone());
        near(s.takeoff.items[0].shown, 2.0);
        run(
            &mut s,
            json!({"type":"set_measurement_scope","region":{"handle":region},"measured":false}),
        );
        assert!(!s.takeoff.items[0].available());
        assert!(s
            .run_input(
                &json!({"type":"measure_distance","points":[[u,u],[3.0*u,u]]}),
                Source::User
            )
            .is_err());
        assert!(s
            .run_input(
                &json!({"type":"measure_room","point":[2.0*u,2.0*u]}),
                Source::User
            )
            .is_err());
        assert!(s.run_input(&measure, Source::User).is_err());
        assert!(s.undo());
        assert!(s.takeoff.items[0].available());
        assert!(s.redo());
        assert!(!s.takeoff.items[0].available());
        s.save_as_headless(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        assert!(reopened.run_input(&measure, Source::User).is_err());
        run(
            &mut reopened,
            json!({"type":"set_measurement_scope","region":{"handle":region},"measured":true}),
        );
        run(&mut reopened, measure);
        assert!(reopened.takeoff.items.iter().all(|m| m.available()));
    }
}

#[test]
fn u10_scaled_detail_is_advice_until_explicitly_marked_and_groups_follow_members() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("details.dxf"), unit);
        let u = 1.0 / s.drawing.units.to_metres();
        let mut copied = Vec::new();
        for (offset, scale) in [(0.0, 1.0), (10.0, 3.0)] {
            let mut handles = Vec::new();
            handles.push(add(&mut s, json!({"type":"draw","kind":"rectangle","a":[offset*u,0],"b":[(offset+2.0*scale)*u,scale*u]})));
            for x in [0.5, 1.5] {
                handles.push(add(&mut s, json!({"type":"draw","kind":"line","a":[(offset+x*scale)*u,0],"b":[(offset+x*scale)*u,scale*u]})));
            }
            if offset > 0.0 {
                copied = handles;
            }
        }
        let before = s.takeoff_check(0, &[]).unwrap();
        assert!(
            before["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["section"] == "possible_detail"),
            "{before}"
        );
        run(
            &mut s,
            json!({"type":"create_group","name":"Detail","targets":copied.iter().map(|h|json!({"handle":h})).collect::<Vec<_>>()}),
        );
        run(
            &mut s,
            json!({"type":"set_measurement_scope","group":"Detail","measured":false}),
        );
        assert!(s
            .run_input(
                &json!({"type":"measure_entity","handle":copied[1]}),
                Source::User
            )
            .is_err());
        let after = s.takeoff_check(0, &[]).unwrap();
        assert_eq!(after["possible_details"], 0, "{after}");
        assert!(s.undo());
        run(&mut s, json!({"type":"measure_entity","handle":copied[1]}));
        assert!((s.takeoff.items.last().unwrap().shown - 3.0).abs() < 1e-8);
    }
}

#[test]
fn u10_view_only_groups_exclude_generated_quantities_and_follow_member_edits() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("quantities.dxf"), unit);
        let (host, children) = measurement_fixture::fixture(&mut s);
        measurement_fixture::apply(&mut s, measurement_fixture::method("test", false));
        let before = s.measurement_report().unwrap();
        assert!((before.items[0].quantity - 32.68).abs() < 1e-8);
        run(
            &mut s,
            json!({"type":"create_group","name":"Display","targets":[{"handle":host}]}),
        );
        run(
            &mut s,
            json!({"type":"set_measurement_scope","group":"Display","measured":false}),
        );
        assert!(s.measurement_report().unwrap().items.is_empty());
        // Hosted geometry follows its host's exclusion even outside the region's outline.
        assert!(s
            .run_input(
                &json!({"type":"measure_entity","handle":children[0]}),
                Source::User
            )
            .is_err());
        assert!(s.undo());
        assert_eq!(s.measurement_report().unwrap(), before);
        assert!(s.redo());
        let extra = add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[100,100],"b":[101,100]}),
        );
        run(
            &mut s,
            json!({"type":"add_to_group","name":"Display","targets":[{"handle":extra}]}),
        );
        assert!(s
            .run_input(
                &json!({"type":"measure_entity","handle":extra}),
                Source::User
            )
            .is_err());
        assert!(s.undo());
        run(&mut s, json!({"type":"measure_entity","handle":extra}));
        assert!(s.takeoff.items.last().unwrap().available());
    }
}

#[test]
fn u12_matching_group_members_are_a_noop_but_different_members_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("groups.dxf"), 6);
    let a = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
    );
    let b = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,1],"b":[1,1]}),
    );
    run(
        &mut s,
        json!({"type":"create_group","name":"Detail","targets":[{"handle":a},{"handle":b}]}),
    );
    let history = s.history_steps();
    let reply = s
        .run_input(
            &json!({"type":"create_group","name":"detail","targets":[{"handle":b},{"handle":a}]}),
            Source::User,
        )
        .unwrap();
    assert!(reply.input_notes().contains("already exists"));
    assert_eq!(s.history_steps(), history);
    assert!(s
        .run_input(
            &json!({"type":"create_group","name":"Detail","targets":[{"handle":a}]}),
            Source::User
        )
        .is_err());
    assert_eq!(s.history_steps(), history);
}
