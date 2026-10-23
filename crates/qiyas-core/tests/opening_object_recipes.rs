//! Legacy opening inserts use the same expression-driven symbol recipes as generic elements.
use opencadcodec::entities::Insert;
use opencadcodec::{EntityType, Handle};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, input: Value) -> Value {
    serde_json::to_value(s.run_input(&input, Source::User).unwrap().outcome).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

fn insert(s: &Session, handle: u64) -> Insert {
    match s.drawing.doc.get_entity(Handle::new(handle)).unwrap() {
        EntityType::Insert(insert) => insert.clone(),
        _ => panic!("opening insert"),
    }
}

fn setup(path: &std::path::Path, units: i16) -> (Session, Vec<u64>) {
    let mut s = Session::new_blank(path, units);
    let m = 1.0 / s.drawing.units.to_metres();
    let result = run(
        &mut s,
        json!({
            "type":"walls", "thickness":0.2*m,
            "runs":[{"points":[[0,0],[10.0*m,0]]}],
            "openings":[
                {"run":0,"at":2.0*m,"width":0.9*m,"kind":"door","swing":"left"},
                {"run":0,"at":4.0*m,"width":0.9*m,"kind":"door","swing":"right"},
                {"run":0,"at":7.0*m,"width":1.2*m,"kind":"window"}
            ]
        }),
    );
    let handles = result["inserts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h.as_u64().unwrap())
        .collect();
    (s, handles)
}

fn baseline(s: &Session, handles: &[u64]) {
    let factor = s.drawing.units.to_metres();
    let left = insert(s, handles[0]);
    let right = insert(s, handles[1]);
    assert_eq!(left.block_name, "QIYAS_DOOR_0900");
    assert_eq!(right.block_name, left.block_name);
    near(left.x_scale(), 1.0);
    near(right.x_scale(), -1.0);
    for (reference, x) in [(&left, 1.55), (&right, 4.45)] {
        near(reference.insert_point.x * factor, x);
        // Hinge on the swing face, corrected by Goal G.
        near(reference.insert_point.y * factor, 0.1);
    }
    let door: Vec<_> = s.drawing.doc.entities_in_block(&left.block_name).collect();
    assert_eq!(door.len(), 2);
    let EntityType::Line(line) = door[0] else {
        panic!("door leaf")
    };
    near(line.start.x, 0.0);
    near(line.start.y, 0.0);
    near(line.end.x, 0.0);
    near(line.end.y * factor, 0.9);
    let EntityType::Arc(arc) = door[1] else {
        panic!("door swing")
    };
    near(arc.radius * factor, 0.9);
    near(arc.start_angle, 0.0);
    near(arc.end_angle, std::f64::consts::FRAC_PI_2);
    let window = insert(s, handles[2]);
    assert_eq!(window.block_name, "QIYAS_WIN_1200_200");
    let geometry: Vec<_> = s
        .drawing
        .doc
        .entities_in_block(&window.block_name)
        .collect();
    assert_eq!(geometry.len(), 2);
    let EntityType::LwPolyline(frame) = geometry[0] else {
        panic!("window frame")
    };
    near(frame.vertices[0].location.x * factor, -0.6);
    near(frame.vertices[0].location.y * factor, -0.1);
    near(frame.vertices[2].location.x * factor, 0.6);
    near(frame.vertices[2].location.y * factor, 0.1);
}

fn definition(s: &Session, name: &str) -> Value {
    serde_json::to_value(
        s.object_types()
            .unwrap()
            .into_iter()
            .find(|d| d.name == name)
            .unwrap(),
    )
    .unwrap()
}

fn same_placement(before: &Insert, after: &Insert) {
    assert_eq!(before.common.handle, after.common.handle);
    assert_eq!(before.insert_point, after.insert_point);
    assert_eq!(before.rotation, after.rotation);
    assert_eq!(before.x_scale(), after.x_scale());
    assert_eq!(before.y_scale(), after.y_scale());
    assert_eq!(before.z_scale(), after.z_scale());
    assert_eq!(before.common.layer, after.common.layer);
}

#[test]
fn stored_recipes_preserve_door_window_geometry_and_both_swings_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for units in [4, 5, 6, 1] {
        let (s, handles) = setup(&dir.path().join(format!("baseline-{units}.dxf")), units);
        baseline(&s, &handles);
        assert_eq!(
            definition(&s, "Legacy door")["plan"]["symbols"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            definition(&s, "Legacy window")["plan"]["symbols"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(s.history_steps().0.len(), 1);
    }
}

#[test]
fn recipe_redefinition_preserves_insert_handles_undo_and_second_save_in_dwg_and_dxf() {
    let dir = tempfile::tempdir().unwrap();
    for units in [4, 5, 6, 1] {
        for extension in ["dwg", "dxf"] {
            let path = dir.path().join(format!("recipe-{units}.{extension}"));
            let (mut s, handles) = setup(&path, units);
            s.save_as(&path).unwrap();
            let mut s = Session::open(&path).unwrap();
            let original: Vec<_> = handles.iter().map(|h| insert(&s, *h)).collect();
            let mut door = definition(&s, "Legacy door");
            door["plan"]["symbols"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":"circle","centre":[0,0],"radius":"=width / (4 * unit)"}));
            run(
                &mut s,
                json!({"type":"redefine_object_type","definition":door}),
            );
            assert_eq!(s.history_steps().0.len(), 1);
            let changed = insert(&s, handles[0]);
            assert_ne!(changed.block_name, original[0].block_name);
            assert_eq!(changed.block_name, insert(&s, handles[1]).block_name);
            let symbols: Vec<_> = s
                .drawing
                .doc
                .entities_in_block(&changed.block_name)
                .collect();
            assert_eq!(symbols.len(), 3);
            let EntityType::Circle(circle) = symbols[2] else {
                panic!("new symbol")
            };
            near(circle.radius * s.drawing.units.to_metres(), 0.225);
            for (handle, before) in handles.iter().zip(&original) {
                same_placement(before, &insert(&s, *handle));
            }
            assert!(s.undo());
            assert_eq!(insert(&s, handles[0]).block_name, original[0].block_name);
            assert!(s.redo());
            assert_eq!(insert(&s, handles[0]).block_name, changed.block_name);
            s.save().unwrap();
            let mut s = Session::open(&path).unwrap();
            assert_eq!(
                s.drawing
                    .doc
                    .entities_in_block(&insert(&s, handles[0]).block_name)
                    .count(),
                3
            );
            for (handle, before) in handles.iter().zip(&original) {
                same_placement(before, &insert(&s, *handle));
            }
            let mut window = definition(&s, "Legacy window");
            window["plan"]["symbols"] = json!([{"kind":"line","a":["=-width / (2 * unit)","=-host_width / (2 * unit)"],"b":["=width / (2 * unit)","=host_width / (2 * unit)"]}]);
            run(
                &mut s,
                json!({"type":"redefine_object_type","definition":window}),
            );
            let reference = insert(&s, handles[2]);
            same_placement(&original[2], &reference);
            let symbols: Vec<_> = s
                .drawing
                .doc
                .entities_in_block(&reference.block_name)
                .collect();
            assert_eq!(symbols.len(), 1);
            let EntityType::Line(line) = symbols[0] else {
                panic!("window recipe")
            };
            near(line.start.y * s.drawing.units.to_metres(), -0.1);
            near(line.end.y * s.drawing.units.to_metres(), 0.1);
            s.save().unwrap();
            let s = Session::open(&path).unwrap();
            assert_eq!(
                s.drawing
                    .doc
                    .entities_in_block(&insert(&s, handles[2]).block_name)
                    .count(),
                1
            );
        }
    }
}

#[test]
fn an_empty_symbol_recipe_hides_only_its_symbols_and_is_undoable() {
    let dir = tempfile::tempdir().unwrap();
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("empty.{extension}"));
        let (mut s, handles) = setup(&path, 6);
        let before = insert(&s, handles[0]);
        let mut door = definition(&s, "Legacy door");
        door["plan"]["symbols"] = json!([]);
        run(
            &mut s,
            json!({"type":"redefine_object_type","definition":door}),
        );
        let after = insert(&s, handles[0]);
        same_placement(&before, &after);
        assert_eq!(
            s.drawing.doc.entities_in_block(&after.block_name).count(),
            0
        );
        assert_eq!(s.wall_groups(None).unwrap()[0].runs[0].openings.len(), 3);
        assert!(s.undo());
        assert_eq!(insert(&s, handles[0]).block_name, before.block_name);
        assert!(s.redo());
        s.save_as(&path).unwrap();
        let s = Session::open(&path).unwrap();
        let reopened = insert(&s, handles[0]);
        same_placement(&after, &reopened);
        assert!(s.drawing.doc.block_records.contains(&reopened.block_name));
        assert_eq!(
            s.drawing
                .doc
                .entities_in_block(&reopened.block_name)
                .count(),
            0
        );
    }
}

#[test]
fn adopted_hand_edited_symbols_stay_intact_without_becoming_new_placement_templates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adopted.dxf");
    let (mut s, handles) = setup(&path, 6);
    let old_block = insert(&s, handles[0]).block_name;
    let leaf = s
        .drawing
        .doc
        .entities_in_block(&old_block)
        .find(|entity| matches!(entity, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle;
    let Some(EntityType::Line(line)) = s.drawing.doc.get_entity_mut(leaf) else {
        panic!("door leaf")
    };
    line.end.y = 1.8;
    for handle in &handles {
        s.drawing
            .doc
            .get_entity_mut(Handle::new(*handle))
            .unwrap()
            .common_mut()
            .extended_data
            .remove_record("QIYAS_OBJECT_SYMBOL_RECIPE");
    }
    s.save_as(&path).unwrap();
    let mut s = Session::open(&path).unwrap();
    assert_eq!(insert(&s, handles[0]).block_name, old_block);
    let result = run(
        &mut s,
        json!({
            "type":"walls","thickness":0.2,
            "runs":[{"points":[[0,3],[10,3]]}],
            "openings":[{"run":0,"at":2,"width":0.9,"kind":"door"}]
        }),
    );
    let new_block = insert(&s, result["inserts"][0].as_u64().unwrap()).block_name;
    assert_ne!(new_block, old_block);
    for (block, expected) in [(&old_block, 1.8), (&new_block, 0.9)] {
        let leaf = s
            .drawing
            .doc
            .entities_in_block(block)
            .find_map(|entity| match entity {
                EntityType::Line(line) => Some(line),
                _ => None,
            })
            .unwrap();
        near(leaf.end.y, expected);
    }
    assert_eq!(insert(&s, handles[0]).block_name, old_block);
}

#[test]
fn a_scaled_copy_applies_physical_symbol_dimensions_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, originals) = setup(&dir.path().join("scaled.dxf"), 6);
    let group = run(&mut s, json!({"type":"wall_groups"}))["groups"][0].clone();
    let targets: Vec<_> = group["handles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| json!({"handle":h}))
        .collect();
    run(
        &mut s,
        json!({"type":"modify","kind":"scale","targets":targets,"base":[0,20],"factor":2,"copy":true}),
    );
    let groups = run(&mut s, json!({"type":"wall_groups"}))["groups"].clone();
    let copied = groups
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["group"] != group["group"])
        .unwrap();
    let copied_openings = copied["runs"][0]["openings"].as_array().unwrap();
    for (i, expected) in [(0, 1.8), (1, 1.8), (2, 2.4)] {
        let reference = insert(&s, copied_openings[i]["insert"].as_u64().unwrap());
        near(reference.x_scale().abs(), 1.0);
        near(reference.y_scale(), 1.0);
        let geometry: Vec<_> = s
            .drawing
            .doc
            .entities_in_block(&reference.block_name)
            .collect();
        if i < 2 {
            let EntityType::Line(line) = geometry[0] else {
                panic!("door leaf")
            };
            near(line.end.y - line.start.y, expected);
        } else {
            let EntityType::LwPolyline(frame) = geometry[0] else {
                panic!("window frame")
            };
            near(
                frame.vertices[2].location.x - frame.vertices[0].location.x,
                expected,
            );
        }
        near(copied_openings[i]["width"].as_f64().unwrap(), expected);
    }
    baseline(&s, &originals);
    assert!(s.undo());
    assert_eq!(
        run(&mut s, json!({"type":"wall_groups"}))["groups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
