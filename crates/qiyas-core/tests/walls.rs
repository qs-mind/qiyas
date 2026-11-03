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
        // The hinge sits on the west (swing) face; the open leaf reaches 900 mm west of it.
        near(start[0], 9.9);
        near(start[1], hinge_y);
        near(end[0], 9.0);
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
        near(door.insert_point.y, 0.1);
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

#[test]
fn opening_commands_report_resolved_or_retained_defaults_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("defaults.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let created = s.run_input(&json!({"type":"walls", "runs":[{"points":[[0,0],[10.0*k,0]]}], "thickness":0.2*k,"height":1.9}), qiyas_core::Source::Ai).unwrap();
        let Outcome::Walls { result } = created.outcome else {
            panic!()
        };
        let added = s
            .run_input(
                &json!({"type":"add_wall_openings","centreline":result.centrelines[0],"openings":[
                    {"at":2.0*k,"width":k,"kind":"window"}, {"at":5.0*k,"width":k,"kind":"door"}
                ]}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
        for (path, expected) in [
            ("/openings/0/sill", 0.9),
            ("/openings/0/head", 1.9),
            ("/openings/1/sill", 0.0),
            ("/openings/1/head", 1.9),
        ] {
            let d = added
                .defaults_used
                .iter()
                .find(|d| d.field == path)
                .unwrap();
            assert_eq!(d.value, json!(expected));
            assert_eq!(d.unit.as_deref(), Some("m"));
        }
        let Outcome::Walls { result } = added.outcome else {
            panic!()
        };
        let edit = s
            .run_input(
                &json!({"type":"edit_wall_opening","insert":result.inserts[0],"width":1.2*k}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
        for (path, expected) in [("/sill", 0.9), ("/head", 1.9), ("/at", 2.0 * k)] {
            assert_eq!(
                edit.defaults_used
                    .iter()
                    .find(|d| d.field == path)
                    .unwrap()
                    .value,
                json!(expected)
            );
        }
        let Outcome::Walls { result } = edit.outcome else {
            panic!()
        };
        let removed = s
            .run_input(
                &json!({"type":"remove_wall_openings","inserts":[result.inserts[0]]}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
        assert!(removed.defaults_used.is_empty());
    }
}

#[test]
fn physical_defaults_reach_elements_boq_and_takeoff_check_and_explicit_values_clear_them() {
    use qiyas_core::measurement::SourceKind;
    use qiyas_core::objects::ObjectFilter;
    use qiyas_core::Source;
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let path = dir.path().join(format!("defaults-{units}.dxf"));
        let mut s = Session::new_blank(&path, units);
        let input = json!({"type":"walls","runs":[{"points":[[0,0],[8.0/k,0]]}],"thickness":0.2/k,
            "openings":[{"run":0,"kind":"window","at":3.0/k,"width":1.2/k}]});
        let report = s.run_input(&input, Source::User).unwrap();
        for (field, value) in [
            ("/height", 3.0),
            ("/openings/0/sill", 0.9),
            ("/openings/0/head", 2.1),
        ] {
            assert!(report
                .defaults_used
                .iter()
                .any(|d| d.field == field && d.value.as_f64() == Some(value)));
        }
        let Outcome::Walls { result } = report.outcome else {
            panic!("walls")
        };
        let wall = result.centrelines[0];
        let window = result.inserts[0];
        let records = s.objects(&ObjectFilter::default()).unwrap();
        for (handle, property) in [(wall, "wall_height"), (window, "sill"), (window, "head")] {
            let object = records.iter().find(|o| o.instance.id == handle).unwrap();
            assert_eq!(
                object.instance.sources[property].kind,
                SourceKind::TypeDefault
            );
        }
        let cite = json!({"source":"Test method","clause":"1"});
        for value in [
            json!({"type":"measurement","kind":"set_mappings","mappings":[
                {"rule":"volume","code":"W-1","description_en":"{type}","description_ar":"{type}","section":"","trade":"","unit":"m3","citation":cite}]}),
            json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
                "version":1,"name":"Test method","title":"Test","based_on":"test","source_note":"test","rules":[
                    {"id":"volume","kind":"volume","basis":"geometric","element_type":"Legacy wall","unit":"m3","citation":cite,
                    "terms":[{"label":"Volume","operation":"add","factors":["geometry.volume"],"citation":cite}]}]}}),
        ] {
            s.run_input(&value, Source::User).unwrap();
        }
        let measurement = s.measurement_report().unwrap();
        assert!(measurement.items[0].assumed);
        assert_eq!(measurement.items[0].description_en, "Wall");
        assert_eq!(measurement.items[0].description_ar, "حائط");
        let check = s.takeoff_check(0, &[]).unwrap().to_string();
        assert!(check.contains("assumed_values") && check.contains("W-1"));
        s.save_as(&path).unwrap();
        let mut s = Session::open(&path).unwrap();
        assert!(s.measurement_report().unwrap().items[0].assumed);
        for (handle, props) in [
            (wall, json!({"wall_height":3})),
            (window, json!({"sill":0.9,"head":2.1})),
        ] {
            s.run_input(
                &json!({"type":"set_object_props","targets":[handle],"props":props}),
                Source::User,
            )
            .unwrap();
        }
        assert!(!s.measurement_report().unwrap().items[0].assumed);
        assert!(s.undo());
        assert!(s.measurement_report().unwrap().items[0].assumed);
    }
}

#[test]
fn default_sources_survive_opening_rebuilds_and_same_value_confirmation() {
    use qiyas_core::measurement::SourceKind;
    use qiyas_core::objects::ObjectFilter;
    use qiyas_core::Source;
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("provenance.dxf"), 6);
    let report = s
        .run_input(
            &json!({"type":"walls","runs":[{"points":[[0,0],[10,0]]}],"thickness":0.2}),
            Source::User,
        )
        .unwrap();
    let Outcome::Walls { result } = report.outcome else {
        panic!("walls")
    };
    let wall = result.centrelines[0];
    let report = s.run_input(&json!({"type":"add_wall_openings","centreline":wall,"openings":[{"kind":"window","at":3,"width":1}]}),Source::User).unwrap();
    assert!(report
        .defaults_used
        .iter()
        .any(|d| d.field == "/openings/0/head"));
    let Outcome::Walls { result } = report.outcome else {
        panic!("walls")
    };
    let window = result.inserts[0];
    let objects = s.objects(&ObjectFilter::default()).unwrap();
    assert_eq!(
        objects
            .iter()
            .find(|o| o.instance.id == wall)
            .unwrap()
            .instance
            .sources["wall_height"]
            .kind,
        SourceKind::TypeDefault
    );
    assert_eq!(
        objects
            .iter()
            .find(|o| o.instance.id == window)
            .unwrap()
            .instance
            .sources["head"]
            .kind,
        SourceKind::TypeDefault
    );
    s.run_input(
        &json!({"type":"edit_wall_opening","insert":window,"head":2.1}),
        Source::User,
    )
    .unwrap();
    let objects = s.objects(&ObjectFilter::default()).unwrap();
    assert_eq!(
        objects
            .iter()
            .find(|o| o.instance.id == window)
            .unwrap()
            .instance
            .sources["head"]
            .kind,
        SourceKind::Engineer
    );
    assert_eq!(
        objects
            .iter()
            .find(|o| o.instance.id == window)
            .unwrap()
            .instance
            .sources["sill"]
            .kind,
        SourceKind::TypeDefault
    );
}

#[test]
fn explicit_ai_inputs_keep_their_actor_and_size_groups_keep_default_sources() {
    use qiyas_core::measurement::SourceKind;
    use qiyas_core::objects::ObjectFilter;
    use qiyas_core::Source;
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("group-sources.dxf"), 6);
    let result = s
        .run_input(
            &json!({"type":"walls","runs":[{"points":[[0,0],[8,0]]}],
        "thickness":0.2,"height":3,"openings":[{"run":0,"at":2,"width":1,"kind":"window"}]}),
            Source::Ai,
        )
        .unwrap();
    let Outcome::Walls { result } = result.outcome else {
        panic!("walls")
    };
    let objects = s.objects(&ObjectFilter::default()).unwrap();
    let wall = objects
        .iter()
        .find(|o| o.instance.id == result.centrelines[0])
        .unwrap();
    assert_eq!(wall.instance.sources["wall_height"].kind, SourceKind::Mizan);
    let window = objects
        .iter()
        .find(|o| o.instance.id == result.inserts[0])
        .unwrap();
    assert_eq!(
        window.instance.sources["head"].kind,
        SourceKind::TypeDefault
    );
    let cite = json!({"source":"Test method","clause":"1"});
    s.run_input(&json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
        "version":1,"name":"Test method","title":"Test","based_on":"test","source_note":"test","rules":[
            {"id":"count","kind":"count","basis":"geometric","element_type":"Legacy window","unit":"nr","citation":cite,
             "terms":[{"label":"Count","operation":"add","factors":["1"],"citation":cite}]}]}}), Source::User).unwrap();
    for description in ["Window", "Window {head}"] {
        s.run_input(&json!({"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"count","code":"WIN","description_en":description,"description_ar":if description.contains("{head}") {"شباك {head}"} else {"شباك"},
             "section":"","trade":"","unit":"nr","citation":cite}]}),Source::User).unwrap();
        let report = s.measurement_report().unwrap();
        assert_eq!(report.items.len(), 1);
        assert!(report.items[0].assumed);
        assert!(report.items[0]
            .lines
            .iter()
            .flat_map(|l| &l.operands)
            .flat_map(|o| &o.sources)
            .any(|s| s.source.kind == SourceKind::TypeDefault && s.name == "head"));
    }
}

#[test]
fn drawing_block_opening_scales_in_the_cut_through_edit_undo_and_reopen() {
    for (units, k) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("terminal.dxf");
        let mut s = Session::new_blank(&path, units);
        s.run_input(
            &json!({"type":"define_block","name":"Concourse door","base":[0,0],"entities":[
                {"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":2.0/k,"y":0,"z":0}}},
                {"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":0,"y":1.0/k,"z":0}}}
            ]}),
            qiyas_core::Source::Ai,
        )
        .unwrap();
        let result=draw(&mut s,serde_json::from_value(json!({"type":"walls","runs":[{"points":[[0,0],[20.0/k,0]]}],"thickness":0.3/k,"height":4,
            "openings":[{"run":0,"at":5.0/k,"width":1.2/k,"kind":"door","symbol":"Concourse door","sill":0,"head":3}]})).unwrap());
        let opening = s.wall_groups(None).unwrap()[0].runs[0].openings[0].insert;
        let assert_insert = |s: &Session, width: f64| {
            let groups = s.wall_groups(None).unwrap();
            let h = groups[0].runs[0].openings[0].insert;
            let EntityType::Insert(i) = s.drawing.doc.get_entity(Handle::new(h)).unwrap() else {
                panic!("insert")
            };
            assert_eq!(i.block_name, "Concourse door");
            near(i.x_scale(), width / 2.0);
            near(i.y_scale(), width / 2.0);
            near(i.insert_point.x * k, 5.0 - width / 2.0);
            near(i.insert_point.y * k, -0.15);
            let leaf = s
                .drawing
                .doc
                .entities_in_block("Concourse door")
                .find(|e| matches!(e, EntityType::Line(l) if l.start.y == l.end.y))
                .unwrap();
            let measured = s.entity_info(leaf.common().handle.value(), &[h]).unwrap();
            near(measured.length.unwrap() * k, width);
            near(measured.start.unwrap()[0] * k, 5.0 - width / 2.0);
            near(measured.end.unwrap()[0] * k, 5.0 + width / 2.0);
        };
        assert_insert(&s, 1.2);
        s.run_input(
            &json!({"type":"edit_wall_opening","insert":opening,"width":1.8/k}),
            qiyas_core::Source::Ai,
        )
        .unwrap();
        assert_insert(&s, 1.8);
        s.undo();
        assert_insert(&s, 1.2);
        s.redo();
        assert_insert(&s, 1.8);
        let h = s.wall_groups(None).unwrap()[0].runs[0].openings[0].insert;
        s.run_input(
            &json!({"type":"edit_wall_opening","insert":h,"swing":"right"}),
            qiyas_core::Source::Ai,
        )
        .unwrap();
        let EntityType::Insert(i) = s.drawing.doc.get_entity(Handle::new(h)).unwrap() else {
            panic!("insert")
        };
        assert_eq!(i.block_name, "Concourse door");
        near(i.x_scale(), -0.9);
        near(i.insert_point.x * k, 5.9);
        s.undo();
        assert_insert(&s, 1.8);
        s.save_as_headless(&path).unwrap();
        let reopened = Session::open_standalone(&path).unwrap();
        assert_insert(&reopened, 1.8);
        near(result.net_area, 5.64);
        let openings = reopened
            .objects(&qiyas_core::objects::ObjectFilter {
                category: Some("opening".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(openings.len(), 1);
        near(openings[0].instance.props["width"].as_f64().unwrap(), 1.8);
    }
}

#[test]
fn added_opening_uses_a_drawing_symbol_and_missing_symbol_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    s.run_input(
        &json!({"type":"define_block","name":"Concourse glazing","base":[10,20],"entities":[
            {"type":"Line","data":{"start":{"x":9,"y":19,"z":0},"end":{"x":11,"y":19,"z":0}}}
        ]}),
        qiyas_core::Source::Ai,
    )
    .unwrap();
    let result = draw(
        &mut s,
        serde_json::from_value(
            json!({"type":"walls","runs":[{"points":[[0,0],[0,20]]}],"thickness":0.3,"height":4}),
        )
        .unwrap(),
    );
    let command = json!({"type":"add_wall_openings","centreline":result.centrelines[0],"openings":[{"at":5,"width":3,"kind":"window","symbol":"Concourse glazing","sill":1,"head":3}]});
    s.run_input(&command, qiyas_core::Source::Ai).unwrap();
    let groups = s.wall_groups(None).unwrap();
    let h = groups[0].runs[0].openings[0].insert;
    let EntityType::Insert(i) = s.drawing.doc.get_entity(Handle::new(h)).unwrap() else {
        panic!("insert")
    };
    assert_eq!(i.block_name, "Concourse glazing");
    near(i.x_scale(), 1.5);
    near(i.insert_point.x, -1.5);
    near(i.insert_point.y, 5.0);
    near(i.rotation, std::f64::consts::FRAC_PI_2);
    let leaf = s
        .drawing
        .doc
        .entities_in_block("Concourse glazing")
        .next()
        .unwrap();
    let measured = s.entity_info(leaf.common().handle.value(), &[h]).unwrap();
    near(measured.length.unwrap(), 3.0);
    near(measured.start.unwrap()[0], 0.0);
    near(measured.start.unwrap()[1], 3.5);
    near(measured.end.unwrap()[0], 0.0);
    near(measured.end.unwrap()[1], 6.5);
    let before = serde_json::to_value(s.wall_groups(None).unwrap()).unwrap();
    let steps = s.history_steps().0.len();
    assert!(s
        .run_input(
            &json!({"type":"edit_wall_opening","insert":h,"symbol":"Missing block"}),
            qiyas_core::Source::Ai
        )
        .is_err());
    assert_eq!(
        serde_json::to_value(s.wall_groups(None).unwrap()).unwrap(),
        before
    );
    assert_eq!(s.history_steps().0.len(), steps);
    s.undo();
    assert!(s.wall_groups(None).unwrap()[0].runs[0].openings.is_empty());
    s.redo();
    assert_eq!(
        serde_json::to_value(s.wall_groups(None).unwrap()).unwrap(),
        before
    );
}

#[test]
fn opening_marks_are_accepted_at_creation_and_survive_rebuilds() {
    let mut s = Session::new_blank(std::path::Path::new("tower-core.dxf"), 6);
    let out = s.run_input(&json!({"type":"walls","runs":[{"points":[[0,0],[48,0]]}],"thickness":0.15,"height":3.9,"openings":[{"run":0,"at":3.7,"width":1,"kind":"door","head":2.4,"mark":"D-M1"}]}), qiyas_core::Source::Ai).unwrap();
    let Outcome::Walls { result } = out.outcome else {
        panic!("walls")
    };
    assert_eq!(
        s.wall_groups(None).unwrap()[0].runs[0].openings[0]
            .mark
            .as_deref(),
        Some("D-M1")
    );
    s.run_input(&json!({"type":"add_wall_openings","centreline":result.centrelines[0],"openings":[{"at":12,"width":1,"kind":"door","head":2.4,"mark":"D-M2"}]}), qiyas_core::Source::Ai).unwrap();
    let groups = s.wall_groups(None).unwrap();
    assert_eq!(groups[0].runs[0].openings[0].mark.as_deref(), Some("D-M1"));
    assert_eq!(groups[0].runs[0].openings[1].mark.as_deref(), Some("D-M2"));
    let insert = groups[0].runs[0].openings[1].insert;
    s.run_input(
        &json!({"type":"edit_wall_opening","insert":insert,"edit":{"mark":"D-M3","width":1.2}}),
        qiyas_core::Source::Ai,
    )
    .unwrap();
    assert_eq!(
        s.wall_groups(None).unwrap()[0].runs[0].openings[1]
            .mark
            .as_deref(),
        Some("D-M3")
    );
    assert!(s.undo());
    assert_eq!(
        s.wall_groups(None).unwrap()[0].runs[0].openings[1]
            .mark
            .as_deref(),
        Some("D-M2")
    );
}
