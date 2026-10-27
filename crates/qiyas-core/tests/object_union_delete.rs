use opencadcodec::{EntityType, Handle};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, command: Value) -> Value {
    serde_json::to_value(session.run_input(&command, Source::User).unwrap().outcome).unwrap()
}

fn field_text(session: &Session, handle: u64) -> String {
    let EntityType::Text(text) = session.drawing.doc.get_entity(Handle::new(handle)).unwrap()
    else {
        panic!("Expected text field")
    };
    text.value.clone()
}

#[test]
fn deleting_union_owner_preserves_shared_material_references_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("union.dxf"), 6);
    run(
        &mut session,
        json!({"type":"define_object_type","definition":{
            "name":"Run","category":"Linear works","geometry":"band","join":"union",
            "properties":{"width":{"kind":"number","unit":"length","default":0.2}},
            "plan":{"band_faces":"width","hatch":true,"centreline":true},
            "quantities":{"material":"geometry.net_area"}
        }}),
    );
    let drivers = run(
        &mut session,
        json!({"type":"place_objects","objects":[
            {"type":"Run","points":[[0,0],[4,0]]},
            {"type":"Run","points":[[4,0],[8,0]]},
            {"type":"Run","points":[[8,0],[12,0]]}
        ]}),
    )["handles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h.as_u64().unwrap())
        .collect::<Vec<_>>();
    let hatch = session
        .drawing
        .doc
        .entities()
        .find_map(|entity| match entity {
            EntityType::Hatch(hatch) => Some(hatch.common.handle.value()),
            _ => None,
        })
        .unwrap();
    run(
        &mut session,
        json!({"type":"measure_entity","handle":hatch}),
    );
    let field = run(
        &mut session,
        json!({"type":"field","at":[0,2],"field":{"kind":"area","target":{"handle":hatch}}}),
    )["handles"][0]
        .as_u64()
        .unwrap();
    assert_eq!(field_text(&session, field), "2.40 m²");
    let steps = session.history_steps().0.len();
    run(
        &mut session,
        json!({"type":"delete_entities","targets":[{"handle":drivers[0]},{"handle":drivers[1]}]}),
    );
    assert_eq!(session.history_steps().0.len(), steps + 1);
    assert_eq!(field_text(&session, field), "0.80 m²");
    assert!((session.takeoff.items[0].shown - 0.8).abs() < 1e-8);
    assert_eq!(session.takeoff.items[0].handles, [hatch]);
    assert!(matches!(
        session.drawing.doc.get_entity(Handle::new(hatch)),
        Some(EntityType::Hatch(_))
    ));
    assert!(session.undo());
    assert_eq!(field_text(&session, field), "2.40 m²");
    assert!((session.takeoff.items[0].shown - 2.4).abs() < 1e-8);
    assert!(session.redo());
    assert_eq!(field_text(&session, field), "0.80 m²");
    for extension in ["dwg", "dxf"] {
        let path = dir.path().join(format!("remaining.{extension}"));
        session.save_as(&path).unwrap();
        let mut reopened = Session::open(&path).unwrap();
        run(
            &mut reopened,
            json!({"type":"set_object_props","targets":[drivers[2]],"props":{"width":0.4}}),
        );
        assert_eq!(field_text(&reopened, field), "1.60 m²");
        assert!((reopened.takeoff.items[0].shown - 1.6).abs() < 1e-8);
        assert_eq!(reopened.takeoff.items[0].handles, [hatch]);
    }
}
