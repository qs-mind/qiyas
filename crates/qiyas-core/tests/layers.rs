//! Layers beyond hide and show: named layer states (as AutoCAD keeps them), layer properties
//! edited in the drawing, new, deleted and renamed layers, the layer translator, per-viewport
//! freezes, and groups. Each change is one undo step.

use opencadcodec::entities::Viewport;
use opencadcodec::{CadDocument, Color, EntityType, Handle, Layer, Line, LineType};
use qiyas_core::fixtures::{sample_doc, write_drawing, write_sample, write_xref_pair};
use qiyas_core::{Command, CoreError, Kind, LayerMapping, LayerProp, Outcome, Session, Target};
use serde_json::json;

const MODEL: &str = "*Model_Space";

fn open(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    let s = Session::open(&path).unwrap();
    (dir, s)
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn layer<'a>(s: &'a Session, name: &str) -> &'a Layer {
    s.drawing.doc.layers.get(name).unwrap()
}

fn set_layer(s: &mut Session, name: &str, prop: LayerProp, value: serde_json::Value) {
    s.run(Command::SetLayer {
        name: name.into(),
        prop,
        value,
    })
    .unwrap();
}

/// The handles of the model space entities on `layer`.
fn on_layer(s: &Session, layer: &str) -> Vec<u64> {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .filter(|e| e.common().layer.eq_ignore_ascii_case(layer))
        .map(|e| e.common().handle.value())
        .collect()
}

/// The handles drawn in the batch of the sheet on screen.
fn drawn(s: &Session) -> std::collections::BTreeSet<u64> {
    s.batch().ranges.iter().map(|r| r.top).collect()
}

fn refused(s: &mut Session, cmd: Command) -> String {
    let undo = s.can_undo();
    let err = s.run(cmd).unwrap_err();
    assert_eq!(s.can_undo(), undo, "a refused command is no step");
    match err {
        CoreError::InvalidCommand(why) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_layer_state_restores_every_layer_and_round_trips_through_dwg() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    s.run(Command::SetLayerHidden {
        name: "A-DOOR".into(),
        hidden: true,
    })
    .unwrap();
    set_layer(&mut s, "A-WALL", LayerProp::Color, json!("ACI 5"));
    s.run(Command::SaveLayerState {
        name: "Plan".into(),
    })
    .unwrap();
    assert_eq!(s.info().layer_states, vec!["Plan".to_string()]);

    // Everything changes afterwards.
    set_layer(&mut s, "A-WALL", LayerProp::Color, json!("ACI 2"));
    set_layer(&mut s, "A-WALL", LayerProp::Frozen, json!(true));
    set_layer(&mut s, "A-DOOR", LayerProp::Locked, json!(true));
    set_layer(&mut s, "A-DOOR", LayerProp::Lineweight, json!(50));
    s.run(Command::SetLayerHidden {
        name: "A-DOOR".into(),
        hidden: false,
    })
    .unwrap();

    s.run(Command::RestoreLayerState {
        name: "PLAN".into(),
    })
    .unwrap();
    let wall = layer(&s, "A-WALL");
    assert_eq!(wall.color, Color::Index(5));
    assert!(!wall.flags.frozen);
    let door = layer(&s, "A-DOOR");
    assert!(!door.flags.locked);
    assert_ne!(door.line_weight.value(), 50);
    // The hidden layers come back too: Qiyas keeps them beside the state.
    assert_eq!(s.hidden_layers(), vec!["A-DOOR".to_string()]);

    // Undo takes the restore back; again the save.
    assert!(s.undo());
    assert!(layer(&s, "A-WALL").flags.frozen);

    // AutoCAD's Layer States Manager sees it: the state is in the DWG.
    s.redo();
    s.save().unwrap();
    let reopened = Session::open(&path).unwrap();
    let states = reopened.drawing.doc.layer_states();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].name, "Plan");
    let saved = states[0]
        .layers
        .iter()
        .find(|l| l.layer_name == "A-WALL")
        .unwrap();
    assert_eq!(saved.color, Color::Index(5));
    assert_eq!(reopened.info().layer_states, vec!["Plan".to_string()]);
}

#[test]
fn layer_states_rename_delete_and_undo() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    s.run(Command::SaveLayerState { name: "A".into() }).unwrap();
    s.run(Command::RenameLayerState {
        from: "a".into(),
        to: "Walls".into(),
    })
    .unwrap();
    assert_eq!(s.info().layer_states, vec!["Walls".to_string()]);
    s.run(Command::SaveLayerState { name: "B".into() }).unwrap();
    let why = refused(
        &mut s,
        Command::RenameLayerState {
            from: "B".into(),
            to: "walls".into(),
        },
    );
    assert!(why.contains("Walls"), "{why}");
    s.run(Command::DeleteLayerState {
        name: "Walls".into(),
    })
    .unwrap();
    assert_eq!(s.info().layer_states, vec!["B".to_string()]);
    let why = refused(
        &mut s,
        Command::RestoreLayerState {
            name: "Walls".into(),
        },
    );
    assert!(why.contains("Walls"), "{why}");
    assert!(refused(&mut s, Command::SaveLayerState { name: " ".into() }).contains("name"));
    assert!(s.undo());
    assert_eq!(
        s.info().layer_states,
        vec!["Walls".to_string(), "B".to_string()]
    );
    // Undoing every step leaves the drawing with no states.
    while s.undo() {}
    assert!(s.drawing.doc.layer_states().is_empty());
}

#[test]
fn off_and_frozen_layers_leave_the_batch_and_the_counts() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let doors = on_layer(&s, "A-DOOR");
    let walls = on_layer(&s, "A-WALL");
    assert!(doors.iter().all(|h| drawn(&s).contains(h)));

    set_layer(&mut s, "A-DOOR", LayerProp::On, json!(false));
    assert!(layer(&s, "A-DOOR").flags.off);
    assert!(doors.iter().all(|h| !drawn(&s).contains(h)));
    let info = s.info();
    let door_info = info.layers.iter().find(|l| l.name == "A-DOOR").unwrap();
    assert!(door_info.off);

    set_layer(&mut s, "A-DOOR", LayerProp::On, json!(true));
    set_layer(&mut s, "A-WALL", LayerProp::Frozen, json!(true));
    assert!(walls.iter().all(|h| !drawn(&s).contains(h)));
    // Frozen doors count nothing.
    set_layer(&mut s, "A-DOOR", LayerProp::Frozen, json!(true));
    let counted = s
        .run(Command::CountBlock {
            block_name: "DOOR".into(),
            layer: None,
        })
        .unwrap();
    match counted {
        Outcome::Measured { measurement } => assert_eq!(measurement.value, 0.0),
        other => panic!("{other:?}"),
    }

    // Each was one step.
    s.undo(); // the count
    s.undo();
    s.undo();
    assert!(!layer(&s, "A-WALL").flags.frozen);
    assert!(walls.iter().all(|h| drawn(&s).contains(h)));
}

#[test]
fn layer_colour_linetype_weight_lock_and_plot_are_edited() {
    let mut doc = sample_doc();
    let mut dashed = LineType::new("DASHED");
    dashed.handle = doc.allocate_handle();
    doc.line_types.add(dashed).unwrap();
    let (_dir, mut s) = open(&doc, "plan.dxf");

    set_layer(&mut s, "a-wall", LayerProp::Color, json!("#102030"));
    assert_eq!(
        layer(&s, "A-WALL").color,
        Color::Rgb {
            r: 0x10,
            g: 0x20,
            b: 0x30
        }
    );
    set_layer(&mut s, "A-WALL", LayerProp::Linetype, json!("dashed"));
    assert_eq!(layer(&s, "A-WALL").line_type, "DASHED");
    set_layer(&mut s, "A-WALL", LayerProp::Lineweight, json!(35));
    assert_eq!(layer(&s, "A-WALL").line_weight.value(), 35);
    set_layer(&mut s, "A-WALL", LayerProp::Locked, json!(true));
    assert!(layer(&s, "A-WALL").flags.locked);
    set_layer(&mut s, "A-WALL", LayerProp::Plot, json!(false));
    assert!(!layer(&s, "A-WALL").is_plottable);
    let info = s.info();
    let wall = info.layers.iter().find(|l| l.name == "A-WALL").unwrap();
    assert_eq!(wall.color_name, "#102030");
    assert_eq!(wall.linetype, "DASHED");
    assert_eq!(wall.lineweight, 35);
    assert!(wall.locked && !wall.plot);
    assert!(info.linetypes.iter().any(|l| l == "DASHED"));

    for (prop, value) in [
        (LayerProp::Color, json!("ByLayer")),
        (LayerProp::Color, json!("red")),
        (LayerProp::Linetype, json!("NOPE")),
        (LayerProp::Lineweight, json!(-1)),
        (LayerProp::Lineweight, json!(7)),
        (LayerProp::On, json!("yes")),
    ] {
        refused(
            &mut s,
            Command::SetLayer {
                name: "A-WALL".into(),
                prop,
                value,
            },
        );
    }
    refused(
        &mut s,
        Command::SetLayer {
            name: "NOPE".into(),
            prop: LayerProp::On,
            value: json!(false),
        },
    );
    // Five steps.
    for _ in 0..5 {
        assert!(s.undo());
    }
    assert_eq!(layer(&s, "A-WALL").color, Color::Index(1));
    assert!(!s.can_undo());
}

#[test]
fn new_and_deleted_layers_and_what_cant_be_deleted() {
    let mut doc = sample_doc();
    let mut defpoints = Layer::new("Defpoints");
    defpoints.handle = doc.allocate_handle();
    doc.layers.add(defpoints).unwrap();
    let (_dir, mut s) = open(&doc, "plan.dxf");
    s.run(Command::NewLayer {
        name: "S-COLS".into(),
    })
    .unwrap();
    assert!(s.drawing.doc.layers.contains("S-COLS"));
    assert!(refused(
        &mut s,
        Command::NewLayer {
            name: "s-cols".into()
        }
    )
    .contains("S-COLS"));
    assert!(refused(&mut s, Command::NewLayer { name: "A|B".into() }).contains("name"));

    for name in ["0", "Defpoints", "A-WALL", "NOPE"] {
        refused(&mut s, Command::DeleteLayer { name: name.into() });
    }
    s.run(Command::SetCurrent {
        current: qiyas_core::Current {
            layer: "S-COLS".into(),
            ..Default::default()
        },
    })
    .unwrap();
    let why = refused(
        &mut s,
        Command::DeleteLayer {
            name: "S-COLS".into(),
        },
    );
    assert!(why.contains("current"), "{why}");
    s.run(Command::SetCurrent {
        current: Default::default(),
    })
    .unwrap();
    s.run(Command::DeleteLayer {
        name: "s-cols".into(),
    })
    .unwrap();
    assert!(!s.drawing.doc.layers.contains("S-COLS"));
    assert!(s.undo());
    assert!(s.drawing.doc.layers.contains("S-COLS"));
}

#[test]
fn a_renamed_layer_takes_its_entities_the_takeoff_and_the_current_layer() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let walls = on_layer(&s, "A-WALL");
    let line = walls[0];
    s.run(Command::MeasureEntity {
        handle: line,
        chain: vec![],
    })
    .unwrap();
    s.run(Command::SetCurrent {
        current: qiyas_core::Current {
            layer: "A-WALL".into(),
            ..Default::default()
        },
    })
    .unwrap();
    s.run(Command::SetLayerHidden {
        name: "A-WALL".into(),
        hidden: true,
    })
    .unwrap();

    s.run(Command::RenameLayer {
        from: "a-wall".into(),
        to: "S-WALL".into(),
    })
    .unwrap();
    assert!(!s.drawing.doc.layers.contains("A-WALL"));
    assert_eq!(on_layer(&s, "S-WALL"), walls);
    assert_eq!(s.takeoff.items[0].layer, "S-WALL");
    assert_eq!(s.current.layer, "S-WALL");
    assert_eq!(s.hidden_layers(), vec!["S-WALL".to_string()]);

    for (from, to) in [
        ("A-DOOR", "s-wall"),
        ("0", "ZERO"),
        ("NOPE", "X"),
        ("A-DOOR", ""),
    ] {
        refused(
            &mut s,
            Command::RenameLayer {
                from: from.into(),
                to: to.into(),
            },
        );
    }

    assert!(s.undo());
    assert_eq!(on_layer(&s, "A-WALL"), walls);
    assert_eq!(s.takeoff.items[0].layer, "A-WALL");
    assert!(s.drawing.doc.layers.contains("A-WALL"));
    // The session follows the undo: drawing goes on, on the layer as it was named.
    assert_eq!(s.current.layer, "A-WALL");
    assert_eq!(s.hidden_layers(), vec!["A-WALL".to_string()]);
    // And the redo.
    assert!(s.redo());
    assert_eq!(s.current.layer, "S-WALL");
    assert_eq!(s.hidden_layers(), vec!["S-WALL".to_string()]);
    assert!(s.undo());
    s.run(Command::Draw(qiyas_core::DrawSpec::Line {
        a: [0.0, 50.0],
        b: [1.0, 50.0],
    }))
    .unwrap();
}

#[test]
fn undoing_a_deleted_layer_state_brings_back_its_hidden_layers() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let hide = |s: &mut Session, hidden: bool| {
        s.run(Command::SetLayerHidden {
            name: "A-DOOR".into(),
            hidden,
        })
        .unwrap();
    };
    hide(&mut s, true);
    s.run(Command::SaveLayerState { name: "A".into() }).unwrap();
    hide(&mut s, false);
    s.run(Command::DeleteLayerState { name: "A".into() })
        .unwrap();
    assert!(s.undo());
    s.run(Command::RestoreLayerState { name: "A".into() })
        .unwrap();
    assert_eq!(s.hidden_layers(), vec!["A-DOOR".to_string()]);
}

#[test]
fn restoring_a_state_never_freezes_the_current_layer() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    set_layer(&mut s, "A-WALL", LayerProp::Frozen, json!(true));
    set_layer(&mut s, "A-DOOR", LayerProp::Frozen, json!(true));
    s.run(Command::SaveLayerState { name: "F".into() }).unwrap();
    set_layer(&mut s, "A-WALL", LayerProp::Frozen, json!(false));
    set_layer(&mut s, "A-DOOR", LayerProp::Frozen, json!(false));
    s.run(Command::SetCurrent {
        current: qiyas_core::Current {
            layer: "A-WALL".into(),
            ..Default::default()
        },
    })
    .unwrap();
    s.run(Command::RestoreLayerState { name: "F".into() })
        .unwrap();
    assert!(
        !layer(&s, "A-WALL").flags.frozen,
        "the current layer stays thawed"
    );
    assert!(layer(&s, "A-DOOR").flags.frozen);
}

#[test]
fn entities_on_a_locked_layer_are_never_changed() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let line = on_layer(&s, "A-WALL")[0];
    let door = on_layer(&s, "A-DOOR")[0];
    set_layer(&mut s, "A-WALL", LayerProp::Locked, json!(true));
    let before = s.drawing.doc.get_entity(Handle::new(line)).cloned();
    let moving = Command::Modify(qiyas_core::ModifySpec::Move {
        targets: vec![t(door), t(line)],
        delta: [1.0, 1.0],
    });
    let why = refused(&mut s, moving);
    assert!(why.contains("locked") && why.contains("A-WALL"), "{why}");
    assert_eq!(s.drawing.doc.get_entity(Handle::new(line)).cloned(), before);
    refused(
        &mut s,
        Command::DeleteEntities {
            targets: vec![t(line)],
        },
    );
    refused(
        &mut s,
        Command::SetProperty {
            target: t(line),
            prop: qiyas_core::EntityProp::Color,
            value: json!("ACI 2"),
        },
    );
    refused(
        &mut s,
        Command::MatchProperties {
            source: t(door),
            targets: vec![t(line)],
            props: vec![qiyas_core::MatchProp::Color],
        },
    );
    // A locked entity may still be the source, and join a group.
    s.run(Command::MatchProperties {
        source: t(line),
        targets: vec![t(door)],
        props: vec![qiyas_core::MatchProp::Color],
    })
    .unwrap();
    s.run(Command::CreateGroup {
        name: "G".into(),
        targets: vec![t(line)],
    })
    .unwrap();
    // Unlocked, it moves.
    set_layer(&mut s, "A-WALL", LayerProp::Locked, json!(false));
    s.run(Command::Modify(qiyas_core::ModifySpec::Move {
        targets: vec![t(line)],
        delta: [1.0, 1.0],
    }))
    .unwrap();
}

#[test]
fn the_translator_renames_merges_and_recolours_in_one_step() {
    let mut doc = sample_doc();
    let mut old = Layer::new("OLD-DOOR");
    old.handle = doc.allocate_handle();
    doc.layers.add(old).unwrap();
    let mut stray = EntityType::Line(Line::from_coords(0.0, 9.0, 0.0, 1.0, 9.0, 0.0));
    stray.common_mut().layer = "OLD-DOOR".into();
    doc.add_entity(stray).unwrap();
    let (dir, mut s) = open(&doc, "plan.dxf");
    let doors = on_layer(&s, "A-DOOR");
    let old_ones = on_layer(&s, "OLD-DOOR");

    // The standard: a drawing whose S-WALL is blue.
    let mut standard = CadDocument::new();
    let mut wall = Layer::new("S-WALL");
    wall.handle = standard.allocate_handle();
    wall.color = Color::Index(5);
    standard.layers.add(wall).unwrap();
    let standard = write_drawing(&standard, &dir.path().join("standard.dxf"));
    let theirs = qiyas_core::drawing_layers(&standard).unwrap();
    assert!(theirs.iter().any(|l| l.name == "S-WALL"));

    s.run(Command::TranslateLayers {
        from_path: Some(standard.display().to_string()),
        mappings: vec![
            LayerMapping {
                from: "A-WALL".into(),
                to: "S-WALL".into(),
                color: None,
            },
            LayerMapping {
                from: "OLD-DOOR".into(),
                to: "A-DOOR".into(),
                color: Some("ACI 6".into()),
            },
        ],
    })
    .unwrap();
    assert!(!s.drawing.doc.layers.contains("A-WALL"));
    assert_eq!(layer(&s, "S-WALL").color, Color::Index(5));
    // Merged: OLD-DOOR's line is on A-DOOR now, and OLD-DOOR is gone.
    assert!(!s.drawing.doc.layers.contains("OLD-DOOR"));
    let mut both: Vec<u64> = doors.iter().chain(&old_ones).copied().collect();
    both.sort();
    let mut now = on_layer(&s, "A-DOOR");
    now.sort();
    assert_eq!(now, both);
    assert_eq!(layer(&s, "A-DOOR").color, Color::Index(6));

    assert!(refused(
        &mut s,
        Command::TranslateLayers {
            from_path: None,
            mappings: vec![LayerMapping {
                from: "NOPE".into(),
                to: "A-DOOR".into(),
                color: None
            }],
        }
    )
    .contains("NOPE"));

    // One step.
    assert!(s.undo());
    assert!(s.drawing.doc.layers.contains("A-WALL"));
    assert!(s.drawing.doc.layers.contains("OLD-DOOR"));
    assert_eq!(on_layer(&s, "OLD-DOOR"), old_ones);
}

#[test]
fn a_viewport_freezes_a_layer_of_its_own() {
    let mut doc = sample_doc();
    let mut vp = Viewport::new();
    vp.id = 2;
    vp.width = 100.0;
    vp.height = 80.0;
    let vp = doc
        .add_entity_to_layout(EntityType::Viewport(vp), "Layout1")
        .unwrap()
        .value();
    let (dir, mut s) = open(&doc, "plan.dwg");
    let paper = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&paper.block).unwrap();
    assert_eq!(s.info().viewports.len(), 1);
    assert!(s.info().viewports[0].frozen.is_empty());

    s.run(Command::SetViewportFrozen {
        viewport: t(vp),
        layer: "a-wall".into(),
        frozen: true,
    })
    .unwrap();
    let wall = layer(&s, "A-WALL").handle;
    match s.drawing.doc.get_entity(Handle::new(vp)) {
        Some(EntityType::Viewport(v)) => assert_eq!(v.frozen_layers, vec![wall]),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.info().viewports[0].frozen, vec!["A-WALL".to_string()]);
    // Only in that viewport: the layer itself stays thawed.
    assert!(!layer(&s, "A-WALL").flags.frozen);

    s.save().unwrap();
    let reopened = Session::open(&dir.path().join("plan.dwg")).unwrap();
    match reopened.drawing.doc.get_entity(Handle::new(vp)) {
        Some(EntityType::Viewport(v)) => assert_eq!(v.frozen_layers.len(), 1),
        other => panic!("{other:?}"),
    }

    s.run(Command::SetViewportFrozen {
        viewport: t(vp),
        layer: "A-WALL".into(),
        frozen: false,
    })
    .unwrap();
    assert!(s.info().viewports[0].frozen.is_empty());
    // A line is no viewport.
    s.set_sheet(MODEL).unwrap();
    let line = on_layer(&s, "A-WALL")[0];
    refused(
        &mut s,
        Command::SetViewportFrozen {
            viewport: t(line),
            layer: "A-WALL".into(),
            frozen: true,
        },
    );
}

#[test]
fn groups_are_made_edited_and_kept_in_the_dwg() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    let walls = on_layer(&s, "A-WALL");
    let doors = on_layer(&s, "A-DOOR");

    s.run(Command::CreateGroup {
        name: "Room 1".into(),
        targets: walls[..2].iter().map(|&h| t(h)).collect(),
    })
    .unwrap();
    let groups = s.info().groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "Room 1");
    assert_eq!(groups[0].handles, walls[..2].to_vec());
    // Each member's kind, so a click on the group can leave out what a tool can't take.
    assert_eq!(
        groups[0].kinds,
        vec!["Line".to_string(), "LwPolyline".to_string()]
    );

    assert!(refused(
        &mut s,
        Command::CreateGroup {
            name: "room 1".into(),
            targets: vec![t(doors[0])],
        }
    )
    .contains("Room 1"));
    refused(
        &mut s,
        Command::CreateGroup {
            name: "Empty".into(),
            targets: vec![],
        },
    );

    s.run(Command::AddToGroup {
        name: "ROOM 1".into(),
        targets: vec![t(doors[0])],
    })
    .unwrap();
    s.run(Command::RemoveFromGroup {
        name: "Room 1".into(),
        targets: vec![t(walls[0])],
    })
    .unwrap();
    assert_eq!(s.info().groups[0].handles, vec![walls[1], doors[0]]);

    s.save().unwrap();
    let reopened = Session::open(&path).unwrap();
    let kept = reopened.info().groups;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "Room 1");
    assert_eq!(kept[0].handles, vec![walls[1], doors[0]]);

    // Ungroup by an entity in it: the group goes, the entities stay.
    s.run(Command::Ungroup {
        targets: vec![t(doors[0])],
    })
    .unwrap();
    assert!(s.info().groups.is_empty());
    assert!(s.drawing.doc.get_entity(Handle::new(doors[0])).is_some());
    assert!(s.undo());
    s.run(Command::DeleteGroup {
        name: "room 1".into(),
    })
    .unwrap();
    assert!(s.info().groups.is_empty());
    refused(
        &mut s,
        Command::DeleteGroup {
            name: "Room 1".into(),
        },
    );
    assert!(s.undo());
    assert_eq!(s.info().groups.len(), 1);
}

#[test]
fn an_xrefs_entities_never_join_a_group() {
    let dir = tempfile::tempdir().unwrap();
    let host = write_xref_pair(dir.path());
    let mut s = Session::open(&host).unwrap();
    let xref = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle
        .value();
    let why = refused(
        &mut s,
        Command::CreateGroup {
            name: "G".into(),
            targets: vec![Target {
                handle: 0x30,
                chain: vec![xref],
            }],
        },
    );
    assert!(why.contains("read-only"), "{why}");
    // The take-off stays as it was through all of this.
    assert!(s.takeoff.items.iter().all(|m| m.kind != Kind::Item));
}

#[test]
fn find_replace_and_sync_attributes_leave_locked_entities_but_fields_follow() {
    let (_dir, mut s) = open(&qiyas_core::fixtures::schedule_doc(), "locked.dxf");
    let added = |o: Outcome| match o {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    let text = added(
        s.run(Command::Text {
            at: [100.0, 90.0],
            height: 0.25,
            rotation: 0.0,
            value: "ZQX".into(),
            style: None,
        })
        .unwrap(),
    );
    let rect = added(
        s.run(Command::Draw(qiyas_core::DrawSpec::Rectangle {
            a: [100.0, 100.0],
            b: [102.0, 101.0],
        }))
        .unwrap(),
    );
    let field = added(
        s.run(Command::Field {
            at: [100.0, 95.0],
            height: 0.25,
            field: qiyas_core::FieldKind::Area { target: t(rect) },
        })
        .unwrap(),
    );
    let tag = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(i) if i.block_name == "TAG"))
        .map(|e| e.common().handle.value())
        .unwrap();
    for h in [text, field, tag] {
        s.run(Command::SetProperty {
            target: t(h),
            prop: qiyas_core::EntityProp::Layer,
            value: json!("A-DOOR"),
        })
        .unwrap();
    }
    set_layer(&mut s, "A-DOOR", LayerProp::Locked, json!(true));
    let value = |s: &Session, h: u64| match s.drawing.doc.get_entity(Handle::new(h)).unwrap() {
        EntityType::Text(t) => t.value.clone(),
        other => panic!("{other:?}"),
    };

    let out = s
        .run(Command::FindReplace {
            find: "ZQX".into(),
            replace: "DONE".into(),
            scope: qiyas_core::TextScope::Drawing,
            match_case: true,
            whole_word: false,
        })
        .unwrap();
    assert!(
        matches!(
            out,
            Outcome::Replaced {
                count: 0,
                skipped: 1,
                ..
            }
        ),
        "{out:?}"
    );
    assert_eq!(value(&s, text), "ZQX");

    // The rectangle (layer 0) grows: its field on the locked layer follows in the same step (a
    // field is a derived value; the lock stops the user, not the field), and undo puts it back.
    assert_eq!(value(&s, field), "2.000");
    s.run(Command::Modify(qiyas_core::ModifySpec::Scale {
        targets: vec![t(rect)],
        base: [100.0, 100.0],
        factor: 2.0,
        copy: false,
    }))
    .unwrap();
    assert_eq!(value(&s, field), "8.000");
    assert!(s.undo());
    assert_eq!(value(&s, field), "2.000");
    assert!(s.redo());
    assert_eq!(value(&s, field), "8.000");
    assert!(matches!(
        s.run(Command::UpdateFields).unwrap(),
        Outcome::Changed
    ));
    assert_eq!(value(&s, field), "8.000");
    // Find and replace still leaves the locked field text as it is.
    let out = s
        .run(Command::FindReplace {
            find: "8.000".into(),
            replace: "9".into(),
            scope: qiyas_core::TextScope::Drawing,
            match_case: true,
            whole_word: false,
        })
        .unwrap();
    assert!(
        matches!(
            out,
            Outcome::Replaced {
                count: 0,
                skipped: 1,
                ..
            }
        ),
        "{out:?}"
    );
    assert_eq!(value(&s, field), "8.000");

    // TAG has no MARK definition: syncing would drop the locked reference's MARK.
    let out = s
        .run(Command::SyncAttributes {
            block: "TAG".into(),
        })
        .unwrap();
    assert!(
        matches!(
            out,
            Outcome::Updated {
                count: 0,
                skipped: 1
            }
        ),
        "{out:?}"
    );
    let marks = |s: &Session| match s.drawing.doc.get_entity(Handle::new(tag)).unwrap() {
        EntityType::Insert(i) => i.attributes.len(),
        _ => unreachable!(),
    };
    assert_eq!(marks(&s), 1);

    // Unlocked, both go through.
    set_layer(&mut s, "A-DOOR", LayerProp::Locked, json!(false));
    s.run(Command::FindReplace {
        find: "ZQX".into(),
        replace: "DONE".into(),
        scope: qiyas_core::TextScope::Drawing,
        match_case: true,
        whole_word: false,
    })
    .unwrap();
    assert_eq!(value(&s, text), "DONE");
    s.run(Command::SyncAttributes {
        block: "TAG".into(),
    })
    .unwrap();
    assert_eq!(marks(&s), 0);
}

// ---- Isolating a layer, and undo leaving layers hidden or shown since as they are ----

fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    (dir, s)
}

fn new_layer(s: &mut Session, name: &str) {
    s.run(Command::NewLayer { name: name.into() }).unwrap();
}

fn hidden(s: &Session) -> Vec<String> {
    s.hidden_layers()
}

/// The founder's case: a layer isolated, an empty layer renamed, every layer shown again, then
/// Ctrl+Z. Undo once put back the whole hidden set of before the rename, and the drawing
/// vanished again; now it takes back only the rename.
#[test]
fn undoing_a_rename_restores_only_that_layers_hidden_entry() {
    let (_dir, mut s) = sample_session();
    new_layer(&mut s, "QIYAS-AREA");
    s.run(Command::IsolateLayer {
        name: "A-WALL".into(),
    })
    .unwrap();
    assert!(hidden(&s).contains(&"A-DOOR".to_string()));
    s.run(Command::RenameLayer {
        from: "QIYAS-AREA".into(),
        to: "QIYAS-AREA-2".into(),
    })
    .unwrap();
    // Show all.
    s.unisolate_layers();
    for name in hidden(&s) {
        s.set_layer_hidden(&name, false);
    }
    assert!(hidden(&s).is_empty());
    assert!(s.undo());
    assert!(
        s.drawing.doc.layers.get("QIYAS-AREA").is_some(),
        "the rename is undone"
    );
    assert!(
        hidden(&s).is_empty(),
        "nothing hidden again: {:?}",
        hidden(&s)
    );
    assert!(drawn(&s).len() > 3, "the drawing is on screen");

    // A layer still hidden under its new name is hidden under its old one again; a layer hidden
    // since stays hidden.
    assert!(s.redo());
    s.set_layer_hidden("QIYAS-AREA-2", true);
    s.run(Command::RenameLayer {
        from: "QIYAS-AREA-2".into(),
        to: "QIYAS-AREA-3".into(),
    })
    .unwrap();
    s.set_layer_hidden("A-DOOR", true);
    assert!(s.undo());
    let now = hidden(&s);
    assert!(now.contains(&"QIYAS-AREA-2".to_string()), "{now:?}");
    assert!(now.contains(&"A-DOOR".to_string()), "{now:?}");
    assert!(!now.contains(&"QIYAS-AREA-3".to_string()), "{now:?}");
}

#[test]
fn isolating_twice_restores_the_hidden_layers_as_before() {
    let (_dir, mut s) = sample_session();
    new_layer(&mut s, "NOTES");
    s.set_layer_hidden("NOTES", true);
    let before = hidden(&s);
    let undo_before = s.can_undo();
    s.run(Command::IsolateLayer {
        name: "a-wall".into(),
    })
    .unwrap();
    assert_eq!(s.info().isolated_layer.as_deref(), Some("A-WALL"));
    assert!(hidden(&s).contains(&"A-DOOR".to_string()));
    assert!(!hidden(&s).iter().any(|n| n == "A-WALL"));
    assert_eq!(s.can_undo(), undo_before, "not an undo step");
    s.run(Command::IsolateLayer {
        name: "A-WALL".into(),
    })
    .unwrap();
    assert_eq!(hidden(&s), before);
    assert_eq!(s.info().isolated_layer, None);
    let err = s
        .run(Command::IsolateLayer {
            name: "NO-SUCH".into(),
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidCommand(_)));
}

#[test]
fn unisolate_keeps_layers_the_user_changed_since() {
    let (_dir, mut s) = sample_session();
    new_layer(&mut s, "NOTES");
    s.set_layer_hidden("A-WALL", true);
    // Isolating a hidden layer shows it; un-isolating hides it again.
    s.run(Command::IsolateLayer {
        name: "A-WALL".into(),
    })
    .unwrap();
    assert!(!hidden(&s).contains(&"A-WALL".to_string()));
    // The user shows A-DOOR and hides NOTES's neighbour while isolated.
    s.set_layer_hidden("A-DOOR", false);
    s.set_layer_hidden("NOTES", true);
    s.run(Command::UnisolateLayers).unwrap();
    let now = hidden(&s);
    assert!(now.contains(&"A-WALL".to_string()), "hidden again: {now:?}");
    assert!(!now.contains(&"A-DOOR".to_string()), "{now:?}");
    assert!(
        now.contains(&"NOTES".to_string()),
        "the user's own hide stays: {now:?}"
    );
    assert_eq!(s.info().isolated_layer, None);
}

#[test]
fn isolation_follows_a_renamed_layer_and_ends_with_a_layer_state() {
    let (_dir, mut s) = sample_session();
    s.run(Command::IsolateLayer {
        name: "A-WALL".into(),
    })
    .unwrap();
    s.run(Command::RenameLayer {
        from: "A-WALL".into(),
        to: "WALLS".into(),
    })
    .unwrap();
    assert_eq!(s.info().isolated_layer.as_deref(), Some("WALLS"));
    s.run(Command::UnisolateLayers).unwrap();
    assert!(hidden(&s).is_empty(), "{:?}", hidden(&s));

    s.run(Command::SaveLayerState { name: "ALL".into() })
        .unwrap();
    s.run(Command::IsolateLayer {
        name: "WALLS".into(),
    })
    .unwrap();
    s.run(Command::RestoreLayerState { name: "ALL".into() })
        .unwrap();
    assert_eq!(s.info().isolated_layer, None);
}
