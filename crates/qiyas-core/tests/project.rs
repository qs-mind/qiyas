use std::path::{Path, PathBuf};

use qiyas_core::{project_path, Command, Kind, Outcome, Project, Session};

/// Writes the sample drawing (DXF, metres) into a fresh temp dir. The dir must outlive the test body.
fn sample() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    (dir, path)
}

fn distance() -> Command {
    Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 0.0]].into(),
    }
}

#[test]
fn project_round_trip() {
    let (_dir, drawing) = sample();
    let mut session = Session::open(&drawing).unwrap();
    session.run(distance()).unwrap();
    session
        .run(Command::MeasureArea {
            points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]].into(),
        })
        .unwrap();
    let ids: Vec<_> = session.takeoff.items.iter().map(|m| m.id.clone()).collect();

    let saved = session.save_project().unwrap();
    assert_eq!(saved, project_path(&drawing));
    assert!(!saved.with_extension("qiyas.tmp").exists());

    let reopened = Session::open(&drawing).unwrap();
    let reloaded: Vec<_> = reopened
        .takeoff
        .items
        .iter()
        .map(|m| m.id.clone())
        .collect();
    assert_eq!(reloaded, ids);
    assert!(
        reopened.drawing.problems.is_empty(),
        "{:?}",
        reopened.drawing.problems
    );
    // Loading is not an edit: there is nothing to undo.
    assert!(!reopened.can_undo());
}

#[test]
fn project_path_rule() {
    assert_eq!(
        project_path(Path::new("/x/villa.DWG")),
        PathBuf::from("/x/villa.qiyas")
    );
}

#[test]
fn project_file_is_pretty_json_with_the_drawing_name() {
    let (_dir, drawing) = sample();
    let mut session = Session::open(&drawing).unwrap();
    session.run(distance()).unwrap();
    let text = std::fs::read_to_string(session.save_project().unwrap()).unwrap();
    assert!(text.contains("\n  \"version\": 2"), "{text}");
    let project: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(project.version, 2);
    assert_eq!(project.drawing, "sample.dxf");
    assert_eq!(project.takeoff.items.len(), 1);
}

#[test]
fn corrupt_project_is_backed_up_and_opens_with_a_problem_and_an_empty_takeoff() {
    let (_dir, drawing) = sample();
    let corrupt = b"{ not json";
    std::fs::write(project_path(&drawing), corrupt).unwrap();
    let session = Session::open(&drawing).unwrap();
    assert!(session.takeoff.items.is_empty());
    let backup = drawing.with_file_name("sample.qiyas.bak");
    assert_eq!(std::fs::read(&backup).unwrap(), corrupt);
    assert!(
        session
            .drawing
            .problems
            .iter()
            .any(|p| p
                .starts_with("Project file could not be read (backed up to sample.qiyas.bak): ")),
        "{:?}",
        session.drawing.problems
    );
}

#[test]
fn project_for_another_drawing_still_loads_with_a_problem() {
    let (_dir, drawing) = sample();
    let mut session = Session::open(&drawing).unwrap();
    session.run(distance()).unwrap();
    let path = session.save_project().unwrap();
    let mut project: Project =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    project.drawing = "other.dwg".to_string();
    std::fs::write(&path, serde_json::to_string(&project).unwrap()).unwrap();

    let session = Session::open(&drawing).unwrap();
    assert_eq!(session.takeoff.items.len(), 1);
    assert!(
        session
            .drawing
            .problems
            .contains(&"Project file was saved for other.dwg".to_string()),
        "{:?}",
        session.drawing.problems
    );
}

#[test]
fn drawing_name_matches_ignoring_case() {
    let (_dir, drawing) = sample();
    let session = Session::open(&drawing).unwrap();
    let path = session.save_project().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("sample.dxf", "SAMPLE.DXF")).unwrap();

    let session = Session::open(&drawing).unwrap();
    assert!(
        session.drawing.problems.is_empty(),
        "{:?}",
        session.drawing.problems
    );
}

#[test]
fn link_boq_round_trips_in_project() {
    let (_dir, drawing) = sample();
    let mut session = Session::open(&drawing).unwrap();
    let Outcome::Measured { measurement } = session
        .run(Command::MeasureArea {
            points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]].into(),
        })
        .unwrap()
    else {
        panic!("expected a measurement")
    };
    let id = measurement.id;
    for cmd in [
        Command::LinkBoq {
            id: id.clone(),
            code: Some("A.1".to_string()),
        },
        Command::SetNote {
            id: id.clone(),
            text: "Check the slab edge".to_string(),
        },
        Command::SetHeight {
            id: id.clone(),
            height: 0.2,
        },
    ] {
        assert!(matches!(session.run(cmd).unwrap(), Outcome::Changed));
    }
    let gone = Command::LinkBoq {
        id: "nope".to_string(),
        code: None,
    };
    assert!(matches!(session.run(gone).unwrap(), Outcome::Nothing));
    session.save_project().unwrap();

    let reopened = Session::open(&drawing).unwrap();
    let m = reopened.takeoff.get(&id).unwrap();
    assert_eq!(m.boq_code.as_deref(), Some("A.1"));
    assert_eq!(m.note, "Check the slab edge");
    assert_eq!((m.kind, m.height), (Kind::Volume, Some(0.2)));

    // Linking and notes are undoable; unlinking clears the code.
    assert!(session.undo() && session.undo());
    assert_eq!(session.takeoff.get(&id).unwrap().note, "");
    assert_eq!(
        session.takeoff.get(&id).unwrap().boq_code.as_deref(),
        Some("A.1")
    );
    session
        .run(Command::LinkBoq {
            id: id.clone(),
            code: None,
        })
        .unwrap();
    assert_eq!(session.takeoff.get(&id).unwrap().boq_code, None);

    // A version-1 file, as Phase 1 wrote it, still loads.
    let v1 = r#"{
  "version": 1,
  "drawing": "sample.dxf",
  "takeoff": {
    "items": [
      {
        "id": "4b0e7a52-3f1c-4d8e-9a57-0c2f4e7b9d11",
        "kind": "area",
        "value": 12.0,
        "unit": "m2",
        "shown": 12.0,
        "description": "Area",
        "layer": "",
        "sheet": "*Model_Space",
        "handles": [],
        "points": [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
        "block_name": null,
        "self_intersecting": false,
        "source": "user",
        "note": "",
        "created": "2026-10-01T09:00:00Z"
      }
    ]
  }
}"#;
    std::fs::write(project_path(&drawing), v1).unwrap();
    let old = Session::open(&drawing).unwrap();
    assert!(
        old.drawing.problems.is_empty(),
        "{:?}",
        old.drawing.problems
    );
    let m = &old.takeoff.items[0];
    assert_eq!((m.kind, m.value, m.height), (Kind::Area, 12.0, None));
    assert_eq!(m.boq_code, None);
    let project: Project = serde_json::from_str(v1).unwrap();
    assert_eq!(project.version, 1);
}

#[test]
fn hand_edited_drafting_settings_are_brought_into_range() {
    let (_dir, drawing) = sample();
    Session::open(&drawing).unwrap().save_project().unwrap();
    let file = project_path(&drawing);
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    json["drafting"]["precision"] = 200.into();
    json["drafting"]["polar_step_deg"] = 0.0.into();
    json["drafting"]["grid_spacing"] = (-2.0).into();
    json["drafting"]["ortho"] = true.into();
    std::fs::write(&file, json.to_string()).unwrap();

    let session = Session::open(&drawing).unwrap();
    assert!(
        session.drawing.problems.is_empty(),
        "{:?}",
        session.drawing.problems
    );
    let d = &session.drafting;
    assert_eq!(d.precision, 6);
    assert_eq!(d.polar_step_deg, 1.0);
    assert_eq!(d.grid_spacing, 1.0);
    // What was in range is kept.
    assert!(d.ortho);
}
