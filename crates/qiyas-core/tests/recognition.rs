//! Evidence, decisions, unit conversion and persistence all use the production command bus.
use qiyas_core::{Command, Outcome, Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) -> Value {
    serde_json::to_value(s.run_input(&v, Source::User).unwrap()).unwrap()
}
fn add(s: &mut Session, v: Value) -> u64 {
    run(s, v)["handles"][0].as_u64().unwrap()
}
fn define(s: &mut Session) {
    run(
        s,
        json!({"type":"define_object_type","definition":{"name":"Run","category":"test","geometry":"path","properties":{"width":{"kind":"number","unit":"length","default":0.0,"min":0.0},"height":{"kind":"number","unit":"length","default":0.0}},"quantities":{"measured_length":"length","surface":"length * height","volume":"length * width * height"}}}),
    );
}
fn rule(s: &mut Session, h: u64, confidence: f64) {
    run(
        s,
        json!({"type":"recognition","kind":"define","rule":{"name":"survey","object_type":"Run","match":{"handles":[h],"layer":{"regex":"^LINES$"},"colour":16777215},"extract":{"width":{"kind":"parallel_distance","other":{"layer":"LINES"},"max_distance_m":0.5},"height":{"kind":"text","regex":"H=([0-9.]+)","nearby":{"layer":"NOTES"},"max_distance_m":20}},"confidence":confidence}}),
    );
}
#[test]
fn extraction_review_and_one_step_undo_work_in_every_unit() {
    for units in [1, 4, 5, 6] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), units);
        define(&mut s);
        let u = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"new_layer","name":"LINES","color":16777215}),
        );
        run(
            &mut s,
            json!({"type":"set_current","current":{"layer":"LINES"}}),
        );
        let h = add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[0,0],"b":[5.0*u,0]}),
        );
        add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[0,0.2*u],"b":[5.0*u,0.2*u]}),
        );
        run(&mut s, json!({"type":"new_layer","name":"NOTES"}));
        run(
            &mut s,
            json!({"type":"set_current","current":{"layer":"NOTES"}}),
        );
        add(
            &mut s,
            json!({"type":"text","at":[0,2.0*u],"value":"H=3.2"}),
        );
        rule(&mut s, h, 0.7);
        let before = s.history_steps().0.len();
        let revision = s.batch_version();
        let dry = run(&mut s, json!({"type":"recognition","kind":"test"}));
        assert_eq!(s.history_steps().0.len(), before);
        assert_eq!(s.batch_version(), revision);
        let c = &dry["result"]["candidates"][0];
        assert_eq!(c["state"], "pending");
        assert!((c["placement"]["props"]["width"].as_f64().unwrap() - 0.2).abs() < 1e-8);
        assert_eq!(c["placement"]["props"]["height"], 3.2);
        run(&mut s, json!({"type":"recognition","kind":"apply"}));
        assert_eq!(s.history_steps().0.len(), before + 1);
        assert!(s.objects(&Default::default()).unwrap().is_empty());
        run(
            &mut s,
            json!({"type":"recognition","kind":"review","handle":h,"decision":"accepted","props":{"height":4.0},"save_as":"corrected"}),
        );
        let objects = s.objects(&Default::default()).unwrap();
        assert_eq!(objects.len(), 1);
        assert!((objects[0].quantities["volume"] - 4.0).abs() < 1e-8);
        assert_eq!(s.history_steps().0.len(), before + 2);
        assert!(s.undo());
        assert!(s.objects(&Default::default()).unwrap().is_empty());
        assert_eq!(s.recognition_rules().unwrap().len(), 1);
        assert!(s.redo());
        assert_eq!(s.recognition_rules().unwrap().len(), 2);
        let before = s.history_steps().0.len();
        run(&mut s, json!({"type":"recognition","kind":"apply"}));
        assert_eq!(s.history_steps().0.len(), before);
        for extension in ["dwg", "dxf"] {
            let path = dir.path().join(format!("saved.{extension}"));
            s.save_as(&path).unwrap();
            let reopened = Session::open(&path).unwrap();
            assert_eq!(
                reopened.recognition_queue(None).unwrap()[0].state,
                qiyas_core::recognition::ReviewState::Accepted
            );
            assert_eq!(reopened.recognition_rules().unwrap().len(), 2);
            assert!(
                (reopened.objects(&Default::default()).unwrap()[0].quantities["volume"] - 4.0)
                    .abs()
                    < 1e-8
            );
        }
    }
}
#[test]
fn missing_evidence_and_conflicts_are_reviewed_and_invalid_rule_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), 6);
    define(&mut s);
    let h = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
    );
    for name in ["first", "second"] {
        run(
            &mut s,
            json!({"type":"recognition","kind":"define","rule":{"name":name,"object_type":"Run","match":{"entity_type":"Line"},"extract":{"height":{"kind":"attribute","tag":"H"}},"confidence":1}}),
        );
    }
    run(&mut s, json!({"type":"recognition","kind":"apply"}));
    let pending = &s.recognition_queue(None).unwrap()[0];
    assert_eq!(pending.unresolved, ["height"]);
    assert!(pending.issues.len() >= 2);
    let before = s.history_steps().0.len();
    assert!(s
        .run_input(
            &json!({"type":"recognition","kind":"review","handle":h,"decision":"accepted"}),
            Source::User
        )
        .is_err());
    assert_eq!(s.history_steps().0.len(), before);
    let bad = json!({"type":"recognition","kind":"define","rule":{"name":"bad","object_type":"Run","match":{"layer":{"regex":"["}},"confidence":1}});
    assert!(s.run_input(&bad, Source::User).is_err());
    assert_eq!(s.recognition_rules().unwrap().len(), 2);
    run(
        &mut s,
        json!({"type":"recognition","kind":"review","handle":h,"decision":"rejected","save_as":"excluded"}),
    );
    assert_eq!(
        s.recognition_rules()
            .unwrap()
            .iter()
            .filter(|r| r.reject)
            .count(),
        1
    );
    assert!(s.objects(&Default::default()).unwrap().is_empty());
}
#[test]
fn drawing_schedule_becomes_types_and_rules_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), 6);
    let h = add(
        &mut s,
        json!({"type":"table","at":[0,10],"rows":3,"cols":3,"col_width":3,"row_height":1,"data":[["Type","Width","Height"],["D-1","٩٠٠","٢١٠٠"],["D-2","1200","2400"]]}),
    );
    let before = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"recognition","kind":"schedule","spec":{"source":{"kind":"table","handle":h,"skip_rows":1},"template":{"name":"Schedule","category":"drawing","geometry":"point","properties":{"width":{"kind":"number","unit":"length","default":0},"height":{"kind":"number","unit":"length","default":0}},"quantities":{"measured_count":"count","area":"width*height"}},"type_column":0,"match_column":0,"match_on":"block","properties":{"width":{"column":1,"factor":0.001},"height":{"column":2,"factor":0.001}},"confidence":0.95}}),
    );
    assert_eq!(s.object_types().unwrap().len(), 2);
    assert_eq!(s.recognition_rules().unwrap().len(), 2);
    assert_eq!(s.history_steps().0.len(), before + 1);
    assert!(s.undo());
    assert!(s.object_types().unwrap().is_empty());
    assert!(s.redo());
    let rules = s.recognition_rules().unwrap();
    assert_eq!(
        serde_json::to_value(&rules[0]).unwrap()["extract"]["width"]["value"],
        0.9
    );
}
#[test]
fn blocks_attributes_hatch_and_text_grids_use_drawing_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), 6);
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"Symbol","category":"drawing","geometry":"point","properties":{"width":{"kind":"number","unit":"length","default":0}},"quantities":{"measured_count":"count"}}}),
    );
    // A text grid remains readable without a CAD table object.
    let a = add(
        &mut s,
        json!({"type":"mtext","at":[0,5],"width":4,"value":"X1|900\nX2|1200"}),
    );
    let grid = run(
        &mut s,
        json!({"type":"recognition","kind":"read_schedule","source":{"kind":"text","handles":[a],"delimiter":"|","row_tolerance_m":0.01}}),
    );
    assert_eq!(
        grid["result"]["rows"],
        json!([["X1", "900"], ["X2", "1200"]])
    );
    let bad:Command=serde_json::from_value(json!({"type":"recognition","kind":"define","rule":{"name":"unknown","object_type":"Missing","match":{},"confidence":0.8}})).unwrap();
    assert!(s.run(bad).is_err());
    assert!(matches!(
        s.run(serde_json::from_value(json!({"type":"recognition","kind":"list"})).unwrap())
            .unwrap(),
        Outcome::Recognition { .. }
    ));
}

#[test]
fn calibrated_pdf_import_and_vision_keep_source_and_use_one_undo_step() {
    if !qiyas_pdf::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let pdf = qiyas_pdf::fixtures::write_wall(dir.path());
    for units in [1, 4, 5, 6] {
        let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), units);
        run(
            &mut s,
            json!({"type":"define_object_type","definition":{"name":"Region","category":"example","geometry":"region","quantities":{"measured_area":"area"}}}),
        );
        let before = s.history_steps().0.len();
        let imported = run(
            &mut s,
            json!({"type":"recognition","kind":"pdf","path":pdf,"page":1,"calibration":{"a":[0,0],"b":[100,0],"distance_m":1}}),
        );
        assert!(imported["result"]["handles"].as_array().unwrap().len() >= 2);
        assert_eq!(s.history_steps().0.len(), before + 1);
        run(
            &mut s,
            json!({"type":"recognition","kind":"define","rule":{"name":"red","object_type":"Region","match":{"layer":"PDF-FF0000","closed_region":true},"confidence":0.99}}),
        );
        run(&mut s, json!({"type":"recognition","kind":"apply"}));
        let objects = s.objects(&Default::default()).unwrap();
        assert_eq!(objects.len(), 1);
        assert!((objects[0].quantities["measured_area"] - 0.5).abs() < 1e-6);
        let queue = s.recognition_queue(None).unwrap();
        assert_eq!(queue[0].evidence[0].page, Some(1));
        assert!(queue[0].evidence[0]
            .path
            .as_ref()
            .unwrap()
            .ends_with("wall.pdf"));
    }
    let mut s = Session::new_blank(&dir.path().join("vision.dxf"), 6);
    define(&mut s);
    let result = run(
        &mut s,
        json!({"type":"recognition","kind":"vision","spec":{"path":pdf,"page":1,"calibration":{"a":[20,20],"b":[120,20],"distance_m":1},"snap_radius_m":0.02,"hints":[{"object_type":"Run","points":[[20,20],[120,20]],"confidence":0.99}]}}),
    );
    let candidate = &result["result"]["candidates"][0];
    assert_eq!(candidate["state"], "pending");
    assert!(s.objects(&Default::default()).unwrap().is_empty());
    let h = candidate["handle"].as_u64().unwrap();
    run(
        &mut s,
        json!({"type":"recognition","kind":"review","handle":h,"decision":"accepted"}),
    );
    assert!(
        (s.objects(&Default::default()).unwrap()[0].quantities["measured_length"] - 1.0).abs()
            < 0.02
    );
    assert!(s.undo());
    assert!(s.objects(&Default::default()).unwrap().is_empty());
}

#[test]
fn attribute_blocks_hatches_and_enclosed_text_recognise_as_types() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), 6);
    let line = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
    );
    let att = add(
        &mut s,
        json!({"type":"att_def","tag":"W","prompt":"Width","default":"900","at":[0,0]}),
    );
    run(
        &mut s,
        json!({"type":"create_block","name":"D-1","base":[0,0],"targets":[{"handle":line},{"handle":att}],"convert_to_insert":false}),
    );
    let insert = add(
        &mut s,
        json!({"type":"insert_block","name":"D-1","at":[20,20],"attribute_values":{"W":"١٢٠٠"}}),
    );
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"Door","category":"schedule","geometry":"point","properties":{"width":{"kind":"number","unit":"length","default":0}},"quantities":{"measured_count":"count"}}}),
    );
    run(
        &mut s,
        json!({"type":"recognition","kind":"define","rule":{"name":"block","object_type":"Door","match":{"block":"D-*","attributes":{"W":"١*"}},"extract":{"width":{"kind":"attribute","tag":"W","factor":0.001}},"confidence":0.99}}),
    );
    let grid = run(
        &mut s,
        json!({"type":"recognition","kind":"read_schedule","source":{"kind":"attributes","handles":[insert],"tags":["W"]}}),
    );
    assert_eq!(grid["result"]["rows"], json!([["١٢٠٠"]]));
    run(&mut s, json!({"type":"recognition","kind":"apply"}));
    assert_eq!(
        s.objects(&Default::default()).unwrap()[0].instance.props["width"],
        1.2
    );
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"Patch","category":"example","geometry":"region","quantities":{"measured_area":"area"}}}),
    );
    add(
        &mut s,
        json!({"type":"draw","kind":"hatch","points":[[30,0],[32,0],[32,3],[30,3]]}),
    );
    run(
        &mut s,
        json!({"type":"recognition","kind":"define","rule":{"name":"hatch","object_type":"Patch","match":{"hatch":"SOLID"},"confidence":0.99}}),
    );
    run(&mut s, json!({"type":"recognition","kind":"apply"}));
    let objects = s.objects(&Default::default()).unwrap();
    assert!(
        (objects
            .iter()
            .find(|o| o.instance.type_name == "Patch")
            .unwrap()
            .quantities["measured_area"]
            - 6.0)
            .abs()
            < 1e-8
    );
    add(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    add(&mut s, json!({"type":"text","at":[2,2],"value":"مساحة"}));
    run(
        &mut s,
        json!({"type":"recognition","kind":"define","rule":{"name":"label","object_type":"Patch","match":{"text":"مساحة","closed_region":true},"confidence":0.99}}),
    );
    run(&mut s, json!({"type":"recognition","kind":"apply"}));
    assert!(s.objects(&Default::default()).unwrap().iter().any(|o| (o
        .quantities
        .get("measured_area")
        .copied()
        .unwrap_or(0.0)
        - 20.0)
        .abs()
        < 1e-8));
}

#[test]
fn correcting_type_replaces_inapplicable_properties_and_refits_source_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), 6);
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"A","category":"example","geometry":"point","properties":{"width":{"kind":"number","unit":"length","default":0}},"quantities":{"items":"count"}}}),
    );
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"B","category":"example","geometry":"region","properties":{"height":{"kind":"number","unit":"length","default":0}},"quantities":{"volume":"area*height"}}}),
    );
    let h = add(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[2,3]}),
    );
    run(
        &mut s,
        json!({"type":"recognition","kind":"define","rule":{"name":"original","object_type":"A","match":{"handles":[h]},"extract":{"width":{"kind":"attribute","tag":"missing"}},"confidence":0.6}}),
    );
    run(&mut s, json!({"type":"recognition","kind":"apply"}));
    let before = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"recognition","kind":"review","handle":h,"decision":"accepted","object_type":"B","props":{"height":3,"width":12},"save_as":"corrected"}),
    );
    assert_eq!(s.history_steps().0.len(), before + 1);
    let objects = s.objects(&Default::default()).unwrap();
    assert_eq!(objects[0].instance.type_name, "B");
    assert_eq!(objects[0].quantities["volume"], 18.0);
    assert!(!objects[0].instance.props.contains_key("width"));
    let learned = s
        .recognition_rules()
        .unwrap()
        .into_iter()
        .find(|r| r.name == "corrected")
        .unwrap();
    assert!(!learned.extract.contains_key("width"));
    assert!(learned.extract.contains_key("height"));
    assert_eq!(learned.priority, 1);
    assert!(s.undo());
    assert!(s.objects(&Default::default()).unwrap().is_empty());
    assert_eq!(s.recognition_queue(None).unwrap()[0].unresolved, ["width"]);
}
