use opencadcodec::{EntityType, Handle};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, command: Value) -> Value {
    serde_json::to_value(session.run_input(&command, Source::User).unwrap().outcome).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

#[test]
fn legacy_arguments_create_ordinary_objects_whose_recipes_and_bindings_control_the_geometry() {
    let dir = tempfile::tempdir().unwrap();
    for units in [4, 5, 6, 1] {
        let mut s = Session::new_blank(&dir.path().join("recipe.dxf"), units);
        let u = 1.0 / s.drawing.units.to_metres();
        let wall = run(
            &mut s,
            json!({
                "type":"walls","thickness":0.2*u,"height":3,
                "runs":[{"points":[[0,0],[10.0*u,0]]}],
                "openings":[{"run":0,"at":4.0*u,"width":1.2*u,"kind":"window","sill":0.9,"head":2.1}]
            }),
        );
        let driver = wall["centrelines"][0].as_u64().unwrap();
        let opening = wall["inserts"][0].as_u64().unwrap();
        let mut definition = serde_json::to_value(
            s.object_types()
                .unwrap()
                .into_iter()
                .find(|d| d.name == "Legacy wall")
                .unwrap(),
        )
        .unwrap();
        definition["plan"]["band_faces"] = json!("width * 2");
        run(
            &mut s,
            json!({"type":"redefine_object_type","definition":definition}),
        );
        let records = s.objects(&Default::default()).unwrap();
        assert!(records.iter().all(|r| r.instance.compatibility.is_none()));
        let record = records.iter().find(|r| r.instance.id == driver).unwrap();
        near(record.facts["gross_area"], 4.0);
        near(record.facts["net_area"], 3.52);
        near(record.facts["volume"], 11.424);
        let child = records.iter().find(|r| r.instance.id == opening).unwrap();
        near(child.instance.props["host_width"].as_f64().unwrap(), 0.4);
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[driver],"props":{"width":0.1}}),
        );
        let records = s.objects(&Default::default()).unwrap();
        let record = records.iter().find(|r| r.instance.id == driver).unwrap();
        near(record.facts["gross_area"], 2.0);
        near(record.facts["volume"], 5.712);
        near(
            records
                .iter()
                .find(|r| r.instance.id == opening)
                .unwrap()
                .instance
                .props["host_width"]
                .as_f64()
                .unwrap(),
            0.2,
        );
        assert!(s.undo());
        near(
            s.objects(&Default::default())
                .unwrap()
                .iter()
                .find(|r| r.instance.id == driver)
                .unwrap()
                .facts["gross_area"],
            4.0,
        );
    }
}

#[test]
fn stored_plan_switches_and_symbols_rederive_under_stable_driver_handles() {
    let dir = tempfile::tempdir().unwrap();
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("plan.{extension}"));
        let mut s = Session::new_blank(&path, 6);
        let wall = run(
            &mut s,
            json!({"type":"walls","thickness":0.2,"runs":[{"points":[[0,0],[5,0]]}]}),
        );
        let driver = wall["centrelines"][0].as_u64().unwrap();
        let hatch = wall["hatches"][0].as_u64().unwrap();
        let mut definition = serde_json::to_value(
            s.object_types()
                .unwrap()
                .into_iter()
                .find(|d| d.name == "Legacy wall")
                .unwrap(),
        )
        .unwrap();
        definition["plan"]["hatch"] = json!(false);
        definition["plan"]["centreline"] = json!(false);
        definition["plan"]["band_justification"] = json!("'left'");
        definition["plan"]["symbols"] =
            json!([{"kind":"circle","centre":[0,0],"radius":"=width / unit"}]);
        run(
            &mut s,
            json!({"type":"redefine_object_type","definition":definition}),
        );
        assert!(s.drawing.doc.get_entity(Handle::new(hatch)).is_none());
        assert!(
            s.drawing
                .doc
                .get_entity(Handle::new(driver))
                .unwrap()
                .common()
                .invisible
        );
        let parts: Vec<_> = s
            .drawing
            .doc
            .entities()
            .filter(|e| {
                qiyas_core::xdata::read_json::<Value>(e.common(), "QIYAS_OBJECT_PART")
                    .is_some_and(|p| p["owner"] == driver)
            })
            .collect();
        assert!(parts.iter().any(|e| matches!(e, EntityType::Circle(_))));
        assert!(!parts.iter().any(|e| matches!(e, EntityType::Hatch(_))));
        s.save_as(&path).unwrap();
        let mut s = Session::open(&path).unwrap();
        run(
            &mut s,
            json!({"type":"set_object_props","targets":[driver],"props":{"width":0.4}}),
        );
        s.save().unwrap();
        let s = Session::open(&path).unwrap();
        near(
            s.objects(&Default::default())
                .unwrap()
                .iter()
                .find(|r| r.instance.id == driver)
                .unwrap()
                .facts["gross_area"],
            2.0,
        );
        assert!(
            s.drawing
                .doc
                .get_entity(Handle::new(driver))
                .unwrap()
                .common()
                .invisible
        );
    }
}
