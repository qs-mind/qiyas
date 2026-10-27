//! Retained enclosures are a geometry source available to every data-defined region type.

use opencadcodec::{EntityType, Handle};
use qiyas_core::objects::ObjectFilter;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, value: Value) -> Outcome {
    session.run_input(&value, Source::User).unwrap().outcome
}

fn add(session: &mut Session, value: Value) -> u64 {
    let Outcome::Added { handles } = run(session, value) else {
        panic!("Expected added entities")
    };
    handles[0]
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn setup(units: i16) -> (tempfile::TempDir, Session, u64, u64, u64) {
    let directory = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&directory.path().join("regions.dwg"), units);
    let k = 1.0 / session.drawing.units.to_metres();
    let outline = add(
        &mut session,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4.0*k,3.0*k]}),
    );
    let island = add(
        &mut session,
        json!({"type":"draw","kind":"rectangle","a":[2.0*k,k],"b":[3.0*k,2.0*k]}),
    );
    run(
        &mut session,
        json!({"type":"define_object_type","definition":{
            "name":"Survey zone","category":"Survey","geometry":"region",
            "plan":{"outline":true,"hatch":true},
            "quantities":{"surface":"area"}
        }}),
    );
    let driver = add(
        &mut session,
        json!({"type":"place_objects","objects":[{
            "type":"Survey zone","points":[[0,0],[4.0*k,0],[4.0*k,3.0*k],[0,3.0*k]],
            "region":{"seed":[k,k],"subtract_islands":true}
        }]}),
    );
    (directory, session, outline, island, driver)
}

fn quantity(session: &Session, driver: u64) -> f64 {
    session
        .objects(&ObjectFilter::default())
        .unwrap()
        .into_iter()
        .find(|record| record.instance.id == driver)
        .unwrap()
        .quantities["surface"]
}

fn hatch_area(session: &Session, driver: u64) -> f64 {
    session
        .drawing
        .doc
        .entities()
        .filter(|entity| {
            matches!(entity, EntityType::Hatch(_))
                && qiyas_core::xdata::read_json::<Value>(entity.common(), "QIYAS_OBJECT_PART")
                    .is_some_and(|part| part["owner"] == driver)
        })
        .map(|entity| {
            session
                .entity_info(entity.common().handle.value(), &[])
                .unwrap()
                .area
                .unwrap()
                * session.drawing.units.to_metres().powi(2)
        })
        .sum()
}

#[test]
fn moving_a_retained_room_reports_derivation_and_keeps_the_unrelated_edit() {
    for units in [4, 5, 6, 1] {
        let (_directory, mut session, _outline, _island, driver) = setup(units);
        let k = 1.0 / session.drawing.units.to_metres();
        let line = add(
            &mut session,
            json!({"type":"draw","kind":"line","a":[8.0*k,0],"b":[9.0*k,0]}),
        );
        let before = session.drawing.doc.entities().cloned().collect::<Vec<_>>();
        let history = session.history_steps();
        let result = session
            .run_input(
                &json!({"type":"modify","kind":"move","targets":[driver,line],"delta":[0.1*k,0]}),
                Source::User,
            )
            .unwrap();
        assert!(result.advice.iter().any(|a| a["handle"] == driver
            && a["code"] == "derived_value_changed"
            && a["requested"] != a["actual"]));
        assert_eq!(session.history_steps().0.len(), history.0.len() + 1);
        let EntityType::Line(moved) = session
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(line))
            .unwrap()
        else {
            panic!("line")
        };
        near(moved.start.x, 8.1 * k);
        near(moved.end.x, 9.1 * k);
        near(quantity(&session, driver), 11.0);
        near(hatch_area(&session, driver), 11.0);
        assert!(session.undo());
        assert_eq!(session.history_steps().0, history.0);
        assert_eq!(session.history_steps().1.len(), 1);
        assert_eq!(
            session.drawing.doc.entities().cloned().collect::<Vec<_>>(),
            before
        );
    }
}

#[test]
fn arbitrary_region_type_follows_islands_and_source_edits_through_undo_and_files() {
    for units in [6, 4] {
        let (directory, mut session, outline, _island, driver) = setup(units);
        near(quantity(&session, driver), 11.0);
        near(hatch_area(&session, driver), 11.0);
        for extension in ["dwg", "dxf"] {
            let path = directory.path().join(format!("retained.{extension}"));
            session.save_as_headless(&path).unwrap();
            let mut reopened = Session::open(&path).unwrap();
            near(quantity(&reopened, driver), 11.0);
            near(hatch_area(&reopened, driver), 11.0);
            let before = reopened.history_steps().0.len();
            run(
                &mut reopened,
                json!({"type":"modify","kind":"scale","targets":[{"handle":outline}],"base":[0,0],"factor":2}),
            );
            assert_eq!(reopened.history_steps().0.len(), before + 1);
            near(quantity(&reopened, driver), 47.0);
            near(hatch_area(&reopened, driver), 47.0);
            assert!(reopened.undo());
            near(quantity(&reopened, driver), 11.0);
            near(hatch_area(&reopened, driver), 11.0);
            assert!(reopened.redo());
            near(quantity(&reopened, driver), 47.0);
        }
    }
}

#[test]
fn copied_region_source_follows_its_transform_and_invalidity_is_local() {
    let (_directory, mut session, outline, island, driver) = setup(6);
    run(
        &mut session,
        json!({"type":"modify","kind":"copy","targets":[{"handle":outline},{"handle":island},{"handle":driver}],"delta":[10,0],"count":1}),
    );
    let copied = session
        .objects(&ObjectFilter::default())
        .unwrap()
        .into_iter()
        .find(|record| record.instance.id != driver)
        .unwrap()
        .instance
        .id;
    near(quantity(&session, copied), 11.0);
    let region: Value = qiyas_core::xdata::read_json(
        session
            .drawing
            .doc
            .get_entity(Handle::new(copied))
            .unwrap()
            .common(),
        "QIYAS_OBJECT_REGION",
    )
    .unwrap();
    assert_eq!(region["seed"], json!([11.0, 1.0]));
    let table = add(
        &mut session,
        json!({"type":"object_schedule","at":[0,10],"columns":["type","surface"]}),
    );
    run(
        &mut session,
        json!({"type":"delete_entities","targets":[{"handle":outline}]}),
    );
    let records = session.objects(&ObjectFilter::default()).unwrap();
    let invalid = records
        .iter()
        .find(|record| record.instance.id == driver)
        .unwrap();
    assert!(!invalid.valid);
    assert!(invalid.quantities.is_empty());
    near(quantity(&session, copied), 11.0);
    let EntityType::Table(table) = session.drawing.doc.get_entity(Handle::new(table)).unwrap()
    else {
        panic!("Table")
    };
    assert_eq!(table.cell_text(1, 1), Some("####"));
    assert_eq!(table.cell_text(2, 1), Some("11.000"));
    assert!(session.undo());
    near(quantity(&session, driver), 11.0);
}

#[test]
fn legacy_room_uses_shared_drawing_recipe_and_quantity_fields() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&directory.path().join("legacy.dwg"), 6);
    add(
        &mut session,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4,3]}),
    );
    add(
        &mut session,
        json!({"type":"draw","kind":"rectangle","a":[2,1],"b":[3,2]}),
    );
    let Outcome::Rooms { report } = run(
        &mut session,
        json!({"type":"label_spaces","query":{"seeds":[{"name":"A","point":[1,1]}]}}),
    ) else {
        panic!("Room report")
    };
    let driver = report.rooms[0].boundary_handle.unwrap();
    let label = report.rooms[0].label_handle.unwrap();
    let object = session.objects(&ObjectFilter::default()).unwrap().remove(0);
    assert!(object.instance.compatibility.is_none());
    assert!(qiyas_core::xdata::read_text(
        session
            .drawing
            .doc
            .get_entity(Handle::new(label))
            .unwrap()
            .common(),
        "QIYAS_FIELD",
    )
    .unwrap()
    .contains("QiyasObject"));
    let mut definition = serde_json::to_value(session.object_types().unwrap().remove(0)).unwrap();
    definition["plan"]["hatch"] = json!(true);
    definition["plan"]["label_template"] = json!("Zone {name}: {net_floor_area}");
    definition["quantities"]["net_floor_area"] = json!("2 * area");
    run(
        &mut session,
        json!({"type":"redefine_object_type","definition":definition}),
    );
    near(hatch_area(&session, driver), 11.0);
    assert_eq!(
        session.entity_info(label, &[]).unwrap().text.as_deref(),
        Some("Area 22.00 m²")
    );
    assert!(session
        .drawing
        .doc
        .entities()
        .any(|entity| matches!(entity,EntityType::Text(text) if text.value=="Zone A: 22.00 m²")));
    assert!(session.undo());
    assert_eq!(
        session.entity_info(label, &[]).unwrap().text.as_deref(),
        Some("Area 11.00 m²")
    );
}

#[test]
fn curved_island_metadata_is_exact_and_accepts_two_bulged_vertices() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&directory.path().join("curved.dwg"), 6);
    let outline = add(
        &mut session,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4,3]}),
    );
    let island = add(
        &mut session,
        json!({"type":"draw","kind":"circle","centre":[2,1.5],"radius":0.25}),
    );
    run(
        &mut session,
        json!({"type":"define_object_type","definition":{
            "name":"Survey zone","category":"Survey","geometry":"region",
            "quantities":{"surface":"area"}
        }}),
    );
    let driver = add(
        &mut session,
        json!({"type":"place_objects","objects":[{
            "type":"Survey zone","points":[[0,0],[4,0],[4,3],[0,3]],
            "region":{"seed":[1,1]}
        }]}),
    );
    let expected = 12.0 - std::f64::consts::PI * 0.25_f64.powi(2);
    near(quantity(&session, driver), expected);
    let mut region: Value = qiyas_core::xdata::read_json(
        session
            .drawing
            .doc
            .get_entity(Handle::new(driver))
            .unwrap()
            .common(),
        "QIYAS_OBJECT_REGION",
    )
    .unwrap();
    // An exact circular hole may be saved as two semicircles instead of many short arcs.
    region["holes"] = json!([[[1.75, 1.5], [2.25, 1.5]]]);
    region["hole_bulges"] = json!([[1.0, 1.0]]);
    let app = qiyas_core::xdata::app_handle(&session.drawing.doc, "QIYAS_OBJECT_REGION");
    qiyas_core::xdata::write_json(
        session
            .drawing
            .doc
            .get_entity_mut(Handle::new(driver))
            .unwrap()
            .common_mut(),
        "QIYAS_OBJECT_REGION",
        app,
        &region,
    )
    .unwrap();
    near(quantity(&session, driver), expected);
    for extension in ["dwg", "dxf"] {
        let path = directory.path().join(format!("curved.{extension}"));
        session.save_as_headless(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        near(quantity(&reopened, driver), expected);
        run(
            &mut reopened,
            json!({"type":"modify","kind":"mirror","targets":[{"handle":outline},{"handle":island},{"handle":driver}],"p1":[0,0],"p2":[0,1],"keep":false}),
        );
        near(quantity(&reopened, driver), expected);
        assert!(reopened.undo());
        near(quantity(&reopened, driver), expected);
    }
    // Omitted bulges mean straight edges, and malformed arrays must not evade validation.
    region["holes"] = json!([[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]]]);
    region["hole_bulges"] = json!([]);
    qiyas_core::xdata::write_json(
        session
            .drawing
            .doc
            .get_entity_mut(Handle::new(driver))
            .unwrap()
            .common_mut(),
        "QIYAS_OBJECT_REGION",
        app,
        &region,
    )
    .unwrap();
    near(quantity(&session, driver), 11.0);
    region["hole_bulges"] = json!([[0.0]]);
    qiyas_core::xdata::write_json(
        session
            .drawing
            .doc
            .get_entity_mut(Handle::new(driver))
            .unwrap()
            .common_mut(),
        "QIYAS_OBJECT_REGION",
        app,
        &region,
    )
    .unwrap();
    assert!(session.objects(&ObjectFilter::default()).is_err());
    region["hole_bulges"] = json!([]);
    qiyas_core::xdata::write_json(
        session
            .drawing
            .doc
            .get_entity_mut(Handle::new(driver))
            .unwrap()
            .common_mut(),
        "QIYAS_OBJECT_REGION",
        app,
        &region,
    )
    .unwrap();
    run(
        &mut session,
        json!({"type":"delete_entities","targets":[{"handle":outline}]}),
    );
    let common = session
        .drawing
        .doc
        .get_entity_mut(Handle::new(driver))
        .unwrap()
        .common_mut();
    let mut invalid_region: Value =
        qiyas_core::xdata::read_json(common, "QIYAS_OBJECT_REGION").unwrap();
    assert_eq!(invalid_region["valid"], json!(false));
    invalid_region["app_handle"] = json!(app + 17);
    qiyas_core::xdata::write_json(common, "QIYAS_OBJECT_REGION", app, &invalid_region).unwrap();
    common.extended_data.raw_dwg_eed.push((app, vec![1, 2, 3]));
    // A retained boundary cannot be moved on its own. Refresh imported metadata through
    // a boundary-source edit instead; unrelated layer edits do not retrace regions.
    let history = session.history_steps();
    let error = session
        .run_input(
            &json!({"type":"modify","kind":"move","targets":[{"handle":driver}],"delta":[0.1,0]}),
            Source::User,
        )
        .unwrap_err();
    assert!(error.to_string().contains("enclosing walls"));
    assert_eq!(session.history_steps(), history);
    run(
        &mut session,
        json!({"type":"draw","kind":"line","a":[100,100],"b":[101,100]}),
    );
    let common = session
        .drawing
        .doc
        .get_entity(Handle::new(driver))
        .unwrap()
        .common();
    let refreshed: Value = qiyas_core::xdata::read_json(common, "QIYAS_OBJECT_REGION").unwrap();
    assert_eq!(refreshed["app_handle"], json!(app));
    assert_eq!(refreshed["valid"], json!(false));
    assert!(!common
        .extended_data
        .raw_dwg_eed
        .iter()
        .any(|(handle, _)| *handle == app));
}
