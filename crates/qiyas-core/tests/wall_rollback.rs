use opencadcodec::Handle;
use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn snapshot(s: &mut Session) -> Value {
    let entities: BTreeMap<_, _> = s
        .drawing
        .doc
        .entities()
        .map(|e| (e.common().handle.value(), json!(e)))
        .collect();
    json!({"entities":entities,"structure":s.drawing.doc.snapshot_structure(),"walls":s.wall_groups(None).unwrap(),"history":s.history_steps(),"revision":s.document_revision(),"takeoff":s.takeoff})
}

#[test]
fn failed_opening_batches_restore_every_record_in_all_units() {
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("walls.dxf"), units);
        let built = s
            .run_input(
                &json!({"type":"walls","thickness":0.25/factor,"runs":[
                    {"points":[[0,0],[12.0/factor,0]]},
                    {"points":[[6.0/factor,0],[6.0/factor,8.0/factor]],"thickness":0.15/factor}
                ]}),
                Source::Ai,
            )
            .unwrap();
        let host = json!(built.outcome)["centrelines"][0].clone();
        let valid = json!({"type":"add_wall_openings","centreline":host,"openings":[{"at":2.0/factor,"width":0.9/factor,"kind":"door"}]});
        let invalid = json!({"type":"add_wall_openings","centreline":host,"openings":[{"at":6.0/factor,"width":0.9/factor,"kind":"door"}]});
        let before = snapshot(&mut s);
        for case in 0..3 {
            let panic = case == 2;
            if panic {
                qiyas_core::fixtures::panic_on_document_edit(2);
            }
            let commands = if panic {
                vec![
                    valid.clone(),
                    json!({"type":"draw","kind":"line","a":[0,0],"b":[0,1]}),
                ]
            } else if case == 1 {
                // A mixed batch applies the first wall edit before the fitting error.
                vec![
                    valid.clone(),
                    json!({"type":"draw","kind":"line","a":[0,0],"b":[0,1]}),
                    invalid.clone(),
                ]
            } else {
                vec![valid.clone(), invalid.clone()]
            };
            let error = s
                .run_input(&json!({"type":"batch","commands":commands}), Source::Ai)
                .unwrap_err()
                .to_string();
            assert!(
                if panic {
                    error.contains("rolled back")
                } else {
                    error.contains("fits")
                },
                "{error}"
            );
            assert_eq!(snapshot(&mut s), before);
            for group in s.wall_groups(None).unwrap() {
                for h in group.handles {
                    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_some());
                }
            }
        }
        qiyas_core::fixtures::panic_on_document_edit(1);
        let grouped = json!({"type":"batch","commands":[valid.clone(),
            {"type":"add_wall_openings","centreline":host,"openings":[{"at":9.0/factor,"width":0.9/factor,"kind":"window"}]}
        ]});
        assert!(s
            .run_input(&grouped, Source::Ai)
            .unwrap_err()
            .to_string()
            .contains("rolled back"));
        assert_eq!(snapshot(&mut s), before);
        s.run_input(&valid, Source::Ai).unwrap();
        assert_eq!(s.wall_groups(None).unwrap()[0].runs[0].openings.len(), 1);
        assert!(s.undo());
        assert!(s.redo());
        assert_eq!(s.wall_groups(None).unwrap()[0].runs[0].openings.len(), 1);
    }
}

#[test]
fn a_panic_between_commands_aborts_the_open_group() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("panic.dxf"), 6);
    let before = snapshot(&mut s);
    let result: Result<(), qiyas_core::CoreError> =
        s.transaction(Source::Ai, "injected failure", |s| {
            s.run_input(
                &json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
                Source::Ai,
            )?;
            panic!("injected panic between commands");
        });
    assert!(result.unwrap_err().to_string().contains("rolled back"));
    assert_eq!(snapshot(&mut s), before);
    s.run_input(
        &json!({"type":"draw","kind":"line","a":[0,0],"b":[2,0]}),
        Source::Ai,
    )
    .unwrap();
    assert_eq!(s.history_steps().0.len(), 1);
    assert!(s.undo());
    assert!(s.redo());
}
