use opencadcodec::{DxfWriter, EntityType};
use qiyas_core::{Command, CoreError, Kind, Measurement, Outcome, Session, Source};

/// Opens the sample drawing (DXF, metres) in a session. The temp dir must outlive the test body.
fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

/// Runs a command that must add a measurement, and returns it.
fn measure(session: &mut Session, cmd: Command) -> Measurement {
    match session.run(cmd).unwrap() {
        Outcome::Measured { measurement } => *measurement,
        other => panic!("expected a measurement, got {other:?}"),
    }
}

fn area(points: &[[f64; 2]]) -> Command {
    Command::MeasureArea {
        points: points.to_vec(),
    }
}

const RECT_4X3: [[f64; 2]; 4] = [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];

/// Handle of the first entity in model space that `is` accepts.
fn handle_of(session: &Session, is: impl Fn(&EntityType) -> bool) -> u64 {
    session
        .drawing
        .entities_in("*Model_Space")
        .find(|e| is(e))
        .expect("entity in the sample")
        .common()
        .handle
        .value()
}

#[test]
fn distance_and_area_commands() {
    let (_dir, mut session) = sample_session();
    let d = measure(
        &mut session,
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]],
        },
    );
    assert_eq!(d.kind, Kind::Length);
    assert_eq!(d.value, 7.0);
    assert_eq!(d.unit, "m");
    assert_eq!(d.shown, 7.0);
    assert_eq!(d.description, "Distance");
    assert_eq!(d.sheet, "*Model_Space");
    assert_eq!(d.source, "user");
    assert_eq!(d.points.len(), 3);

    let a = measure(&mut session, area(&RECT_4X3));
    assert_eq!(a.kind, Kind::Area);
    assert_eq!(a.value, 12.0);
    assert_eq!(a.unit, "m2");
    assert_eq!(a.shown, 12.0);
    assert_eq!(a.description, "Area");
    assert!(!a.self_intersecting);
    assert_ne!(a.id, d.id);

    let ids: Vec<_> = session.takeoff.items.iter().map(|m| m.id.clone()).collect();
    assert_eq!(ids, [d.id.clone(), a.id.clone()]);
    assert_eq!(session.takeoff.get(&a.id).unwrap().value, 12.0);

    assert!(matches!(
        session.run(Command::MeasureDistance {
            points: vec![[0.0, 0.0]]
        }),
        Err(CoreError::InvalidCommand(_))
    ));
    assert!(matches!(
        session.run(area(&RECT_4X3[..2])),
        Err(CoreError::InvalidCommand(_))
    ));
    assert_eq!(session.takeoff.items.len(), 2);
}

#[test]
fn bowtie_area_is_flagged() {
    let (_dir, mut session) = sample_session();
    let m = measure(
        &mut session,
        area(&[[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]]),
    );
    assert!(m.self_intersecting);
    assert_eq!(m.value, 2.0);
}

#[test]
fn measure_entity_rect_line_and_insert() {
    let (_dir, mut session) = sample_session();
    let rect = handle_of(&session, |e| matches!(e, EntityType::LwPolyline(_)));
    let line = handle_of(&session, |e| matches!(e, EntityType::Line(_)));
    let door = handle_of(&session, |e| matches!(e, EntityType::Insert(_)));

    let m = measure(
        &mut session,
        Command::MeasureEntity {
            handle: rect,
            chain: Vec::new(),
        },
    );
    assert_eq!((m.kind, m.value, m.unit.as_str()), (Kind::Area, 12.0, "m2"));
    assert_eq!(m.description, "LwPolyline on A-WALL");
    assert_eq!(m.layer, "A-WALL");
    assert_eq!(m.handles, [rect]);

    let m = measure(
        &mut session,
        Command::MeasureEntity {
            handle: line,
            chain: Vec::new(),
        },
    );
    assert_eq!(
        (m.kind, m.value, m.unit.as_str()),
        (Kind::Length, 10.0, "m")
    );
    assert_eq!(m.description, "Line on A-WALL");

    let m = measure(
        &mut session,
        Command::MeasureEntity {
            handle: door,
            chain: Vec::new(),
        },
    );
    assert_eq!(
        (m.kind, m.value, m.unit.as_str()),
        (Kind::Count, 1.0, "pcs")
    );
    assert_eq!(m.block_name.as_deref(), Some("DOOR"));
    assert_eq!(m.layer, "A-DOOR");

    assert!(matches!(
        session.run(Command::MeasureEntity {
            handle: 0xDEAD_BEEF,
            chain: Vec::new()
        }),
        Err(CoreError::NoSuchEntity(0xDEAD_BEEF))
    ));
}

#[test]
fn count_blocks_respects_hidden_layers() {
    let (_dir, mut session) = sample_session();
    let count = |layer: Option<&str>| Command::CountBlock {
        // Block names match case-insensitively, as in DXF.
        block_name: "door".to_string(),
        layer: layer.map(str::to_string),
    };

    let m = measure(&mut session, count(None));
    assert_eq!((m.kind, m.value, m.shown), (Kind::Count, 3.0, 3.0));
    assert_eq!(m.unit, "pcs");
    assert_eq!(m.handles.len(), 3);
    assert_eq!(m.description, "door count");
    assert_eq!(measure(&mut session, count(Some("a-door"))).value, 3.0);
    assert_eq!(measure(&mut session, count(Some("A-WALL"))).value, 0.0);

    let hide = Command::SetLayerHidden {
        name: "a-door".to_string(),
        hidden: true,
    };
    assert!(matches!(session.run(hide).unwrap(), Outcome::Changed));
    let m = measure(&mut session, count(None));
    assert_eq!(m.value, 0.0);
    assert!(m.handles.is_empty());
}

#[test]
fn unknown_units_assume_mm() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unitless.dxf");
    let mut doc = qiyas_core::fixtures::sample_doc();
    doc.header.insertion_units = 0;
    DxfWriter::new(&doc).write_to_file(&path).unwrap();
    let mut session = Session::open(&path).unwrap();
    assert_eq!(session.info().units, "unknown");

    let m = measure(
        &mut session,
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [1000.0, 0.0]],
        },
    );
    assert_eq!(m.value, 1000.0);
    assert_eq!(m.unit, "m");
    assert_eq!(m.shown, 1.0);

    let m = measure(&mut session, area(&RECT_4X3));
    assert!((m.shown - 12e-6).abs() < 1e-15, "{}", m.shown);
}

#[test]
fn undo_redo_measurements() {
    let (_dir, mut session) = sample_session();
    assert!(!session.can_undo() && !session.can_redo());
    assert!(!session.undo());

    let first = measure(&mut session, area(&RECT_4X3));
    measure(
        &mut session,
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [1.0, 0.0]],
        },
    );
    assert_eq!(session.takeoff.items.len(), 2);

    assert!(session.undo());
    assert_eq!(session.takeoff.items.len(), 1);
    assert_eq!(session.takeoff.items[0].id, first.id);
    assert!(session.undo());
    assert!(session.takeoff.items.is_empty());
    assert!(!session.undo());
    assert!(session.redo());
    assert_eq!(session.takeoff.items.len(), 1);
    assert!(session.can_undo() && session.can_redo());

    // Description and removal are undoable too.
    let rename = Command::SetDescription {
        id: first.id.clone(),
        text: "Slab".to_string(),
    };
    assert!(matches!(session.run(rename).unwrap(), Outcome::Changed));
    // A new command drops what could have been redone.
    assert!(!session.can_redo());
    assert_eq!(session.takeoff.get(&first.id).unwrap().description, "Slab");
    let remove = Command::RemoveMeasurement {
        id: first.id.clone(),
    };
    assert!(matches!(session.run(remove).unwrap(), Outcome::Removed { id } if id == first.id));
    assert!(session.takeoff.items.is_empty());
    assert!(session.undo());
    assert_eq!(session.takeoff.get(&first.id).unwrap().description, "Slab");
    assert!(session.undo());
    assert_eq!(session.takeoff.get(&first.id).unwrap().description, "Area");

    // An unknown id changes nothing and leaves no undo step.
    assert!(session.undo());
    let gone = Command::RemoveMeasurement {
        id: "nope".to_string(),
    };
    assert!(matches!(session.run(gone).unwrap(), Outcome::Nothing));
    assert!(!session.can_undo());
    assert!(session.can_redo());

    // Sheet and layer changes are not undo steps.
    let sheet = Command::SetSheet {
        block: "*model_space".to_string(),
    };
    assert!(matches!(session.run(sheet).unwrap(), Outcome::Changed));
    assert!(!session.can_undo());
}

#[test]
fn totals_group_by_kind_and_unit() {
    let (_dir, mut session) = sample_session();
    measure(&mut session, area(&RECT_4X3));
    measure(
        &mut session,
        area(&[[10.0, 0.0], [13.0, 0.0], [13.0, 1.0], [10.0, 1.0]]),
    );
    let totals = session.takeoff.totals();
    assert_eq!(totals.len(), 1);
    let t = &totals[0];
    assert_eq!(
        (t.kind, t.unit.as_str(), t.count, t.sum),
        (Kind::Area, "m2", 2, 15.0)
    );

    measure(
        &mut session,
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [2.0, 0.0]],
        },
    );
    let kinds: Vec<_> = session.takeoff.totals().iter().map(|t| t.kind).collect();
    assert_eq!(kinds, [Kind::Area, Kind::Length]);
}

/// An `AddMeasurement` of `kind` with no value, unit, height, geometry or text; tests fill in
/// what they need.
fn add(kind: Kind) -> Command {
    Command::AddMeasurement {
        kind,
        value: None,
        unit: None,
        height: None,
        points: Vec::new(),
        handles: Vec::new(),
        chain: Vec::new(),
        description: String::new(),
        note: String::new(),
    }
}

/// `cmd` with its `AddMeasurement` fields changed by `f`.
fn with(
    mut cmd: Command,
    f: impl FnOnce(
        &mut Option<f64>,
        &mut Option<String>,
        &mut Option<f64>,
        &mut Vec<[f64; 2]>,
        &mut Vec<u64>,
    ),
) -> Command {
    let Command::AddMeasurement {
        value,
        unit,
        height,
        points,
        handles,
        ..
    } = &mut cmd
    else {
        unreachable!()
    };
    f(value, unit, height, points, handles);
    cmd
}

fn is_invalid(result: Result<Outcome, CoreError>) -> bool {
    matches!(result, Err(CoreError::InvalidCommand(_)))
}

#[test]
fn volume_from_area_and_height() {
    let (_dir, mut session) = sample_session();
    let slab = with(add(Kind::Volume), |_, _, h, p, _| {
        *h = Some(0.2);
        *p = RECT_4X3.to_vec();
    });
    let m = measure(&mut session, slab.clone());
    assert_eq!(m.kind, Kind::Volume);
    assert_eq!(m.unit, "m3");
    assert!((m.value - 2.4).abs() < 1e-12, "{}", m.value);
    assert!((m.shown - 2.4).abs() < 1e-12, "{}", m.shown);
    assert_eq!(m.height, Some(0.2));
    assert_eq!(m.points, RECT_4X3);

    // A volume needs a positive height, and its value comes from the geometry.
    let no_height = with(slab.clone(), |_, _, h, _, _| *h = None);
    let flat = with(slab.clone(), |_, _, h, _, _| *h = Some(0.0));
    let given = with(slab.clone(), |v, _, _, _, _| *v = Some(2.4));
    let unit = with(slab, |_, u, _, _, _| *u = Some("m3".to_string()));
    for cmd in [no_height, flat, given, unit] {
        assert!(is_invalid(session.run(cmd)));
    }

    // From a closed entity too: the 4 x 3 rectangle.
    let rect = handle_of(&session, |e| matches!(e, EntityType::LwPolyline(_)));
    let m = measure(
        &mut session,
        with(add(Kind::Volume), |_, _, h, _, hs| {
            *h = Some(0.5);
            *hs = vec![rect];
        }),
    );
    assert!((m.shown - 6.0).abs() < 1e-9, "{}", m.shown);
    assert_eq!(m.layer, "A-WALL");

    // In millimetres the volume is shown in cubic metres.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mm.dxf");
    let mut doc = qiyas_core::fixtures::sample_doc();
    doc.header.insertion_units = 4;
    DxfWriter::new(&doc).write_to_file(&path).unwrap();
    let mut session = Session::open(&path).unwrap();
    let m = measure(
        &mut session,
        with(add(Kind::Volume), |_, _, h, p, _| {
            *h = Some(200.0);
            *p = vec![[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [0.0, 3000.0]];
        }),
    );
    assert!((m.shown - 2.4).abs() < 1e-9, "{}", m.shown);
}

#[test]
fn item_measurement_keeps_its_unit() {
    let (_dir, mut session) = sample_session();
    let bags = with(add(Kind::Item), |v, u, _, _, _| {
        *v = Some(5.0);
        *u = Some("bags".to_string());
    });
    let m = measure(&mut session, bags.clone());
    assert_eq!(m.kind, Kind::Item);
    assert_eq!((m.value, m.shown), (5.0, 5.0));
    assert_eq!(m.unit, "bags");
    assert!(m.points.is_empty() && m.handles.is_empty());

    // An item needs both its value and its unit.
    let no_value = with(bags.clone(), |v, _, _, _, _| *v = None);
    let no_unit = with(bags.clone(), |_, u, _, _, _| *u = None);
    let blank_unit = with(bags, |_, u, _, _, _| *u = Some(" ".to_string()));
    for cmd in [no_value, no_unit, blank_unit] {
        assert!(is_invalid(session.run(cmd)));
    }
}

#[test]
fn add_measurement_requires_geometry() {
    let (_dir, mut session) = sample_session();
    for kind in [Kind::Length, Kind::Area, Kind::Count] {
        match session.run(add(kind)) {
            Err(CoreError::InvalidCommand(why)) => assert_eq!(why, "A measurement needs geometry"),
            other => panic!("expected InvalidCommand, got {other:?}"),
        }
    }
    assert!(is_invalid(
        session.run(with(add(Kind::Volume), |_, _, h, _, _| { *h = Some(1.0) }))
    ));
    assert!(session.takeoff.items.is_empty());

    // Lengths, areas and counts are worked out from the geometry, never given.
    let given = with(add(Kind::Length), |v, _, _, p, _| {
        *v = Some(3.0);
        *p = vec![[0.0, 0.0], [3.0, 0.0]];
    });
    assert!(is_invalid(session.run(given)));
    let height = with(add(Kind::Area), |_, _, h, p, _| {
        *h = Some(1.0);
        *p = RECT_4X3.to_vec();
    });
    assert!(is_invalid(session.run(height)));

    let rect = handle_of(&session, |e| matches!(e, EntityType::LwPolyline(_)));
    let line = handle_of(&session, |e| matches!(e, EntityType::Line(_)));
    let doors: Vec<u64> = session
        .drawing
        .entities_in("*Model_Space")
        .filter(|e| matches!(e, EntityType::Insert(_)))
        .map(|e| e.common().handle.value())
        .collect();
    let m = measure(
        &mut session,
        with(add(Kind::Area), |_, _, _, _, hs| *hs = vec![rect]),
    );
    assert_eq!((m.kind, m.value, m.unit.as_str()), (Kind::Area, 12.0, "m2"));
    assert_eq!(m.handles, [rect]);
    let m = measure(
        &mut session,
        with(add(Kind::Length), |_, _, _, _, hs| *hs = vec![line, rect]),
    );
    assert_eq!((m.kind, m.value), (Kind::Length, 24.0));
    let m = measure(
        &mut session,
        with(add(Kind::Count), |_, _, _, _, hs| *hs = doors.clone()),
    );
    assert_eq!(
        (m.kind, m.value, m.unit.as_str()),
        (Kind::Count, 3.0, "pcs")
    );
    let m = measure(
        &mut session,
        with(add(Kind::Count), |_, _, _, p, _| {
            *p = vec![[1.0, 1.0], [2.0, 2.0]]
        }),
    );
    assert_eq!(m.value, 2.0);
    let m = measure(
        &mut session,
        with(add(Kind::Length), |_, _, _, p, _| {
            *p = vec![[0.0, 0.0], [3.0, 4.0]]
        }),
    );
    assert_eq!(m.value, 5.0);

    // A line has no area; a handle must name an entity.
    assert!(is_invalid(
        session.run(with(add(Kind::Area), |_, _, _, _, hs| *hs = vec![line]))
    ));
    assert!(matches!(
        session.run(with(add(Kind::Length), |_, _, _, _, hs| *hs = vec![0xDEAD_BEEF])),
        Err(CoreError::NoSuchEntity(0xDEAD_BEEF))
    ));
}

#[test]
fn run_as_ai_marks_source() {
    let (_dir, mut session) = sample_session();
    let distance = || Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 0.0]],
    };
    let sources: Vec<String> = [Source::Ai, Source::Mcp, Source::User]
        .into_iter()
        .map(|source| match session.run_as(distance(), source).unwrap() {
            Outcome::Measured { measurement } => measurement.source,
            other => panic!("expected a measurement, got {other:?}"),
        })
        .collect();
    assert_eq!(sources, ["ai", "mcp", "user"]);
    assert_eq!(measure(&mut session, distance()).source, "user");

    let cmd = Command::AddMeasurement {
        kind: Kind::Area,
        value: None,
        unit: None,
        height: None,
        points: RECT_4X3.to_vec(),
        handles: Vec::new(),
        chain: Vec::new(),
        description: "Slab".to_string(),
        note: "From the AI".to_string(),
    };
    let m = match session.run_as(cmd, Source::Ai).unwrap() {
        Outcome::Measured { measurement } => *measurement,
        other => panic!("expected a measurement, got {other:?}"),
    };
    assert_eq!(
        (m.source.as_str(), m.description.as_str(), m.note.as_str()),
        ("ai", "Slab", "From the AI")
    );
    // An AI change is undoable like any other.
    assert!(session.undo());
    assert!(session.takeoff.get(&m.id).is_none());
}

#[test]
fn set_height_is_undoable() {
    let (_dir, mut session) = sample_session();
    let a = measure(&mut session, area(&RECT_4X3));
    let set = |height: f64| Command::SetHeight {
        id: a.id.clone(),
        height,
    };
    assert!(matches!(session.run(set(0.2)).unwrap(), Outcome::Changed));
    let v = session.takeoff.get(&a.id).unwrap().clone();
    assert_eq!(
        (v.kind, v.unit.as_str(), v.height),
        (Kind::Volume, "m3", Some(0.2))
    );
    assert!((v.value - 2.4).abs() < 1e-12 && (v.shown - 2.4).abs() < 1e-12);
    assert_eq!(v.points, RECT_4X3);

    // A new height on a volume keeps its area.
    session.run(set(0.5)).unwrap();
    let v = session.takeoff.get(&a.id).unwrap();
    assert!((v.shown - 6.0).abs() < 1e-12, "{}", v.shown);

    assert!(session.undo());
    assert!((session.takeoff.get(&a.id).unwrap().shown - 2.4).abs() < 1e-12);
    assert!(session.undo());
    let back = session.takeoff.get(&a.id).unwrap();
    assert_eq!(
        (back.kind, back.value, back.unit.as_str(), back.height),
        (Kind::Area, 12.0, "m2", None)
    );
    assert!(session.redo());
    assert_eq!(session.takeoff.get(&a.id).unwrap().kind, Kind::Volume);

    assert!(is_invalid(session.run(set(0.0))));
    assert!(is_invalid(session.run(set(-1.0))));
    let d = measure(
        &mut session,
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [1.0, 0.0]],
        },
    );
    assert!(is_invalid(session.run(Command::SetHeight {
        id: d.id,
        height: 1.0
    })));
    let none = Command::SetHeight {
        id: "nope".to_string(),
        height: 1.0,
    };
    assert!(matches!(session.run(none).unwrap(), Outcome::Nothing));
}

#[test]
fn undo_if_refuses_when_top_step_is_users() {
    let (_dir, mut session) = sample_session();
    assert_eq!(session.top_step_source(), None);
    assert!(!session.undo_if(Source::Ai), "nothing to undo");

    session.run_as(area(&RECT_4X3), Source::Ai).unwrap();
    measure(&mut session, area(&RECT_4X3));
    assert_eq!(session.top_step_source(), Some(Source::User));
    let ids = |s: &Session| {
        s.takeoff
            .items
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>()
    };
    let before = ids(&session);
    assert_eq!(before.len(), 2);
    assert!(!session.undo_if(Source::Ai), "the user's change is on top");
    assert_eq!(ids(&session), before, "nothing changed");
    assert!(session.can_undo());

    assert!(session.undo());
    assert_eq!(session.top_step_source(), Some(Source::Ai));
    assert!(session.undo_if(Source::Ai));
    assert!(session.takeoff.items.is_empty());

    // Redo keeps who made the step.
    assert!(session.redo());
    assert_eq!(session.top_step_source(), Some(Source::Ai));
}

#[test]
fn default_descriptions_are_marked_auto() {
    let (_dir, mut session) = sample_session();
    let rect = handle_of(&session, |e| matches!(e, EntityType::LwPolyline(_)));
    let distance = Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 0.0]],
    };
    let count = Command::CountBlock {
        block_name: "DOOR".to_string(),
        layer: None,
    };
    for cmd in [
        distance,
        area(&RECT_4X3),
        Command::MeasureEntity {
            handle: rect,
            chain: Vec::new(),
        },
        count,
    ] {
        let m = measure(&mut session, cmd);
        assert!(m.auto_description, "{}", m.description);
    }

    // A description the caller gives is not a default.
    let given = with(add(Kind::Area), |_, _, _, points, _| {
        *points = RECT_4X3.to_vec()
    });
    let m = measure(&mut session, given);
    assert!(!m.auto_description);

    // Renaming clears the flag.
    let id = session.takeoff.items[0].id.clone();
    session
        .run(Command::SetDescription {
            id: id.clone(),
            text: "Wall".to_string(),
        })
        .unwrap();
    assert!(!session.takeoff.get(&id).unwrap().auto_description);
}

#[test]
fn a_hatch_with_an_island_measures_without_it_and_after_a_move() {
    let (_dir, mut s) = sample_session();
    let square = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let island = vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]];
    let Outcome::Added { handles } = s
        .run(Command::Draw(qiyas_core::DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: square,
            holes: vec![island],
            bulges: Vec::new(),
            pattern: qiyas_core::HatchPattern::Solid,
        }))
        .unwrap()
    else {
        panic!("a hatch")
    };
    let h = handles[0];
    let m = measure(
        &mut s,
        Command::MeasureEntity {
            handle: h,
            chain: Vec::new(),
        },
    );
    assert_eq!(m.kind, Kind::Area);
    assert!((m.value - 96.0).abs() < 1e-9, "{}", m.value);
    // Moved: the row is measured again, still without the island.
    s.run(Command::Modify(qiyas_core::ModifySpec::Move {
        targets: vec![qiyas_core::Target {
            handle: h,
            chain: Vec::new(),
        }],
        delta: [5.0, 0.0],
    }))
    .unwrap();
    let row = s.takeoff.items.iter().find(|r| r.handles == [h]).unwrap();
    assert!((row.value - 96.0).abs() < 1e-9, "{}", row.value);
}

#[test]
fn quantity_provenance_survives_json_and_embedded_drawing_without_net_inference() {
    use qiyas_core::{AreaOrigin, MeasurementBasis, Units};
    let (_dir, mut session) = sample_session();
    let original = measure(&mut session, area(&RECT_4X3));
    assert_eq!(original.area_origin, AreaOrigin::ManualOrEntity);
    assert_eq!(original.measurement_basis, MeasurementBasis::Unspecified);
    let mut rows = Vec::new();
    for (origin, basis) in [
        (AreaOrigin::ExistingBoundary, MeasurementBasis::Gross),
        (AreaOrigin::WallFace, MeasurementBasis::WallFace),
    ] {
        let mut m = Measurement::new(
            Kind::Area,
            12.0,
            Units::Metres,
            "Same region".into(),
            "0".into(),
            "*Model_Space".into(),
        );
        m.points = RECT_4X3.to_vec();
        m.area_origin = origin;
        m.measurement_basis = basis;
        rows.push(m);
    }
    qiyas_core::embed_measurements(&mut session.drawing.doc, &rows);
    let read = qiyas_core::read_embedded(&session.drawing.doc);
    assert_eq!(read.len(), 2);
    for (a, b) in rows.iter().zip(&read) {
        assert_eq!(a.area_origin, b.area_origin);
        assert_eq!(a.measurement_basis, b.measurement_basis);
        let restored: Measurement =
            serde_json::from_value(serde_json::to_value(a).unwrap()).unwrap();
        assert_eq!(restored.area_origin, a.area_origin);
    }
    let mut legacy = serde_json::to_value(&original).unwrap();
    for key in ["area_origin", "measurement_basis", "count_scope"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    let old: Measurement = serde_json::from_value(legacy).unwrap();
    assert_eq!(old.area_origin, AreaOrigin::Unspecified);
    assert_eq!(old.measurement_basis, MeasurementBasis::Unspecified);
    assert_eq!(old.count_scope, None);
}

#[test]
fn count_results_keep_a_bounded_scope_even_for_unsupported_occurrences() {
    let (_dir, mut session) = sample_session();
    let m = measure(
        &mut session,
        Command::CountBlock {
            block_name: "DOOR".into(),
            layer: None,
        },
    );
    assert_eq!(
        m.count_scope.as_deref(),
        Some(qiyas_core::INSERT_COUNT_SCOPE)
    );
    for limitation in [
        "Nested", "array", "parent", "cycles", "xrefs", "dynamic", "clipping",
    ] {
        assert!(m.note.contains(limitation), "{limitation}: {}", m.note);
    }
    let summary = session.block_summary(None);
    assert!(summary
        .iter()
        .all(|r| r.count_scope == qiyas_core::INSERT_COUNT_SCOPE
            && r.visibility_policy == "as_shown"));
    assert!(session
        .block_summary_in_file(None)
        .iter()
        .all(|r| r.visibility_policy == "in_file"));
    qiyas_core::embed_measurements(&mut session.drawing.doc, &[m]);
    assert_eq!(
        qiyas_core::read_embedded(&session.drawing.doc)[0]
            .count_scope
            .as_deref(),
        Some(qiyas_core::INSERT_COUNT_SCOPE)
    );
}

#[test]
fn nested_layer_zero_array_cycle_and_missing_xref_remain_bounded_records() {
    use opencadcodec::entities::Insert;
    use opencadcodec::{BlockRecord, Vector3};
    let dir = tempfile::tempdir().unwrap();
    for invisible in [false, true] {
        let mut doc = qiyas_core::fixtures::sample_doc();
        let mut block = BlockRecord::new("PARENT");
        block.handle = doc.allocate_handle();
        block.block_entity_handle = doc.allocate_handle();
        block.block_end_handle = doc.allocate_handle();
        let owner = block.handle;
        doc.block_records.add(block).unwrap();
        let mut child = Insert::new("DOOR", Vector3::new(0.0, 0.0, 0.0));
        child.common.owner_handle = owner;
        child.common.layer = "0".into();
        child.column_count = 2;
        child.row_count = 3;
        doc.add_entity(EntityType::Insert(child)).unwrap();
        let mut cycle = Insert::new("PARENT", Vector3::new(0.0, 0.0, 0.0));
        cycle.common.owner_handle = owner;
        doc.add_entity(EntityType::Insert(cycle)).unwrap();
        let mut parent = Insert::new("PARENT", Vector3::new(20.0, 0.0, 0.0));
        parent.common.invisible = invisible;
        parent.common.layer = "A-WALL".into();
        doc.add_entity(EntityType::Insert(parent)).unwrap();
        let mut array = Insert::new("DOOR", Vector3::new(30.0, 0.0, 0.0));
        array.column_count = 2;
        array.row_count = 3;
        array.common.layer = "A-DOOR".into();
        doc.add_entity(EntityType::Insert(array)).unwrap();
        qiyas_core::fixtures::add_xref(&mut doc, "MISSING", "absent.dxf", [0.0, 0.0], 0.0);
        let path = dir.path().join(format!("bounded-{invisible}.dxf"));
        DxfWriter::new(&doc).write_to_file(&path).unwrap();
        let mut session = Session::open(&path).unwrap();
        let count = |s: &mut Session| {
            measure(
                s,
                Command::CountBlock {
                    block_name: "DOOR".into(),
                    layer: None,
                },
            )
        };
        let m = count(&mut session);
        // Three plain records plus one array record. Neither the six nested cells nor
        // the six top-level array occurrences are claimed as enumerated visible objects.
        assert_eq!(m.value, 4.0);
        assert_eq!(
            m.count_scope.as_deref(),
            Some(qiyas_core::INSERT_COUNT_SCOPE)
        );
        assert!(m.note.contains("Not a complete visible count"));
        session.set_layer_hidden("A-WALL", true);
        assert_eq!(count(&mut session).value, 4.0);
        session.set_layer_hidden("A-DOOR", true);
        assert_eq!(count(&mut session).value, 0.0);
        assert_eq!(
            session
                .block_summary_in_file(None)
                .iter()
                .find(|r| r.block_name == "DOOR")
                .unwrap()
                .count,
            4
        );
    }
}

#[test]
fn file_off_and_frozen_layers_are_excluded_from_both_policies() {
    use qiyas_core::LayerProp;
    for (prop, value) in [(LayerProp::On, false), (LayerProp::Frozen, true)] {
        let (_dir, mut session) = sample_session();
        session
            .run(Command::SetLayer {
                name: "A-DOOR".into(),
                prop,
                value: serde_json::json!(value),
            })
            .unwrap();
        let m = measure(
            &mut session,
            Command::CountBlock {
                block_name: "DOOR".into(),
                layer: None,
            },
        );
        assert_eq!(m.value, 0.0);
        assert!(session.block_summary(None).is_empty());
        assert!(session.block_summary_in_file(None).is_empty());
    }
}
