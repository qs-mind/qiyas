//! A data-defined symbol keeps its driver's local frame through normal CAD edits and files.

use opencadcodec::{EntityType, Handle};
use qiyas_core::objects::ObjectFilter;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, input: Value) -> Outcome {
    s.run_input(&input, Source::User).unwrap().outcome
}

fn added(s: &mut Session, input: Value) -> u64 {
    match run(s, input) {
        Outcome::Added { handles } => handles[0],
        other => panic!("Expected added handles: {other:?}"),
    }
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

fn setup(units: i16) -> (tempfile::TempDir, Session, u64, u64) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("symbol.dxf"), units);
    run(
        &mut s,
        json!({"type":"define_object_type", "definition":{
            "name":"Column", "category":"Structure", "geometry":"point",
            "properties":{
                "width":{"kind":"number","unit":"length","default":0.4},
                "depth":{"kind":"number","unit":"length","default":0.8},
                "rise":{"kind":"number","unit":"length","default":3.0}
            },
            "plan":{"symbols":[{"kind":"rectangle", "a":["=-width/(2*unit)","=-depth/(2*unit)"], "b":["=width/(2*unit)","=depth/(2*unit)"]}]},
            "quantities":{"volume":"width * depth * rise", "direction":"geometry.rotation"}
        }}),
    );
    let k = 1.0 / s.drawing.units.to_metres();
    let driver = added(
        &mut s,
        json!({"type":"place_objects", "objects":[{"type":"Column","points":[[2.0*k,3.0*k]]}]}),
    );
    let part = s
        .drawing
        .doc
        .entities()
        .find(|e| {
            qiyas_core::xdata::read_json::<Value>(e.common(), "QIYAS_OBJECT_PART")
                .is_some_and(|part| part["owner"] == driver && part["role"] == "symbol:0")
        })
        .unwrap()
        .common()
        .handle
        .value();
    (dir, s, driver, part)
}

fn assert_symbol(
    s: &Session,
    driver: u64,
    part: u64,
    centre: [f64; 2],
    size: [f64; 2],
    angle: f64,
    volume: f64,
) {
    let factor = s.drawing.units.to_metres();
    let EntityType::Point(point) = s.drawing.doc.get_entity(Handle::new(driver)).unwrap() else {
        panic!("Point driver")
    };
    near(point.location.x * factor, centre[0]);
    near(point.location.y * factor, centre[1]);
    near(point.x_axis_angle.to_degrees(), angle);
    let EntityType::LwPolyline(poly) = s.drawing.doc.get_entity(Handle::new(part)).unwrap() else {
        panic!("Rectangle symbol")
    };
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for v in &poly.vertices {
        for (axis, coordinate) in [v.location.x, v.location.y].into_iter().enumerate() {
            min[axis] = min[axis].min(coordinate * factor);
            max[axis] = max[axis].max(coordinate * factor);
        }
    }
    for axis in 0..2 {
        near(max[axis] - min[axis], size[axis]);
        near((max[axis] + min[axis]) / 2.0, centre[axis]);
    }
    let objects = s.objects(&ObjectFilter::default()).unwrap();
    let object = objects.iter().find(|o| o.instance.id == driver).unwrap();
    near(object.facts["x"], centre[0]);
    near(object.facts["y"], centre[1]);
    near(object.quantities["volume"], volume);
    near(object.quantities["direction"], angle);
}

#[test]
fn rotating_a_selected_symbol_keeps_orientation_in_undo_and_dwg_dxf() {
    for units in [6, 4] {
        let (dir, mut s, driver, part) = setup(units);
        let k = 1.0 / s.drawing.units.to_metres();
        assert_symbol(&s, driver, part, [2.0, 3.0], [0.4, 0.8], 0.0, 0.96);
        let before = s.history_steps().0.len();
        run(
            &mut s,
            json!({"type":"modify","kind":"rotate","targets":[{"handle":part}],"base":[2.0*k,3.0*k],"angle":90}),
        );
        assert_eq!(s.history_steps().0.len(), before + 1);
        assert_symbol(&s, driver, part, [2.0, 3.0], [0.8, 0.4], 90.0, 0.96);
        assert!(s.undo());
        assert_symbol(&s, driver, part, [2.0, 3.0], [0.4, 0.8], 0.0, 0.96);
        assert!(s.redo());
        for extension in ["dwg", "dxf"] {
            let path = dir.path().join(format!("rotated.{extension}"));
            s.save_as_headless(&path).unwrap();
            let project = path.with_extension("qiyas");
            if project.exists() {
                std::fs::remove_file(project).unwrap();
            }
            let mut reopened = Session::open(&path).unwrap();
            assert_symbol(&reopened, driver, part, [2.0, 3.0], [0.8, 0.4], 90.0, 0.96);
            // Force regeneration after reading: stored graphics alone cannot prove the frame survived.
            run(
                &mut reopened,
                json!({"type":"set_object_props","targets":[driver],"props":{"rise":4.0}}),
            );
            assert_symbol(&reopened, driver, part, [2.0, 3.0], [0.8, 0.4], 90.0, 1.28);
            assert!(reopened.undo());
            assert_symbol(&reopened, driver, part, [2.0, 3.0], [0.8, 0.4], 90.0, 0.96);
        }
    }
}

#[test]
fn scaling_a_point_symbol_moves_its_insertion_and_keeps_explicit_physical_dimensions() {
    let (_dir, mut s, driver, part) = setup(6);
    run(
        &mut s,
        json!({"type":"modify","kind":"rotate","targets":[{"handle":part}],"base":[2,3],"angle":90}),
    );
    run(
        &mut s,
        json!({"type":"modify","kind":"scale","targets":[{"handle":part}],"base":[0,0],"factor":2}),
    );
    assert_symbol(&s, driver, part, [4.0, 6.0], [0.8, 0.4], 90.0, 0.96);
    assert!(s.undo());
    assert_symbol(&s, driver, part, [2.0, 3.0], [0.8, 0.4], 90.0, 0.96);
    // Properties, rather than the point transform, change physical sizes and quantities.
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[driver],"props":{"width":0.6}}),
    );
    assert_symbol(&s, driver, part, [2.0, 3.0], [0.8, 0.6], 90.0, 1.44);
}

#[test]
fn an_asymmetric_point_symbol_retains_mirror_handedness_through_files_and_regeneration() {
    let (dir, mut s, driver, part) = setup(6);
    let mut definition = serde_json::to_value(s.object_types().unwrap().remove(0)).unwrap();
    definition["plan"]["symbols"] = json!([{"kind":"polyline", "closed":true,
        "points":[[0,0],["=width/unit",0],[0,"=depth/unit"]]
    }]);
    run(
        &mut s,
        json!({"type":"redefine_object_type","definition":definition}),
    );
    let check = |s: &Session, mirrored: bool, volume: f64| {
        let EntityType::LwPolyline(poly) = s.drawing.doc.get_entity(Handle::new(part)).unwrap()
        else {
            panic!("Triangle symbol")
        };
        assert_eq!(poly.vertices.len(), 3);
        let sign = if mirrored { -1.0 } else { 1.0 };
        for [x, y] in [[2.0 * sign, 3.0], [2.4 * sign, 3.0], [2.0 * sign, 3.8]] {
            assert!(poly
                .vertices
                .iter()
                .any(|v| (v.location.x - x).abs() < 1e-8 && (v.location.y - y).abs() < 1e-8));
        }
        let EntityType::Point(point) = s.drawing.doc.get_entity(Handle::new(driver)).unwrap()
        else {
            panic!("Point driver")
        };
        near(point.normal.z, sign);
        near(
            s.objects(&ObjectFilter::default()).unwrap()[0].quantities["volume"],
            volume,
        );
    };
    check(&s, false, 0.96);
    let before = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"modify","kind":"mirror","targets":[{"handle":part}],"p1":[0,0],"p2":[0,1],"keep":false}),
    );
    assert_eq!(s.history_steps().0.len(), before + 1);
    check(&s, true, 0.96);
    assert!(s.undo());
    check(&s, false, 0.96);
    assert!(s.redo());
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("reflected.{extension}"));
        s.save_as_headless(&path).unwrap();
        let project = path.with_extension("qiyas");
        if project.exists() {
            std::fs::remove_file(project).unwrap();
        }
        let mut reopened = Session::open(&path).unwrap();
        check(&reopened, true, 0.96);
        run(
            &mut reopened,
            json!({"type":"set_object_props","targets":[driver],"props":{"rise":4.0}}),
        );
        check(&reopened, true, 1.28);
        assert!(reopened.undo());
        check(&reopened, true, 0.96);
        run(
            &mut reopened,
            json!({"type":"modify","kind":"mirror","targets":[{"handle":part}],"p1":[0,0],"p2":[0,1],"keep":false}),
        );
        check(&reopened, false, 0.96);
        assert!(reopened.undo());
        check(&reopened, true, 0.96);
    }
}
