//! Generic object mechanics: joins, hosted cuts, copied drivers, lookup data and rollback.

use opencadcodec::{EntityType, Handle};
use qiyas_core::objects::{ObjectFilter, ObjectRecord};
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, input: Value) -> Outcome {
    s.run_input(&input, Source::User).unwrap().outcome
}

fn added(s: &mut Session, input: Value) -> Vec<u64> {
    match run(s, input) {
        Outcome::Added { handles } => handles,
        other => panic!("Expected added handles, got {other:?}"),
    }
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn objects(s: &Session, type_name: &str) -> Vec<ObjectRecord> {
    s.objects(&ObjectFilter {
        type_name: Some(type_name.into()),
        ..ObjectFilter::default()
    })
    .unwrap()
}

fn band(join: &str) -> Value {
    json!({
        "name":"Band", "category":"Test geometry", "geometry":"band", "join":join,
        "properties":{
            "width":{"kind":"number","unit":"length","default":0.2,"min":0.01},
            "rise":{"kind":"number","unit":"length","default":3.0,"min":0.01}
        },
        "vertical":{"height":"rise"},
        "plan":{"band_faces":"width","hatch":true,"centreline":true},
        "quantities":{"plan_area":"net_area","solid":"geometry.net_area * height"}
    })
}

fn define(s: &mut Session, definition: Value) {
    run(
        s,
        json!({"type":"define_object_type","definition":definition}),
    );
}

#[test]
fn explicit_assemblies_integrate_height_and_bind_host_properties_as_data() {
    for units in [4, 6] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("assembly.dwg"), units);
        let u = 1.0 / s.drawing.units.to_metres();
        let mut definition = band("union");
        definition["quantities"]["solid"] = json!("geometry.volume");
        define(&mut s, definition);
        let drivers = added(
            &mut s,
            json!({"type":"place_objects","objects":[
                {"type":"Band","assembly":"frame","points":[[0,0],[4.0*u,0]]},
                {"type":"Band","assembly":"frame","points":[[2.0*u,-2.0*u],[2.0*u,2.0*u]],"props":{"rise":2.0}}
            ]}),
        );
        near(
            objects(&s, "Band")
                .iter()
                .map(|r| r.quantities["plan_area"])
                .sum(),
            1.56,
        );
        near(
            objects(&s, "Band")
                .iter()
                .map(|r| r.quantities["solid"])
                .sum(),
            3.92,
        );
        define(
            &mut s,
            json!({"name":"Void","category":"Geometry","geometry":"hosted",
                "properties":{"span":{"kind":"number","unit":"length","default":0.5},
                    "station":{"kind":"number","unit":"length","default":1.0},
                    "depth":{"kind":"number","unit":"length","default":0.0}},
                "property_bindings":{"depth":"host.band_width"},
                "vertical":{"base":"0.7","height":"1.2"},
                "host_cut":{"width":"span","at":"station"},
                "quantities":{"opening_depth":"depth"}
            }),
        );
        let child = added(
            &mut s,
            json!({"type":"place_objects","objects":[
                {"type":"Void","points":[[u,0]],"host":drivers[0]}
            ]}),
        )[0];
        near(
            objects(&s, "Band")
                .iter()
                .map(|r| r.quantities["solid"])
                .sum(),
            3.80,
        );
        near(objects(&s, "Void")[0].quantities["opening_depth"], 0.2);
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[drivers[0]],"props":{"width":0.3}}),
        );
        near(
            objects(&s, "Void")[0].instance.props["depth"]
                .as_f64()
                .unwrap(),
            0.3,
        );
        assert!(s.undo());
        near(objects(&s, "Void")[0].quantities["opening_depth"], 0.2);
        let total: f64 = objects(&s, "Band")
            .iter()
            .map(|r| r.quantities["solid"])
            .sum();
        run(
            &mut s,
            json!({"type":"modify","kind":"copy",
            "targets":[{"handle":drivers[0]},{"handle":drivers[1]},{"handle":child}],
            "delta":[0,10.0*u],"count":1}),
        );
        near(
            objects(&s, "Band")
                .iter()
                .map(|r| r.quantities["solid"])
                .sum(),
            total * 2.0,
        );
        for extension in ["dwg", "dxf"] {
            let path = dir.path().join(format!("saved.{extension}"));
            s.save_as_headless(&path).unwrap();
            let reopened = Session::open(&path).unwrap();
            near(
                objects(&reopened, "Band")
                    .iter()
                    .map(|r| r.quantities["solid"])
                    .sum(),
                total * 2.0,
            );
        }
    }
}

fn opening(s: &mut Session) {
    define(
        s,
        json!({
            "name":"Cut", "category":"Test geometry", "geometry":"hosted",
            "properties":{
                "width":{"kind":"number","unit":"length","default":0.5,"min":0.01},
                "station":{"kind":"number","unit":"length","default":1.0,"min":0.0}
            },
            "host_cut":{"width":"width","at":"station"},
            "plan":{"symbols":[{"kind":"circle","centre":[0,0],"radius":"=width / (2 * unit)"}]},
            "quantities":{"pieces":"count"}
        }),
    );
}

fn hatch_area(s: &Session) -> f64 {
    s.drawing
        .doc
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Hatch(hatch) => Some(
                s.entity_info(hatch.common.handle.value(), &[])
                    .unwrap()
                    .area
                    .unwrap(),
            ),
            _ => None,
        })
        .sum::<f64>()
        * s.drawing.units.to_metres().powi(2)
}

fn part_handles(s: &Session) -> Vec<u64> {
    let mut handles: Vec<_> = s
        .drawing
        .doc
        .entities()
        .filter(|entity| {
            entity
                .common()
                .extended_data
                .get_record("QIYAS_OBJECT_PART")
                .is_some()
        })
        .map(|entity| entity.common().handle.value())
        .collect();
    handles.sort_unstable();
    handles
}

fn cells(s: &Session, handle: u64) -> Vec<Vec<String>> {
    let EntityType::Table(table) = s.drawing.doc.get_entity(Handle::new(handle)).unwrap() else {
        panic!("Expected an object schedule")
    };
    (0..table.rows.len())
        .map(|row| {
            (0..table.columns.len())
                .map(|column| table.cell_text(row, column).unwrap_or("").to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn union_bands_with_per_instance_width_and_hosted_cuts_match_graphics_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for units in [4, 5, 6, 1] {
        let mut s = Session::new_blank(&dir.path().join(format!("union-{units}.dxf")), units);
        let m = 1.0 / s.drawing.units.to_metres();
        define(&mut s, band("union"));
        opening(&mut s);
        let hosts = added(
            &mut s,
            json!({"type":"place_objects","objects":[
                {"type":"Band","points":[[0,0],[4.0*m,0]]},
                {"type":"Band","points":[[2.0*m,-2.0*m],[2.0*m,2.0*m]],"props":{"width":0.4}}
            ]}),
        );
        let rows = objects(&s, "Band");
        near(rows[0].quantities["plan_area"], 0.8);
        near(rows[1].quantities["plan_area"], 1.52);
        near(hatch_area(&s), 2.32);
        let steps = s.history_steps().0.len();
        let cuts = added(
            &mut s,
            json!({"type":"place_objects","objects":[
                {"type":"Cut","points":[[m,0]],"host":hosts[0]},
                {"type":"Cut","points":[[2.0*m,-m]],"host":hosts[1]}
            ]}),
        );
        assert_eq!(s.history_steps().0.len(), steps + 1);
        let rows = objects(&s, "Band");
        near(rows.iter().map(|r| r.quantities["plan_area"]).sum(), 2.02);
        near(rows.iter().map(|r| r.quantities["solid"]).sum(), 6.06);
        near(hatch_area(&s), 2.02);
        let radii: Vec<_> = s
            .drawing
            .doc
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Circle(circle) => Some(circle.radius / m),
                _ => None,
            })
            .collect();
        assert_eq!(radii.len(), 2);
        for radius in radii {
            near(radius, 0.25);
        }

        run(
            &mut s,
            json!({"type":"set_object_props","targets":[cuts[1]],"props":{"width":0.75}}),
        );
        near(hatch_area(&s), 1.92);
        assert!(s.undo());
        near(hatch_area(&s), 2.02);
        assert!(s.undo());
        assert!(objects(&s, "Cut").is_empty());
        near(hatch_area(&s), 2.32);
        assert!(s.redo());
        near(hatch_area(&s), 2.02);
    }
}

#[test]
fn union_rebuilds_on_move_and_reassigns_graphics_after_lead_driver_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("union-edit.dxf"), 6);
    define(&mut s, band("union"));
    let hosts = added(
        &mut s,
        json!({"type":"place_objects","objects":[
            {"type":"Band","points":[[0,0],[4,0]]},
            {"type":"Band","points":[[2,-2],[2,2]],"props":{"width":0.4}}
        ]}),
    );
    let initial_parts = part_handles(&s);
    run(
        &mut s,
        json!({"type":"modify","kind":"move","targets":[{"handle":hosts[1]}],"delta":[0,5]}),
    );
    near(hatch_area(&s), 2.4);
    near(
        objects(&s, "Band")
            .iter()
            .map(|r| r.quantities["plan_area"])
            .sum(),
        2.4,
    );
    assert!(s.undo());
    assert_eq!(part_handles(&s), initial_parts);
    near(hatch_area(&s), 2.32);
    run(
        &mut s,
        json!({"type":"delete_entities","targets":[{"handle":hosts[0]}]}),
    );
    assert_eq!(objects(&s, "Band").len(), 1);
    near(objects(&s, "Band")[0].quantities["plan_area"], 1.6);
    near(hatch_area(&s), 1.6);
    assert!(s.undo());
    assert_eq!(part_handles(&s), initial_parts);
    near(hatch_area(&s), 2.32);
    for extension in ["dxf", "dwg"] {
        let path = dir.path().join(format!("union-saved.{extension}"));
        s.save_as_headless(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        near(hatch_area(&reopened), 2.32);
        near(
            objects(&reopened, "Band")
                .iter()
                .map(|r| r.quantities["plan_area"])
                .sum(),
            2.32,
        );
        run(
            &mut reopened,
            json!({"type":"set_object_props","targets":[hosts[1]],"props":{"width":0.6}}),
        );
        near(hatch_area(&reopened), 3.08);
        near(
            objects(&reopened, "Band")
                .iter()
                .map(|r| r.quantities["plan_area"])
                .sum(),
            3.08,
        );
        assert!(reopened.undo());
        near(hatch_area(&reopened), 2.32);
    }
}

#[test]
fn copying_a_driver_gets_new_identity_and_deleting_it_removes_its_parts_in_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("copy.dxf"), 6);
    define(&mut s, band("separate"));
    let original = added(
        &mut s,
        json!({"type":"place_objects","objects":[
            {"type":"Band","points":[[0,0],[4,0]]}
        ]}),
    )[0];
    let original_parts = part_handles(&s);
    let before = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"modify","kind":"copy","targets":[{"handle":original}],"delta":[0,5],"count":1}),
    );
    assert_eq!(s.history_steps().0.len(), before + 1);
    let rows = objects(&s, "Band");
    assert_eq!(rows.len(), 2);
    let copied = rows
        .iter()
        .find(|r| r.instance.id != original)
        .unwrap()
        .instance
        .id;
    assert!(s.drawing.doc.get_entity(Handle::new(copied)).is_some());
    near(hatch_area(&s), 1.6);
    assert_eq!(part_handles(&s).len(), 2 * original_parts.len());
    assert!(s.undo());
    assert_eq!(part_handles(&s), original_parts);
    assert_eq!(objects(&s, "Band").len(), 1);
    assert!(s.redo());
    let copied_parts = part_handles(&s);
    run(
        &mut s,
        json!({"type":"delete_entities","targets":[{"handle":copied}]}),
    );
    assert_eq!(part_handles(&s), original_parts);
    near(hatch_area(&s), 0.8);
    assert!(s.undo());
    assert_eq!(part_handles(&s), copied_parts);
}

#[test]
fn lookup_and_level_data_rederive_quantities_graphics_and_fields_with_atomic_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("lookup.dxf"), 6);
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"tables","name":"dimensions","data":{"A":{"width":0.2}}}),
    );
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"levels","name":"Upper","data":{"elevation":4.0}}),
    );
    let mut definition = band("separate");
    definition["properties"]["reference"] =
        json!({"kind":"choice","default":"A","choices":["A","B"]});
    definition["plan"]["band_faces"] = json!("lookup('dimensions', reference, 'width')");
    definition["vertical"]["base"] = json!("level_elevation + 0.2");
    definition["vertical"]["level"] = json!("Upper");
    define(&mut s, definition);
    let driver = added(
        &mut s,
        json!({"type":"place_objects","objects":[
            {"type":"Band","points":[[0,0],[4,0]]}
        ]}),
    )[0];
    near(objects(&s, "Band")[0].facts["base"], 4.2);
    let table = added(
        &mut s,
        json!({"type":"object_schedule","at":[0,10],"columns":["type","solid"],"group_by":["type"]}),
    )[0];
    assert_eq!(cells(&s, table)[1], ["Band", "2.400"]);
    let parts = part_handles(&s);
    let steps = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"tables","name":"dimensions","data":{"A":{"width":0.4}}}),
    );
    assert_eq!(s.history_steps().0.len(), steps + 1);
    assert_eq!(part_handles(&s), parts);
    near(hatch_area(&s), 1.6);
    assert_eq!(cells(&s, table)[1], ["Band", "4.800"]);
    assert!(s.undo());
    near(hatch_area(&s), 0.8);
    assert_eq!(cells(&s, table)[1], ["Band", "2.400"]);
    let before_entities: Vec<_> = s.drawing.doc.entities().cloned().collect();
    let before_knowledge =
        qiyas_core::knowledge::list(&s.drawing.doc, qiyas_core::knowledge::Kind::Tables).unwrap();
    let before_steps = s.history_steps().0.len();
    for input in [
        json!({"type":"set_knowledge","kind":"tables","name":"dimensions","data":{"A":{"width":-1}}}),
        json!({"type":"set_object_props","targets":[driver],"props":{"reference":"B"}}),
        json!({"type":"delete_object_type","name":"Band"}),
    ] {
        assert!(s.run_input(&input, Source::User).is_err());
        assert_eq!(
            s.drawing.doc.entities().cloned().collect::<Vec<_>>(),
            before_entities
        );
        assert_eq!(
            qiyas_core::knowledge::list(&s.drawing.doc, qiyas_core::knowledge::Kind::Tables)
                .unwrap(),
            before_knowledge
        );
        assert_eq!(s.history_steps().0.len(), before_steps);
        assert_eq!(cells(&s, table)[1], ["Band", "2.400"]);
    }
}

#[test]
fn a_host_cannot_disappear_until_its_children_are_removed_and_undo_restores_the_cut() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("host.dxf"), 6);
    define(&mut s, band("separate"));
    opening(&mut s);
    let host = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[4,0]]}]}),
    )[0];
    let child = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Cut","points":[[1,0]],"host":host}]}),
    )[0];
    near(hatch_area(&s), 0.7);
    let parts = part_handles(&s);
    let steps = s.history_steps().0.len();
    assert!(s
        .run_input(
            &json!({"type":"delete_entities","targets":[{"handle":host}]}),
            Source::User
        )
        .is_err());
    assert_eq!(part_handles(&s), parts);
    assert_eq!(s.history_steps().0.len(), steps);
    assert_eq!(objects(&s, "Band").len(), 1);
    assert_eq!(objects(&s, "Cut").len(), 1);
    run(
        &mut s,
        json!({"type":"delete_entities","targets":[{"handle":child}]}),
    );
    near(hatch_area(&s), 0.8);
    assert!(objects(&s, "Cut").is_empty());
    assert!(s.undo());
    near(hatch_area(&s), 0.7);
    assert_eq!(part_handles(&s), parts);
}

#[test]
fn hosted_drivers_follow_the_host_and_a_copied_host_gets_its_own_copied_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("host-copy.dxf"), 6);
    define(&mut s, band("separate"));
    opening(&mut s);
    let host = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[4,0]]}]}),
    )[0];
    let child = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Cut","points":[[1,0]],"host":host}]}),
    )[0];
    run(
        &mut s,
        json!({"type":"modify","kind":"move","targets":[{"handle":host}],"delta":[4,5]}),
    );
    let position = |session: &Session, handle| {
        let EntityType::Point(point) = session.drawing.doc.get_entity(Handle::new(handle)).unwrap()
        else {
            panic!("Expected a hosted point driver")
        };
        [point.location.x, point.location.y]
    };
    assert_eq!(position(&s, child), [5.0, 5.0]);
    near(hatch_area(&s), 0.7);
    let steps = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"modify","kind":"copy","targets":[{"handle":host},{"handle":child}],"delta":[0,10],"count":1}),
    );
    assert_eq!(s.history_steps().0.len(), steps + 1);
    let copied_host = objects(&s, "Band")
        .into_iter()
        .find(|r| r.instance.id != host)
        .unwrap()
        .instance
        .id;
    let copied_child = objects(&s, "Cut")
        .into_iter()
        .find(|r| r.instance.id != child)
        .unwrap()
        .instance;
    assert_eq!(copied_child.host, Some(copied_host));
    assert_eq!(position(&s, copied_child.id), [5.0, 15.0]);
    assert_eq!(position(&s, child), [5.0, 5.0]);
    near(hatch_area(&s), 1.4);
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[copied_child.id],"props":{"width":0.75}}),
    );
    near(hatch_area(&s), 1.35);
    near(
        objects(&s, "Cut")
            .into_iter()
            .find(|r| r.instance.id == child)
            .unwrap()
            .instance
            .props["width"]
            .as_f64()
            .unwrap(),
        0.5,
    );
    assert!(s.undo());
    assert!(s.undo());
    assert_eq!(objects(&s, "Band").len(), 1);
    assert_eq!(objects(&s, "Cut").len(), 1);
    assert!(s.undo());
    assert_eq!(position(&s, child), [1.0, 0.0]);
}

#[test]
fn generated_parts_modify_copy_and_erase_their_driver_once_and_undo_together() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("part-edit.dxf"), 6);
    define(&mut s, band("separate"));
    let driver = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[4,0]]}]}),
    )[0];
    let parts = part_handles(&s);
    assert!(parts.len() >= 3);
    let targets: Vec<_> = parts
        .iter()
        .chain([&driver])
        .map(|handle| json!({"handle":handle}))
        .collect();
    let steps = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"modify","kind":"move","targets":targets,"delta":[10,5]}),
    );
    assert_eq!(s.history_steps().0.len(), steps + 1);
    assert_eq!(part_handles(&s), parts);
    let EntityType::LwPolyline(path) = s.drawing.doc.get_entity(Handle::new(driver)).unwrap()
    else {
        panic!("Expected a polyline driver")
    };
    assert_eq!(
        [path.vertices[0].location.x, path.vertices[0].location.y],
        [10.0, 5.0]
    );
    near(hatch_area(&s), 0.8);
    run(
        &mut s,
        json!({"type":"modify","kind":"copy","targets":[{"handle":parts[0]},{"handle":parts[1]}],"delta":[0,5],"count":1}),
    );
    assert_eq!(objects(&s, "Band").len(), 2);
    assert_eq!(part_handles(&s).len(), parts.len() * 2);
    near(hatch_area(&s), 1.6);
    let copied_part = part_handles(&s)
        .into_iter()
        .find(|handle| !parts.contains(handle))
        .unwrap();
    run(
        &mut s,
        json!({"type":"delete_entities","targets":[{"handle":copied_part}]}),
    );
    assert_eq!(objects(&s, "Band").len(), 1);
    assert_eq!(part_handles(&s), parts);
    assert!(s.undo());
    assert_eq!(objects(&s, "Band").len(), 2);
    near(hatch_area(&s), 1.6);
    assert!(s.undo());
    assert_eq!(objects(&s, "Band").len(), 1);
    assert!(s.undo());
    let EntityType::LwPolyline(path) = s.drawing.doc.get_entity(Handle::new(driver)).unwrap()
    else {
        panic!("Expected the restored driver")
    };
    assert_eq!(
        [path.vertices[0].location.x, path.vertices[0].location.y],
        [0.0, 0.0]
    );
    assert_eq!(part_handles(&s), parts);
}

#[test]
fn geometry_facts_remain_independent_of_same_named_quantities() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("facts.dxf"), 6);
    define(
        &mut s,
        json!({
            "name":"Facts", "category":"Test geometry", "geometry":"path",
            "properties":{"rise":{"kind":"number","unit":"length","default":2.0}},
            "vertical":{"base":"3", "height":"rise"},
            "plan":{"symbols":[{"kind":"circle","centre":[0,0],"radius":"=geometry.height / unit"}]},
            "quantities":{
                "height":"geometry.height * 2", "length":"geometry.length * 3",
                "reported":"geometry.height", "base":"geometry.base + 1"
            }
        }),
    );
    added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Facts","points":[[0,0],[4,0]]}]}),
    );
    let records = objects(&s, "Facts");
    let object = &records[0];
    near(object.quantities["height"], 4.0);
    near(object.quantities["length"], 12.0);
    near(object.quantities["base"], 4.0);
    near(object.quantities["reported"], 2.0);
    near(object.facts["height"], 2.0);
    near(object.facts["base"], 3.0);
    near(object.facts["length"], 4.0);
    let radius = s
        .drawing
        .doc
        .entities()
        .find_map(|entity| match entity {
            EntityType::Circle(circle) => Some(circle.radius),
            _ => None,
        })
        .unwrap();
    near(radius, 2.0);
}

#[test]
fn measured_hatch_and_area_field_follow_all_material_when_a_hosted_cut_splits_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("part-fields.dxf"), 6);
    define(&mut s, band("separate"));
    opening(&mut s);
    let host = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[4,0]]}]}),
    )[0];
    let hatch = s
        .drawing
        .doc
        .entities()
        .find_map(|entity| match entity {
            EntityType::Hatch(hatch) => Some(hatch.common.handle.value()),
            _ => None,
        })
        .unwrap();
    run(&mut s, json!({"type":"measure_entity","handle":hatch}));
    let field = added(
        &mut s,
        json!({"type":"field","at":[0,2],"field":{"kind":"area","target":{"handle":hatch}}}),
    )[0];
    let field_text = |session: &Session| {
        let EntityType::Text(text) = session.drawing.doc.get_entity(Handle::new(field)).unwrap()
        else {
            panic!("Expected an area text field")
        };
        text.value.clone()
    };
    near(s.takeoff.items[0].shown, 0.8);
    assert_eq!(field_text(&s), "0.80 m²");
    let cut = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Cut","points":[[1,0]],"host":host}]}),
    )[0];
    near(hatch_area(&s), 0.7);
    near(s.takeoff.items[0].shown, 0.7);
    assert_eq!(s.takeoff.items[0].handles.len(), 2);
    for handle in &s.takeoff.items[0].handles {
        assert!(matches!(
            s.drawing.doc.get_entity(Handle::new(*handle)),
            Some(EntityType::Hatch(_))
        ));
    }
    assert_eq!(field_text(&s), "0.70 m²");
    assert!(s.undo());
    near(s.takeoff.items[0].shown, 0.8);
    assert_eq!(s.takeoff.items[0].handles, [hatch]);
    assert_eq!(field_text(&s), "0.80 m²");
    assert!(s.redo());
    near(s.takeoff.items[0].shown, 0.7);
    run(
        &mut s,
        json!({"type":"delete_entities","targets":[{"handle":cut}]}),
    );
    near(hatch_area(&s), 0.8);
    near(s.takeoff.items[0].shown, 0.8);
    assert_eq!(s.takeoff.items[0].handles.len(), 1);
    assert_eq!(
        s.drawing
            .doc
            .entities()
            .filter(|entity| matches!(entity, EntityType::Hatch(_)))
            .count(),
        1
    );
    assert_eq!(field_text(&s), "0.80 m²");
    assert!(s.undo());
    near(s.takeoff.items[0].shown, 0.7);
    assert_eq!(s.takeoff.items[0].handles.len(), 2);
    assert_eq!(field_text(&s), "0.70 m²");
}

#[test]
fn damaged_generic_metadata_is_reported_without_hiding_objects_or_erasing_their_parts() {
    use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};

    let dir = tempfile::tempdir().unwrap();
    for (app, non_string) in [
        ("QIYAS_OBJECT", false),
        ("QIYAS_OBJECT_PART", false),
        ("QIYAS_OBJECT_SCHEDULE", false),
        ("QIYAS_OBJECT", true),
    ] {
        let mut s = Session::new_blank(&dir.path().join("damaged.dxf"), 6);
        define(&mut s, band("separate"));
        let driver = added(
            &mut s,
            json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[4,0]]}]}),
        )[0];
        let table = added(
            &mut s,
            json!({"type":"object_schedule","at":[0,10],"columns":["type","solid"]}),
        )[0];
        let parts = part_handles(&s);
        let target = match app {
            "QIYAS_OBJECT_PART" => parts[0],
            "QIYAS_OBJECT_SCHEDULE" => table,
            _ => driver,
        };
        let common = s
            .drawing
            .doc
            .get_entity_mut(Handle::new(target))
            .unwrap()
            .common_mut();
        let values = if non_string {
            let mut values = common.extended_data.get_record(app).unwrap().values.clone();
            values.push(XDataValue::Real(1.0));
            values
        } else {
            vec![XDataValue::String("{".into())]
        };
        common.extended_data.upsert_record(ExtendedDataRecord {
            application_name: app.into(),
            values,
        });
        common.raw_record = None;
        common.extended_data.raw_dwg_eed.clear();
        let expected = s.drawing.doc.entities().cloned().collect::<Vec<_>>();
        let steps = s.history_steps().0.len();
        let error = s.objects(&ObjectFilter::default()).unwrap_err().to_string();
        assert!(error.contains(app), "{error}");
        assert!(error.contains(&target.to_string()), "{error}");
        assert!(s
            .run_input(
                &json!({"type":"draw","kind":"point","p":[10,10]}),
                Source::User
            )
            .is_err());
        assert_eq!(s.history_steps().0.len(), steps);
        assert_eq!(
            s.drawing.doc.entities().cloned().collect::<Vec<_>>(),
            expected
        );
        assert_eq!(part_handles(&s), parts);
        near(hatch_area(&s), 0.8);
        if app == "QIYAS_OBJECT" && !non_string {
            for extension in ["dxf", "dwg"] {
                let path = dir.path().join(format!("damaged-saved.{extension}"));
                s.save_as_headless(&path).unwrap();
                let reopened = Session::open(&path).unwrap();
                assert!(reopened
                    .drawing
                    .problems
                    .iter()
                    .any(|problem| problem.contains("Object metadata needs review")
                        && problem.contains(app)));
                assert!(reopened.objects(&ObjectFilter::default()).is_err());
                assert_eq!(part_handles(&reopened), parts);
                near(hatch_area(&reopened), 0.8);
            }
        }
    }
}

#[test]
fn initially_empty_schedule_registers_live_fields_when_the_first_object_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("empty.dxf"), 6);
    define(&mut s, band("separate"));
    let table = added(
        &mut s,
        json!({"type":"object_schedule","at":[0,10],"columns":["type","solid"]}),
    )[0];
    assert!(!s.drawing.doc.app_ids.contains("QIYAS_FIELD"));
    let driver = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Band","points":[[0,0],[4,0]]}]}),
    )[0];
    assert!(s.drawing.doc.app_ids.contains("QIYAS_FIELD"));
    assert_eq!(cells(&s, table)[1], ["Band", "2.400"]);
    for ext in ["dwg", "dxf"] {
        let path = dir.path().join(format!("with-fields.{ext}"));
        s.save_as(&path).unwrap();
        let mut reopened = Session::open_standalone(&path).unwrap();
        run(
            &mut reopened,
            json!({"type":"set_object_props","targets":[driver],"props":{"width":0.4}}),
        );
        assert_eq!(cells(&reopened, table)[1], ["Band", "4.800"]);
        assert!(reopened.undo());
        assert_eq!(cells(&reopened, table)[1], ["Band", "2.400"]);
    }
}
