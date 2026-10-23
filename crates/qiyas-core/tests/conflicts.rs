use qiyas_core::{
    Command, Conflict, ConflictKind, ConflictStatus, Outcome, Schedule, Session, Source,
};

/// Opens the sample drawing (DXF) in a session. The temp dir must outlive the test body.
fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

fn conflict(id: &str) -> Conflict {
    Conflict {
        query: None,
        id: id.to_string(),
        kind: ConflictKind::QuantityMismatch,
        boq_code: Some("A.1".to_string()),
        measurements: vec!["m1".to_string()],
        schedule: None,
        sheets: vec!["Model".to_string()],
        expected: Some(120.0),
        found: Some(98.4),
        unit: "m2".to_string(),
        reason: "BOQ says 120.000 m2, drawings measure 98.400 m2 (−18.0 %)".to_string(),
        reason_ar: "جدول الكميات يذكر 120.000 م²، والمقاس من الرسومات 98.400 م² (−18.0 %)"
            .to_string(),
        status: ConflictStatus::Open,
        note: String::new(),
        created: "2026-10-09T00:00:00Z".to_string(),
        hash_key: "quantity_mismatch|A.1|m1||Model|".to_string(),
    }
}

fn schedule() -> Schedule {
    Schedule {
        name: "DOOR SCHEDULE".to_string(),
        sheet: "*Model_Space".to_string(),
        source: 0,
        columns: vec!["MARK".to_string(), "QTY".to_string()],
        rows: vec![vec!["D1".to_string(), "2".to_string()]],
    }
}

fn resolve(id: &str, status: ConflictStatus, note: &str) -> Command {
    Command::ResolveConflict {
        id: id.to_string(),
        status,
        note: note.to_string(),
    }
}

#[test]
fn set_conflicts_and_resolve_are_undoable() {
    let (_dir, mut session) = sample_session();
    let set = Command::SetConflicts {
        conflicts: vec![conflict("c1"), conflict("c2")],
        schedules: vec![schedule()],
    };
    session.run_as(set, Source::Ai).unwrap();
    assert_eq!(session.conflicts.len(), 2);
    assert_eq!(session.schedules, [schedule()]);

    let outcome = session
        .run(resolve(
            "c2",
            ConflictStatus::Ignored,
            "Shown on the roof sheet",
        ))
        .unwrap();
    assert!(matches!(outcome, Outcome::Changed));
    assert_eq!(session.conflicts[1].status, ConflictStatus::Ignored);
    assert_eq!(session.conflicts[1].note, "Shown on the roof sheet");
    assert_eq!(session.top_step_source(), Some(Source::User));

    // An unknown id changes nothing and is no undo step.
    let outcome = session
        .run(resolve("nope", ConflictStatus::Resolved, ""))
        .unwrap();
    assert!(matches!(outcome, Outcome::Nothing));

    assert!(session.undo());
    assert_eq!(session.conflicts[1].status, ConflictStatus::Open);
    assert_eq!(session.conflicts[1].note, "");
    assert!(session.undo());
    assert!(session.conflicts.is_empty() && session.schedules.is_empty());
    assert!(!session.can_undo());

    assert!(session.redo() && session.redo());
    assert_eq!(session.conflicts[1].status, ConflictStatus::Ignored);
    assert_eq!(session.schedules.len(), 1);
}

#[test]
fn conflicts_and_schedules_are_kept_in_the_project_file() {
    let (dir, mut session) = sample_session();
    let set = Command::SetConflicts {
        conflicts: vec![conflict("c1")],
        schedules: vec![schedule()],
    };
    session.run(set).unwrap();
    session
        .run(resolve("c1", ConflictStatus::Resolved, "Checked on site"))
        .unwrap();
    session.save_project().unwrap();

    let reopened = Session::open(&dir.path().join("sample.dxf")).unwrap();
    assert_eq!(reopened.conflicts, session.conflicts);
    assert_eq!(reopened.schedules, session.schedules);
    let text = std::fs::read_to_string(dir.path().join("sample.qiyas")).unwrap();
    assert!(text.contains("\"kind\": \"quantity_mismatch\""), "{text}");
    assert!(text.contains("\"status\": \"resolved\""), "{text}");
}
