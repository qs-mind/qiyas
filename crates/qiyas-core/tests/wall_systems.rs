//! Editable wall systems share one definition across phased drafting and opening edits.

use opencadcodec::{EntityType, Handle};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, command: Value) -> Value {
    serde_json::to_value(session.run_input(&command, Source::User).unwrap().outcome).unwrap()
}

fn near(value: &Value, expected: f64) {
    let value = value.as_f64().unwrap();
    assert!((value - expected).abs() < 1e-7, "{value} != {expected}");
}

fn groups(session: &mut Session, width: f64) -> Value {
    run(session, json!({"type":"wall_groups","width":width}))["groups"].clone()
}

fn t_runs(factor: f64) -> [Value; 2] {
    [
        json!({"points":[[0,0],[10.0/factor,0]],"thickness":0.25/factor,"height":3}),
        json!({"points":[[5.0/factor,0],[5.0/factor,4.0/factor]],"thickness":0.15/factor,"height":2}),
    ]
}

#[test]
fn staged_mixed_walls_equal_one_command_in_every_unit_and_undo_restores_the_first_phase() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut phased = Session::new_blank(&dir.path().join("phased.dxf"), units);
        let mut combined = Session::new_blank(&dir.path().join("combined.dxf"), units);
        let runs = t_runs(factor);
        let first = run(
            &mut phased,
            json!({"type":"walls","thickness":0.25/factor,"runs":[runs[0]]}),
        );
        let first_read = groups(&mut phased, 0.9 / factor);
        let joined = run(
            &mut phased,
            json!({"type":"walls","thickness":0.15/factor,"runs":[runs[1]]}),
        );
        let once = run(
            &mut combined,
            json!({"type":"walls","thickness":0.25/factor,"runs":runs}),
        );
        // The partition ends at the outer wall's face: .15 * (4 - .125) m².
        near(&joined["gross_area"], 3.08125);
        near(&joined["net_area"], 3.08125);
        near(&joined["volume"], 8.6625);
        for field in ["centreline_length", "gross_area", "net_area", "volume"] {
            near(&joined[field], once[field].as_f64().unwrap());
        }
        for field in ["handles", "outlines", "hatches", "centrelines", "inserts"] {
            assert_eq!(
                joined[field].as_array().unwrap().len(),
                once[field].as_array().unwrap().len(),
                "{field}"
            );
        }
        assert_eq!(phased.history_steps().0.len(), 2);
        let read = groups(&mut phased, 0.9 / factor);
        assert_eq!(read.as_array().unwrap().len(), 1);
        assert_eq!(read[0]["runs"].as_array().unwrap().len(), 2);
        for (i, (length, thickness, height)) in [(10.0, 0.25, 3.0), (4.0, 0.15, 2.0)]
            .into_iter()
            .enumerate()
        {
            near(&read[0]["runs"][i]["length"], length / factor);
            near(&read[0]["runs"][i]["thickness"], thickness / factor);
            near(&read[0]["runs"][i]["height"], height);
        }
        for old in first["handles"].as_array().unwrap() {
            assert!(joined["handle_map"]
                .get(old.as_u64().unwrap().to_string())
                .is_some());
        }
        assert!(phased.undo());
        assert_eq!(groups(&mut phased, 0.9 / factor), first_read);
        assert!(phased.redo());
        assert_eq!(groups(&mut phased, 0.9 / factor), read);
    }
}

#[test]
fn a_bridge_joins_both_groups_but_join_false_and_different_layers_stay_separate() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("bridge.dxf"), 6);
    for y in [0, 4] {
        run(
            &mut session,
            json!({"type":"walls","thickness":0.25,"runs":[{"points":[[0,y],[10,y]]}]}),
        );
    }
    assert_eq!(groups(&mut session, 0.9).as_array().unwrap().len(), 2);
    run(
        &mut session,
        json!({"type":"walls","thickness":0.15,"runs":[{"points":[[5,0],[5,4]]}]}),
    );
    assert_eq!(groups(&mut session, 0.9).as_array().unwrap().len(), 1);
    assert!(session.undo());
    assert_eq!(groups(&mut session, 0.9).as_array().unwrap().len(), 2);
    run(
        &mut session,
        json!({"type":"walls","join":false,"thickness":0.15,"runs":[{"points":[[5,0],[5,4]]}]}),
    );
    assert_eq!(groups(&mut session, 0.9).as_array().unwrap().len(), 3);
    run(
        &mut session,
        json!({"type":"walls","thickness":0.15,"layers":{"outline":"OTHER-WALL"},"runs":[{"points":[[7,0],[7,4]]}]}),
    );
    assert_eq!(groups(&mut session, 0.9).as_array().unwrap().len(), 4);
}

#[test]
fn opening_commands_use_current_handles_and_are_atomic_with_usable_free_ranges() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut session = Session::new_blank(&dir.path().join("openings.dxf"), units);
        run(
            &mut session,
            json!({"type":"walls","thickness":0.25/factor,"runs":t_runs(factor)}),
        );
        let before = groups(&mut session, 1.2 / factor);
        let centreline = &before[0]["runs"][0]["centreline"];
        let revision = session.batch_version();
        let bad = json!({"type":"add_wall_openings","centreline":centreline,"openings":[
            {"at":2.0/factor,"width":0.9/factor,"kind":"door"},
            {"at":5.0/factor,"width":1.2/factor,"kind":"window"}
        ]});
        let error = session
            .run_input(&bad, Source::User)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("junction") && error.contains("fits"),
            "{error}"
        );
        assert_eq!(groups(&mut session, 1.2 / factor), before);
        assert_eq!(session.history_steps().0.len(), 1);
        assert_eq!(session.batch_version(), revision);
        let ranges = before[0]["runs"][0]["free_ranges"].as_array().unwrap();
        assert!(ranges.len() >= 2);
        for range in ranges {
            for at in range.as_array().unwrap() {
                run(
                    &mut session,
                    json!({"type":"add_wall_openings","centreline":centreline,"openings":[{"at":at,"width":1.2/factor,"kind":"window"}]}),
                );
                assert!(session.undo());
                assert_eq!(groups(&mut session, 1.2 / factor), before);
            }
        }
        let added = run(
            &mut session,
            json!({"type":"add_wall_openings","centreline":centreline,"openings":[{"at":2.0/factor,"width":1.2/factor,"kind":"window"}]}),
        );
        near(&added["net_area"], 2.78125);
        let insert = &added["inserts"][0];
        let with_window = groups(&mut session, 1.2 / factor);
        let error = session
            .run_input(
                &json!({"type":"delete_entities","targets":[{"handle":insert}]}),
                Source::User,
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("remove_wall_openings"), "{error}");
        assert_eq!(groups(&mut session, 1.2 / factor), with_window);
        let edited = run(
            &mut session,
            json!({"type":"edit_wall_opening","insert":insert,"at":3.0/factor,"width":1.0/factor,"kind":"door","sill":0,"head":2.1,"swing":"right"}),
        );
        near(&edited["net_area"], 2.83125);
        let after_edit = groups(&mut session, 1.2 / factor);
        let opening = &after_edit[0]["runs"][0]["openings"][0];
        near(&opening["at"], 3.0 / factor);
        near(&opening["width"], 1.0 / factor);
        assert_eq!(opening["kind"], "door");
        assert_eq!(opening["swing"], "right");
        assert!(session.undo());
        assert_eq!(groups(&mut session, 1.2 / factor), with_window);
        assert!(session.redo());
        let closed = run(
            &mut session,
            json!({"type":"remove_wall_openings","inserts":edited["inserts"]}),
        );
        near(&closed["net_area"], 3.08125);
        assert!(groups(&mut session, 1.2 / factor)[0]["runs"][0]["openings"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(session.history_steps().0.len(), 4);
        assert!(session.undo());
        assert_eq!(groups(&mut session, 1.2 / factor), after_edit);
    }
}

fn text(session: &Session, handle: u64) -> String {
    match session.drawing.doc.get_entity(Handle::new(handle)).unwrap() {
        EntityType::Text(text) => text.value.clone(),
        EntityType::MText(text) => text.value.clone(),
        other => panic!("field text expected: {other:?}"),
    }
}

#[test]
fn takeoff_and_fields_follow_a_split_hatch_and_replaced_centreline_in_the_same_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("follow.dxf"), 6);
    let wall = run(
        &mut session,
        json!({"type":"walls","thickness":0.25,"runs":[{"points":[[0,0],[10,0]]}]}),
    );
    assert_eq!(wall["hatches"].as_array().unwrap().len(), 1);
    let hatch = &wall["hatches"][0];
    let centreline = &wall["centrelines"][0];
    for handle in [hatch, centreline] {
        run(
            &mut session,
            json!({"type":"measure_entity","handle":handle}),
        );
    }
    let mut fields = Vec::new();
    for (kind, target, y) in [("area", hatch, 2), ("length", centreline, 3)] {
        let result = run(
            &mut session,
            json!({"type":"field","at":[0,y],"field":{"kind":kind,"target":{"handle":target}}}),
        );
        fields.push(result["handles"][0].as_u64().unwrap());
    }
    assert_eq!(text(&session, fields[0]), "2.50 m²");
    assert_eq!(text(&session, fields[1]), "10.00 m");
    let before = session.history_steps().0.len();
    let opened = run(
        &mut session,
        json!({"type":"add_wall_openings","centreline":centreline,"openings":[{"at":5,"width":1.2,"kind":"window"}]}),
    );
    assert_eq!(opened["hatches"].as_array().unwrap().len(), 2);
    assert_eq!(session.history_steps().0.len(), before + 1);
    assert_eq!(text(&session, fields[0]), "2.20 m²");
    assert_eq!(text(&session, fields[1]), "10.00 m");
    near(&json!(session.takeoff.items[0].shown), 2.2);
    near(&json!(session.takeoff.items[1].shown), 10.0);
    assert_eq!(json!(session.takeoff.items[0].handles), opened["hatches"]);
    assert_eq!(
        json!(session.takeoff.items[1].handles),
        opened["centrelines"]
    );
    assert!(session.undo());
    assert_eq!(text(&session, fields[0]), "2.50 m²");
    assert_eq!(json!(session.takeoff.items[0].handles), wall["hatches"]);
    assert_eq!(json!(session.takeoff.items[1].handles), wall["centrelines"]);
    assert!(session.redo());
    assert_eq!(text(&session, fields[0]), "2.20 m²");
}

#[test]
fn rebuilding_keeps_constraints_on_surviving_centrelines_through_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("constraints.dxf"), 6);
    let wall = run(
        &mut session,
        json!({"type":"walls","thickness":0.25,"runs":[{"points":[[0,0],[10,0]]}]}),
    );
    let centreline = &wall["centrelines"][0];
    run(
        &mut session,
        json!({"type":"add_constraint","kind":"horizontal","targets":[{"handle":centreline}],"points":[[5,0]]}),
    );
    let before = run(&mut session, json!({"type":"list_constraints"}));
    assert_eq!(before["constraints"].as_array().unwrap().len(), 1);
    let count = session.history_steps().0.len();
    run(
        &mut session,
        json!({"type":"add_wall_openings","centreline":centreline,"openings":[{"at":5,"width":1.2,"kind":"window"}]}),
    );
    assert_eq!(
        run(&mut session, json!({"type":"list_constraints"})),
        before
    );
    assert_eq!(session.history_steps().0.len(), count + 1);
    assert!(session.undo());
    assert_eq!(
        run(&mut session, json!({"type":"list_constraints"})),
        before
    );
    assert!(session.redo());
    assert_eq!(
        run(&mut session, json!({"type":"list_constraints"})),
        before
    );
}

#[test]
fn measuring_one_hatch_follows_its_material_when_another_piece_splits_or_merges() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("hatch-pieces.dxf"), 6);
    let first = run(
        &mut session,
        json!({
            "type":"walls", "thickness":0.25,
            "runs":[{"points":[[0,0],[10,0]]}],
            "openings":[{"run":0,"at":4,"width":1.2,"kind":"door"}]
        }),
    );
    assert_eq!(first["hatches"].as_array().unwrap().len(), 2);
    // The left piece is 3.4 m long; the other is 5.4 m. Select by its independently
    // known area, so the test makes no assumption about boolean polygon ordering.
    let left = first["hatches"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|value| {
            let handle = value.as_u64().unwrap();
            let info = session.entity_info(handle, &[]).unwrap();
            info.area
                .filter(|area| (*area - 0.85).abs() < 1e-7)
                .map(|_| handle)
        })
        .expect("the 3.4 m left wall piece");
    run(&mut session, json!({"type":"measure_entity","handle":left}));
    let field = run(
        &mut session,
        json!({
            "type":"field", "at":[0,2],
            "field":{"kind":"area","target":{"handle":left}}
        }),
    )["handles"][0]
        .as_u64()
        .unwrap();
    near(&json!(session.takeoff.items[0].shown), 0.85);
    assert_eq!(text(&session, field), "0.85 m²");

    let second = run(
        &mut session,
        json!({
            "type":"add_wall_openings", "centreline":first["centrelines"][0],
            "openings":[{"at":8,"width":1,"kind":"window"}]
        }),
    );
    assert_eq!(second["hatches"].as_array().unwrap().len(), 3);
    near(&json!(session.takeoff.items[0].shown), 0.85);
    assert_eq!(session.takeoff.items[0].handles.len(), 1);
    assert_eq!(text(&session, field), "0.85 m²");
    assert_eq!(
        second["handle_map"][left.to_string()]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let after_split = session.takeoff.items[0].handles.clone();

    let closed = run(
        &mut session,
        json!({
            "type":"remove_wall_openings", "inserts":[second["inserts"][0]]
        }),
    );
    assert_eq!(closed["hatches"].as_array().unwrap().len(), 2);
    // Closing the first opening merges the left piece into the 7.5 m piece.
    near(&json!(session.takeoff.items[0].shown), 1.875);
    assert_eq!(session.takeoff.items[0].handles.len(), 1);
    assert_eq!(text(&session, field), "1.88 m²");
    assert!(session.undo());
    near(&json!(session.takeoff.items[0].shown), 0.85);
    assert_eq!(session.takeoff.items[0].handles, after_split);
    assert_eq!(text(&session, field), "0.85 m²");
    assert!(session.redo());
    near(&json!(session.takeoff.items[0].shown), 1.875);
    assert_eq!(text(&session, field), "1.88 m²");
}

/// Undoing joined runs and openings, then opening the remaining runs again, must work and
/// leave the group usable in every unit (written while fixing a live rebuild panic, 24 Oct).
#[test]
fn openings_added_after_undoing_joined_runs_and_openings_rebuild_without_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut s = Session::new_blank(&dir.path().join(format!("undo-{units}.dxf")), units);
        let k = 1.0 / factor;
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[12.0*k,0],[12.0*k,9.0*k],[0,9.0*k]],"closed":true}],"thickness":0.25*k}),
        );
        // A second phase joins partitions, then a batch adds openings; both are undone.
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[6.0*k,0],[6.0*k,9.0*k]]},{"points":[[0,4.5*k],[6.0*k,4.5*k]]}],"thickness":0.15*k}),
        );
        let runs = groups(&mut s, 0.9 * k)[0]["runs"].clone();
        let centreline = |i: usize| runs[i]["centreline"].as_u64().unwrap();
        s.run_input(
            &json!({"type":"batch","commands":[
                {"type":"add_wall_openings","centreline":centreline(0),"openings":[{"at":2.0*k,"width":0.9*k,"kind":"door"}]},
                {"type":"add_wall_openings","centreline":centreline(1),"openings":[{"at":1.5*k,"width":1.2*k,"kind":"window"}]}
            ]}),
            Source::Ai,
        )
        .unwrap();
        assert!(s.undo_if(Source::Ai));
        assert!(s.undo_if(Source::User));
        // The runs now are the first phase's; open them again, as Mizan did.
        let runs = groups(&mut s, 0.9 * k)[0]["runs"].clone();
        for run_ in runs.as_array().unwrap() {
            let c = run_["centreline"].as_u64().unwrap();
            let result = s.run_input(
                &json!({"type":"add_wall_openings","centreline":c,"openings":[{"at":3.0*k,"width":0.9*k,"kind":"door"}]}),
                Source::Ai,
            );
            assert!(result.is_ok(), "{units}: {result:?}");
        }
        assert!(!groups(&mut s, 0.9 * k).as_array().unwrap().is_empty());
        // Redo after the new edit is gone; undo still walks back cleanly.
        while s.undo_if(Source::Ai) {}
    }
}
