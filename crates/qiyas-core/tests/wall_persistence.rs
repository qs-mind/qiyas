//! Editable wall definitions travel with the CAD entities through files and copies.

use opencadcodec::Handle;
use qiyas_core::{project_path, Session, Source};
use serde_json::{json, Value};

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn run(session: &mut Session, command: Value) -> Value {
    serde_json::to_value(session.run_input(&command, Source::User).unwrap().outcome).unwrap()
}

fn groups(session: &mut Session) -> Vec<Value> {
    run(session, json!({"type":"wall_groups"}))["groups"]
        .as_array()
        .unwrap()
        .clone()
}

fn draw(session: &mut Session) -> Value {
    let factor = session.drawing.units.to_metres();
    run(
        session,
        json!({
            "type":"walls", "thickness":0.25/factor, "height":3.2,
            "runs":[
                {"points":[[0,0],[6.0/factor,0]]},
                {"points":[[3.0/factor,0],[3.0/factor,3.0/factor]], "thickness":0.15/factor, "height":2.7, "justification":"left"}
            ],
            "openings":[
                {"run":0,"at":1.0/factor,"width":0.9/factor,"kind":"door","swing":"right"},
                {"run":0,"at":4.5/factor,"width":1.2/factor,"kind":"window","sill":0.8,"head":2.2},
                {"run":1,"at":1.0/factor,"width":0.7/factor,"kind":"door"}
            ],
            "layers":{"outline":"A-TEST-WALL","hatch":"A-TEST-FILL","centreline":"A-TEST-CENTRE","doors":"A-TEST-DOOR","windows":"A-TEST-WINDOW"}
        }),
    )
}

fn targets(group: &Value) -> Value {
    Value::Array(
        group["handles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|handle| json!({"handle":handle}))
            .collect(),
    )
}

/// Definitions are independent of the handles assigned by the file reader.
fn definition(group: &Value) -> Value {
    let mut runs = group["runs"].clone();
    for run in runs.as_array_mut().unwrap() {
        run.as_object_mut().unwrap().remove("centreline");
        for opening in run["openings"].as_array_mut().unwrap() {
            opening.as_object_mut().unwrap().remove("insert");
        }
    }
    json!({"layers":group["layers"],"runs":runs})
}

fn add_window(session: &mut Session, group: &Value) -> Value {
    let factor = session.drawing.units.to_metres();
    run(
        session,
        json!({
            "type":"add_wall_openings", "centreline":group["runs"][1]["centreline"],
            "openings":[{"at":2.2/factor,"width":0.6/factor,"kind":"window"}]
        }),
    )
}

#[test]
fn dwg_and_dxf_keep_complete_definitions_without_the_project_file_and_can_be_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    for extension in ["dwg", "dxf"] {
        let mut session = Session::new_blank(&dir.path().join("source.dxf"), 4);
        let original = draw(&mut session);
        let before = groups(&mut session).remove(0);
        let path = dir.path().join(format!("wall-system.{extension}"));
        session.save_as(&path).unwrap();
        std::fs::remove_file(project_path(&path)).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        let saved = groups(&mut reopened);
        assert_eq!(saved.len(), 1, "{extension}");
        assert_eq!(definition(&saved[0]), definition(&before), "{extension}");
        for run in saved[0]["runs"].as_array().unwrap() {
            assert!(reopened
                .drawing
                .doc
                .get_entity(Handle::new(run["centreline"].as_u64().unwrap()))
                .is_some());
            for opening in run["openings"].as_array().unwrap() {
                assert!(reopened
                    .drawing
                    .doc
                    .get_entity(Handle::new(opening["insert"].as_u64().unwrap()))
                    .is_some());
            }
        }
        let revision = reopened.history_steps().0.len();
        let rebuilt = add_window(&mut reopened, &saved[0]);
        near(
            rebuilt["net_area"].as_f64().unwrap(),
            original["net_area"].as_f64().unwrap() - 0.6 * 0.15,
        );
        assert_eq!(reopened.history_steps().0.len(), revision + 1);
        let after = groups(&mut reopened);
        assert_eq!(after[0]["runs"][1]["openings"].as_array().unwrap().len(), 2);
        reopened.undo();
        assert_eq!(groups(&mut reopened), saved);
        reopened.redo();
        assert_eq!(groups(&mut reopened), after);

        // A second format trip must preserve the rebuilt definition too.
        let second = dir.path().join(format!("rebuilt.{extension}"));
        reopened.save_as(&second).unwrap();
        std::fs::remove_file(project_path(&second)).unwrap();
        let mut reloaded = Session::open(&second).unwrap();
        assert_eq!(definition(&groups(&mut reloaded)[0]), definition(&after[0]));
    }
}

#[test]
fn complete_wall_copies_are_independent_translated_editable_systems() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("copies.dxf"), 6);
    let original = draw(&mut session);
    let source = groups(&mut session).remove(0);
    let before = session.history_steps().0.len();
    run(
        &mut session,
        json!({"type":"modify","kind":"copy","targets":targets(&source),"delta":[0,10],"count":2}),
    );
    assert_eq!(session.history_steps().0.len(), before + 1);
    let copied = groups(&mut session);
    assert_eq!(copied.len(), 3);
    assert_eq!(
        copied
            .iter()
            .find(|group| group["group"] == source["group"]),
        Some(&source)
    );
    let mut identities = std::collections::BTreeSet::new();
    for group in &copied {
        assert!(identities.insert(group["group"].as_u64().unwrap()));
    }
    let translated = copied
        .iter()
        .find(|group| group["runs"][0]["points"][0][1] == 10.0)
        .unwrap();
    let other_copy = copied
        .iter()
        .find(|group| group["runs"][0]["points"][0][1] == 20.0)
        .unwrap();
    for (copy, y) in [(translated, 10.0), (other_copy, 20.0)] {
        near(copy["runs"][0]["length"].as_f64().unwrap(), 6.0);
        near(copy["runs"][1]["points"][1][1].as_f64().unwrap(), y + 3.0);
        assert_eq!(copy["runs"][1]["justification"], "left");
        assert_eq!(copy["runs"][0]["openings"][0]["swing"], "right");
        assert!(copy["handles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|handle| !source["handles"].as_array().unwrap().contains(handle)));
    }
    let rebuilt = add_window(&mut session, translated);
    near(
        rebuilt["net_area"].as_f64().unwrap(),
        original["net_area"].as_f64().unwrap() - 0.6 * 0.15,
    );
    let after = groups(&mut session);
    assert_eq!(after.len(), 3);
    assert!(after.contains(&source), "editing a copy changed its source");
    assert!(
        after.contains(other_copy),
        "editing a copy changed another copy"
    );
    session.undo();
    assert_eq!(groups(&mut session), copied);
    session.undo();
    assert_eq!(groups(&mut session), [source]);
    session.redo();
    assert_eq!(groups(&mut session), copied);
}

#[test]
fn clipboard_wall_definitions_convert_drawing_units_but_keep_vertical_sizes_in_metres() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = Session::new_blank(&dir.path().join("source-mm.dxf"), 4);
    let original = draw(&mut source);
    let source_group = groups(&mut source).remove(0);
    let copied = run(
        &mut source,
        json!({"type":"copy_to_clipboard","targets":targets(&source_group),"base":[0,0]}),
    );
    let text = copied["text"].as_str().unwrap();
    for units in [5, 6, 1] {
        let mut destination =
            Session::new_blank(&dir.path().join(format!("pasted-{units}.dxf")), units);
        let factor = destination.drawing.units.to_metres();
        run(
            &mut destination,
            json!({"type":"paste_from_clipboard","text":text,"at":[20.0/factor,10.0/factor],"as_block":false}),
        );
        assert_eq!(destination.history_steps().0.len(), 1);
        let pasted = groups(&mut destination);
        assert_eq!(pasted.len(), 1);
        let group = &pasted[0];
        let outer = &group["runs"][0];
        let inner = &group["runs"][1];
        near(outer["points"][0][0].as_f64().unwrap() * factor, 20.0);
        near(outer["points"][0][1].as_f64().unwrap() * factor, 10.0);
        near(outer["length"].as_f64().unwrap() * factor, 6.0);
        near(outer["thickness"].as_f64().unwrap() * factor, 0.25);
        near(outer["height"].as_f64().unwrap(), 3.2);
        near(inner["length"].as_f64().unwrap() * factor, 3.0);
        near(inner["thickness"].as_f64().unwrap() * factor, 0.15);
        near(inner["height"].as_f64().unwrap(), 2.7);
        near(outer["openings"][0]["at"].as_f64().unwrap() * factor, 1.0);
        near(
            outer["openings"][0]["width"].as_f64().unwrap() * factor,
            0.9,
        );
        near(outer["openings"][1]["sill"].as_f64().unwrap(), 0.8);
        near(outer["openings"][1]["head"].as_f64().unwrap(), 2.2);
        assert_eq!(group["layers"], source_group["layers"]);
        let rebuilt = add_window(&mut destination, group);
        near(
            rebuilt["net_area"].as_f64().unwrap(),
            original["net_area"].as_f64().unwrap() - 0.6 * 0.15,
        );
        assert_eq!(destination.history_steps().0.len(), 2);
        assert_eq!(
            groups(&mut destination)[0]["runs"][1]["openings"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        destination.undo();
        assert_eq!(groups(&mut destination), pasted);
        destination.undo();
        assert!(groups(&mut destination).is_empty());
        assert_eq!(
            groups(&mut source).as_slice(),
            std::slice::from_ref(&source_group)
        );
    }
}
