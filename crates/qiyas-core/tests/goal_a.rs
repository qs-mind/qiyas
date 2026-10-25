//! Analytic enclosure controls: physical wall faces, explicit source layers and persisted validity.
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) -> Outcome {
    s.run_input(&v, Source::User).unwrap().outcome
}
fn area(s: &mut Session, point: [f64; 2]) -> f64 {
    let Outcome::Rooms { report } = run(
        s,
        json!({"type":"rooms","query":{"seeds":[{"point":point}]}}),
    ) else {
        panic!()
    };
    report.rooms[0].net_area_m2
}
#[test]
fn wall_faces_ignore_doors_windows_grid_and_annotation_in_all_units() {
    for unit in [4, 5, 6, 1] {
        for ext in ["dwg", "dxf"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("room.{ext}"));
            let mut s = Session::new_blank(&path, unit);
            let u = 1.0 / s.drawing.units.to_metres();
            run(
                &mut s,
                json!({"type":"walls","thickness":0.2*u,"runs":[{"points":[[0,0],[5.2*u,0],[5.2*u,4.2*u],[0,4.2*u]],"closed":true}],"openings":[{"run":0,"at":2.0*u,"width":0.9*u,"kind":"door"},{"run":0,"at":7.2*u,"width":1.2*u,"kind":"window"}]}),
            );
            let p = [u, u];
            assert_eq!(area(&mut s, p), 20.0);
            run(
                &mut s,
                json!({"type":"grid","x":[{"label":"A","at":2.6*u}],"extent":{"min":[-u,-u],"max":[6.2*u,5.2*u]}}),
            );
            let Outcome::Added { handles } = run(
                &mut s,
                json!({"type":"text","at":[2.0*u,2.0*u],"height":0.1,"value":"D1"}),
            ) else {
                panic!()
            };
            let tag = handles[0];
            run(
                &mut s,
                json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[5.2*u,0],"line_point":[2.0*u,2.0*u]}),
            );
            assert_eq!(area(&mut s, p), 20.0);
            let Outcome::Rooms { report } = run(
                &mut s,
                json!({"type":"label_spaces","query":{"seeds":[{"point":p,"name":"Room"}]},"language":"ar"}),
            ) else {
                panic!()
            };
            let label = report.rooms[0].label_handle.unwrap();
            assert_eq!(
                // Wrapping keeps the value and unit together with a non-breaking space.
                s.entity_info(label, &[])
                    .unwrap()
                    .text
                    .unwrap()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                "المساحة ٢٠٫٠٠ م²"
            );
            run(
                &mut s,
                json!({"type":"measure_room","label":{"handle":label}}),
            );
            run(
                &mut s,
                json!({"type":"set_property","target":{"handle":tag},"prop":"text_height","value":0.8*u}),
            );
            assert!((s.takeoff.items[0].shown - 20.0).abs() < 1e-8);
            assert_eq!(area(&mut s, p), 20.0);
            let door = s.wall_groups(None).unwrap()[0].runs[0].openings[0].insert;
            run(
                &mut s,
                json!({"type":"edit_wall_opening","insert":door,"at":2.2*u}),
            );
            assert!((s.takeoff.items[0].shown - 20.0).abs() < 1e-8);
            assert!(s.undo());
            assert!((s.takeoff.items[0].shown - 20.0).abs() < 1e-8);
            s.save_as_headless(&path).unwrap();
            let mut reopened = Session::open(&path).unwrap();
            assert_eq!(area(&mut reopened, p), 20.0);
            assert!((reopened.takeoff.items[0].shown - 20.0).abs() < 1e-8);
        }
    }
}
#[test]
fn l_shape_and_layer_policy_do_not_use_grid_or_text_as_boundaries() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("l.dxf"), unit);
        let u = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"draw","kind":"polyline","points":[[0,0],[6.0*u,0],[6.0*u,2.0*u],[4.0*u,2.0*u],[4.0*u,4.0*u],[0,4.0*u]],"closed":true}),
        );
        run(
            &mut s,
            json!({"type":"grid","y":[{"label":"A","at":u}],"extent":{"min":[-u,-u],"max":[7.0*u,5.0*u]}}),
        );
        assert_eq!(area(&mut s, [u, 0.5 * u]), 20.0);
        // The user's hatch still sees the crossing grid.
        let qiyas_core::Boundary::Ring { ring, .. } = s.boundary_at([u, 0.5 * u], u) else {
            panic!()
        };
        assert!(qiyas_geom::walls::ring_area(&ring) * s.drawing.units.to_metres().powi(2) < 20.0);
    }
}
#[test]
fn explicit_exclusions_survive_retrace_save_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policy.dwg");
    let mut s = Session::new_blank(&path, 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    run(&mut s, json!({"type":"new_layer","name":"Reference"}));
    run(
        &mut s,
        json!({"type":"set_current","current":{"layer":"Reference"}}),
    );
    run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,2],"b":[5,2]}),
    );
    let Outcome::Rooms { report } = run(
        &mut s,
        json!({"type":"label_spaces","query":{"seeds":[{"point":[1,1]}],"exclude_layers":["Reference"]}}),
    ) else {
        panic!()
    };
    assert_eq!(report.rooms[0].net_area_m2, 20.0);
    run(
        &mut s,
        json!({"type":"measure_room","label":{"handle":report.rooms[0].label_handle.unwrap()}}),
    );
    s.save_as_headless(&path).unwrap();
    let reopened = Session::open(&path).unwrap();
    assert_eq!(reopened.takeoff.items[0].shown, 20.0);
}

#[test]
fn room_reader_skips_old_labels_and_grid_letters_even_inside_an_enclosure() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("names.dxf"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    run(
        &mut s,
        json!({"type":"grid","x":[{"label":"A","at":2.5}],"extent":{"min":[1,1],"max":[4,3]},"bubble":0.1}),
    );
    for (i, text) in [
        "Room",
        "Unapproved candidate 20.00 m²",
        "مساحة مرشحة غير معتمدة 20.00 م²",
    ]
    .iter()
    .enumerate()
    {
        run(
            &mut s,
            json!({"type":"text","at":[0.5,1.0+i as f64*0.5],"height":0.1,"value":text}),
        );
    }
    let Outcome::Rooms { report } = run(&mut s, json!({"type":"rooms","query":{}})) else {
        panic!()
    };
    assert_eq!(report.rooms.len(), 1);
    assert_eq!(report.rooms[0].name.as_deref(), Some("Room"));
    assert_eq!(report.rooms[0].net_area_m2, 20.0);
    assert!(report.rooms[0].also_named.is_empty());
}

#[test]
fn older_grids_recover_axes_and_letters_from_bubble_provenance_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old-grid.dxf");
    let mut s = Session::new_blank(&path, 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    let Outcome::Added { handles } = run(
        &mut s,
        json!({"type":"grid","y":[{"label":"A","at":2}],"extent":{"min":[-1,-1],"max":[6,5]}}),
    ) else {
        panic!()
    };
    for handle in handles {
        let entity = s
            .drawing
            .doc
            .get_entity_mut(opencadcodec::Handle::new(handle))
            .unwrap();
        if !matches!(entity, opencadcodec::EntityType::Circle(_)) {
            entity
                .common_mut()
                .extended_data
                .remove_record("QIYAS_GRID");
        }
    }
    s.save_as_headless(&path).unwrap();
    let mut reopened = Session::open(&path).unwrap();
    assert_eq!(area(&mut reopened, [1.0, 1.0]), 20.0);
}

mod mizan_tools {
    //! Survey placement, independent revisions and optional fixture data. No model calls.
    use qiyas_core::{Outcome, Session, Source, Target};
    use serde_json::{json, Value};

    fn run(s: &mut Session, input: Value) -> Outcome {
        s.run_input(&input, Source::Mcp).unwrap().outcome
    }
    fn added(s: &mut Session, input: Value) -> u64 {
        match run(s, input) {
            Outcome::Added { handles } => handles[0],
            other => panic!("{other:?}"),
        }
    }
    fn target(handle: u64) -> Target {
        Target {
            handle,
            chain: vec![],
        }
    }

    #[test]
    fn exact_spatial_reads_work_in_all_units_and_at_survey_coordinates() {
        for units in [4, 5, 6, 1] {
            for site in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let mut s = Session::new_blank(&dir.path().join("spatial.dxf"), units);
                let k = 1. / s.drawing.units.to_metres();
                let [x, y] = if site {
                    [674250. * k, 2734180. * k]
                } else {
                    [0., 0.]
                };
                let circle = added(
                    &mut s,
                    json!({"type":"draw","kind":"circle","centre":[x,y],"radius":k}),
                );
                let line = added(
                    &mut s,
                    json!({"type":"draw","kind":"line","a":[x-2.*k,y],"b":[x+2.*k,y]}),
                );
                let before = s.revision();
                let query = |kind: &str, extra: Value| {
                    let mut input = extra;
                    input["kind"] = json!(kind);
                    serde_json::from_value::<qiyas_core::spatial::Query>(input).unwrap()
                };
                let contains = s
                    .spatial_query(&query(
                        "contains",
                        json!({"target":{"handle":circle},"point":[x,y]}),
                    ))
                    .unwrap();
                assert_eq!(contains["result"]["contains"], true);
                let Outcome::SpatialQuery { report } = run(
                    &mut s,
                    json!({"type":"spatial_query","kind":"contains","target":circle,"point":[x,y]}),
                ) else {
                    panic!("spatial read outcome")
                };
                assert_eq!(report["result"]["contains"], true);
                assert_eq!(
                    s.spatial_query(&query(
                        "contains",
                        json!({"target":{"handle":circle},"point":[x+2.*k,y]})
                    ))
                    .unwrap()["result"]["contains"],
                    false
                );
                let nearest = s
                    .spatial_query(&query(
                        "nearest",
                        json!({"targets":[{"handle":circle}],"point":[x+2.*k,y]}),
                    ))
                    .unwrap();
                assert!((nearest["result"]["distance_m"].as_f64().unwrap() - 1.).abs() < 1e-6);
                let intersections = s
                    .spatial_query(&query(
                        "intersections",
                        json!({"a":{"handle":circle},"b":{"handle":line}}),
                    ))
                    .unwrap();
                assert_eq!(intersections["result"]["count"], 2);
                assert!(s
                    .spatial_query(&query(
                        "contains",
                        json!({"target":{"handle":line},"point":[x,y]})
                    ))
                    .is_err());
                assert_eq!(s.revision(), before);
            }
        }
    }

    #[test]
    fn revision_parts_are_independent_and_partial_stale_reports_identify_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("parts.dxf"), 4);
        let first = s.revision();
        let h = added(
            &mut s,
            json!({"type":"draw","kind":"line","a":[0,0],"b":[1000,0]}),
        );
        assert!(s.revision().geometry_revision > first.geometry_revision);
        assert_eq!(s.revision().knowledge_revision, first.knowledge_revision);
        s.check_revision(&json!({"knowledge_revision":first.knowledge_revision,"selection_revision":first.selection_revision,"sheet_revision":first.sheet_revision})).unwrap();
        let before = s.revision();
        run(
            &mut s,
            json!({"type":"set_knowledge","kind":"levels","name":"Example","data":{"elevation":1.2}}),
        );
        assert!(s.revision().knowledge_revision > before.knowledge_revision);
        assert_eq!(s.revision().geometry_revision, before.geometry_revision);
        s.check_revision(&json!({"geometry_revision":before.geometry_revision}))
            .unwrap();
        s.set_read_selection(&[target(h)]);
        assert!(s
            .check_revision(&json!({"selection_revision":before.selection_revision}))
            .is_err());
        s.check_revision(&json!({"geometry_revision":before.geometry_revision,"sheet_revision":before.sheet_revision})).unwrap();
        let before = s.revision();
        run(&mut s, json!({"type":"annotation_scale","name":"1:50"}));
        assert_ne!(s.revision().sheet_revision, before.sheet_revision);
        assert_eq!(s.revision().geometry_revision, before.geometry_revision);
        s.check_revision(&json!({"knowledge_revision":before.knowledge_revision,"selection_revision":before.selection_revision})).unwrap();
        let error = s
            .check_revision(&json!({"geometry_revision":first.geometry_revision}))
            .unwrap_err();
        let qiyas_core::CoreError::StaleRead { details } = error else {
            panic!("wrong refusal")
        };
        assert_eq!(details["changed_components"], json!(["geometry"]));
        assert!(details["added"].as_array().unwrap().contains(&json!(h)));
        s.check_revision(&details["revision"]).unwrap();
        assert!(s.check_revision(&json!({"geomtry_revision":0})).is_err());
        let before = s.revision();
        assert!(s.run_input(&json!({"type":"batch","commands":[{"type":"draw","kind":"line","a":[1,1],"b":[2,2]},{"type":"draw","kind":"circle","centre":[0,0],"radius":-1}]}),Source::Mcp).is_err());
        assert_eq!(
            s.revision(),
            before,
            "a failed atomic batch must not advance a revision part"
        );
    }

    #[test]
    fn site_geometry_basepoint_and_ucs_roundtrip_and_undo_in_all_units() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Session::new_blank(&dir.path().join("site.dxf"), units);
            let k = 1. / s.drawing.units.to_metres();
            let h = added(
                &mut s,
                json!({"type":"draw","kind":"line","a":[0,0],"b":[4.*k,0]}),
            );
            let site = [674250. * k, 2734180. * k];
            let old = s.ucs;
            let revision = s.revision();
            assert!(s
                .run_input(
                    &json!({"type":"batch","commands":[
                        {"type":"set_site_origin","base_point":[0,0],"site_point":site},
                        {"type":"draw","kind":"circle","centre":[0,0],"radius":-1}
                    ]}),
                    Source::Mcp
                )
                .is_err());
            assert_eq!(s.revision(), revision);
            assert_eq!(s.ucs, old);
            assert_eq!(
                s.site_coordinates(&[target(h)]).unwrap()["rows"][0]["site"][0],
                json!([0.0, 0.0])
            );
            let steps = s.history_steps().0.len();
            run(
                &mut s,
                json!({"type":"set_site_origin","base_point":[0,0],"site_point":site,"x_axis":[0,1]}),
            );
            assert_eq!(s.history_steps().0.len(), steps + 1);
            let coordinates = s.site_coordinates(&[target(h)]).unwrap();
            let point: [f64; 2] =
                serde_json::from_value(coordinates["rows"][0]["site"][0].clone()).unwrap();
            for i in 0..2 {
                assert!((point[i] - site[i]).abs() < 1e-6 * k);
            }
            let end: [f64; 2] =
                serde_json::from_value(coordinates["rows"][0]["site"][1].clone()).unwrap();
            assert!((end[1] - site[1] - 4. * k).abs() < 1e-6 * k);
            assert_eq!(coordinates["base_point"], json!(site));
            assert!(s.undo());
            assert_eq!(s.ucs, old);
            assert_eq!(
                s.site_coordinates(&[target(h)]).unwrap()["rows"][0]["site"][0],
                json!([0.0, 0.0])
            );
            assert!(s.redo());
            assert_eq!(s.ucs.origin, site);
            for ext in ["dxf", "dwg"] {
                let path = dir.path().join(format!("saved.{ext}"));
                s.save_as_headless(&path).unwrap();
                let reopened = Session::open(&path).unwrap();
                let read = reopened.site_coordinates(&[target(h)]).unwrap();
                assert_eq!(read["base_point"], json!(site));
                assert_eq!(read["rows"][0]["site"], coordinates["rows"][0]["site"]);
            }
        }
    }

    #[test]
    fn fixture_pack_values_drive_contact_clearance_and_host_motion_without_a_cut() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Session::new_blank(&dir.path().join("fixtures.dxf"), units);
            let k = 1. / s.drawing.units.to_metres();
            let Outcome::Walls { result } = run(
                &mut s,
                json!({"type":"walls","runs":[{"points":[[0,0],[6.*k,0]]}],"thickness":0.2*k}),
            ) else {
                panic!("wall")
            };
            let host = result.centrelines[0];
            let before = s
                .objects(&Default::default())
                .unwrap()
                .into_iter()
                .find(|o| o.instance.id == host)
                .unwrap()
                .quantities;
            let mut pack = qiyas_core::placement_pack::shipped_pack("interiors").unwrap();
            pack["types"][0]["name"] = json!("Test specimen");
            pack["types"][0]["properties"]["width"]["default"] = json!(0.47);
            for (i, rule) in pack["checks"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                rule["citation"] = json!(format!("TEST-CLAUSE-A7-{i}"));
            }
            run(&mut s, json!({"type":"install_object_pack","data":pack}));
            let fixture = added(
                &mut s,
                json!({"type":"place_objects","objects":[{"type":"Test specimen","host":host,"props":{"distance":2.}}]}),
            );
            let report = || qiyas_core::check::CheckScope {
                rules: vec!["placement".into()],
                ..Default::default()
            };
            assert!(s.check_drawing(&report()).findings.is_empty());
            added(
                &mut s,
                json!({"type":"draw","kind":"line","a":[20.*k,-10.*k],"b":[20.*k,10.*k]}),
            );
            assert!(
                s.check_drawing(&report()).findings.is_empty(),
                "a remote open line does not enclose the fixture"
            );
            assert!(s.undo());
            let after = s
                .objects(&Default::default())
                .unwrap()
                .into_iter()
                .find(|o| o.instance.id == host)
                .unwrap()
                .quantities;
            assert_eq!(before, after, "a fixture does not cut its host");
            let zones = s.object_zones(&[target(fixture)]).unwrap();
            let body = zones["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|z| z["name"] == "body")
                .unwrap();
            let b: [[f64; 2]; 2] = serde_json::from_value(body["bounds"].clone()).unwrap();
            assert!(((b[1][0] - b[0][0]) / k - 0.47).abs() < 1e-7);
            run(
                &mut s,
                json!({"type":"set_object_props","targets":[fixture],"props":{"back_gap":0.1}}),
            );
            let findings = s.check_drawing(&report()).findings;
            assert!(findings
                .iter()
                .any(|f| f.message.contains("TEST-CLAUSE-A7-0") && f.handles.contains(&fixture)));
            assert!(s.undo());
            assert!(s.check_drawing(&report()).findings.is_empty());
            let obstacle = added(
                &mut s,
                json!({"type":"draw","kind":"circle","centre":[2.*k,1.2*k],"radius":0.1*k}),
            );
            let findings = s.check_drawing(&report()).findings;
            assert!(findings
                .iter()
                .any(|f| f.message.contains("TEST-CLAUSE-A7-1") && f.handles.contains(&obstacle)));
            assert!(s.undo());
            run(
                &mut s,
                json!({"type":"modify","kind":"move","targets":[host],"delta":[0,2.*k]}),
            );
            let moved = s.object_zones(&[target(fixture)]).unwrap();
            let moved_body = moved["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|z| z["name"] == "body")
                .unwrap();
            assert!(
                (moved_body["bounds"][0][1].as_f64().unwrap() - b[0][1] - 2. * k).abs() < 1e-6 * k
            );
            assert!(s.check_drawing(&report()).findings.is_empty());
            let path = dir.path().join("fixtures-saved.dxf");
            s.save_as_headless(&path).unwrap();
            let reopened = Session::open(&path).unwrap();
            assert_eq!(reopened.object_zones(&[target(fixture)]).unwrap(), moved);
        }
    }

    #[test]
    fn optional_pack_install_is_atomic_editable_and_one_undo_step() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("pack.dxf"), 4);
        let before = s.revision();
        let mut broken = qiyas_core::placement_pack::shipped_pack("interiors").unwrap();
        broken["types"][1]["properties"]["width"]["default"] = json!(-1);
        assert!(s
            .run_input(
                &json!({"type":"install_object_pack","data":broken}),
                Source::Mcp
            )
            .is_err());
        assert_eq!(
            s.revision(),
            before,
            "a later invalid type restores earlier type writes too"
        );
        assert!(!s.object_types().unwrap().iter().any(|t| t.name == "WC"));
        let count = s.history_steps().0.len();
        run(
            &mut s,
            json!({"type":"install_object_pack","name":"interiors"}),
        );
        assert_eq!(s.history_steps().0.len(), count + 1);
        assert!(s.object_types().unwrap().iter().any(|t| t.name == "WC"));
        let records = s
            .records(&qiyas_core::records::RecordQuery {
                collection: qiyas_core::records::Collection::KnowledgePlacement,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(records.total, 1);
        assert!(s.undo());
        assert!(!s.object_types().unwrap().iter().any(|t| t.name == "WC"));
        assert!(s.redo());
        assert!(s.object_types().unwrap().iter().any(|t| t.name == "WC"));
    }

    #[test]
    fn analytic_door_sector_checks_clearance_even_when_the_fixture_body_is_clear() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Session::new_blank(&dir.path().join("swing.dxf"), units);
            let k = 1. / s.drawing.units.to_metres();
            let Outcome::Walls { result } = run(
                &mut s,
                json!({"type":"walls","runs":[{"points":[[0,0],[6.*k,0]]}],"thickness":0.2*k,
            "openings":[{"run":0,"at":3.*k,"width":0.9*k,"kind":"door"}]}),
            ) else {
                panic!("wall")
            };
            let host = result.centrelines[0];
            run(
                &mut s,
                json!({"type":"install_object_pack","name":"interiors"}),
            );
            let fixture = added(
                &mut s,
                json!({"type":"place_objects","objects":[{"type":"WC","host":host,"props":{"distance":3.9,"clear_width":2.}}]}),
            );
            let scope = qiyas_core::check::CheckScope {
                rules: vec!["placement".into()],
                ..Default::default()
            };
            let before = s.check_drawing(&scope);
            assert!(before.complete, "{before:?}");
            assert!(
                before
                    .findings
                    .iter()
                    .any(|f| f.message.contains("Door swing") && f.handles.contains(&fixture)),
                "{before:?}"
            );
            run(
                &mut s,
                json!({"type":"set_object_props","targets":[fixture],"props":{"clear_width":0.762}}),
            );
            assert!(s.check_drawing(&scope).findings.is_empty());
        }
    }
}
