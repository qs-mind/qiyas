use std::path::Path;

use opencadcodec::entities::{BoundaryEdge, BoundaryPath, Hatch, Insert, LineEdge};
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{Color, DxfWriter, EntityType, Point, Vector2, Vector3};
use qiyas_core::{
    embed_measurements, project_path, read_embedded, Command, CoreError, Kind, Measurement,
    Session, Units,
};

/// Measures a distance, an area and the three DOOR references in `session`.
fn measure_sample(session: &mut Session) {
    for cmd in [
        Command::MeasureDistance {
            points: vec![[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]],
        },
        Command::MeasureArea {
            points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
        },
        Command::CountBlock {
            block_name: "DOOR".to_string(),
            layer: None,
        },
    ] {
        session.run(cmd).unwrap();
    }
}

/// Saves the sample (written as `source_ext`) with its measurements to `out_name`, deletes the
/// project file, reopens the saved drawing and checks the take-off was rebuilt from it.
fn round_trip(source_ext: &str, out_name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), source_ext);
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    let out = dir.path().join(out_name);
    session.save_as(&out).unwrap();
    assert_eq!(session.drawing.path, out);
    assert!(project_path(&out).exists());

    std::fs::remove_file(project_path(&out)).unwrap();
    let reopened = Session::open(&out).unwrap();
    let before = &session.takeoff.items;
    let after = &reopened.takeoff.items;
    assert_eq!(after.len(), 3, "{after:?}");
    for (b, a) in before.iter().zip(after) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.kind, b.kind);
        assert!(
            (a.value - b.value).abs() < 1e-6,
            "{} vs {}",
            a.value,
            b.value
        );
        assert!((a.shown - b.shown).abs() < 1e-6);
        assert_eq!(a.unit, b.unit);
        assert_eq!(a.description, b.description);
        assert_eq!(a.layer, b.layer);
        assert_eq!(a.sheet, b.sheet);
        assert_eq!(a.block_name, b.block_name);
        assert_eq!(a.source, "user");
        assert!(a.handles.is_empty());
    }
    assert_eq!(after[0].points, before[0].points);
    assert_eq!(after[1].points, before[1].points);
    assert_eq!(after[2].kind, Kind::Count);
    assert_eq!(after[2].value, 3.0);

    let layers = &reopened.drawing.doc.layers;
    for (name, index) in [("QIYAS-LENGTH", 6), ("QIYAS-AREA", 5), ("QIYAS-COUNT", 4)] {
        let layer = layers.get(name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(layer.color, Color::Index(index), "{name}");
    }
}

#[test]
fn save_dwg_reopen_without_project_rebuilds_measurements() {
    round_trip("dxf", "out.dwg");
}

#[test]
fn save_dxf_from_dwg_too() {
    round_trip("dwg", "out.dxf");
}

/// Entities in the saved file that carry a QIYAS xdata record.
fn embedded_count(path: &Path) -> usize {
    let session = Session::open(path).unwrap();
    session
        .drawing
        .doc
        .entities()
        .filter(|e| e.common().extended_data.get_record("QIYAS").is_some())
        .count()
}

#[test]
fn saving_twice_does_not_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    let out = dir.path().join("out.dwg");
    session.save_as(&out).unwrap();
    session.save().unwrap();
    // A distance polyline, an area polyline and one point per door.
    assert_eq!(embedded_count(&out), 5);
    let kinds: Vec<_> = Session::open(&out)
        .unwrap()
        .drawing
        .doc
        .entities()
        .filter(|e| e.common().extended_data.get_record("QIYAS").is_some())
        .map(|e| match e {
            EntityType::LwPolyline(p) => (e.common().layer.clone(), p.is_closed),
            _ => (e.common().layer.clone(), false),
        })
        .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|k| *k == &("QIYAS-COUNT".to_string(), false))
            .count(),
        3
    );
    assert!(kinds.contains(&("QIYAS-LENGTH".to_string(), false)));
    assert!(kinds.contains(&("QIYAS-AREA".to_string(), true)));

    // Reopened and saved again (the project file wins): still no duplicates.
    let mut reopened = Session::open(&out).unwrap();
    assert_eq!(reopened.takeoff.items.len(), 3);
    reopened.save().unwrap();
    assert_eq!(embedded_count(&out), 5);
}

#[test]
fn project_file_wins_over_embedded_measurements() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    let out = dir.path().join("out.dxf");
    session.save_as(&out).unwrap();
    let id = session.takeoff.items[0].id.clone();
    session.run(Command::RemoveMeasurement { id }).unwrap();
    session.save_project().unwrap();
    assert_eq!(Session::open(&out).unwrap().takeoff.items.len(), 2);
}

#[test]
fn save_as_other_formats_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    let out = dir.path().join("out.pdf");
    assert!(matches!(
        session.save_as(&out),
        Err(CoreError::InvalidCommand(_))
    ));
    assert!(!out.exists());
    assert_eq!(session.drawing.path, source);
}

#[test]
fn measured_entities_are_embedded_from_their_shape() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    let handle_of = |session: &Session, is: fn(&EntityType) -> bool| {
        session
            .drawing
            .entities_in("*Model_Space")
            .find(|e| is(e))
            .unwrap()
            .common()
            .handle
            .value()
    };
    let rect = handle_of(&session, |e| matches!(e, EntityType::LwPolyline(_)));
    let line = handle_of(&session, |e| matches!(e, EntityType::Line(_)));
    let door = handle_of(&session, |e| matches!(e, EntityType::Insert(_)));
    for handle in [rect, line, door] {
        session
            .run(Command::MeasureEntity {
                handle,
                chain: Vec::new(),
            })
            .unwrap();
    }
    let out = dir.path().join("out.dwg");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();

    let items = Session::open(&out).unwrap().takeoff.items;
    let summary: Vec<_> = items.iter().map(|m| (m.kind, m.value)).collect();
    assert_eq!(
        summary,
        [(Kind::Area, 12.0), (Kind::Length, 10.0), (Kind::Count, 1.0)]
    );
    assert_eq!(
        items[0].points,
        [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]]
    );
    assert_eq!(items[1].points, [[0.0, 0.0], [10.0, 0.0]]);
    assert_eq!(items[0].layer, "A-WALL");
    assert_eq!(items[2].block_name.as_deref(), Some("DOOR"));
}

#[test]
fn count_survives_two_trips_through_the_drawing() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    let first = dir.path().join("first.dwg");
    session.save_as(&first).unwrap();
    std::fs::remove_file(project_path(&first)).unwrap();

    // Rebuilt from the drawing: no handles, so the next save writes the count from its points.
    let mut rebuilt = Session::open(&first).unwrap();
    assert!(rebuilt.takeoff.items[2].handles.is_empty());
    let second = dir.path().join("second.dwg");
    rebuilt.save_as(&second).unwrap();
    std::fs::remove_file(project_path(&second)).unwrap();

    let count = Session::open(&second).unwrap().takeoff.items[2].clone();
    assert_eq!((count.kind, count.value), (Kind::Count, 3.0));
    assert_eq!(count.points, [[1.0, 1.0], [5.0, 1.0], [9.0, 1.0]]);
}

/// A square ring of side `size` with its corner at (x, 0), as a hatch boundary.
fn square(x: f64, size: f64) -> BoundaryPath {
    let corners = [[x, 0.0], [x + size, 0.0], [x + size, size], [x, size]];
    let mut path = BoundaryPath::new();
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        path.add_edge(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(a[0], a[1]),
            end: Vector2::new(b[0], b[1]),
        }));
    }
    path
}

#[test]
fn measured_entity_with_several_paths_writes_one_polyline_per_path() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("hatch.dxf");
    let mut doc = qiyas_core::fixtures::sample_doc();
    let mut hatch = Hatch::new();
    hatch.paths.push(square(30.0, 1.0));
    hatch.paths.push(square(40.0, 2.0));
    let handle = doc.add_entity(EntityType::Hatch(hatch)).unwrap().value();
    DxfWriter::new(&doc).write_to_file(&source).unwrap();
    let mut session = Session::open(&source).unwrap();
    session
        .run(Command::MeasureEntity {
            handle,
            chain: Vec::new(),
        })
        .unwrap();
    let out = dir.path().join("out.dxf");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();

    let reopened = Session::open(&out).unwrap();
    let rings = reopened
        .drawing
        .doc
        .entities()
        .filter(|e| e.common().layer == "QIYAS-AREA")
        .count();
    assert_eq!(rings, 2);
    let items = &reopened.takeoff.items;
    assert_eq!(items.len(), 1);
    assert!((items[0].value - 5.0).abs() < 1e-9, "{}", items[0].value);
    assert_eq!(items[0].points.len(), 4);
}

/// A length measurement drawn by the user, in metres.
fn user_length() -> Measurement {
    let mut m = Measurement::new(
        Kind::Length,
        3.0,
        Units::Metres,
        "Distance".to_string(),
        String::new(),
        "*Model_Space".to_string(),
    );
    m.points = vec![[0.0, 0.0], [3.0, 0.0]];
    m
}

#[test]
fn malformed_records_are_skipped() {
    let mut doc = qiyas_core::fixtures::sample_doc();
    let good = user_length();
    embed_measurements(&mut doc, std::slice::from_ref(&good));
    let short = vec![
        XDataValue::String("short".to_string()),
        XDataValue::String("count".to_string()),
        XDataValue::Real(1.0),
    ];
    let wrong_type = vec![
        XDataValue::String("wrong".to_string()),
        XDataValue::String("count".to_string()),
        XDataValue::String("1".to_string()),
        XDataValue::String(String::new()),
        XDataValue::String(String::new()),
        XDataValue::String("*Model_Space".to_string()),
        XDataValue::String(String::new()),
    ];
    let mut unknown_kind = wrong_type.clone();
    unknown_kind[0] = XDataValue::String("unknown".to_string());
    unknown_kind[1] = XDataValue::String("weight".to_string());
    unknown_kind[2] = XDataValue::Real(1.0);
    // A volume needs its height in an 8th slot.
    let mut flat_volume = unknown_kind.clone();
    flat_volume[0] = XDataValue::String("flat".to_string());
    flat_volume[1] = XDataValue::String("volume".to_string());
    for values in [short, wrong_type, unknown_kind, flat_volume] {
        let mut record = ExtendedDataRecord::new("QIYAS");
        record.values = values;
        let mut point = Point::new();
        point.common.extended_data.add_record(record);
        doc.add_entity(EntityType::Point(point)).unwrap();
    }

    let items = read_embedded(&doc);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, good.id);
    assert_eq!(items[0].points, good.points);
}

#[test]
fn upper_case_extension_saves() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    let out = dir.path().join("OUT.DWG");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();
    assert_eq!(Session::open(&out).unwrap().takeoff.items.len(), 3);
}

#[test]
fn failed_write_keeps_the_original_and_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let original = std::fs::read(&source).unwrap();
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);

    // A non-empty folder where the drawing should go: the rename fails.
    let taken = dir.path().join("taken.dwg");
    std::fs::create_dir(&taken).unwrap();
    std::fs::write(taken.join("keep"), "").unwrap();
    for target in [dir.path().join("missing").join("out.dwg"), taken.clone()] {
        assert!(matches!(session.save_as(&target), Err(CoreError::Io(_))));
        assert_eq!(session.drawing.path, source);
    }
    assert!(!dir.path().join("taken.dwg.tmp").exists());
    assert!(!project_path(&taken).exists());
    assert_eq!(std::fs::read(&source).unwrap(), original);
}

#[test]
fn save_over_the_drawing_keeps_a_backup_of_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let original = std::fs::read(&source).unwrap();
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    session.save().unwrap();

    let bak = dir.path().join(format!(
        "{}.bak",
        source.file_name().unwrap().to_string_lossy()
    ));
    assert_eq!(std::fs::read(&bak).unwrap(), original);
    assert_ne!(std::fs::read(&source).unwrap(), original);
}

#[test]
fn save_as_a_new_file_writes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    session.save_as(&dir.path().join("out.dwg")).unwrap();
    assert!(!dir.path().join("out.dwg.bak").exists());
}

#[cfg(unix)]
#[test]
fn save_keeps_the_drawing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640)).unwrap();
    let mut session = Session::open(&source).unwrap();
    session.save().unwrap();
    let mode = std::fs::metadata(&source).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

#[test]
fn failed_save_leaves_the_drawing_in_memory_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    let before = session.drawing.doc.clone();
    let target = dir.path().join("missing").join("out.dwg");
    assert!(session.save_as(&target).is_err());
    assert!(session.drawing.doc == before);
    assert!(session.drawing.doc.layers.get("QIYAS-LENGTH").is_none());
}

#[test]
fn project_file_failure_says_the_drawing_was_saved() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    measure_sample(&mut session);
    // A non-empty folder where the project file should go: its rename fails.
    let out = dir.path().join("out.dwg");
    std::fs::create_dir(project_path(&out)).unwrap();
    std::fs::write(project_path(&out).join("keep"), "").unwrap();

    let err = session.save_as(&out).unwrap_err();
    assert!(
        err.to_string()
            .starts_with("Drawing saved, but the project file could not be written"),
        "{err}"
    );
    assert!(out.exists());
    assert_eq!(session.drawing.path, out);
    // The saved drawing is the one in memory now.
    assert!(session.drawing.doc.layers.get("QIYAS-LENGTH").is_some());
}

#[test]
fn volume_embeds_and_rebuilds_with_height() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    let add =
        |kind, value, unit: Option<&str>, height, points: Vec<[f64; 2]>| Command::AddMeasurement {
            empty_item: false,
            kind,
            value,
            unit: unit.map(str::to_string),
            height,
            points,
            handles: Vec::new(),
            chain: Vec::new(),
            description: "Slab".to_string(),
            note: String::new(),
        };
    let rect = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
    for cmd in [
        add(Kind::Volume, None, None, Some(0.2), rect.clone()),
        add(
            Kind::Item,
            Some(5.0),
            Some("bags"),
            None,
            vec![[1.0, 1.0], [2.0, 1.0]],
        ),
        // No geometry: stays in the project file only.
        add(Kind::Item, Some(2.0), Some("t"), None, Vec::new()),
    ] {
        session.run(cmd).unwrap();
    }
    let out = dir.path().join("out.dwg");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();

    let reopened = Session::open(&out).unwrap();
    let on_area: Vec<bool> = reopened
        .drawing
        .doc
        .entities()
        .filter(|e| e.common().layer == "QIYAS-AREA")
        .map(|e| matches!(e, EntityType::LwPolyline(p) if p.is_closed))
        .collect();
    assert_eq!(on_area, [true]);
    let items = &reopened.takeoff.items;
    assert_eq!(items.len(), 2, "{items:?}");
    let before = &session.takeoff.items;
    let v = &items[0];
    assert_eq!(
        (v.id.as_str(), v.kind, v.unit.as_str()),
        (before[0].id.as_str(), Kind::Volume, "m3")
    );
    assert_eq!(v.height, Some(0.2));
    assert!((v.value - before[0].value).abs() < 1e-9 && (v.shown - 2.4).abs() < 1e-9);
    assert_eq!(v.points, rect);
    let item = &items[1];
    assert_eq!(
        (item.kind, item.value, item.shown, item.unit.as_str()),
        (Kind::Item, 5.0, 5.0, "bags")
    );
    assert_eq!(item.points, [[1.0, 1.0], [2.0, 1.0]]);
}

#[test]
fn markup_item_embeds_on_the_markup_layer() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    for unit in ["markup", "bags"] {
        session
            .run(Command::AddMeasurement {
                empty_item: false,
                kind: Kind::Item,
                value: Some(0.0),
                unit: Some(unit.to_string()),
                height: None,
                points: vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0]],
                handles: Vec::new(),
                chain: Vec::new(),
                description: unit.to_string(),
                note: String::new(),
            })
            .unwrap();
    }
    let out = dir.path().join("out.dxf");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();

    let reopened = Session::open(&out).unwrap();
    let doc = &reopened.drawing.doc;
    let layer_of = |description: &str| {
        doc.entities()
            .find(|e| {
                e.common()
                    .extended_data
                    .get_record("QIYAS")
                    .is_some_and(|r| {
                        r.values.get(3) == Some(&XDataValue::String(description.to_string()))
                    })
            })
            .map(|e| e.common().layer.clone())
    };
    assert_eq!(layer_of("markup").as_deref(), Some("QIYAS-MARKUP"));
    assert_eq!(layer_of("bags").as_deref(), Some("QIYAS-ITEM"));
    assert_eq!(
        doc.layers.get("QIYAS-MARKUP").unwrap().color,
        Color::Index(1)
    );
    let units: Vec<&str> = reopened
        .takeoff
        .items
        .iter()
        .map(|m| m.unit.as_str())
        .collect();
    assert_eq!(units, ["markup", "bags"]);
}

#[test]
fn mirrored_insert_count_point_lands_where_it_is_drawn() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    // Mirrored in X: object point (40, 1) is drawn at world (-40, 1).
    let mut door =
        Insert::new("DOOR", Vector3::new(40.0, 1.0, 0.0)).with_normal(Vector3::new(0.0, 0.0, -1.0));
    door.common.layer = "A-DOOR".to_string();
    let handle = session
        .drawing
        .doc
        .add_entity(EntityType::Insert(door))
        .unwrap()
        .value();
    session
        .run(Command::MeasureEntity {
            handle,
            chain: Vec::new(),
        })
        .unwrap();
    let out = dir.path().join("out.dxf");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();

    let items = Session::open(&out).unwrap().takeoff.items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, Kind::Count);
    assert_eq!(items[0].points, [[-40.0, 1.0]]);
}

#[test]
fn a_saved_dwg_leaves_out_an_erased_dimensions_block_and_a_grouped_lines_handle() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&qiyas_core::fixtures::write_sample(dir.path(), "dxf")).unwrap();
    let added = |s: &mut Session, cmd: Command| match s.run(cmd).unwrap() {
        qiyas_core::Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    let line = |a: [f64; 2], b: [f64; 2]| Command::Draw(qiyas_core::DrawSpec::Line { a, b });
    let a = added(&mut s, line([100.0, 0.0], [110.0, 0.0]));
    let b = added(&mut s, line([100.0, 5.0], [110.0, 5.0]));
    let t = |h: u64| qiyas_core::Target {
        handle: h,
        chain: Vec::new(),
    };
    s.run(Command::CreateGroup {
        name: "PAIR".into(),
        targets: vec![t(a), t(b)],
    })
    .unwrap();
    let anonymous_d = |doc: &opencadcodec::CadDocument| {
        doc.block_records
            .iter()
            .filter(|r| r.name.to_uppercase().starts_with("*D"))
            .count()
    };
    let before = anonymous_d(&s.drawing.doc);
    let dim = added(
        &mut s,
        Command::Dimension(qiyas_core::DimSpec::Linear {
            p1: [100.0, 0.0],
            p2: [110.0, 0.0],
            line_point: [105.0, -2.0],
            rotation: None,
        }),
    );
    assert_eq!(anonymous_d(&s.drawing.doc), before + 1);
    s.run(Command::DeleteEntities {
        targets: vec![t(dim), t(a)],
    })
    .unwrap();
    let out = dir.path().join("purged.dwg");
    s.save_as(&out).unwrap();

    let back = Session::open(&out).unwrap();
    assert_eq!(
        anonymous_d(&back.drawing.doc),
        before,
        "the erased dimension's block"
    );
    let group = back
        .info()
        .groups
        .into_iter()
        .find(|g| g.name == "PAIR")
        .unwrap();
    assert_eq!(group.handles.len(), 1, "{group:?}");
    // In memory nothing went: an undo brings the dimension back with its block.
    assert_eq!(anonymous_d(&s.drawing.doc), before + 1);
    assert!(s.undo());
    let EntityType::Dimension(d) = s
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(dim))
        .unwrap()
    else {
        panic!("the dimension");
    };
    assert!(s.drawing.doc.block_records.contains(&d.base().block_name));
    assert_eq!(
        s.info()
            .groups
            .iter()
            .find(|g| g.name == "PAIR")
            .unwrap()
            .handles
            .len(),
        2
    );
}

#[test]
fn embedded_count_rows_keep_zero_scope_note_and_source_in_dwg_and_dxf() {
    for ext in ["dwg", "dxf"] {
        let dir = tempfile::tempdir().unwrap();
        let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
        let mut session = Session::open(&source).unwrap();
        session
            .run(Command::CountBlock {
                block_name: "DOOR".into(),
                layer: None,
            })
            .unwrap();
        let mut count = session.takeoff.items[0].clone();
        count.source = "mcp".into();
        count.note = "Top-level references only; nested references are excluded. حدود العد".into();
        let mut zero = count.clone();
        zero.id = "zero-count".into();
        zero.value = 0.0;
        zero.shown = 0.0;
        zero.handles.clear();
        zero.points.clear();
        session.takeoff.items = vec![count.clone(), zero.clone()];
        let out = dir.path().join(format!("counts.{ext}"));
        for _ in 0..2 {
            session.save_as(&out).unwrap();
            std::fs::remove_file(project_path(&out)).unwrap();
            session = Session::open(&out).unwrap();
            assert_eq!(session.takeoff.items.len(), 2);
            for expected in [&count, &zero] {
                let actual = session
                    .takeoff
                    .items
                    .iter()
                    .find(|m| m.id == expected.id)
                    .unwrap();
                assert_eq!(actual.value, expected.value);
                assert_eq!(actual.note, expected.note);
                assert_eq!(actual.source, "mcp");
                assert_eq!(actual.count_scope, expected.count_scope);
                assert_eq!(actual.points.len(), if actual.value == 0.0 { 0 } else { 3 });
            }
            let carriers: Vec<_> = session
                .drawing
                .doc
                .entities()
                .filter(|e| e.common().layer == "QIYAS-COUNT")
                .collect();
            assert_eq!(carriers.len(), 4);
            assert_eq!(carriers.iter().filter(|e| e.common().invisible).count(), 1);
        }
    }
}

#[test]
fn legacy_embedded_count_metadata_remains_unspecified() {
    for len in [7, 8, 9, 10] {
        let mut doc = qiyas_core::fixtures::sample_doc();
        let mut m = user_length();
        m.kind = Kind::Count;
        m.points = vec![[1.0, 2.0]];
        embed_measurements(&mut doc, &[m]);
        let handles: Vec<_> = doc
            .entities()
            .filter(|e| e.common().layer == "QIYAS-COUNT")
            .map(|e| e.common().handle)
            .collect();
        for handle in handles {
            let entity = doc.get_entity_mut(handle).unwrap();
            let mut record = entity
                .common()
                .extended_data
                .get_record("QIYAS")
                .unwrap()
                .clone();
            record.values.truncate(len);
            if len == 10 {
                record.values[9] = XDataValue::String(
                    r#"{"area_origin":null,"measurement_basis":null,"count_scope":null}"#.into(),
                );
            }
            entity.common_mut().extended_data.add_record(record);
        }
        let read = read_embedded(&doc);
        assert_eq!(read.len(), 1, "legacy length {len}");
        assert_eq!(read[0].source, "user");
        assert!(read[0].note.is_empty());
        assert!(read[0].count_scope.is_none());
        assert_eq!(read[0].points, vec![[1.0, 2.0]]);
    }
}
