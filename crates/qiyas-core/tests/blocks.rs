//! Picking and measuring inside blocks, exact curves, dynamic blocks and over-deep blocks.

use std::f64::consts::{PI, TAU};

use opencadcodec::entities::Insert;
use opencadcodec::objects::{Dictionary, ObjectType};
use opencadcodec::{BlockRecord, CadDocument, Circle, EntityType, Handle, Line, Vector3};
use qiyas_core::{project_path, Command, CoreError, Kind, Measurement, Outcome, Picked, Session};

/// Opens the sample drawing (DXF, metres) in a session. The temp dir must outlive the test body.
fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

fn measure(session: &mut Session, cmd: Command) -> Measurement {
    match session.run(cmd).unwrap() {
        Outcome::Measured { measurement } => *measurement,
        other => panic!("expected a measurement, got {other:?}"),
    }
}

/// The sample's DOOR references (plain, rotated, scaled 2) and the leaf line inside DOOR.
fn doors(session: &Session) -> ([u64; 3], u64) {
    let inserts: Vec<u64> = session
        .drawing
        .entities_in("*Model_Space")
        .filter(|e| matches!(e, EntityType::Insert(_)))
        .map(|e| e.common().handle.value())
        .collect();
    let leaf = session
        .drawing
        .entities_in("DOOR")
        .next()
        .unwrap()
        .common()
        .handle
        .value();
    (inserts.try_into().unwrap(), leaf)
}

/// Adds a block definition named `name` holding `entities`; returns the record's handle.
fn add_block(doc: &mut CadDocument, name: &str, entities: Vec<EntityType>) -> Handle {
    let mut record = BlockRecord::new(name);
    record.handle = doc.allocate_handle();
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    for mut entity in entities {
        entity.common_mut().owner_handle = owner;
        doc.add_entity(entity).unwrap();
    }
    owner
}

#[test]
fn pick_inside_the_door_returns_the_leaf_line() {
    let (_dir, session) = sample_session();
    let ([_, _, scaled], leaf) = doors(&session);
    // The scaled DOOR at (9, 1): its leaf runs to (10.8, 1).
    let picked = session.pick([10.0, 1.0], 0.05).unwrap();
    assert_eq!(
        picked,
        Picked {
            top: scaled,
            entity: leaf,
            chain: vec![scaled],
            xref: None,
            kind: Some("Insert".to_string()),
        }
    );

    let info = session.entity_info(leaf, &picked.chain).unwrap();
    assert_eq!(info.kind, "Line");
    // On layer 0 inside the block, so drawn on the insert's layer.
    assert_eq!(info.layer, "A-DOOR");
    assert!((info.length.unwrap() - 1.8).abs() < 1e-9);
    assert_eq!(info.inside_block.as_deref(), Some("DOOR"));
    assert_eq!(info.chain, [scaled]);
    assert_eq!(info.block_path, ["DOOR"]);
    assert_eq!(info.block_name, None);

    // The whole block, as before.
    let block = session.entity_info(scaled, &[]).unwrap();
    assert_eq!(block.block_name.as_deref(), Some("DOOR"));
    assert_eq!(block.inside_block, None);
    // A block reference is counted: no length or area from the lines inside it.
    assert_eq!((block.length, block.area), (None, None));
}

#[test]
fn measure_entity_with_a_chain_measures_in_world_units() {
    let (_dir, mut session) = sample_session();
    let ([plain, rotated, scaled], leaf) = doors(&session);
    for (door, scale) in [(plain, 1.0), (rotated, 1.0), (scaled, 2.0)] {
        let m = measure(
            &mut session,
            Command::MeasureEntity {
                handle: leaf,
                chain: vec![door],
            },
        );
        assert_eq!(m.kind, Kind::Length);
        assert!((m.value - 0.9 * scale).abs() < 1e-9, "{}", m.value);
        assert_eq!(m.description, "Line on A-DOOR");
        assert_eq!(m.layer, "A-DOOR");
        assert_eq!(m.handles, [leaf]);
        assert_eq!(m.chain, [door]);
        assert_eq!(m.block_path, ["DOOR"]);
        assert!(m.auto_description);
    }

    // A chain that does not lead to the entity is refused.
    let line = session
        .drawing
        .entities_in("*Model_Space")
        .find(|e| matches!(e, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle
        .value();
    for (handle, chain) in [(line, vec![plain]), (leaf, vec![line])] {
        assert!(matches!(
            session.run(Command::MeasureEntity { handle, chain }),
            Err(CoreError::InvalidCommand(_))
        ));
    }
}

#[test]
fn arcs_and_circles_measure_exactly() {
    let (_dir, mut session) = sample_session();
    let circle = Circle::from_center_radius(Vector3::new(30.0, 0.0, 0.0), 1.0);
    let circle = session
        .drawing
        .doc
        .add_entity(EntityType::Circle(circle))
        .unwrap()
        .value();
    let arc = session
        .drawing
        .entities_in("*Model_Space")
        .find(|e| matches!(e, EntityType::Arc(_)))
        .unwrap()
        .common()
        .handle
        .value();

    // The half circle of radius 2: 2π, not the 6.282 of its chords.
    let m = measure(
        &mut session,
        Command::MeasureEntity {
            handle: arc,
            chain: Vec::new(),
        },
    );
    assert!((m.value - 2.0 * PI).abs() < 1e-9, "{}", m.value);

    let info = session.entity_info(circle, &[]).unwrap();
    assert!((info.length.unwrap() - TAU).abs() < 1e-9);
    assert!((info.area.unwrap() - PI).abs() < 1e-9);
}

#[test]
fn chain_survives_a_trip_through_the_drawing() {
    let dir = tempfile::tempdir().unwrap();
    let source = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&source).unwrap();
    let ([_, _, scaled], leaf) = doors(&session);
    session
        .run(Command::MeasureEntity {
            handle: leaf,
            chain: vec![scaled],
        })
        .unwrap();
    let out = dir.path().join("out.dwg");
    session.save_as(&out).unwrap();
    std::fs::remove_file(project_path(&out)).unwrap();

    let items = Session::open(&out).unwrap().takeoff.items;
    assert_eq!(items.len(), 1);
    let m = &items[0];
    assert_eq!(m.chain, [scaled]);
    assert_eq!(m.block_path, ["DOOR"]);
    assert!((m.value - 1.8).abs() < 1e-9);
    // Drawn where the scaled DOOR places the leaf.
    assert_eq!(m.points, [[9.0, 1.0], [10.8, 1.0]]);
    // "Line on A-DOOR" is a default name.
    assert!(m.auto_description);
}

#[test]
fn dynamic_block_copies_count_as_their_block() {
    let (_dir, mut session) = sample_session();
    let doc = &mut session.drawing.doc;
    let definition = add_block(doc, "DOOR-DYN", Vec::new());
    let leaf = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    add_block(doc, "*U7", vec![leaf]);

    // The anonymous copy's insert links back to DOOR-DYN through its extension dictionary.
    let mut insert = Insert::new("*U7", Vector3::new(40.0, 0.0, 0.0));
    insert.common.layer = "A-DOOR".to_string();
    let xdict = doc.allocate_handle();
    insert.common.xdictionary_handle = Some(xdict);
    let handle = doc.add_entity(EntityType::Insert(insert)).unwrap();
    let representation = doc.allocate_handle();
    for (h, owner) in [(xdict, handle), (representation, xdict)] {
        let mut dictionary = Dictionary::new();
        dictionary.handle = h;
        dictionary.owner = owner;
        doc.objects.insert(h, ObjectType::Dictionary(dictionary));
    }
    doc.block_representations.insert(representation, definition);
    let handle = handle.value();

    let info = session.entity_info(handle, &[]).unwrap();
    assert_eq!(info.block_name.as_deref(), Some("DOOR-DYN"));
    let summary = session.block_summary(None);
    assert!(
        summary
            .iter()
            .any(|b| b.block_name == "DOOR-DYN" && b.count == 1),
        "{summary:?}"
    );
    let m = measure(
        &mut session,
        Command::CountBlock {
            block_name: "DOOR-DYN".to_string(),
            layer: None,
        },
    );
    assert_eq!(m.handles, [handle]);
}

#[test]
fn too_deep_blocks_are_reported() {
    let (_dir, session) = sample_session();
    assert!(session.info().problems.is_empty());

    let (_dir, mut session) = sample_session();
    let doc = &mut session.drawing.doc;
    let inner = Insert::new("LOOP", Vector3::new(1.0, 0.0, 0.0));
    let line = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    add_block(doc, "LOOP", vec![line, EntityType::Insert(inner)]);
    let top = Insert::new("LOOP", Vector3::new(0.0, 50.0, 0.0));
    doc.add_entity(EntityType::Insert(top)).unwrap();
    assert_eq!(
        session.info().problems,
        ["Drawing too complex, some block content not drawn"]
    );
}

// ---- Blocks and attributes (Phase 3, Task 8) ----

use std::collections::BTreeMap;

use qiyas_core::{Current, DrawSpec, ModifySpec, Target};

fn top(h: u64) -> Target {
    Target {
        handle: h,
        chain: Vec::new(),
    }
}

fn added(outcome: Outcome) -> Vec<u64> {
    match outcome {
        Outcome::Added { handles } => handles,
        other => panic!("expected added entities, got {other:?}"),
    }
}

fn draw(session: &mut Session, spec: DrawSpec) -> u64 {
    added(session.run(Command::Draw(spec)).unwrap())[0]
}

fn set_current(session: &mut Session, layer: &str, color: &str) {
    session
        .run(Command::SetCurrent {
            current: Current {
                layer: layer.to_string(),
                color: color.to_string(),
                ..Current::default()
            },
        })
        .unwrap();
}

/// The handles at the top of model space.
fn sheet_handles(session: &Session) -> Vec<u64> {
    let mut handles: Vec<u64> = session
        .drawing
        .entities_in("*Model_Space")
        .map(|e| e.common().handle.value())
        .collect();
    handles.sort_unstable();
    handles
}

/// The ends of the line `h` (of the drawing, wherever it is).
fn line_ends(session: &Session, h: u64) -> [[f64; 2]; 2] {
    match session.drawing.doc.get_entity(Handle::new(h)) {
        Some(EntityType::Line(l)) => [[l.start.x, l.start.y], [l.end.x, l.end.y]],
        other => panic!("not a line: {other:?}"),
    }
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

fn insert_block(session: &mut Session, name: &str, at: [f64; 2], rotation: f64) -> u64 {
    added(
        session
            .run(Command::InsertBlock {
                name: name.to_string(),
                at,
                scale: 1.0,
                rotation,
                attribute_values: BTreeMap::new(),
                from_path: None,
            })
            .unwrap(),
    )[0]
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn a_block_from_three_layers_inserted_turned_draws_and_counts_its_parts_right() {
    let (_dir, mut session) = sample_session();
    // A red A-WALL line, a green A-DOOR line, and a ByBlock circle on layer 0.
    set_current(&mut session, "A-WALL", "ByLayer");
    let wall = draw(
        &mut session,
        DrawSpec::Line {
            a: [100.0, 0.0],
            b: [110.0, 0.0],
        },
    );
    set_current(&mut session, "A-DOOR", "ByLayer");
    let door = draw(
        &mut session,
        DrawSpec::Line {
            a: [100.0, 1.0],
            b: [102.0, 1.0],
        },
    );
    set_current(&mut session, "0", "ByBlock");
    let ring = draw(
        &mut session,
        DrawSpec::Circle {
            centre: [105.0, 5.0],
            radius: 1.0,
        },
    );
    let before = sheet_handles(&session);

    let outcome = session
        .run(Command::CreateBlock {
            name: "UNIT".to_string(),
            base: [100.0, 0.0],
            targets: vec![top(wall), top(door), top(ring)],
            convert_to_insert: false,
        })
        .unwrap();
    assert!(
        matches!(outcome, Outcome::Edited { ref added, unlinked: 0, .. } if added.is_empty()),
        "{outcome:?}"
    );
    // They left the sheet, and sit in the block relative to the base point.
    let now = sheet_handles(&session);
    assert!(![wall, door, ring].iter().any(|h| now.contains(h)));
    assert_eq!(now.len(), before.len() - 3);
    let inside: Vec<&EntityType> = session.drawing.entities_in("UNIT").collect();
    assert_eq!(inside.len(), 3);
    let lines: Vec<[[f64; 2]; 2]> = inside
        .iter()
        .filter_map(|e| match e {
            EntityType::Line(l) => Some([[l.start.x, l.start.y], [l.end.x, l.end.y]]),
            _ => None,
        })
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| near(l[0], [0.0, 0.0]) && near(l[1], [10.0, 0.0])),
        "{lines:?}"
    );

    // Inserted at (50, 0) turned a quarter turn, on A-DOOR (green).
    set_current(&mut session, "A-DOOR", "ByLayer");
    let unit = insert_block(&mut session, "UNIT", [50.0, 0.0], 90.0);

    // The wall line now runs from (50, 0) to (50, 10): red, on A-WALL.
    let picked = session.pick([50.0, 5.0], 0.05).unwrap();
    assert_eq!(picked.chain, [unit]);
    let wall_in = session.entity_info(picked.entity, &picked.chain).unwrap();
    assert_eq!(wall_in.layer, "A-WALL");
    assert_eq!(wall_in.color_rgb, [255, 0, 0]);
    assert!((wall_in.length.unwrap() - 10.0).abs() < 1e-9);
    // The door line from (49, 0) to (49, 2): green, on A-DOOR.
    let picked = session.pick([49.0, 1.0], 0.05).unwrap();
    let door_in = session.entity_info(picked.entity, &picked.chain).unwrap();
    assert_eq!(
        (door_in.layer.as_str(), door_in.color_rgb),
        ("A-DOOR", [0, 255, 0])
    );
    // The layer 0 ByBlock circle (centre (45, 5)) takes the reference's layer and colour.
    let picked = session.pick([46.0, 5.0], 0.05).unwrap();
    assert_eq!(picked.chain, [unit]);
    let ring_in = session.entity_info(picked.entity, &picked.chain).unwrap();
    assert_eq!(
        (ring_in.layer.as_str(), ring_in.color_rgb),
        ("A-DOOR", [0, 255, 0])
    );

    // It counts and measures in the world.
    let count = measure(
        &mut session,
        Command::CountBlock {
            block_name: "UNIT".to_string(),
            layer: None,
        },
    );
    assert_eq!(count.value, 1.0);
    let wall_row = measure(
        &mut session,
        Command::MeasureEntity {
            handle: wall_in.handle,
            chain: vec![unit],
        },
    );
    assert!((wall_row.value - 10.0).abs() < 1e-9);
    assert_eq!(wall_row.layer, "A-WALL");

    // Two measurements, the insert, then the block: each one undo step.
    for _ in 0..3 {
        assert!(session.undo());
    }
    assert!(session.drawing.doc.get_entity(Handle::new(unit)).is_none());
    assert!(session.undo());
    assert_eq!(sheet_handles(&session), before);
    assert!(session.drawing.doc.block_records.get("UNIT").is_none());
    assert!(session.redo());
    assert!(session.drawing.doc.block_records.get("UNIT").is_some());
}

#[test]
fn an_existing_or_invalid_block_name_is_refused() {
    let (_dir, mut session) = sample_session();
    let line = draw(
        &mut session,
        DrawSpec::Line {
            a: [0.0, 9.0],
            b: [1.0, 9.0],
        },
    );
    for name in ["DOOR", "door", "", "  ", "A<B", "X/Y", "*U9", "ok?"] {
        let refused = session.run(Command::CreateBlock {
            name: name.to_string(),
            base: [0.0, 0.0],
            targets: vec![top(line)],
            convert_to_insert: true,
        });
        assert!(
            matches!(refused, Err(CoreError::InvalidCommand(_))),
            "{name}: {refused:?}"
        );
    }
    // Nothing changed: the line is still on the sheet.
    assert!(sheet_handles(&session).contains(&line));
    // Inside a block is refused too.
    let ([plain, ..], leaf) = doors(&session);
    let refused = session.run(Command::CreateBlock {
        name: "LEAF".to_string(),
        base: [0.0, 0.0],
        targets: vec![Target {
            handle: leaf,
            chain: vec![plain],
        }],
        convert_to_insert: false,
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
}

#[test]
fn converting_to_a_reference_unlinks_the_rows_and_says_so() {
    let (_dir, mut session) = sample_session();
    let line = draw(
        &mut session,
        DrawSpec::Line {
            a: [0.0, 9.0],
            b: [4.0, 9.0],
        },
    );
    let row = measure(
        &mut session,
        Command::MeasureEntity {
            handle: line,
            chain: Vec::new(),
        },
    );
    let outcome = session
        .run(Command::CreateBlock {
            name: "BEAM".to_string(),
            base: [0.0, 9.0],
            targets: vec![top(line)],
            convert_to_insert: true,
        })
        .unwrap();
    let Outcome::Edited {
        added,
        removed,
        unlinked,
        ..
    } = outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(removed, [line]);
    assert_eq!(unlinked, 1);
    let insert = added[0];
    match session.drawing.doc.get_entity(Handle::new(insert)) {
        Some(EntityType::Insert(i)) => {
            assert_eq!(i.block_name, "BEAM");
            assert_eq!((i.insert_point.x, i.insert_point.y), (0.0, 9.0));
        }
        other => panic!("{other:?}"),
    }
    // The row keeps its value, without the link.
    let kept = session.takeoff.get(&row.id).unwrap();
    assert!((kept.value - 4.0).abs() < 1e-9);
    assert!(kept.handles.is_empty());
    // One undo puts the line and the row's link back.
    assert!(session.undo());
    assert_eq!(session.takeoff.get(&row.id).unwrap().handles, [line]);
    assert!(sheet_handles(&session).contains(&line));
}

/// A block TAGGED: a line 1 long and the attribute MARK ("Door mark", default "D1") at (0.5, 0),
/// 0.2 tall.
fn tagged_block(session: &mut Session) {
    let line = draw(
        session,
        DrawSpec::Line {
            a: [200.0, 0.0],
            b: [201.0, 0.0],
        },
    );
    let attdef = added(
        session
            .run(Command::AttDef {
                tag: "mark".to_string(),
                prompt: "Door mark".to_string(),
                default: "D1".to_string(),
                at: [200.5, 0.0],
                height: 0.2,
            })
            .unwrap(),
    )[0];
    // Drawn on the sheet as its tag, and picked.
    assert_eq!(
        session.pick([200.55, 0.2], 0.05).map(|p| p.entity),
        Some(attdef)
    );
    session
        .run(Command::CreateBlock {
            name: "TAGGED".to_string(),
            base: [200.0, 0.0],
            targets: vec![top(line), top(attdef)],
            convert_to_insert: false,
        })
        .unwrap();
}

#[test]
fn inserting_a_block_with_attributes_takes_the_values_given_for_its_tags() {
    let (_dir, mut session) = sample_session();
    tagged_block(&mut session);
    let blocks = session.blocks();
    let tagged = blocks.iter().find(|b| b.name == "TAGGED").unwrap();
    assert_eq!(tagged.attdefs.len(), 1);
    let a = &tagged.attdefs[0];
    assert_eq!(
        (a.tag.as_str(), a.prompt.as_str(), a.default.as_str()),
        ("MARK", "Door mark", "D1")
    );
    assert!(!a.constant);
    assert!(
        session
            .drawing
            .doc
            .block_records
            .get("TAGGED")
            .unwrap()
            .flags
            .has_attributes
    );
    // DOOR is listed with no attributes; layouts and anonymous blocks are not.
    assert!(blocks
        .iter()
        .any(|b| b.name == "DOOR" && b.attdefs.is_empty()));
    assert!(!blocks.iter().any(|b| b.name.starts_with('*')));

    let given = added(
        session
            .run(Command::InsertBlock {
                name: "tagged".to_string(),
                at: [20.0, 20.0],
                scale: 2.0,
                rotation: 90.0,
                attribute_values: values(&[("MARK", "D7")]),
                from_path: None,
            })
            .unwrap(),
    )[0];
    let info = session.entity_info(given, &[]).unwrap();
    assert_eq!(info.attributes, [("MARK".to_string(), "D7".to_string())]);
    match session.drawing.doc.get_entity(Handle::new(given)) {
        Some(EntityType::Insert(i)) => {
            let a = &i.attributes[0];
            // (0.5, 0) scaled 2 and turned a quarter turn from (20, 20).
            assert!(near(
                [a.insertion_point.x, a.insertion_point.y],
                [20.0, 21.0]
            ));
            assert!((a.height - 0.4).abs() < 1e-9);
            assert!((a.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
            assert!(!a.common.handle.is_null());
            assert_eq!(a.common.owner_handle.value(), given);
        }
        other => panic!("{other:?}"),
    }
    // No value given: the default.
    let plain = insert_block(&mut session, "TAGGED", [30.0, 20.0], 0.0);
    let info = session.entity_info(plain, &[]).unwrap();
    assert_eq!(info.attributes, [("MARK".to_string(), "D1".to_string())]);
    // A tag the block does not have is refused.
    let refused = session.run(Command::InsertBlock {
        name: "TAGGED".to_string(),
        at: [0.0, 0.0],
        scale: 1.0,
        rotation: 0.0,
        attribute_values: values(&[("SIZE", "900")]),
        from_path: None,
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
    // So is a block that does not exist, and a scale of 0.
    for (name, scale) in [("NOPE", 1.0), ("TAGGED", 0.0)] {
        let refused = session.run(Command::InsertBlock {
            name: name.to_string(),
            at: [0.0, 0.0],
            scale,
            rotation: 0.0,
            attribute_values: BTreeMap::new(),
            from_path: None,
        });
        assert!(
            matches!(refused, Err(CoreError::InvalidCommand(_))),
            "{name}"
        );
    }

    // Edit the attributes by tag: one step.
    session
        .run(Command::EditAttributes {
            target: top(given),
            values: values(&[("mark", "D8")]),
        })
        .unwrap();
    let info = session.entity_info(given, &[]).unwrap();
    assert_eq!(info.attributes, [("MARK".to_string(), "D8".to_string())]);
    let refused = session.run(Command::EditAttributes {
        target: top(given),
        values: values(&[("SIZE", "1")]),
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
    assert!(session.undo());
    let info = session.entity_info(given, &[]).unwrap();
    assert_eq!(info.attributes, [("MARK".to_string(), "D7".to_string())]);
}

#[test]
fn the_attribute_manager_edits_a_definition_and_syncs_the_references() {
    let (_dir, mut session) = sample_session();
    tagged_block(&mut session);
    let insert = added(
        session
            .run(Command::InsertBlock {
                name: "TAGGED".to_string(),
                at: [0.0, 30.0],
                scale: 1.0,
                rotation: 0.0,
                attribute_values: values(&[("MARK", "D7")]),
                from_path: None,
            })
            .unwrap(),
    )[0];
    session
        .run(Command::EditAttDef {
            block: "TAGGED".to_string(),
            tag: "MARK".to_string(),
            prompt: "Mark".to_string(),
            default: "D0".to_string(),
        })
        .unwrap();
    let a = &session
        .blocks()
        .into_iter()
        .find(|b| b.name == "TAGGED")
        .unwrap()
        .attdefs[0];
    assert_eq!((a.prompt.as_str(), a.default.as_str()), ("Mark", "D0"));

    // A second tag added in place, then the references synced: MARK keeps its value.
    session
        .run(Command::EditBlockInPlace {
            target: top(insert),
        })
        .unwrap();
    session
        .run(Command::AttDef {
            tag: "SIZE".to_string(),
            prompt: "Width".to_string(),
            default: "900".to_string(),
            at: [0.0, 29.0],
            height: 0.2,
        })
        .unwrap();
    session.run(Command::SaveBlockEdits).unwrap();
    assert_eq!(
        session.entity_info(insert, &[]).unwrap().attributes,
        [("MARK".to_string(), "D7".to_string())]
    );
    session
        .run(Command::SyncAttributes {
            block: "TAGGED".to_string(),
        })
        .unwrap();
    assert_eq!(
        session.entity_info(insert, &[]).unwrap().attributes,
        [
            ("MARK".to_string(), "D7".to_string()),
            ("SIZE".to_string(), "900".to_string())
        ]
    );
    assert!(session.undo());
    assert_eq!(
        session.entity_info(insert, &[]).unwrap().attributes.len(),
        1
    );
}

#[test]
fn a_written_block_reopens_with_its_entities_at_the_origin() {
    let (dir, mut session) = sample_session();
    let path = dir.path().join("door.dwg");
    session
        .run(Command::WriteBlock {
            name: "DOOR".to_string(),
            path: path.display().to_string(),
        })
        .unwrap();
    // Not an undo step, nothing changed in the drawing.
    assert!(!session.can_undo());
    let written = Session::open(&path).unwrap();
    let lines: Vec<&EntityType> = written.drawing.entities_in("*Model_Space").collect();
    assert_eq!(lines.len(), 1);
    match lines[0] {
        EntityType::Line(l) => {
            assert!(near([l.start.x, l.start.y], [0.0, 0.0]));
            assert!(near([l.end.x, l.end.y], [0.9, 0.0]));
        }
        other => panic!("{other:?}"),
    }

    // A block with entities on its own layers brings them along.
    set_current(&mut session, "A-WALL", "ByLayer");
    let wall = draw(
        &mut session,
        DrawSpec::Line {
            a: [5.0, 5.0],
            b: [6.0, 5.0],
        },
    );
    session
        .run(Command::CreateBlock {
            name: "W".to_string(),
            base: [5.0, 5.0],
            targets: vec![top(wall)],
            convert_to_insert: false,
        })
        .unwrap();
    let sub = dir.path().join("blocks");
    std::fs::create_dir(&sub).unwrap();
    let path = sub.join("w.dwg");
    session.write_block("W", &path).unwrap();
    let written = Session::open(&path).unwrap();
    assert!(written.drawing.doc.layers.contains("A-WALL"));
    let e = written.drawing.entities_in("*Model_Space").next().unwrap();
    assert_eq!(e.common().layer, "A-WALL");
}

#[test]
fn a_block_written_over_a_file_keeps_its_backup() {
    let (dir, session) = sample_session();
    // As the Save dialog does once the user said to replace it: a file already there.
    let path = dir.path().join("door.dwg");
    std::fs::write(&path, b"the old file").unwrap();
    session.write_block("DOOR", &path).unwrap();
    let bak = dir.path().join("door.dwg.bak");
    assert_eq!(std::fs::read(&bak).unwrap(), b"the old file");
    assert!(Session::open(&path).is_ok(), "the block was written whole");
    assert!(!dir.path().join("door.dwg.tmp").exists());
}

#[test]
fn a_block_written_outside_the_drawings_folder_is_refused() {
    let (_dir, mut session) = sample_session();
    let elsewhere = tempfile::tempdir().unwrap();
    let path = elsewhere.path().join("door.dwg");
    let refused = session.run(Command::WriteBlock {
        name: "DOOR".to_string(),
        path: path.display().to_string(),
    });
    assert!(
        matches!(refused, Err(CoreError::InvalidCommand(_))),
        "{refused:?}"
    );
    assert!(!path.exists());
    // Climbing out with .. is outside too.
    let sneaky = session.drawing.path.parent().unwrap().join("../door.dwg");
    let refused = session.run(Command::WriteBlock {
        name: "DOOR".to_string(),
        path: sneaky.display().to_string(),
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
    // An unknown block is refused.
    let refused = session.run(Command::WriteBlock {
        name: "NOPE".to_string(),
        path: "nope.dwg".to_string(),
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
}

#[test]
fn editing_a_block_in_place_changes_all_three_doors() {
    let (_dir, mut session) = sample_session();
    let ([plain, rotated, scaled], leaf) = doors(&session);
    let wall = session
        .drawing
        .entities_in("*Model_Space")
        .find(|e| matches!(e, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle
        .value();
    let before = sheet_handles(&session);

    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    assert_eq!(temps.len(), 1);
    assert_eq!(session.info().block_edit.as_deref(), Some("DOOR"));
    // The leaf, in the world where the turned door draws it.
    let [a, b] = line_ends(&session, temps[0]);
    assert!(near(a, [5.0, 1.0]) && near(b, [5.0, 1.9]), "{a:?} {b:?}");
    // Everything else is locked.
    let refused = session.run(Command::Modify(ModifySpec::Move {
        targets: vec![top(wall)],
        delta: [1.0, 0.0],
    }));
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
    // Saving the drawing waits for the edit to close.
    assert!(session.save().is_err());

    // The leaf doubles in length.
    session
        .run(Command::Modify(ModifySpec::Scale {
            targets: vec![top(temps[0])],
            base: [5.0, 1.0],
            factor: 2.0,
            copy: false,
        }))
        .unwrap();
    let closed = session.run(Command::SaveBlockEdits).unwrap();
    // The close is no undo step: the edit's own steps go with it, and the outcome says so.
    assert!(
        matches!(
            closed,
            Outcome::BlockEditClosed {
                saved: true,
                dropped: 1,
                unlinked: 0,
                ..
            }
        ),
        "{closed:?}"
    );
    assert!(!session.can_undo());
    assert_eq!(session.info().block_edit, None);
    assert_eq!(sheet_handles(&session), before);
    // The block's own leaf changed, under its own handle: every door draws it longer.
    let [a, b] = line_ends(&session, leaf);
    assert!(near(a, [0.0, 0.0]) && near(b, [1.8, 0.0]), "{a:?} {b:?}");
    for (door, scale) in [(plain, 1.0), (rotated, 1.0), (scaled, 2.0)] {
        let m = measure(
            &mut session,
            Command::MeasureEntity {
                handle: leaf,
                chain: vec![door],
            },
        );
        assert!((m.value - 1.8 * scale).abs() < 1e-9, "{}", m.value);
    }
    // The plain door's leaf now reaches (2.8, 1).
    assert_eq!(session.pick([2.6, 1.0], 0.05).map(|p| p.top), Some(plain));
    // Undo takes back the measurements only; the saved block stays.
    for _ in 0..3 {
        assert!(session.undo());
    }
    assert!(!session.undo());
    let [_, b] = line_ends(&session, leaf);
    assert!(near(b, [1.8, 0.0]));
}

#[test]
fn an_edit_begun_on_a_full_undo_stack_saves_without_duplicates() {
    let (_dir, mut session) = sample_session();
    let ([plain, rotated, scaled], leaf) = doors(&session);
    // 200 steps already: the oldest drop as the edit adds its own.
    for i in 0..200 {
        let x = f64::from(i);
        session
            .run(Command::MeasureDistance {
                points: vec![[x, 50.0], [x + 1.0, 50.0]].into(),
            })
            .unwrap();
    }
    let before = sheet_handles(&session);
    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    for _ in 0..3 {
        session
            .run(Command::Modify(ModifySpec::Move {
                targets: vec![top(temps[0])],
                delta: [0.0, 0.0],
            }))
            .unwrap();
    }
    session
        .run(Command::Modify(ModifySpec::Scale {
            targets: vec![top(temps[0])],
            base: [5.0, 1.0],
            factor: 2.0,
            copy: false,
        }))
        .unwrap();
    session.run(Command::SaveBlockEdits).unwrap();
    assert_eq!(sheet_handles(&session), before);
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
    for (door, scale) in [(plain, 1.0), (rotated, 1.0), (scaled, 2.0)] {
        let m = measure(
            &mut session,
            Command::MeasureEntity {
                handle: leaf,
                chain: vec![door],
            },
        );
        assert!((m.value - 1.8 * scale).abs() < 1e-9, "{}", m.value);
    }
    // Saving clears the history: the measurements just made are all there is to undo.
    for _ in 0..3 {
        assert!(session.undo());
    }
    assert!(!session.undo());
}

#[test]
fn undo_after_saving_an_edit_never_brings_back_an_old_take_off() {
    let (_dir, mut session) = sample_session();
    let ([plain, rotated, _], leaf) = doors(&session);
    let row = measure(
        &mut session,
        Command::MeasureEntity {
            handle: leaf,
            chain: vec![plain],
        },
    );
    let wall = session
        .drawing
        .entities_in("*Model_Space")
        .find(|e| matches!(e, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle
        .value();
    measure(
        &mut session,
        Command::MeasureEntity {
            handle: wall,
            chain: Vec::new(),
        },
    );
    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    session
        .run(Command::Modify(ModifySpec::Scale {
            targets: vec![top(temps[0])],
            base: [5.0, 1.0],
            factor: 2.0,
            copy: false,
        }))
        .unwrap();
    session.run(Command::SaveBlockEdits).unwrap();
    assert!((session.takeoff.get(&row.id).unwrap().value - 1.8).abs() < 1e-9);
    assert!(!session.can_undo());
    assert!(!session.undo());
    assert!((session.takeoff.get(&row.id).unwrap().value - 1.8).abs() < 1e-9);
}

#[test]
fn the_edited_block_cannot_be_redefined_meanwhile() {
    let (dir, mut session) = sample_session();
    let other = other_drawing(dir.path());
    let ([_, rotated, _], _) = doors(&session);
    session
        .run(Command::EditBlockInPlace {
            target: top(rotated),
        })
        .unwrap();
    let refused = session.run(Command::RedefineBlock {
        name: "door".to_string(),
        from_path: other.display().to_string(),
    });
    assert!(
        matches!(refused, Err(CoreError::InvalidCommand(_))),
        "{refused:?}"
    );
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
    session.run(Command::SaveBlockEdits).unwrap();
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
}

#[test]
fn a_batch_cannot_put_the_edited_block_inside_itself() {
    let (_dir, mut session) = sample_session();
    let ([_, rotated, _], _) = doors(&session);
    session
        .run(Command::EditBlockInPlace {
            target: top(rotated),
        })
        .unwrap();
    let before = sheet_handles(&session);
    let refused = session.run(Command::Batch {
        commands: vec![Command::InsertBlock {
            name: "door".to_string(),
            at: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            attribute_values: BTreeMap::new(),
            from_path: None,
        }],
        layer: None,
        color: None,
    });
    assert!(
        matches!(&refused, Err(CoreError::Step { index: 0, error, .. })
            if matches!(error.as_ref(), CoreError::InvalidCommand(m) if m.contains("inside itself"))),
        "{refused:?}"
    );
    assert_eq!(sheet_handles(&session), before);
}

#[test]
fn discarding_drops_what_was_added_and_keeps_work_outside_the_block() {
    let (_dir, mut session) = sample_session();
    let ([_, rotated, _], leaf) = doors(&session);
    let before = sheet_handles(&session);
    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    session
        .run(Command::DeleteEntities {
            targets: vec![top(temps[0])],
        })
        .unwrap();
    draw(
        &mut session,
        DrawSpec::Line {
            a: [0.0, 50.0],
            b: [1.0, 50.0],
        },
    );
    // Find and replace leaves the title alone: outside the block, it is locked.
    let replaced = session
        .run(Command::FindReplace {
            find: "GROUND".to_string(),
            replace: "FIRST".to_string(),
            scope: qiyas_core::TextScope::Sheet,
            match_case: true,
            whole_word: false,
        })
        .unwrap();
    assert!(
        matches!(
            replaced,
            Outcome::Replaced {
                count: 0,
                skipped: 1,
                ..
            }
        ),
        "{replaced:?}"
    );
    let closed = session.run(Command::DiscardBlockEdits).unwrap();
    assert!(
        matches!(closed, Outcome::BlockEditClosed { saved: false, .. }),
        "{closed:?}"
    );
    assert_eq!(sheet_handles(&session), before);
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
    assert!(session.drawing.doc.get_entity(Handle::new(leaf)).is_some());
    let title = session
        .drawing
        .entities_in("*Model_Space")
        .find_map(|e| match e {
            EntityType::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(title, "GROUND FLOOR PLAN");
}

#[test]
fn a_dimension_drawn_in_place_keeps_its_block_after_saving() {
    let (_dir, mut session) = sample_session();
    let ([plain, ..], _) = doors(&session);
    session
        .run(Command::EditBlockInPlace { target: top(plain) })
        .unwrap();
    let dim = added(
        session
            .run(Command::Dimension(qiyas_core::DimSpec::Linear {
                p1: [1.0, 1.0],
                p2: [1.9, 1.0],
                line_point: [1.5, 1.5],
                rotation: None,
            }))
            .unwrap(),
    )[0];
    let block = match session.drawing.doc.get_entity(Handle::new(dim)) {
        Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
        other => panic!("{other:?}"),
    };
    session.run(Command::SaveBlockEdits).unwrap();
    assert!(
        session.drawing.doc.block_records.get(&block).is_some(),
        "{block}"
    );
    let saved = session
        .drawing
        .entities_in("DOOR")
        .find_map(|e| match e {
            EntityType::Dimension(d) => Some(d.base().block_name.clone()),
            _ => None,
        })
        .expect("the dimension went into the block");
    assert_eq!(saved, block);
}

#[test]
fn picking_while_editing_in_place_sees_only_the_block() {
    let (_dir, mut session) = sample_session();
    let ([_, rotated, _], _) = doors(&session);
    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    // The wall line along y 0 is locked: not picked.
    assert_eq!(session.pick([7.0, 0.0], 0.05), None);
    assert_eq!(
        session.pick([5.0, 1.5], 0.05).map(|p| p.top),
        Some(temps[0])
    );
    let window = session.pick_window([[-100.0, -100.0], [100.0, 100.0]], false);
    assert_eq!(window.iter().map(|p| p.top).collect::<Vec<_>>(), temps);
    let crossing = session.pick_window([[-100.0, -100.0], [100.0, 100.0]], true);
    assert_eq!(crossing.iter().map(|p| p.top).collect::<Vec<_>>(), temps);
}

#[test]
fn saving_reports_the_rows_that_lose_their_link() {
    let (_dir, mut session) = sample_session();
    let ([_, rotated, _], _) = doors(&session);
    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    // Take-off works on the copies while editing; the copy goes when the edit closes.
    measure(
        &mut session,
        Command::MeasureEntity {
            handle: temps[0],
            chain: Vec::new(),
        },
    );
    let closed = session.run(Command::SaveBlockEdits).unwrap();
    assert!(
        matches!(closed, Outcome::BlockEditClosed { unlinked: 1, .. }),
        "{closed:?}"
    );
}

#[test]
fn only_block_making_commands_wait_for_the_edit() {
    let (dir, mut session) = sample_session();
    let ([plain, rotated, _], _) = doors(&session);
    let line = draw(
        &mut session,
        DrawSpec::Line {
            a: [0.0, 60.0],
            b: [1.0, 60.0],
        },
    );
    session
        .run(Command::EditBlockInPlace {
            target: top(rotated),
        })
        .unwrap();
    for cmd in [
        Command::EditBlockInPlace { target: top(plain) },
        Command::CreateBlock {
            name: "NEW".to_string(),
            base: [0.0, 0.0],
            targets: vec![top(line)],
            convert_to_insert: false,
        },
        Command::WriteBlock {
            name: "DOOR".to_string(),
            path: dir.path().join("d.dwg").display().to_string(),
        },
        Command::InsertBlock {
            name: "door".to_string(),
            at: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            attribute_values: BTreeMap::new(),
            from_path: None,
        },
    ] {
        assert!(matches!(
            session.run(cmd),
            Err(CoreError::InvalidCommand(_))
        ));
    }
    // The take-off and the sheets are not held up.
    session
        .run(Command::MeasureDistance {
            points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
        })
        .unwrap();
    session
        .run(Command::SetSheet {
            block: "*Model_Space".to_string(),
        })
        .unwrap();
}

#[test]
fn blocks_attribute_definitions_and_values_survive_save_and_reopen() {
    for ext in ["dwg", "dxf"] {
        let (dir, mut session) = sample_session();
        tagged_block(&mut session);
        let wall = draw(
            &mut session,
            DrawSpec::Line {
                a: [5.0, 5.0],
                b: [6.0, 5.0],
            },
        );
        session
            .run(Command::CreateBlock {
                name: "W".to_string(),
                base: [5.0, 5.0],
                targets: vec![top(wall)],
                convert_to_insert: true,
            })
            .unwrap();
        session
            .run(Command::InsertBlock {
                name: "TAGGED".to_string(),
                at: [40.0, 40.0],
                scale: 1.0,
                rotation: 0.0,
                attribute_values: values(&[("MARK", "D7")]),
                from_path: None,
            })
            .unwrap();
        let out = dir.path().join(format!("out.{ext}"));
        session.save_as(&out).unwrap();
        let back = Session::open(&out).unwrap();
        let blocks = back.blocks();
        let w = blocks.iter().find(|b| b.name == "W").expect(ext);
        assert!(w.attdefs.is_empty());
        assert_eq!(back.drawing.entities_in("W").count(), 1, "{ext}");
        let tagged = blocks.iter().find(|b| b.name == "TAGGED").expect(ext);
        let a = &tagged.attdefs[0];
        let read = (a.tag.as_str(), a.prompt.as_str(), a.default.as_str());
        assert_eq!(read, ("MARK", "Door mark", "D1"), "{ext}");
        let insert = back
            .drawing
            .entities_in("*Model_Space")
            .find_map(|e| match e {
                EntityType::Insert(i) if i.block_name == "TAGGED" => Some(i.clone()),
                _ => None,
            })
            .expect(ext);
        let attributes: Vec<(&str, &str)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.as_str(), a.value.as_str()))
            .collect();
        assert_eq!(attributes, [("MARK", "D7")], "{ext}");
        assert!(back
            .drawing
            .entities_in("*Model_Space")
            .any(|e| matches!(e, EntityType::Insert(i) if i.block_name == "W")));
    }
}

#[test]
fn a_block_is_never_written_over_the_drawing_or_an_existing_file() {
    let (dir, mut session) = sample_session();
    let copy = dir.path().join("sample.dwg");
    session.save_as(&copy).unwrap();
    // The drawing itself, by the command and by the Save dialog's path.
    let refused = session.run(Command::WriteBlock {
        name: "DOOR".to_string(),
        path: copy.display().to_string(),
    });
    assert!(
        matches!(refused, Err(CoreError::InvalidCommand(_))),
        "{refused:?}"
    );
    assert!(session.write_block("DOOR", &copy).is_err());
    // Any file there already, through the command.
    std::fs::copy(&copy, dir.path().join("other.dwg")).unwrap();
    let refused = session.run(Command::WriteBlock {
        name: "DOOR".to_string(),
        path: "other.dwg".to_string(),
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
}

#[test]
fn discarding_block_edits_puts_everything_back() {
    let (_dir, mut session) = sample_session();
    let ([_, rotated, _], leaf) = doors(&session);
    let before = sheet_handles(&session);
    let could_undo = session.can_undo();
    let temps = added(
        session
            .run(Command::EditBlockInPlace {
                target: top(rotated),
            })
            .unwrap(),
    );
    // Erase the leaf and draw another line: both undoable while editing.
    session
        .run(Command::DeleteEntities {
            targets: vec![top(temps[0])],
        })
        .unwrap();
    draw(
        &mut session,
        DrawSpec::Line {
            a: [0.0, 50.0],
            b: [1.0, 50.0],
        },
    );
    assert!(session.undo());
    // Nothing before the edit can be undone from inside it.
    assert!(session.undo());
    assert!(!session.undo());
    session.run(Command::DiscardBlockEdits).unwrap();
    assert_eq!(session.info().block_edit, None);
    assert_eq!(sheet_handles(&session), before);
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
    assert!(session.drawing.doc.get_entity(Handle::new(leaf)).is_some());
    assert_eq!(session.can_undo(), could_undo);
    assert!(!session.is_dirty());
    // Closing with nothing open is refused.
    assert!(session.run(Command::DiscardBlockEdits).is_err());
}

/// Writes `other.dxf` in `dir`: a DOOR block of two lines and a WIN block holding a line on its
/// own layer A-GLAZ.
fn other_drawing(dir: &std::path::Path) -> std::path::PathBuf {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    let mut glaz = opencadcodec::Layer::new("A-GLAZ");
    glaz.handle = doc.allocate_handle();
    glaz.color = opencadcodec::Color::Index(5);
    doc.layers.add(glaz).unwrap();
    let a = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    let b = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 0.0, 1.0, 0.0));
    add_block(&mut doc, "DOOR", vec![a, b]);
    let mut pane = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.2, 0.0, 0.0));
    pane.common_mut().layer = "A-GLAZ".to_string();
    add_block(&mut doc, "WIN", vec![pane]);
    qiyas_core::fixtures::write_drawing(&doc, &dir.join("other.dxf"))
}

#[test]
fn a_block_redefined_from_another_drawing_changes_every_reference() {
    let (dir, mut session) = sample_session();
    let other = other_drawing(dir.path());
    let blocks = qiyas_core::drawing_blocks(&other).unwrap();
    let names: Vec<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["DOOR", "WIN"]);
    session
        .run(Command::RedefineBlock {
            name: "DOOR".to_string(),
            from_path: other.display().to_string(),
        })
        .unwrap();
    assert_eq!(session.drawing.entities_in("DOOR").count(), 2);
    assert!(session.undo());
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
    // A block the other drawing lacks is refused.
    let refused = session.run(Command::RedefineBlock {
        name: "TAGGED".to_string(),
        from_path: other.display().to_string(),
    });
    assert!(matches!(refused, Err(CoreError::InvalidCommand(_))));
}

#[test]
fn inserting_from_another_drawing_copies_the_definition_and_its_layer() {
    let (dir, mut session) = sample_session();
    let other = other_drawing(dir.path());
    let insert = added(
        session
            .run(Command::InsertBlock {
                name: "WIN".to_string(),
                at: [60.0, 0.0],
                scale: 1.0,
                rotation: 0.0,
                attribute_values: BTreeMap::new(),
                from_path: Some(other.display().to_string()),
            })
            .unwrap(),
    )[0];
    assert_eq!(session.drawing.entities_in("WIN").count(), 1);
    assert_eq!(
        session.drawing.doc.layers.get("A-GLAZ").unwrap().color,
        opencadcodec::Color::Index(5)
    );
    let picked = session.pick([60.6, 0.0], 0.05).unwrap();
    assert_eq!(picked.chain, [insert]);
    // The host's own DOOR stays: a block it has already is never replaced.
    session
        .run(Command::InsertBlock {
            name: "DOOR".to_string(),
            at: [70.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            attribute_values: BTreeMap::new(),
            from_path: Some(other.display().to_string()),
        })
        .unwrap();
    assert_eq!(session.drawing.entities_in("DOOR").count(), 1);
    // One undo each: the second insert, then WIN with its definition and layer.
    assert!(session.undo());
    assert!(session.undo());
    assert!(session.drawing.doc.block_records.get("WIN").is_none());
    assert!(!session.drawing.doc.layers.contains("A-GLAZ"));
}

#[test]
fn block_commands_have_their_wire_forms() {
    let wire = serde_json::json!([
        {"type": "create_block", "name": "B", "base": [0.0, 0.0], "targets": [{"handle": 1}], "convert_to_insert": true},
        {"type": "insert_block", "name": "B", "at": [1.0, 2.0], "scale": 1.0, "rotation": 0.0, "attribute_values": {"MARK": "D1"}},
        {"type": "write_block", "name": "B", "path": "b.dwg"},
        {"type": "edit_block_in_place", "target": {"handle": 2}},
        {"type": "save_block_edits"},
        {"type": "discard_block_edits"},
        {"type": "att_def", "tag": "T", "prompt": "P", "default": "D", "at": [0.0, 0.0], "height": 1.0},
        {"type": "edit_attributes", "target": {"handle": 2}, "values": {"T": "x"}},
        {"type": "edit_att_def", "block": "B", "tag": "T", "prompt": "P", "default": "D"},
        {"type": "sync_attributes", "block": "B"},
        {"type": "redefine_block", "name": "B", "from_path": "o.dwg"}
    ]);
    let commands: Vec<Command> = serde_json::from_value(wire).unwrap();
    assert_eq!(commands.len(), 11);
    assert!(matches!(
        &commands[1],
        Command::InsertBlock {
            from_path: None,
            ..
        }
    ));
}

#[test]
fn saving_an_edit_that_changed_nothing_is_no_step() {
    let (_dir, mut session) = sample_session();
    let ([plain, ..], leaf) = doors(&session);
    let before = session.drawing.doc.get_entity(Handle::new(leaf)).cloned();
    session
        .run(Command::EditBlockInPlace { target: top(plain) })
        .unwrap();
    session.run(Command::SaveBlockEdits).unwrap();
    assert!(!session.can_undo());
    assert_eq!(
        session.drawing.doc.get_entity(Handle::new(leaf)).cloned(),
        before
    );
}
