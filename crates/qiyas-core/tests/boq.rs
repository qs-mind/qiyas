use qiyas_core::{Boq, BoqItem, Command, Outcome, Session, Source};

/// Opens the sample drawing (DXF) in a session. The temp dir must outlive the test body.
fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

fn item(code: &str, quantity: f64) -> BoqItem {
    BoqItem {
        code: code.to_string(),
        description: format!("Item {code}"),
        unit: "m2".to_string(),
        quantity,
        rate: None,
        section: String::new(),
        tolerance: None,
        row: 2,
    }
}

fn boq(items: Vec<BoqItem>) -> Boq {
    Boq::new("boq.xlsx".to_string(), items, 0)
}

#[test]
fn set_boq_is_undoable() {
    let (_dir, mut session) = sample_session();
    let first = boq(vec![item("A.1", 10.0)]);
    let second = boq(vec![item("B.1", 5.0), item("B.2", 7.0)]);

    session.run(Command::SetBoq { boq: first.clone() }).unwrap();
    // A take-off change in between: undoing it leaves the BOQ alone.
    session
        .run(Command::MeasureDistance {
            points: vec![[0.0, 0.0], [3.0, 0.0]].into(),
        })
        .unwrap();
    session
        .run_as(
            Command::SetBoq {
                boq: second.clone(),
            },
            Source::Ai,
        )
        .unwrap();
    assert_eq!(session.boq.as_ref(), Some(&second));
    assert_eq!(session.top_step_source(), Some(Source::Ai));

    assert!(session.undo());
    assert_eq!(session.boq.as_ref(), Some(&first));
    assert_eq!(session.takeoff.items.len(), 1);

    assert!(session.undo());
    assert_eq!(session.boq.as_ref(), Some(&first));
    assert!(session.takeoff.items.is_empty());

    assert!(session.undo());
    assert_eq!(session.boq, None);
    assert!(!session.can_undo());

    assert!(session.redo() && session.redo() && session.redo());
    assert_eq!(session.boq.as_ref(), Some(&second));
    assert_eq!(session.takeoff.items.len(), 1);
}

#[test]
fn set_boq_tolerance_sets_and_undoes() {
    let (_dir, mut session) = sample_session();
    let tolerance = |code: &str, percent| Command::SetBoqTolerance {
        code: code.to_string(),
        percent,
    };
    // No BOQ yet: nothing to change.
    assert!(matches!(
        session.run(tolerance("A.1", Some(5.0))).unwrap(),
        Outcome::Nothing
    ));
    assert!(!session.can_undo());

    session
        .run(Command::SetBoq {
            boq: boq(vec![item("A.1", 10.0), item("A.2", 4.0)]),
        })
        .unwrap();
    assert!(matches!(
        session.run(tolerance("A.2", Some(2.5))).unwrap(),
        Outcome::Changed
    ));
    let items = &session.boq.as_ref().unwrap().items;
    assert_eq!((items[0].tolerance, items[1].tolerance), (None, Some(2.5)));

    assert!(matches!(
        session.run(tolerance("Z.9", Some(1.0))).unwrap(),
        Outcome::Nothing
    ));
    assert!(session.run(tolerance("A.1", Some(-1.0))).is_err());
    assert!(session.run(tolerance("A.1", Some(f64::NAN))).is_err());

    assert!(session.undo());
    assert_eq!(session.boq.as_ref().unwrap().items[1].tolerance, None);
}
