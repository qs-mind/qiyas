//! Blocks defined from record entities belong to their block from the start, and roll back
//! together with their table entries when undone or when any supplied entity is refused.

use std::collections::BTreeMap;

use opencadcodec::{EntityType, Handle, Vector3};
use qiyas_core::{Command, NewEntity, Outcome, Session};
use serde_json::json;

fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::new_blank(&dir.path().join("blocks.dxf"), 6);
    (dir, session)
}

fn line(layer: &str) -> NewEntity {
    NewEntity {
        kind: "Line".to_string(),
        data: json!({
            "common": {"layer": layer},
            "start": {"x": 1.0, "y": 2.0, "z": 0.0},
            "end": {"x": 1.9, "y": 2.0, "z": 0.0}
        }),
    }
}

#[test]
fn definition_and_new_layers_are_one_undo_step_with_correct_ownership() {
    let (_dir, mut s) = session();
    let before = s.drawing.entities_in("*Model_Space").count();
    let handles = s
        .define_block("DOOR", [1.0, 2.0], vec![line("A-DOOR")])
        .unwrap();
    assert_eq!(handles.len(), 1);
    let block = s.drawing.doc.block_records.get("DOOR").unwrap();
    let owner = block.handle;
    assert_eq!(block.base_point, Vector3::new(1.0, 2.0, 0.0));
    assert_eq!(block.units, 6);
    assert_ne!(block.block_entity_handle, Handle::NULL);
    assert_ne!(block.block_end_handle, Handle::NULL);
    let entity = s.drawing.doc.get_entity(Handle::new(handles[0])).unwrap();
    assert_eq!(entity.common().owner_handle, owner);
    assert_eq!(s.drawing.entities_in("DOOR").count(), 1);
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), before);
    assert!(s.drawing.doc.layers.contains("A-DOOR"));
    assert_eq!(s.history_steps().0.len(), 1);
    assert_eq!(s.history_steps().0[0].label, "define_block");

    assert!(s.undo());
    assert!(!s.drawing.doc.block_records.contains("DOOR"));
    assert!(!s.drawing.doc.layers.contains("A-DOOR"));
    assert!(s.drawing.doc.get_entity(Handle::new(handles[0])).is_none());
    assert!(!s.undo());

    assert!(s.redo());
    assert_eq!(
        s.drawing.doc.block_records.get("DOOR").unwrap().handle,
        owner
    );
    assert!(s.drawing.doc.layers.contains("A-DOOR"));
    assert_eq!(s.drawing.entities_in("DOOR").count(), 1);
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), before);
}

#[test]
fn definition_base_point_places_geometry_at_the_insert_point() {
    let (_dir, mut s) = session();
    let leaf = s.define_block("DOOR", [1.0, 2.0], vec![line("0")]).unwrap()[0];
    let Outcome::Added { handles } = s
        .run(Command::InsertBlock {
            name: "DOOR".to_string(),
            at: [10.0, 20.0],
            scale: 1.0,
            rotation: 90.0,
            attribute_values: BTreeMap::new(),
            from_path: None,
        })
        .unwrap()
    else {
        panic!("insert returns its handle");
    };
    let leaf = s.entity_info(leaf, &handles).unwrap();
    let start = leaf.start.unwrap();
    let end = leaf.end.unwrap();
    assert!((start[0] - 10.0).abs() < 1e-9);
    assert!((start[1] - 20.0).abs() < 1e-9);
    assert!((end[0] - 10.0).abs() < 1e-9);
    assert!((end[1] - 20.9).abs() < 1e-9);
}

#[test]
fn invalid_names_and_empty_definitions_leave_no_step() {
    let (_dir, mut s) = session();
    for name in ["", " ", "*DOOR", "A|DOOR", "A/DOOR"] {
        assert!(s
            .define_block(name, [0.0, 0.0], vec![line("A-DOOR")])
            .is_err());
    }
    assert!(s.define_block("EMPTY", [0.0, 0.0], vec![]).is_err());
    assert!(s
        .define_block("BAD-BASE", [f64::NAN, 0.0], vec![line("A-DOOR")])
        .is_err());
    assert!(s.history_steps().0.is_empty());
    assert!(!s.drawing.doc.layers.contains("A-DOOR"));
    assert!(!s.drawing.doc.block_records.contains("EMPTY"));
}

#[test]
fn taken_name_is_case_insensitive_and_preserves_the_existing_definition() {
    let (_dir, mut s) = session();
    let handles = s
        .define_block("Door", [1.0, 2.0], vec![line("A-DOOR")])
        .unwrap();
    let before = s.drawing.doc.block_records.get("Door").unwrap().clone();
    let error = s
        .define_block("door", [0.0, 0.0], vec![line("OTHER")])
        .unwrap_err();
    assert!(error.to_string().contains("already"), "{error}");
    assert_eq!(s.drawing.doc.block_records.get("Door").unwrap(), &before);
    assert!(s.drawing.doc.get_entity(Handle::new(handles[0])).is_some());
    assert!(!s.drawing.doc.layers.contains("OTHER"));
    assert_eq!(s.history_steps().0.len(), 1);
}

#[test]
fn rejected_entity_rolls_back_the_definition_its_entities_and_its_new_layers() {
    let (_dir, mut s) = session();
    let before = s.drawing.doc.entity_count();
    let error = s
        .define_block(
            "BROKEN",
            [0.0, 0.0],
            vec![
                line("A-DOOR"),
                NewEntity {
                    kind: "Insert".to_string(),
                    data: json!({}),
                },
            ],
        )
        .unwrap_err();
    assert!(error.to_string().contains("insert_block"), "{error}");
    assert!(!s.drawing.doc.block_records.contains("BROKEN"));
    assert!(!s.drawing.doc.layers.contains("A-DOOR"));
    assert_eq!(s.drawing.doc.entity_count(), before);
    assert!(s.history_steps().0.is_empty());

    let mut forged = line("0");
    forged.data["common"]["owner_handle"] = json!(1);
    assert!(s.define_block("FORGED", [0.0, 0.0], vec![forged]).is_err());
    assert!(!s.drawing.doc.block_records.contains("FORGED"));
    assert!(s.history_steps().0.is_empty());
}

#[test]
fn definitions_survive_dwg_and_dxf_with_their_base_and_entities() {
    let (dir, mut s) = session();
    s.define_block("DOOR", [1.0, 2.0], vec![line("A-DOOR")])
        .unwrap();
    for ext in ["dwg", "dxf"] {
        let path = dir.path().join(format!("out.{ext}"));
        s.save_as(&path).unwrap();
        let opened = Session::open(&path).unwrap();
        let block = opened.drawing.doc.block_records.get("DOOR").unwrap();
        assert_eq!(block.base_point, Vector3::new(1.0, 2.0, 0.0));
        assert_eq!(block.units, 6);
        let entities: Vec<_> = opened.drawing.entities_in("DOOR").collect();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].common().owner_handle, block.handle);
        let EntityType::Line(line) = entities[0] else {
            panic!("definition holds a line");
        };
        assert_eq!(line.start, Vector3::new(1.0, 2.0, 0.0));
        assert_eq!(line.end, Vector3::new(1.9, 2.0, 0.0));
    }
}
