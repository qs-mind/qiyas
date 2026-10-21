//! Walls are ordinary editable CAD entities, with physical quantities and one complete undo.

use opencadcodec::{EntityType, Handle};
use qiyas_core::{Command, Outcome, Session, WallResult, WALL_APP};
use serde_json::{json, Value};

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn wall(factor: f64, y: f64) -> Command {
    serde_json::from_value(json!({
        "type": "walls",
        "runs": [{"points": [[0, y / factor], [5.0 / factor, y / factor]]}],
        "thickness": 0.2 / factor,
        "height": 3,
        "openings": [
            {"run": 0, "at": 1.5 / factor, "width": 0.9 / factor, "kind": "door", "swing": "right"},
            {"run": 0, "at": 3.5 / factor, "width": 1.2 / factor, "kind": "window", "sill": 0.9, "head": 2.1}
        ]
    })).unwrap()
}

fn draw(session: &mut Session, command: Command) -> WallResult {
    match session.run(command).unwrap() {
        Outcome::Walls { result } => result,
        other => panic!("Expected walls, got {other:?}"),
    }
}

fn table_names(session: &Session) -> Value {
    let doc = &session.drawing.doc;
    json!({
        "layers": doc.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "blocks": doc.block_records.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
        "apps": doc.app_ids.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
    })
}

#[test]
fn walls_undo_and_redo_include_entities_layers_blocks_and_app() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("walls.dxf"), 6);
    let before = table_names(&session);
    let entities_before = session.drawing.doc.entities().count();
    let current = session.current.clone();
    let result = draw(&mut session, wall(1.0, 0.0));
    assert_eq!(session.history_steps().0.len(), 1);
    assert_eq!(session.current, current);
    assert!(session.drawing.doc.app_ids.contains(WALL_APP));
    assert!(!result.hatches.is_empty());
    assert!(result.handles.iter().all(|&h| session
        .drawing
        .doc
        .get_entity(Handle::new(h))
        .is_some()));
    session.undo();
    assert_eq!(session.drawing.doc.entities().count(), entities_before);
    assert_eq!(table_names(&session), before);
    assert!(!session.can_undo());
    session.redo();
    assert!(result.handles.iter().all(|&h| session
        .drawing
        .doc
        .get_entity(Handle::new(h))
        .is_some()));
    assert!(session
        .drawing
        .doc
        .block_records
        .contains("QIYAS_DOOR_0900"));
    assert!(session
        .drawing
        .doc
        .block_records
        .contains("QIYAS_WIN_1200_200"));
}

#[test]
fn two_wall_commands_reuse_the_same_size_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("walls.dxf"), 6);
    draw(&mut session, wall(1.0, 0.0));
    let names = table_names(&session);
    let first: Vec<_> = session
        .drawing
        .doc
        .entities_in_block("QIYAS_DOOR_0900")
        .map(|e| e.common().handle)
        .collect();
    let window: Vec<_> = session
        .drawing
        .doc
        .entities_in_block("QIYAS_WIN_1200_200")
        .map(|e| e.common().handle)
        .collect();
    let second = draw(&mut session, wall(1.0, 5.0));
    assert_eq!(table_names(&session), names);
    assert_eq!(
        session
            .drawing
            .doc
            .entities_in_block("QIYAS_DOOR_0900")
            .map(|e| e.common().handle)
            .collect::<Vec<_>>(),
        first
    );
    assert_eq!(
        session
            .drawing
            .doc
            .entities_in_block("QIYAS_WIN_1200_200")
            .map(|e| e.common().handle)
            .collect::<Vec<_>>(),
        window
    );
    assert_eq!(second.inserts.len(), 2);
    session.undo();
    assert!(session
        .drawing
        .doc
        .block_records
        .contains("QIYAS_DOOR_0900"));
    assert_eq!(
        session
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .filter(|e| matches!(e, EntityType::Insert(_)))
            .count(),
        2
    );
}

#[test]
fn mm_cm_m_and_inches_have_the_same_quantities_and_symbols() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut session = Session::new_blank(&dir.path().join("walls.dxf"), units);
        let result = draw(&mut session, wall(factor, 0.0));
        near(result.centreline_length, 5.0);
        near(result.gross_area, 1.0);
        near(result.net_area, 0.58);
        // 5 × .2 × 3 minus the door (.9 × .2 × 2.1) and window (1.2 × .2 × 1.2).
        near(result.volume, 2.334);
        let measured_area: f64 = result
            .hatches
            .iter()
            .map(|&h| session.entity_info(h, &[]).unwrap().area.unwrap() * factor * factor)
            .sum();
        near(measured_area, result.net_area);
        let door = session
            .drawing
            .doc
            .entities_in_block("QIYAS_DOOR_0900")
            .find_map(|e| match e {
                EntityType::Arc(a) => Some(a),
                _ => None,
            })
            .unwrap();
        near(door.radius * factor, 0.9);
        let EntityType::Insert(insert) = session
            .drawing
            .doc
            .get_entity(Handle::new(result.inserts[0]))
            .unwrap()
        else {
            panic!("door reference")
        };
        near(insert.x_scale(), -1.0);
        near(insert.y_scale(), 1.0);
        let centreline = session
            .drawing
            .doc
            .get_entity(Handle::new(result.centrelines[0]))
            .unwrap();
        let data = centreline
            .common()
            .extended_data
            .get_record(WALL_APP)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&data.values[..2]).unwrap(),
            json!([
                {"Real": 0.2 / factor}, {"Real": 3.0}
            ])
        );
    }
}

#[test]
fn door_leaf_and_hinge_follow_the_wall_rotation_and_swing() {
    let dir = tempfile::tempdir().unwrap();
    for (swing, hinge_y) in [("left", 21.55), ("right", 22.45)] {
        let mut session = Session::new_blank(&dir.path().join("door.dxf"), 6);
        let result = draw(
            &mut session,
            serde_json::from_value(json!({
                "type": "walls", "thickness": 0.2, "height": 3,
                "runs": [{"points": [[10, 20], [10, 25]]}],
                "openings": [{"run": 0, "at": 2, "width": 0.9, "kind": "door", "swing": swing}]
            }))
            .unwrap(),
        );
        let leaf = session
            .drawing
            .doc
            .entities_in_block("QIYAS_DOOR_0900")
            .find(|e| matches!(e, EntityType::Line(_)))
            .unwrap()
            .common()
            .handle
            .value();
        let leaf = session.entity_info(leaf, &result.inserts).unwrap();
        let start = leaf.start.unwrap();
        let end = leaf.end.unwrap();
        // The hinge sits on the east face; the open leaf reaches 900 mm west of it.
        near(start[0], 10.1);
        near(start[1], hinge_y);
        near(end[0], 9.2);
        near(end[1], hinge_y);
        near(leaf.length.unwrap(), 0.9);
        near(result.gross_area, 1.0);
        near(result.net_area, 0.82);
        near(result.volume, 2.622);
    }
}

#[test]
fn dwg_and_dxf_keep_wall_outlines_hatches_blocks_and_xdata() {
    let dir = tempfile::tempdir().unwrap();
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("walls.{extension}"));
        let mut session = Session::new_blank(&path, 6);
        let result = draw(&mut session, wall(1.0, 0.0));
        session.save_as_headless(&path).unwrap();
        let reopened = Session::open(&path).unwrap();
        let entities: Vec<_> = reopened
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .collect();
        assert_eq!(
            entities
                .iter()
                .filter(|e| e.common().layer == "A-WALL")
                .count(),
            result.outlines.len()
        );
        assert_eq!(
            entities
                .iter()
                .filter(|e| matches!(e, EntityType::Hatch(_)))
                .count(),
            result.hatches.len()
        );
        assert_eq!(
            entities
                .iter()
                .filter(|e| matches!(e, EntityType::Insert(_)))
                .count(),
            2
        );
        assert!(reopened
            .drawing
            .doc
            .block_records
            .contains("QIYAS_DOOR_0900"));
        assert!(reopened
            .drawing
            .doc
            .block_records
            .contains("QIYAS_WIN_1200_200"));
        assert_eq!(
            reopened
                .drawing
                .doc
                .entities_in_block("QIYAS_DOOR_0900")
                .count(),
            2
        );
        assert_eq!(
            reopened
                .drawing
                .doc
                .entities_in_block("QIYAS_WIN_1200_200")
                .count(),
            2
        );
        let door = entities
            .iter()
            .find_map(|e| match e {
                EntityType::Insert(insert) if insert.block_name == "QIYAS_DOOR_0900" => {
                    Some(insert)
                }
                _ => None,
            })
            .unwrap();
        near(door.x_scale(), -1.0);
        near(door.y_scale(), 1.0);
        near(door.insert_point.x, 1.95);
        near(door.insert_point.y, -0.1);
        assert_eq!(
            entities
                .iter()
                .filter(|e| e.common().extended_data.get_record(WALL_APP).is_some())
                .count(),
            result.handles.len()
        );
        let area: f64 = entities
            .iter()
            .filter(|e| matches!(e, EntityType::Hatch(_)))
            .map(|e| {
                reopened
                    .entity_info(e.common().handle.value(), &[])
                    .unwrap()
                    .area
                    .unwrap()
            })
            .sum();
        near(area, 0.58);
    }
}

#[test]
fn closed_room_hatch_measures_walls_with_the_room_as_a_hole() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("room.dxf"), 6);
    let result = draw(
        &mut session,
        serde_json::from_value(json!({
            "type": "walls", "thickness": 0.2, "height": 3,
            "runs": [{"points": [[0,0], [5,0], [5,4], [0,4]], "closed": true}]
        }))
        .unwrap(),
    );
    // Outer 5.2 × 4.2, inner 4.8 × 3.8.
    near(result.net_area, 3.6);
    assert_eq!(result.outlines.len(), 2);
    assert_eq!(result.hatches.len(), 1);
    near(
        session
            .entity_info(result.hatches[0], &[])
            .unwrap()
            .area
            .unwrap(),
        3.6,
    );
}

#[test]
fn wall_validation_leaves_no_partial_layers_or_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("walls.dxf"), 6);
    let before = table_names(&session);
    let mut bad = serde_json::to_value(wall(1.0, 0.0)).unwrap();
    bad["openings"][1]["head"] = json!(5);
    assert!(session.run(serde_json::from_value(bad).unwrap()).is_err());
    let mut bad = serde_json::to_value(wall(1.0, 0.0)).unwrap();
    // Fails after one valid custom layer was made; tables must roll back too.
    bad["layers"] = json!({"outline": "CUSTOM-WALL", "hatch": "bad/name"});
    assert!(session.run(serde_json::from_value(bad).unwrap()).is_err());
    assert_eq!(table_names(&session), before);
    assert!(!session.can_undo());
}
