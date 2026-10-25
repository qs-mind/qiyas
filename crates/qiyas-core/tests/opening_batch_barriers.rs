use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn run(s: &mut Session, value: Value) -> Value {
    json!(s
        .run_input(&value, Source::User)
        .unwrap_or_else(|e| panic!("{value}: {e}")))
}
fn entities(s: &Session) -> Value {
    json!(s
        .drawing
        .doc
        .entities()
        .map(|e| (e.common().handle.value(), json!(e)))
        .collect::<BTreeMap<_, _>>())
}
#[test]
fn mixed_opening_changes_keep_identities_ordered_validity_and_one_undo_step() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("barriers.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let wall = run(
            &mut s,
            json!({"type":"walls","thickness":0.2*k,"runs":[{"points":[[0,0],[12.0*k,0]]}],
            "openings":[{"run":0,"at":2.0*k,"width":0.8*k,"kind":"door"},{"run":0,"at":5.0*k,"width":0.8*k,"kind":"door"}]}),
        );
        let run_id = wall["centrelines"][0].as_u64().unwrap();
        let removed = wall["inserts"][0].as_u64().unwrap();
        let kept = wall["inserts"][1].as_u64().unwrap();
        let before = entities(&s);
        let history = s.history_steps().0.len();
        let commands = vec![
            json!({"type":"edit_wall_opening","insert":kept,"at":6.0*k,"width":1.0*k,"mark":"D-test"}),
            json!({"type":"add_wall_openings","centreline":run_id,"openings":[{"at":9.0*k,"width":0.9*k,"kind":"door"}]}),
            json!({"type":"remove_wall_openings","inserts":[removed]}),
            json!({"type":"text","at":[0,2.0*k],"value":"After openings"}),
        ];
        let mut reference = Session::new_blank(&dir.path().join("sequential.dxf"), units);
        reference.drawing.doc = s.drawing.doc.clone();
        let expected: Vec<_> = commands
            .iter()
            .map(|command| json!(reference.run_input(command, Source::User).unwrap().outcome))
            .collect();
        let batch = s
            .run_input(&json!({"type":"batch","commands":commands}), Source::User)
            .unwrap();
        let qiyas_core::Outcome::Batch { outcomes, .. } = batch.outcome else {
            panic!("batch outcome");
        };
        assert_eq!(
            json!(outcomes),
            json!(expected),
            "intermediate results in {units}"
        );
        assert_eq!(
            entities(&s),
            entities(&reference),
            "allocated handles and records in {units}"
        );
        assert_eq!(
            s.drawing.doc.snapshot_structure(),
            reference.drawing.doc.snapshot_structure()
        );
        let groups = s.wall_groups(None).unwrap();
        let openings = &groups[0].runs[0].openings;
        assert_eq!(openings.len(), 2);
        let kept_opening = openings
            .iter()
            .find(|o| o.insert == kept)
            .expect("edited identity survives removal and addition");
        assert_eq!(kept_opening.opening.at, 6.0 * k);
        assert_eq!(kept_opening.opening.width, 1.0 * k);
        assert_eq!(
            s.object_quantity(kept).unwrap().unwrap().instance.props["mark"],
            json!("D-test")
        );
        assert!(s
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(removed))
            .is_none());
        assert_eq!(s.history_steps().0.len(), history + 1);
        let after = entities(&s);
        assert!(s.undo());
        assert_eq!(entities(&s), before);
        assert!(s.redo());
        assert_eq!(entities(&s), after);
        let structure = s.drawing.doc.snapshot_structure();
        let revision = s.document_revision();
        let error = s
            .run_input(
                &json!({"type":"batch","commands":[
                    {"type":"text","at":[0,3.0*k],"value":"Rolled back"},
                    {"type":"edit_wall_opening","insert":kept,"width":20.0*k},
                    {"type":"edit_wall_opening","insert":kept,"width":0.8*k}
                ]}),
                Source::User,
            )
            .unwrap_err();
        fn step(error: &qiyas_core::CoreError) -> Option<usize> {
            match error {
                qiyas_core::CoreError::Step { index, .. } => Some(*index),
                qiyas_core::CoreError::InputWarnings { error, .. } => step(error),
                _ => None,
            }
        }
        assert_eq!(
            step(&error),
            Some(1),
            "the invalid second step must remain first failure: {error}"
        );
        assert_eq!(entities(&s), after);
        assert_eq!(s.drawing.doc.snapshot_structure(), structure);
        assert_eq!(s.document_revision(), revision);
    }
}

#[test]
fn a_wall_group_read_sees_the_completed_prefix_before_later_edits() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("read-barrier.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let wall = run(
            &mut s,
            json!({"type":"walls","thickness":0.2*k,
            "runs":[{"points":[[0,0],[12.0*k,0]]}],
            "openings":[{"run":0,"at":2.0*k,"width":0.8*k,"kind":"door"}]}),
        );
        let door = wall["inserts"][0].as_u64().unwrap();
        let result = run(
            &mut s,
            json!({"type":"batch","commands":[
                {"type":"edit_wall_opening","insert":door,"at":4.0*k},
                {"type":"edit_wall_opening","insert":door,"width":1.0*k},
                {"type":"wall_groups"},
                {"type":"edit_wall_opening","insert":door,"at":7.0*k}
            ]}),
        );
        let read = &result["outcomes"][2]["groups"][0]["runs"][0]["openings"][0];
        assert_eq!(read["at"], json!(4.0 * k));
        assert_eq!(read["width"], json!(1.0 * k));
        let after = &s.wall_groups(None).unwrap()[0].runs[0].openings[0];
        assert_eq!(after.opening.at, 7.0 * k);
        assert_eq!(after.opening.width, 1.0 * k);
    }
}

#[test]
fn a_literal_handle_created_by_an_earlier_step_is_a_barrier() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("new-handle.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let wall = run(
            &mut s,
            json!({"type":"walls","thickness":0.2*k,
            "runs":[{"points":[[0,0],[12.0*k,0]]}]}),
        );
        let host = wall["centrelines"][0].as_u64().unwrap();
        let add = json!({"type":"add_wall_openings","centreline":host,
            "openings":[{"at":3.0*k,"width":0.8*k,"kind":"door"}]});
        let mut reference = Session::new_blank(&dir.path().join("ordered.dxf"), units);
        reference.drawing.doc = s.drawing.doc.clone();
        let created = run(&mut reference, add.clone())["inserts"][0]
            .as_u64()
            .unwrap();
        let edit = json!({"type":"edit_wall_opening","insert":created,"width":1.0*k});
        run(&mut reference, edit.clone());
        assert!(s
            .run_opening_batch(&[add.clone(), edit.clone()], Source::User)
            .is_none());
        run(&mut s, json!({"type":"batch","commands":[add,edit]}));
        assert_eq!(
            json!(s.wall_groups(None).unwrap()),
            json!(reference.wall_groups(None).unwrap())
        );
        assert_eq!(
            json!(s.object_quantity(created).unwrap()),
            json!(reference.object_quantity(created).unwrap())
        );
    }
}
