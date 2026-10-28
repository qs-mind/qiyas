//! Reviewed villa: physical union quantities and three independent generated BOQ items.
use qiyas_core::{objects::ObjectFilter, Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, command: Value) -> Value {
    json!(session.run_input(&command, Source::User).unwrap().outcome)
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
}

#[test]
fn reviewed_villa_faces_and_boq_in_metres_millimetres_centimetres_and_inches() {
    for units in [6, 4, 5, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("villa.dxf"), units);
        let k = 1.0 / session.drawing.units.to_metres();
        let points = [
            vec![[0.1, 0.1], [11.9, 0.1], [11.9, 14.9], [0.1, 14.9]],
            vec![[0.1, 6.0], [11.9, 6.0]],
            vec![[0.1, 9.0], [11.9, 9.0]],
            vec![[6.0, 9.0], [6.0, 14.9]],
            vec![[3.5, 9.0], [3.5, 11.5]],
            vec![[0.1, 11.5], [3.5, 11.5]],
            vec![[8.5, 9.0], [8.5, 11.5]],
            vec![[8.5, 11.5], [11.9, 11.5]],
            vec![[8.0, 0.1], [8.0, 6.0]],
            vec![[8.0, 3.5], [11.9, 3.5]],
        ];
        let points: Vec<Vec<_>> = points
            .iter()
            .map(|run| run.iter().map(|p| [p[0] * k, p[1] * k]).collect())
            .collect();
        let runs: Vec<_> = points.iter().enumerate().map(|(i, p)| json!({
            "points":p,"closed":i == 0,"thickness":if i == 0 {0.2*k} else {0.15*k},"height":3
        })).collect();
        run(
            &mut session,
            json!({"type":"walls","thickness":0.2*k,"height":3,"runs":runs}),
        );
        let groups = run(&mut session, json!({"type":"wall_groups","width":0.8*k}));
        let wall_runs: Vec<_> = groups["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["runs"].as_array().unwrap())
            .collect();
        let openings = [
            (
                0,
                vec![
                    (2.0, 1.2, 0.0, 2.2),
                    (5.5, 1.8, 0.9, 2.1),
                    (9.8, 1.2, 1.1, 2.1),
                    (16.8, 0.8, 1.5, 2.1),
                    (22.1, 0.8, 1.5, 2.1),
                    (29.5, 1.5, 0.9, 2.1),
                    (35.2, 1.5, 0.9, 2.1),
                    (43.0, 0.8, 1.5, 2.1),
                ],
            ),
            (1, vec![(4.5, 1.2, 0.0, 2.2), (10.0, 0.8, 0.0, 2.1)]),
            (2, vec![(4.7, 0.9, 0.0, 2.1), (7.2, 0.9, 0.0, 2.1)]),
            (4, vec![(1.2, 0.8, 0.0, 2.1)]),
            (6, vec![(1.2, 0.8, 0.0, 2.1)]),
            (8, vec![(2.0, 0.9, 0.0, 2.1)]),
        ];
        for (i, openings) in openings {
            let centreline = wall_runs
                .iter()
                .find(|r| {
                    let actual: Vec<[f64; 2]> =
                        serde_json::from_value(r["points"].clone()).unwrap();
                    actual.len() == points[i].len()
                        && actual.iter().zip(&points[i]).all(|(a, b)| {
                            (a[0] - b[0]).abs() < 1e-7 * k && (a[1] - b[1]).abs() < 1e-7 * k
                        })
                })
                .unwrap()["centreline"]
                .clone();
            let openings: Vec<_> = openings
                .into_iter()
                .map(|(at, width, sill, head)| {
                    json!({
                        "at":at*k,"width":width*k,"sill":sill,"head":head,
                        "kind":if sill == 0.0 {"door"} else {"window"}
                    })
                })
                .collect();
            run(
                &mut session,
                json!({"type":"add_wall_openings","centreline":centreline,"openings":openings}),
            );
        }
        let walls = session
            .objects(&ObjectFilter {
                type_name: Some("Legacy wall".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(walls.len(), 10);
        for (name, expected) in [
            ("gross_volume", 54.34125),
            ("volume", 50.13075),
            ("gross_external_face_area", 162.0),
            ("gross_internal_face_area", 449.85),
            ("external_face_area", 150.96),
            ("internal_face_area", 412.11),
        ] {
            near(walls.iter().map(|r| r.facts[name]).sum(), expected);
            for wall in &walls {
                assert!(
                    wall.facts[name] >= 0.0,
                    "driver {}: {name}",
                    wall.instance.id
                );
            }
        }
        let citation = json!({"source":"Invented villa test method; not a published standard","clause":"Joined geometry"});
        let items = [
            ("masonry", "volume", "m3", 50.13),
            ("plaster", "internal_face_area", "m2", 412.11),
            ("render", "external_face_area", "m2", 150.96),
        ];
        let rules: Vec<_> = items.iter().map(|(id,fact,unit,_)| json!({
            "id":id,"element_type":"Legacy wall","kind":if *unit == "m3" {"volume"} else {"area"},
            "basis":"geometric","unit":unit,"citation":citation,
            "terms":[{"label":id,"operation":"add","factors":[format!("geometry.{fact}")],"citation":citation}]
        })).collect();
        run(
            &mut session,
            json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
                "version":1,"name":"Villa test","title":"Villa test","based_on":"Synthetic geometry",
                "source_note":"Invented regression only","rules":rules
            }}),
        );
        let mappings: Vec<_> = items.iter().map(|(id,_,unit,_)| json!({
            "rule":id,"code":id,"description_en":id,"description_ar":id,"unit":unit,"section":"Villa test","trade":id,"citation":citation
        })).collect();
        run(
            &mut session,
            json!({"type":"measurement","kind":"set_mappings","mappings":mappings}),
        );
        let report = session.measurement_report().unwrap();
        assert_eq!(report.items.len(), 3);
        for (id, _, unit, expected) in items {
            let item = report.items.iter().find(|item| item.code == id).unwrap();
            assert_eq!(item.unit, unit);
            near(item.quantity, expected);
        }
    }
}

#[test]
fn explicit_assembly_integrates_vertical_spans_and_clips_openings_to_host_height() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("spans.dxf"), 6);
    run(
        &mut session,
        json!({"type":"define_object_type","definition":{
            "name":"Band","category":"Test geometry","geometry":"band","join":"union",
            "properties":{
                "width":{"kind":"number","unit":"length","default":0.2},
                "rise":{"kind":"number","unit":"length","default":3},
                "lift":{"kind":"number","unit":"length","default":0}
            },
            "vertical":{"base":"lift","height":"rise"},"plan":{"band_faces":"width"},
            "quantities":{"outside":"geometry.external_face_area","inside":"internal_face_area",
                "gross_outside":"gross_external_face_area","gross_inside":"geometry.gross_internal_face_area"}
        }}),
    );
    let placed = run(
        &mut session,
        json!({"type":"place_objects","objects":[
            {"type":"Band","assembly":"spans","points":[[0,0],[3,0]]},
            {"type":"Band","assembly":"spans","points":[[3,0],[6,0]],"props":{"lift":1,"rise":2}}
        ]}),
    );
    let host = placed["handles"][0].as_u64().unwrap();
    run(
        &mut session,
        json!({"type":"define_object_type","definition":{
            "name":"Cut","category":"Test geometry","geometry":"hosted",
            "vertical":{"base":"-1","height":"3"},"host_cut":{"at":"1.5","width":"1"}
        }}),
    );
    run(
        &mut session,
        json!({"type":"place_objects","objects":[{"type":"Cut","host":host,"points":[[1.5,0]]}]}),
    );
    let records = session
        .objects(&ObjectFilter {
            type_name: Some("Band".into()),
            ..Default::default()
        })
        .unwrap();
    for (record, gross, net) in [(&records[0], 18.8, 14.8), (&records[1], 12.4, 12.4)] {
        near(record.quantities["gross_outside"], gross);
        near(record.quantities["outside"], net);
        near(record.quantities["inside"], 0.);
        near(record.quantities["gross_inside"], 0.);
    }
}

#[test]
fn two_hundred_joined_runs_derive_faces_once_for_all_driver_quantities() {
    use qiyas_core::fixtures::profile;

    let dir = tempfile::tempdir().unwrap();
    let mut source = Session::new_blank(&dir.path().join("grid.dxf"), 6);
    // 10 by 11 rooms, 3 m between centrelines. Split each partition at junctions:
    // 100 horizontal runs + 99 vertical runs + one closed outer run = 200 drivers.
    let mut runs = vec![json!({"points":[[0,0],[30,0],[30,33],[0,33]],"closed":true})];
    for row in 1..11 {
        for column in 0..10 {
            runs.push(json!({"points":[[column*3,row*3],[(column+1)*3,row*3]]}));
        }
    }
    for column in 1..10 {
        for row in 0..11 {
            runs.push(json!({"points":[[column*3,row*3],[column*3,(row+1)*3]]}));
        }
    }
    assert_eq!(runs.len(), 200);
    run(
        &mut source,
        json!({"type":"walls","thickness":0.2,"height":3,"runs":runs}),
    );
    // Read a cold session so graphics/command evaluation cannot prefill the tested cache.
    let mut session = Session::new_blank(&dir.path().join("cold-grid.dxf"), 6);
    session.drawing.doc = source.drawing.doc.clone();
    profile::begin();
    let records = session.objects(&ObjectFilter::default()).unwrap();
    assert_eq!(records.len(), 200);
    for record in &records {
        let single = session
            .object_quantity(record.instance.id)
            .unwrap()
            .unwrap();
        assert_eq!(single.facts, record.facts);
        assert_eq!(single.quantities, record.quantities);
    }
    let calls = profile::finish();
    assert_eq!(calls["object_bands.derive_faces"][0], 1);
    // Exercise every prefix rather than merely repeating a cached driver read.
    assert_eq!(calls["object_bands.derive_band"][0], 200);
    let external = 2.0 * (30.2 + 33.2) * 3.0;
    let internal = 110.0 * (4.0 * 2.8) * 3.0;
    for (name, expected) in [
        ("gross_external_face_area", external),
        ("external_face_area", external),
        ("gross_internal_face_area", internal),
        ("internal_face_area", internal),
        ("volume", (30.2 * 33.2 - 110.0 * 2.8 * 2.8) * 3.0),
    ] {
        near(records.iter().map(|r| r.facts[name]).sum(), expected);
        assert!(records.iter().all(|r| r.facts[name] >= 0.0));
    }
}

#[test]
fn cached_faces_follow_opening_edits_undo_redo_and_failed_batches() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("face-cache.dxf"), 6);
    let walls = run(
        &mut session,
        json!({"type":"walls","thickness":0.2,"height":3,
            "runs":[{"points":[[0,0],[4,0],[4,4],[0,4]],"closed":true}],
            "openings":[{"run":0,"at":2,"width":1,"kind":"door","head":2}]
        }),
    );
    let driver = walls["centrelines"][0].as_u64().unwrap();
    let opening = walls["inserts"][0].as_u64().unwrap();
    let check = |s: &Session, width: f64| {
        let record = s.object_quantity(driver).unwrap().unwrap();
        near(record.facts["external_face_area"], 16.8 * 3.0 - width * 2.0);
        near(record.facts["internal_face_area"], 15.2 * 3.0 - width * 2.0);
    };
    check(&session, 1.0);
    run(
        &mut session,
        json!({"type":"edit_wall_opening","insert":opening,"width":1.2}),
    );
    check(&session, 1.2);
    assert!(session.undo());
    check(&session, 1.0);
    assert!(session.redo());
    check(&session, 1.2);
    qiyas_core::fixtures::profile::begin();
    let error = session.run_input(&json!({"type":"batch","commands":[
        {"type":"edit_wall_opening","insert":opening,"width":1.4},
        {"type":"object_schedule","at":[0,10],"columns":["external_face_area"],"ids":[driver]},
        {"type":"edit_wall_opening","insert":opening,"width":99}
    ]}),Source::User).unwrap_err().to_string();
    assert!(error.contains("does not fit"), "{error}");
    assert!(qiyas_core::fixtures::profile::finish()["object_bands.derive_faces"][0] > 0);
    qiyas_core::fixtures::profile::begin();
    check(&session, 1.2);
    assert_eq!(
        qiyas_core::fixtures::profile::finish()["object_bands.derive_faces"][0],
        1
    );
}
