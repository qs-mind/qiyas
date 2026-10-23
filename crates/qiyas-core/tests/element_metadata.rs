use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use std::path::Path;

fn add(s: &mut Session, input: Value) -> u64 {
    serde_json::to_value(s.run_input(&input, Source::User).unwrap()).unwrap()["handles"][0]
        .as_u64()
        .unwrap()
}

#[test]
fn metadata_uses_area_centroid_and_unique_verbatim_label_in_drawing_units() {
    let mut s = Session::new_blank(Path::new("unused.dxf"), 4);
    let h = add(
        &mut s,
        json!({"type":"draw","kind":"polyline","closed":true,
        "points":[[1000,2000],[5000,2000],[5000,3000],[2000,3000],[2000,6000],[1000,6000]]}),
    );
    add(
        &mut s,
        json!({"type":"text","at":[1200,2200],"height":100,"value":"01 Living room"}),
    );
    // This is inside the bounding box but outside the concave enclosure.
    add(
        &mut s,
        json!({"type":"text","at":[4000,5000],"height":100,"value":"Unrelated note"}),
    );
    let metadata = s.drawing.element_metadata(h).unwrap();
    assert!((metadata.position[0] - (1000.0 + 9500.0 / 7.0)).abs() < 1e-8);
    assert!((metadata.position[1] - (2000.0 + 9500.0 / 7.0)).abs() < 1e-8);
    assert_eq!(metadata.label.as_deref(), Some("01 Living room"));
    add(
        &mut s,
        json!({"type":"text","at":[1300,2300],"height":100,"value":"Second enclosed note"}),
    );
    assert!(s.drawing.element_metadata(h).unwrap().label.is_none());
}

#[test]
fn metadata_uses_insert_origin_and_owned_label_not_symbol_bounds() {
    let mut s = Session::new_blank(Path::new("unused.dxf"), 6);
    let line = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[10,10]}),
    );
    s.run_input(
        &json!({"type":"create_block","name":"Symbol","base":[0,0],
        "targets":[{"handle":line}],"convert_to_insert":false}),
        Source::User,
    )
    .unwrap();
    let insert = add(
        &mut s,
        json!({"type":"insert_block","name":"Symbol","at":[20,30]}),
    );
    assert_eq!(
        s.drawing.element_metadata(insert).unwrap().position,
        [20.0, 30.0]
    );
    let path = add(
        &mut s,
        json!({"type":"draw","kind":"polyline","points":[[0,0],[6,0],[6,2]]}),
    );
    assert_eq!(
        s.drawing.element_metadata(path).unwrap().position,
        [4.0, 0.0]
    );
    s.run_input(
        &json!({"type":"define_object_type","definition":{"name":"Named","category":"test",
        "geometry":"point","properties":{"name":{"kind":"text","default":"Named element"}},
        "plan":{"label_template":"{name}"}}}),
        Source::User,
    )
    .unwrap();
    let outcome = s
        .run_input(
            &json!({"type":"place_objects","objects":[{"type":"Named","points":[[40,50]]}]}),
            Source::User,
        )
        .unwrap();
    let object = serde_json::to_value(outcome).unwrap()["handles"][0]
        .as_u64()
        .unwrap();
    let metadata = s.drawing.element_metadata(object).unwrap();
    assert_eq!(metadata.position, [40.0, 50.0]);
    assert_eq!(metadata.label.as_deref(), Some("Named element"));
}

#[test]
fn pdf_labels_use_nearest_enclosed_text_but_equal_distances_stay_ambiguous() {
    let mut s = Session::new_blank(Path::new("unused.dxf"), 6);
    let h = add(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[10,10]}),
    );
    let app = qiyas_core::xdata::register(&mut s.drawing.doc, qiyas_core::recognition::SOURCE_APP);
    let entity = s
        .drawing
        .doc
        .get_entity_mut(opencadcodec::Handle::from(h))
        .unwrap();
    qiyas_core::xdata::write_text(
        entity.common_mut(),
        qiyas_core::recognition::SOURCE_APP,
        app,
        Some("page 1"),
    );
    add(
        &mut s,
        json!({"type":"text","at":[4,5],"height":0.1,"value":"Nearest room"}),
    );
    add(
        &mut s,
        json!({"type":"text","at":[1,1],"height":0.1,"value":"Remote note"}),
    );
    add(
        &mut s,
        json!({"type":"text","at":[11,5],"height":0.1,"value":"Outside"}),
    );
    assert_eq!(
        s.drawing.element_metadata(h).unwrap().label.as_deref(),
        Some("Nearest room")
    );
    add(
        &mut s,
        json!({"type":"text","at":[6,5],"height":0.1,"value":"Equally near"}),
    );
    assert!(s.drawing.element_metadata(h).unwrap().label.is_none());
}
