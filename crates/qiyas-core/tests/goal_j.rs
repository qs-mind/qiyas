use qiyas_core::{Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) -> Value {
    serde_json::to_value(s.run_input(&v, Source::Ai).unwrap()).unwrap()
}
#[test]
fn quantities_in_notes_need_real_citations_in_every_drawing_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in [(4, 1000.0), (5, 100.0), (6, 1.0), (1, 1.0 / 0.0254)] {
        let mut s = Session::new_blank(&dir.path().join("units.dxf"), units);
        let h = run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5.0*k,4.0*k]}),
        )["handles"][0]
            .clone();
        run(&mut s, json!({"type":"measure_entity","handle":h}));
        let row = &s.takeoff.items[0];
        assert!((row.shown - 20.0).abs() < 1e-7);
        let id = row.id.clone();
        for source in [Source::Ai, Source::Mcp] {
            for text in [
                "Area 20 m²",
                "Length 5 m",
                "Volume 60 m³",
                "المساحة ٢٠ م²",
                "الحجم ٦٠ متر مكعب",
            ] {
                let before = s.takeoff.items.len();
                let history = s.history_revision();
                let v = json!({"type":"add_markup","kind":"note","points":[[k,k]],"text":text});
                let e = s.run_input(&v, source).unwrap_err().to_string();
                assert!(e.contains("measure_entity"));
                assert_eq!(s.takeoff.items.len(), before);
                assert_eq!(s.history_revision(), history);
                let mut cited = v;
                cited["text"] = json!(format!("{text} [takeoff:{id}]"));
                s.run_input(&cited, source).unwrap();
                assert!(s.takeoff.items.last().unwrap().description.contains(&id));
            }
        }
        run(
            &mut s,
            json!({"type":"add_markup","kind":"note","points":[[k,k]],"text":"Confirm this room"}),
        );
        let markup = s.takeoff.items.last().unwrap().id.clone();
        assert!(s
            .run_input(
                &json!({"type":"set_description","id":markup,"text":"20 m²"}),
                Source::Mcp
            )
            .is_err());
        assert!(s
            .run_input(
                &json!({"type":"comment","points":[[0,0],[k,0],[k,k]],"text":"20 m²"}),
                Source::Ai
            )
            .is_err());
        assert!(s
            .run_input(
                &json!({"type":"set_note","id":markup,"text":"٢٠ م²"}),
                Source::Ai
            )
            .is_err());
        assert!(s.run_input(&json!({"type":"add_markup","kind":"note","points":[[k,k]],"text":"20 m² [takeoff:missing]"}), Source::Mcp).is_err());
        assert!(s.run_input(&json!({"type":"add_markup","kind":"note","points":[[k,k]],"text":format!("20 m² [takeoff:{id}-missing]")}), Source::Mcp).is_err());
        let note = json!({"type":"add_measurement","kind":"item","unit":"markup","value":0,"points":[[k,k]],"description":"Check this room","note":"٢٠ م²","empty_item":true});
        assert!(s.run_input(&note, Source::Mcp).is_err());
        let mut cited_note = note;
        cited_note["note"] = json!(format!("٢٠ م² [takeoff:{id}]"));
        s.run_input(&cited_note, Source::Mcp).unwrap();
        // Manual user notes retain their existing behaviour.
        s.run_input(
            &json!({"type":"add_markup","kind":"note","points":[[k,k]],"text":"20 m²"}),
            Source::User,
        )
        .unwrap();
    }
}
#[test]
fn rooms_names_candidate_layers_with_counts_only_inside_closed_regions() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in [(4, 1000.0), (5, 100.0), (6, 1.0), (1, 1.0 / 0.0254)] {
        let mut s = Session::new_blank(&dir.path().join("rooms.dxf"), units);
        run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5.0*k,4.0*k]}),
        );
        run(
            &mut s,
            json!({"type":"batch","layer":"Space names","commands":[{"type":"text","at":[k,k],"height":0.2*k,"value":"Living"},{"type":"text","at":[2.0*k,2.0*k],"height":0.2*k,"value":"غرفة المعيشة"},{"type":"text","at":[10.0*k,10.0*k],"height":0.2*k,"value":"Outside"}]}),
        );
        let err = s
            .run_input(
                &json!({"type":"rooms","query":{"text_layer":"Wrong"}}),
                Source::Mcp,
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("Space names (2)"), "{err}");
    }
}
#[test]
fn registry_normalises_single_handles_and_where_but_refuses_ambiguous_edits() {
    for source in [
        json!({"kind":"text","handles":[42],"delimiter":"|","row_tolerance_m":0.01}),
        json!({"kind":"attributes","handles":[42],"tags":["W"]}),
    ] {
        let original = json!({"type":"recognition","kind":"read_schedule","source":source});
        let mut canonical = original.clone();
        qiyas_core::registry::canonicalize(&mut canonical);
        assert_eq!(canonical, original);
        assert!(qiyas_core::registry::normalization_warnings(&original).is_empty());
    }
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("registry.dxf"), 6);
    let h = run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
    )["handles"][0]
        .clone();
    for command in [
        json!({"type":"measure_entity","handles":[h]}),
        json!({"type":"records","query":{"collection":"entities","where":{"path":"type","op":"eq","value":"Line"}}}),
    ] {
        let report = s.validate_input(&command);
        assert!(report.valid, "{report:?}");
        assert!(report.warnings.iter().any(|w| w.code == "normalised_input"));
        let out = run(&mut s, command);
        assert!(out["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "normalised_input"));
    }
    for command in [
        json!({"type":"measure_entity","handles":[h,h]}),
        json!({"type":"measure_entity","handle":h,"handles":[h]}),
    ] {
        assert!(s.run_input(&command, Source::Ai).is_err());
    }
}
#[test]
fn generated_texts_and_dimensions_keep_handles_when_edited_in_all_units() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in [(4, 1000.0), (5, 100.0), (6, 1.0), (1, 1.0 / 0.0254)] {
        let mut s = Session::new_blank(&dir.path().join("edits.dxf"), units);
        run(&mut s, json!({"type":"annotation_scale","name":"1:50"}));
        let t = run(
            &mut s,
            json!({"type":"text","at":[k,k],"value":"Bedrom","height":0.125*k}),
        )["handles"][0]
            .clone();
        let d=run(&mut s,json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[5.0*k,0],"line_point":[2.5*k,-k]}))["handles"][0].clone();
        let d2=run(&mut s,json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[2.0*k,0],"line_point":[k,-1.1*k]}))["handles"][0].clone();
        for c in [
            json!({"type":"edit_text","target":t,"value":"Bedroom"}),
            json!({"type":"modify","kind":"move","targets":[t],"delta":[k,k]}),
            json!({"type":"auto_place","targets":[t]}),
            json!({"type":"dim_space","base":d,"targets":[d2],"spacing":0.5*k}),
            json!({"type":"dim_edit","target":d,"op":{"move_text":[3.0*k,-k]}}),
            json!({"type":"modify","kind":"move","targets":[d],"delta":[k,0]}),
            json!({"type":"auto_place","targets":[d]}),
        ] {
            run(&mut s, c);
            for h in [&t, &d, &d2] {
                assert!(s.entity_info(h.as_u64().unwrap(), &[]).is_ok());
            }
        }
        let dim = s
            .read_dimensions(None)
            .into_iter()
            .find(|r| r.handle == d.as_u64().unwrap())
            .unwrap();
        assert!((dim.measurement.unwrap() / k - 5.0).abs() < 1e-7);
        assert!(s
            .read_text(None, None)
            .iter()
            .any(|r| r.handle == t.as_u64().unwrap() && r.text == "Bedroom"));
    }
}
#[test]
fn version_error_and_kit_name_the_supported_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("method.dxf"), 6);
    let error=s.run_input(&json!({"type":"measurement","kind":"set_method","scope":"project","method":{"version":2,"name":"geometry","title":"Geometry","based_on":"drawing","source_note":"Observed geometry","rules":[]}}),Source::Ai).unwrap_err().to_string();
    assert!(error.contains("supported versions: 1"), "{error}");
    let kit = qiyas_core::takeoff_kit::kit();
    let method = kit["batch"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "measurement" && c["kind"] == "set_method")
        .unwrap();
    assert_eq!(method["method"]["version"], 1);
}

#[test]
fn auto_place_dimension_can_shift_text_clear_of_a_grid_without_rebuilding() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in [(4, 1000.0), (5, 100.0), (6, 1.0), (1, 1.0 / 0.0254)] {
        let mut s = Session::new_blank(&dir.path().join("grid.dxf"), units);
        run(&mut s, json!({"type":"annotation_scale","name":"1:50"}));
        run(
            &mut s,
            json!({"type":"grid","x":[{"label":"A","at":0},{"label":"B","at":4.0*k},{"label":"C","at":8.0*k}],"y":[{"label":"1","at":0},{"label":"2","at":7.0*k}],"extent":{"min":[-0.7*k,-1.5*k],"max":[8.7*k,7.7*k]}}),
        );
        let h=run(&mut s,json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[8.0*k,0],"line_point":[4.0*k,-0.7*k]}))["handles"][0].clone();
        let chain=run(&mut s,json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[4.0*k,0],"line_point":[2.0*k,-0.35*k]}))["handles"][0].clone();
        run(&mut s, json!({"type":"auto_place","targets":[h,chain]}));
        let rows = s.read_dimensions(None);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.at[1] < 0.0));
        assert_eq!(rows[0].handle, h.as_u64().unwrap());
        assert!((rows[0].measurement.unwrap() / k - 8.0).abs() < 1e-8);
        let report = run(
            &mut s,
            json!({"type":"check_drawing","scope":{"handles":[h],"rules":["text_over_lines","annotation_over_grid"]}}),
        );
        assert_eq!(report["report"]["warnings"], 0, "{report}");
    }
}
