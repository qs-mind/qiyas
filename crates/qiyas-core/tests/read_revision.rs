use qiyas_core::{CoreError, Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, input: Value) -> Value {
    json!(s.run_input(&input, Source::Mcp).unwrap())
}

#[test]
fn stale_edits_report_rebuilds_and_fresh_edits_apply_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("revision.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let made = run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[8.0*k,0]]}],"thickness":0.2*k}),
        );
        let handle = made["centrelines"][0].as_u64().unwrap();
        let read = json!(s.revision());
        let rebuilt = run(
            &mut s,
            json!({"type":"add_wall_openings","centreline":handle,"openings":[{"at":2.0*k,"width":0.9*k,"kind":"door"}]}),
        );
        assert!(rebuilt["handle_map"].is_object());
        let current = s.revision();
        let history = s.history_steps();
        let mut edit = json!({"type":"set_property","target":{"handle":handle},"prop":"color","value":"red","expected_revision":read});
        let CoreError::StaleRead { details } = s.run_input(&edit, Source::Mcp).unwrap_err() else {
            panic!("stale read")
        };
        assert_eq!(details["history_complete"], true, "{details}");
        assert!(!details["handle_maps"].as_array().unwrap().is_empty());
        assert!(details["changed"]
            .as_array()
            .unwrap()
            .contains(&json!(handle)));
        assert_eq!(s.revision(), current);
        assert_eq!(s.history_steps(), history);
        edit["expected_revision"] = json!(s.revision());
        let changed = run(&mut s, edit);
        assert_eq!(changed["revision"], json!(s.revision()));
        assert_eq!(
            s.drawing
                .doc
                .get_entity(opencadcodec::Handle::new(handle))
                .unwrap()
                .common()
                .color,
            opencadcodec::Color::Index(1)
        );
    }
}

#[test]
fn rollback_undo_context_and_history_expiry_are_honest() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("revision.dxf"), 6);
    let initial = json!(s.revision());
    let result=s.run_input(&json!({"type":"batch","commands":[{"type":"draw","kind":"line","a":[0,0],"b":[1,0]},{"type":"set_property","target":{"handle":999999},"prop":"color","value":"red"}]}),Source::Mcp);
    assert!(result.is_err());
    assert_eq!(json!(s.revision()), initial);
    let h = run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    let before_undo = json!(s.revision());
    assert!(s.undo());
    let CoreError::StaleRead { details } = s.check_revision(&before_undo).unwrap_err() else {
        panic!()
    };
    assert!(details["removed"].as_array().unwrap().contains(&json!(h)));
    assert!(s.redo());
    let before_context = json!(s.revision());
    run(&mut s, json!({"type":"new_layer","name":"Names"}));
    run(
        &mut s,
        json!({"type":"set_current","current":{"layer":"Names"}}),
    );
    assert!(s.check_revision(&before_context).is_err());
    let before_gap = json!(s.revision());
    run(
        &mut s,
        json!({"type":"text","at":[0,2],"height":0.2,"value":"Before"}),
    );
    // A window context change outside the command bus leaves a gap in retained history.
    s.current.layer = "0".into();
    run(
        &mut s,
        json!({"type":"text","at":[0,3],"height":0.2,"value":"After"}),
    );
    let CoreError::StaleRead { details } = s.check_revision(&before_gap).unwrap_err() else {
        panic!()
    };
    assert_eq!(details["history_complete"], false);
    for n in 0..130 {
        run(
            &mut s,
            json!({"type":"text","at":[n,0],"height":0.2,"value":"Name"}),
        );
    }
    let CoreError::StaleRead { details } = s.check_revision(&initial).unwrap_err() else {
        panic!()
    };
    assert_eq!(details["history_complete"], false);
    let another = Session::new_blank(&dir.path().join("revision.dxf"), 6);
    assert_ne!(s.revision().session, another.revision().session);
}
