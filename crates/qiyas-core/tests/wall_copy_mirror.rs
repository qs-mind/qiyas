//! Mirrored wall copies keep door hinges and opening sides when their group is rebuilt.

use opencadcodec::{EntityType, Handle};
use qiyas_core::{Session, Source};
use qiyas_geom::{insert_instances, Xform};
use serde_json::{json, Value};

fn run(session: &mut Session, command: Value) -> Value {
    serde_json::to_value(session.run_input(&command, Source::User).unwrap().outcome).unwrap()
}

fn door_points(session: &Session, handle: u64, width: f64) -> [[f64; 2]; 4] {
    let doc = &session.drawing.doc;
    let EntityType::Insert(insert) = doc.get_entity(Handle::new(handle)).unwrap() else {
        panic!("a door insert");
    };
    let identity = Xform::identity();
    let transform = insert_instances(doc, insert, &identity).next().unwrap();
    // Hinge, open leaf tip, closed leaf tip and the middle of its swing arc.
    let diagonal = width / 2.0_f64.sqrt();
    [[0.0, 0.0], [0.0, width], [width, 0.0], [diagonal, diagonal]]
        .map(|point| transform.apply(point))
}

fn same_points(actual: [[f64; 2]; 4], expected: [[f64; 2]; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual[0] - expected[0]).hypot(actual[1] - expected[1]) < 1e-8,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn mirrored_open_and_closed_wall_systems_preserve_door_geometry_after_adding_an_opening() {
    let dir = tempfile::tempdir().unwrap();
    for closed in [false, true] {
        for swing in ["left", "right"] {
            let mut session = Session::new_blank(&dir.path().join("mirror.dxf"), 6);
            let points = if closed {
                json!([[0, 0], [6, 0], [6, 4], [0, 4]])
            } else {
                json!([[0, 0], [6, 0]])
            };
            let original = run(
                &mut session,
                json!({
                    "type":"walls", "thickness":0.25, "justification":"left",
                    "runs":[{"points":points,"closed":closed}],
                    "openings":[{"run":0,"at":2,"width":0.9,"kind":"door","swing":swing}]
                }),
            );
            let original_insert = original["inserts"][0].as_u64().unwrap();
            let source_points = door_points(&session, original_insert, 0.9);
            let source = run(&mut session, json!({"type":"wall_groups"}))["groups"][0].clone();
            let targets: Vec<_> = source["handles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|handle| json!({"handle":handle}))
                .collect();
            run(
                &mut session,
                json!({
                    "type":"modify", "kind":"mirror", "targets":targets,
                    "p1":[0,10], "p2":[1,10], "keep":true
                }),
            );
            let groups =
                run(&mut session, json!({"type":"wall_groups","width":0.6}))["groups"].clone();
            assert_eq!(groups.as_array().unwrap().len(), 2);
            let copied = groups
                .as_array()
                .unwrap()
                .iter()
                .find(|group| group["group"] != source["group"])
                .unwrap();
            let copied_run = &copied["runs"][0];
            assert_eq!(copied_run["justification"], "left");
            let old_insert = copied_run["openings"][0]["insert"].as_u64().unwrap();
            let before = door_points(&session, old_insert, 0.9);
            same_points(before, source_points.map(|[x, y]| [x, 20.0 - y]));
            if closed {
                assert_eq!(
                    copied_run["points"][0],
                    json!([0.0, 20.0]),
                    "reversing a closed run preserves its start point"
                );
                assert_eq!(copied_run["openings"][0]["at"], 18.0);
            } else {
                assert_eq!(copied_run["openings"][0]["at"], 4.0);
            }
            let range = &copied_run["free_ranges"][0];
            let at = (range[0].as_f64().unwrap() + range[1].as_f64().unwrap()) / 2.0;
            let rebuilt = run(
                &mut session,
                json!({
                    "type":"add_wall_openings", "centreline":copied_run["centreline"],
                    "openings":[{"at":at,"width":0.6,"kind":"window"}]
                }),
            );
            let new_insert = rebuilt["inserts"][0].as_u64().unwrap();
            same_points(door_points(&session, new_insert, 0.9), before);
            same_points(door_points(&session, original_insert, 0.9), source_points);
            assert_eq!(
                rebuilt["handle_map"][old_insert.to_string()],
                json!([new_insert])
            );
            assert_eq!(session.history_steps().0.len(), 3);
            assert!(session.undo());
            same_points(door_points(&session, old_insert, 0.9), before);
            assert!(session.redo());
            same_points(door_points(&session, new_insert, 0.9), before);
        }
    }
}
