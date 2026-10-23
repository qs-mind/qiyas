//! Analytic enclosure controls: physical wall faces, explicit source layers and persisted validity.
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) -> Outcome {
    s.run_input(&v, Source::User).unwrap().outcome
}
fn area(s: &mut Session, point: [f64; 2]) -> f64 {
    let Outcome::Rooms { report } = run(
        s,
        json!({"type":"rooms","query":{"seeds":[{"point":point}]}}),
    ) else {
        panic!()
    };
    report.rooms[0].net_area_m2
}
#[test]
fn wall_faces_ignore_doors_windows_grid_and_annotation_in_all_units() {
    for unit in [4, 5, 6, 1] {
        for ext in ["dwg", "dxf"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("room.{ext}"));
            let mut s = Session::new_blank(&path, unit);
            let u = 1.0 / s.drawing.units.to_metres();
            run(
                &mut s,
                json!({"type":"walls","thickness":0.2*u,"runs":[{"points":[[0,0],[5.2*u,0],[5.2*u,4.2*u],[0,4.2*u]],"closed":true}],"openings":[{"run":0,"at":2.0*u,"width":0.9*u,"kind":"door"},{"run":0,"at":7.2*u,"width":1.2*u,"kind":"window"}]}),
            );
            let p = [u, u];
            assert_eq!(area(&mut s, p), 20.0);
            run(
                &mut s,
                json!({"type":"grid","x":[{"label":"A","at":2.6*u}],"extent":{"min":[-u,-u],"max":[6.2*u,5.2*u]}}),
            );
            let Outcome::Added { handles } = run(
                &mut s,
                json!({"type":"text","at":[2.0*u,2.0*u],"height":0.1,"value":"D1"}),
            ) else {
                panic!()
            };
            let tag = handles[0];
            run(
                &mut s,
                json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[5.2*u,0],"line_point":[2.0*u,2.0*u]}),
            );
            assert_eq!(area(&mut s, p), 20.0);
            let Outcome::Rooms { report } = run(
                &mut s,
                json!({"type":"label_spaces","query":{"seeds":[{"point":p,"name":"Room"}]},"language":"ar"}),
            ) else {
                panic!()
            };
            let label = report.rooms[0].label_handle.unwrap();
            assert_eq!(
                // Wrapping keeps the value and unit together with a non-breaking space.
                s.entity_info(label, &[])
                    .unwrap()
                    .text
                    .unwrap()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                "المساحة ٢٠٫٠٠ م²"
            );
            run(
                &mut s,
                json!({"type":"measure_room","label":{"handle":label}}),
            );
            run(
                &mut s,
                json!({"type":"set_property","target":{"handle":tag},"prop":"text_height","value":0.8*u}),
            );
            assert!((s.takeoff.items[0].shown - 20.0).abs() < 1e-8);
            assert_eq!(area(&mut s, p), 20.0);
            let door = s.wall_groups(None).unwrap()[0].runs[0].openings[0].insert;
            run(
                &mut s,
                json!({"type":"edit_wall_opening","insert":door,"at":2.2*u}),
            );
            assert!((s.takeoff.items[0].shown - 20.0).abs() < 1e-8);
            assert!(s.undo());
            assert!((s.takeoff.items[0].shown - 20.0).abs() < 1e-8);
            s.save_as_headless(&path).unwrap();
            let mut reopened = Session::open(&path).unwrap();
            assert_eq!(area(&mut reopened, p), 20.0);
            assert!((reopened.takeoff.items[0].shown - 20.0).abs() < 1e-8);
        }
    }
}
#[test]
fn l_shape_and_layer_policy_do_not_use_grid_or_text_as_boundaries() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("l.dxf"), unit);
        let u = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"draw","kind":"polyline","points":[[0,0],[6.0*u,0],[6.0*u,2.0*u],[4.0*u,2.0*u],[4.0*u,4.0*u],[0,4.0*u]],"closed":true}),
        );
        run(
            &mut s,
            json!({"type":"grid","y":[{"label":"A","at":u}],"extent":{"min":[-u,-u],"max":[7.0*u,5.0*u]}}),
        );
        assert_eq!(area(&mut s, [u, 0.5 * u]), 20.0);
        // The user's hatch still sees the crossing grid.
        let qiyas_core::Boundary::Ring { ring, .. } = s.boundary_at([u, 0.5 * u], u) else {
            panic!()
        };
        assert!(qiyas_geom::walls::ring_area(&ring) * s.drawing.units.to_metres().powi(2) < 20.0);
    }
}
#[test]
fn explicit_exclusions_survive_retrace_save_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policy.dwg");
    let mut s = Session::new_blank(&path, 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    run(&mut s, json!({"type":"new_layer","name":"Reference"}));
    run(
        &mut s,
        json!({"type":"set_current","current":{"layer":"Reference"}}),
    );
    run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,2],"b":[5,2]}),
    );
    let Outcome::Rooms { report } = run(
        &mut s,
        json!({"type":"label_spaces","query":{"seeds":[{"point":[1,1]}],"exclude_layers":["Reference"]}}),
    ) else {
        panic!()
    };
    assert_eq!(report.rooms[0].net_area_m2, 20.0);
    run(
        &mut s,
        json!({"type":"measure_room","label":{"handle":report.rooms[0].label_handle.unwrap()}}),
    );
    s.save_as_headless(&path).unwrap();
    let reopened = Session::open(&path).unwrap();
    assert_eq!(reopened.takeoff.items[0].shown, 20.0);
}

#[test]
fn room_reader_skips_old_labels_and_grid_letters_even_inside_an_enclosure() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("names.dxf"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    run(
        &mut s,
        json!({"type":"grid","x":[{"label":"A","at":2.5}],"extent":{"min":[1,1],"max":[4,3]},"bubble":0.1}),
    );
    for (i, text) in [
        "Room",
        "Unapproved candidate 20.00 m²",
        "مساحة مرشحة غير معتمدة 20.00 م²",
    ]
    .iter()
    .enumerate()
    {
        run(
            &mut s,
            json!({"type":"text","at":[0.5,1.0+i as f64*0.5],"height":0.1,"value":text}),
        );
    }
    let Outcome::Rooms { report } = run(&mut s, json!({"type":"rooms","query":{}})) else {
        panic!()
    };
    assert_eq!(report.rooms.len(), 1);
    assert_eq!(report.rooms[0].name.as_deref(), Some("Room"));
    assert_eq!(report.rooms[0].net_area_m2, 20.0);
    assert!(report.rooms[0].also_named.is_empty());
}

#[test]
fn older_grids_recover_axes_and_letters_from_bubble_provenance_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old-grid.dxf");
    let mut s = Session::new_blank(&path, 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    let Outcome::Added { handles } = run(
        &mut s,
        json!({"type":"grid","y":[{"label":"A","at":2}],"extent":{"min":[-1,-1],"max":[6,5]}}),
    ) else {
        panic!()
    };
    for handle in handles {
        let entity = s
            .drawing
            .doc
            .get_entity_mut(opencadcodec::Handle::new(handle))
            .unwrap();
        if !matches!(entity, opencadcodec::EntityType::Circle(_)) {
            entity
                .common_mut()
                .extended_data
                .remove_record("QIYAS_GRID");
        }
    }
    s.save_as_headless(&path).unwrap();
    let mut reopened = Session::open(&path).unwrap();
    assert_eq!(area(&mut reopened, [1.0, 1.0]), 20.0);
}
