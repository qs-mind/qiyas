//! `Command::Batch`: drawing commands as one undo step, with the guards `run_as` gives each.

use opencadcodec::EntityType;
use qiyas_core::{Command, Session, Target};

fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    (dir, Session::open(&path).unwrap())
}

fn insert_door() -> Command {
    Command::InsertBlock {
        name: "DOOR".into(),
        at: [0.0, 0.0],
        scale: 1.0,
        rotation: 0.0,
        attribute_values: Default::default(),
        from_path: None,
    }
}

#[test]
fn a_batch_obeys_the_block_edit_guards_for_each_command() {
    let (_dir, mut s) = session();
    let insert = s
        .drawing
        .entities_in("*Model_Space")
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle
        .value();
    s.run(Command::EditBlockInPlace {
        target: Target {
            handle: insert,
            chain: vec![],
        },
    })
    .unwrap();
    let before = s.drawing.entities_in("*Model_Space").count();
    // The block being edited can't be put into itself, alone or in a batch.
    assert!(s.run(insert_door()).is_err());
    let err = s
        .run(Command::Batch {
            commands: vec![insert_door()],
            layer: None,
            color: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("DOOR"), "{err}");
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), before);
    s.run(Command::SaveBlockEdits).unwrap();
    assert!(
        !s.drawing
            .entities_in("DOOR")
            .any(|e| matches!(e, EntityType::Insert(i) if i.block_name == "DOOR")),
        "DOOR must not hold a reference to DOOR"
    );
}

#[test]
fn a_failed_batch_puts_back_the_layer_and_the_current_state_and_undo_restores_them() {
    let (_dir, mut s) = session();
    let current = s.current.clone();
    let mut other = current.clone();
    other.layer = "ZZ".into();
    let failed = s.run(Command::Batch {
        commands: vec![
            Command::NewLayer { name: "ZZ".into() },
            Command::SetCurrent { current: other },
            Command::InsertBlock {
                name: "NOPE".into(),
                at: [0.0, 0.0],
                scale: 1.0,
                rotation: 0.0,
                attribute_values: Default::default(),
                from_path: None,
            },
        ],
        layer: None,
        color: None,
    });
    assert!(failed.is_err());
    assert_eq!(s.current, current);
    assert!(s.drawing.doc.layers.get("ZZ").is_none());

    let mut other = current.clone();
    other.layer = "ZY".into();
    s.run(Command::Batch {
        commands: vec![
            Command::NewLayer { name: "ZY".into() },
            Command::SetCurrent { current: other },
        ],
        layer: None,
        color: None,
    })
    .unwrap();
    assert_eq!(s.current.layer, "ZY");
    assert!(s.undo());
    assert_eq!(s.current, current);
    assert!(s.drawing.doc.layers.get("ZY").is_none());
    assert!(s.redo());
    assert_eq!(s.current.layer, "ZY");
}

// ---- Transactions: any commands, one undo step, all or nothing ----

use qiyas_core::{CoreError, DrawSpec, Outcome, Source};

fn line(a: [f64; 2], b: [f64; 2]) -> Command {
    Command::Draw(DrawSpec::Line { a, b })
}

fn batch(commands: Vec<Command>) -> Command {
    Command::Batch {
        commands,
        layer: None,
        color: None,
    }
}

fn model_count(s: &Session) -> usize {
    s.drawing.entities_in("*Model_Space").count()
}

#[test]
fn a_failing_step_puts_back_the_drawing_take_off_layers_and_view() {
    let (_dir, mut s) = session();
    let entities = model_count(&s);
    let rows = s.takeoff.items.len();
    let current = s.current.clone();
    let sheet = s.sheet.clone();
    let layout = s.drawing.sheets().into_iter().find(|sh| !sh.is_model);
    let undo_before = s.history_steps().0.len();
    let mut on_x = current.clone();
    on_x.layer = "X".into();
    let mut commands = vec![
        Command::NewLayer { name: "X".into() },
        Command::SetCurrent { current: on_x },
        line([0.0, 0.0], [5.0, 0.0]),
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [5.0, 0.0]],
        },
        Command::SetLayerHidden {
            name: "0".into(),
            hidden: true,
        },
    ];
    if let Some(layout) = &layout {
        commands.push(Command::SetSheet {
            block: layout.block.clone(),
        });
    }
    commands.push(Command::DeleteLayer {
        name: "NO-SUCH-LAYER".into(),
    });
    let failing = commands.len() - 1;
    let err = s.run(batch(commands)).unwrap_err();
    match &err {
        CoreError::Step { index, label, .. } => {
            assert_eq!(*index, failing);
            assert_eq!(label, "delete_layer");
        }
        other => panic!("{other:?}"),
    }
    assert!(err
        .to_string()
        .starts_with(&format!("Batch step {failing} (delete_layer)")));
    assert_eq!(model_count(&s), entities);
    assert_eq!(s.takeoff.items.len(), rows);
    assert!(s.drawing.doc.layers.get("X").is_none());
    assert_eq!(s.current, current);
    assert_eq!(s.sheet, sheet);
    assert!(!s.visibility.hidden_layers.contains("0"));
    assert_eq!(s.history_steps().0.len(), undo_before, "no step left");
    assert!(!s.is_dirty(), "nothing to save");
}

#[test]
fn a_batch_is_one_undo_step_undone_last_first_and_redone_in_order() {
    let (_dir, mut s) = session();
    let entities = model_count(&s);
    let rows = s.takeoff.items.len();
    let outcome = s
        .run(batch(vec![
            Command::NewLayer { name: "X".into() },
            line([0.0, 0.0], [5.0, 0.0]),
            Command::MeasureArea {
                points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
            },
            Command::RenameLayer {
                from: "X".into(),
                to: "Y".into(),
            },
        ]))
        .unwrap();
    let Outcome::Batch { outcomes, added } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(outcomes.len(), 4);
    assert_eq!(added.len(), 1);
    assert_eq!(s.history_steps().0.last().unwrap().label, "batch");
    assert!(s.drawing.doc.layers.get("Y").is_some());

    assert!(s.undo());
    assert_eq!(model_count(&s), entities);
    assert_eq!(s.takeoff.items.len(), rows);
    assert!(s.drawing.doc.layers.get("X").is_none());
    assert!(s.drawing.doc.layers.get("Y").is_none());

    assert!(s.redo());
    assert_eq!(model_count(&s), entities + 1);
    assert_eq!(s.takeoff.items.len(), rows + 1);
    assert!(s.drawing.doc.layers.get("X").is_none());
    assert!(s.drawing.doc.layers.get("Y").is_some());
}

#[test]
fn commands_that_cannot_batch_are_refused_and_nothing_runs() {
    let (_dir, mut s) = session();
    let entities = model_count(&s);
    let err = s
        .run(batch(vec![
            line([0.0, 0.0], [5.0, 0.0]),
            Command::WriteBlock {
                name: "DOOR".into(),
                path: "door.dwg".into(),
            },
        ]))
        .unwrap_err();
    assert!(
        err.to_string().contains("can't run inside a batch"),
        "{err}"
    );
    assert_eq!(model_count(&s), entities);
    let nested = s.run(batch(vec![batch(vec![line([0.0, 0.0], [1.0, 0.0])])]));
    assert!(nested.unwrap_err().to_string().contains("another batch"));
    assert_eq!(model_count(&s), entities);
}

#[test]
fn a_batch_on_a_layer_makes_it_draws_on_it_and_puts_the_current_state_back() {
    let (_dir, mut s) = session();
    let current = s.current.clone();
    let outcome = s
        .run(Command::Batch {
            commands: vec![line([0.0, 0.0], [5.0, 0.0])],
            layer: Some("Q-NEW".into()),
            color: Some("ACI 3".into()),
        })
        .unwrap();
    let Outcome::Batch { added, .. } = outcome else {
        panic!()
    };
    let drawn = s
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(added[0]))
        .unwrap();
    assert_eq!(drawn.common().layer, "Q-NEW");
    assert_eq!(s.current, current, "put back after");
    assert!(s.undo());
    assert!(s.drawing.doc.layers.get("Q-NEW").is_none());
    assert_eq!(s.current, current);
}

#[test]
fn last_change_lists_what_a_command_or_transaction_did() {
    let (_dir, mut s) = session();
    s.run(line([0.0, 0.0], [5.0, 0.0])).unwrap();
    let h = s.last_change().added[0];
    assert!(s.last_change().changed.is_empty());
    s.run(Command::Modify(qiyas_core::ModifySpec::Move {
        targets: vec![Target {
            handle: h,
            chain: vec![],
        }],
        delta: [1.0, 0.0],
    }))
    .unwrap();
    assert_eq!(s.last_change().changed, vec![h]);
    assert!(s.last_change().added.is_empty());

    // Inside a transaction a later command can use what an earlier one added; drawn and erased
    // again, the line is in no list, and no step is left.
    let steps = s.history_steps().0.len();
    s.transaction(Source::Ai, "scratch", |s| {
        s.run_as(line([0.0, 0.0], [2.0, 0.0]), Source::Ai)?;
        let tmp = *s.last_change().added.last().unwrap();
        s.run_as(
            Command::DeleteEntities {
                targets: vec![Target {
                    handle: tmp,
                    chain: vec![],
                }],
            },
            Source::Ai,
        )?;
        Ok(())
    })
    .unwrap();
    assert!(s.last_change().is_empty(), "{:?}", s.last_change());
    // Two steps that cancel out still make one step (each put back the drawing as it was).
    assert!(s.history_steps().0.len() <= steps + 1);
}

#[test]
fn a_failed_inner_transaction_leaves_the_outer_one_going() {
    let (_dir, mut s) = session();
    let entities = model_count(&s);
    s.transaction(Source::User, "outer", |s| {
        s.run(line([0.0, 0.0], [1.0, 0.0]))?;
        let inner = s.transaction(Source::User, "inner", |s| {
            s.run(line([0.0, 1.0], [1.0, 1.0]))?;
            s.run(Command::DeleteLayer {
                name: "NO-SUCH-LAYER".into(),
            })
        });
        assert!(inner.is_err());
        s.run(line([0.0, 2.0], [1.0, 2.0]))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(model_count(&s), entities + 2);
    assert_eq!(s.last_change().added.len(), 2);
    assert_eq!(s.history_steps().0.last().unwrap().label, "outer");
    assert!(s.undo());
    assert_eq!(model_count(&s), entities);
}

#[test]
fn the_history_names_each_step() {
    let (_dir, mut s) = session();
    s.run(line([0.0, 0.0], [5.0, 0.0])).unwrap();
    s.run(Command::NewLayer { name: "Q".into() }).unwrap();
    let (undo, redo) = s.history_steps();
    let labels: Vec<&str> = undo.iter().map(|h| h.label.as_str()).collect();
    assert_eq!(labels[labels.len() - 2..], ["draw.line", "new_layer"]);
    assert!(redo.is_empty());
    s.undo();
    assert_eq!(s.history_steps().1[0].label, "new_layer");
}

#[test]
fn batch_with_runs_a_closure_on_its_layer_and_restores_failed_work() {
    let (_dir, mut s) = session();
    let entities = model_count(&s);
    let current = s.current.clone();
    let revision = s.document_revision();
    let history = s.history_revision();
    let failed: Result<(), CoreError> = s.batch_with(
        Source::Ai,
        "draft",
        Some("Q-TEMP".into()),
        Some("ACI 3".into()),
        |s| {
            assert_eq!(s.current.layer, "Q-TEMP");
            assert_eq!(s.current.color, "ACI 3");
            s.run_as(line([0.0, 0.0], [3.0, 0.0]), Source::Ai)?;
            Err(CoreError::InvalidCommand("stop here".into()))
        },
    );
    assert!(failed.is_err());
    assert_eq!(model_count(&s), entities);
    assert_eq!(s.current, current);
    assert!(s.drawing.doc.layers.get("Q-TEMP").is_none());
    assert_eq!(s.document_revision(), revision);
    assert_eq!(s.history_revision(), history);

    let handle = s
        .batch_with(
            Source::Ai,
            "draft",
            Some("Q-TEMP".into()),
            Some("ACI 3".into()),
            |s| {
                s.run_as(line([0.0, 0.0], [3.0, 0.0]), Source::Ai)?;
                Ok(s.last_change().added[0])
            },
        )
        .unwrap();
    assert_eq!(
        s.drawing
            .doc
            .get_entity(opencadcodec::Handle::new(handle))
            .unwrap()
            .common()
            .layer,
        "Q-TEMP"
    );
    assert_eq!(s.current, current);
    assert_eq!(s.history_revision(), history + 1);
    let step = s.history_steps().0.pop().unwrap();
    assert_eq!(step.label, "draft");
    assert_eq!(step.source, Source::Ai);
    assert!(s.undo_if(Source::Ai));
    assert_eq!(model_count(&s), entities);
    assert!(s.drawing.doc.layers.get("Q-TEMP").is_none());
}

#[test]
fn history_revision_distinguishes_no_op_from_a_step_after_the_history_is_full() {
    let (_dir, mut s) = session();
    let was_locked = s.drawing.doc.layers.get("0").unwrap().flags.locked;
    let revision = s.history_revision();
    for index in 0..201 {
        s.run_as(
            Command::SetLayer {
                name: "0".into(),
                prop: qiyas_core::LayerProp::Locked,
                value: serde_json::json!(if index % 2 == 0 {
                    !was_locked
                } else {
                    was_locked
                }),
            },
            Source::User,
        )
        .unwrap();
        assert_eq!(s.history_revision(), revision + index + 1);
    }
    assert_eq!(s.history_steps().0.len(), 200);
    let revision = s.history_revision();
    for _ in 0..2 {
        s.run_as(
            Command::SetLayer {
                name: "0".into(),
                prop: qiyas_core::LayerProp::Locked,
                value: serde_json::json!(!was_locked),
            },
            Source::Ai,
        )
        .unwrap();
        assert_eq!(s.history_revision(), revision);
        assert_eq!(s.history_steps().0.last().unwrap().source, Source::User);
    }
    s.run_as(
        Command::SetLayer {
            name: "0".into(),
            prop: qiyas_core::LayerProp::Locked,
            value: serde_json::json!(was_locked),
        },
        Source::Ai,
    )
    .unwrap();
    assert_eq!(s.history_steps().0.len(), 200);
    assert_eq!(s.history_revision(), revision + 1);
    assert_eq!(s.history_steps().0.last().unwrap().source, Source::Ai);
}
