//! A single-object read must preserve the complete engine's exact union allocation.
use qiyas_core::{objects::ObjectFilter, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Value {
    json!(s.run_input(&command, Source::User).unwrap())
}

#[test]
fn narrow_quantities_equal_full_records_through_edits_undo_redo_and_rollback() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("quantities.dxf"), unit);
        let k = 1.0 / s.drawing.units.to_metres();
        let walls = run(
            &mut s,
            json!({"type":"walls", "thickness":0.2*k,"runs":[
            {"points":[[0,0],[6.0*k,0]]},
            {"points":[[3.0*k,0],[3.0*k,4.0*k]],"height":2.0}
        ],"openings":[{"run":0,"at":1.0*k,"width":0.8*k,"kind":"door"}]}),
        );
        let door = walls["inserts"][0].as_u64().unwrap();
        let compare = |s: &Session| {
            let all = s.objects(&ObjectFilter::default()).unwrap();
            assert!(!all.is_empty());
            for row in &all {
                let mut cold = Session::new_blank(&dir.path().join("narrow.dxf"), unit);
                cold.drawing.doc = s.drawing.doc.clone();
                assert_eq!(
                    json!(cold.object_quantity(row.instance.id).unwrap().unwrap()),
                    json!(row)
                );
                assert_eq!(
                    json!(s.object_quantity(row.instance.id).unwrap().unwrap()),
                    json!(row)
                );
                assert_eq!(
                    json!(s.objects_for(&[row.instance.id, row.instance.id]).unwrap()),
                    json!([row])
                );
            }
            assert!(s.objects_for(&[]).unwrap().is_empty());
            assert!(s.object_quantity(u64::MAX).unwrap().is_none());
            json!(all)
        };
        let before = compare(&s);
        run(
            &mut s,
            json!({"type":"edit_wall_opening","insert":door,"width":1.0*k}),
        );
        let after = compare(&s);
        assert_ne!(before, after);
        assert!(s.undo());
        assert_eq!(compare(&s), before);
        assert!(s.redo());
        assert_eq!(compare(&s), after);
        let history = json!(s.history_steps());
        let structure = s.drawing.doc.snapshot_structure();
        assert!(s
            .run_input(
                &json!({"type":"batch","commands":[
                    {"type":"edit_wall_opening","insert":door,"width":0.7*k},
                    {"type":"edit_wall_opening","insert":door,"width":99.0*k}
                ]}),
                Source::User
            )
            .is_err());
        assert_eq!(compare(&s), after);
        assert_eq!(json!(s.history_steps()), history);
        assert_eq!(s.drawing.doc.snapshot_structure(), structure);
    }
}

#[test]
fn a_narrow_read_does_not_evaluate_an_unrelated_quantity_and_detects_unrecorded_writes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("direct-writes.dxf"), 6);
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{
            "name":"Measured path","category":"Test geometry","geometry":"path",
            "properties":{},"quantities":{"ratio":"1 / (length - 2)"}
        }}),
    );
    let placed = run(
        &mut s,
        json!({"type":"place_objects","objects":[
            {"type":"Measured path","points":[[0,0],[4,0]]},
            {"type":"Measured path","points":[[0,5],[3,5]]}
        ]}),
    );
    let a = placed["handles"][0].as_u64().unwrap();
    let b = placed["handles"][1].as_u64().unwrap();
    assert_eq!(
        s.object_quantity(a).unwrap().unwrap().quantities["ratio"],
        0.5
    );
    assert_eq!(
        s.object_quantity(b).unwrap().unwrap().quantities["ratio"],
        1.0
    );
    let revision = s.document_revision();
    let Some(opencadcodec::EntityType::LwPolyline(path)) =
        s.drawing.doc.get_entity_mut(opencadcodec::Handle::new(b))
    else {
        panic!("path driver");
    };
    path.vertices[1].location.x = 2.0;
    assert_eq!(
        s.document_revision(),
        revision,
        "the cache cannot rely on this revision alone"
    );
    assert_eq!(
        s.object_quantity(a).unwrap().unwrap().quantities["ratio"],
        0.5
    );
    assert!(s.object_quantity(b).is_err());
    assert!(s.objects(&ObjectFilter::default()).is_err());
    for path in ["id", "/id", "key", "/key", "handle"] {
        let value = if path.ends_with("id") {
            json!(a)
        } else {
            json!(a.to_string())
        };
        let query = qiyas_core::records::RecordQuery {
            collection: qiyas_core::records::Collection::Objects,
            conditions: vec![qiyas_core::records::Where {
                path: path.into(),
                value,
                op: Default::default(),
            }],
            ..Default::default()
        };
        let page = s.records(&query).unwrap();
        assert_eq!(page.total, 1, "narrow selector {path}");
    }
    let Some(opencadcodec::EntityType::LwPolyline(path)) =
        s.drawing.doc.get_entity_mut(opencadcodec::Handle::new(b))
    else {
        panic!("path driver");
    };
    path.vertices[1].location.x = 3.0;
    assert_eq!(
        s.object_quantity(b).unwrap().unwrap().quantities["ratio"],
        1.0
    );
    let mut definition = qiyas_core::knowledge::get(
        &s.drawing.doc,
        qiyas_core::knowledge::Kind::Types,
        "Measured path",
    )
    .unwrap()
    .unwrap()
    .data;
    definition["quantities"]["ratio"] = json!("2 / (length - 2)");
    qiyas_core::knowledge::put(
        &mut s.drawing.doc,
        qiyas_core::knowledge::Kind::Types,
        "Measured path",
        &definition,
    )
    .unwrap();
    assert_eq!(s.document_revision(), revision);
    assert_eq!(
        s.object_quantity(a).unwrap().unwrap().quantities["ratio"],
        1.0
    );
    for unit in [
        qiyas_core::Units::Millimetres,
        qiyas_core::Units::Centimetres,
        qiyas_core::Units::Metres,
        qiyas_core::Units::Inches,
    ] {
        s.drawing.units = unit;
        assert_eq!(
            s.object_quantity(a).unwrap().unwrap().quantities["ratio"],
            2.0 / (4.0 * unit.to_metres() - 2.0)
        );
    }
}
