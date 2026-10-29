//! One drawing value shared by decisions, properties, fields and quantity provenance.
use opencadcodec::{EntityType, Handle};
use qiyas_core::{knowledge, registry, Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, input: Value) -> Outcome {
    s.run_input(&input, Source::User).unwrap().outcome
}
fn added(s: &mut Session, input: Value) -> u64 {
    match run(s, input) {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}
fn decision(name: &str, value: Value) -> Value {
    json!({"name":name,"value":value,"unit":"m","reason":"Set from the brief / من الموجز",
        "source":"engineer","citation":"Brief / الموجز","set_by":"engineer","status":"answered"})
}
fn set(s: &mut Session, entry: Value) {
    run(s, json!({"type":"decision","kind":"set","entries":entry}));
}
fn text(s: &Session, h: u64) -> String {
    match s.drawing.doc.get_entity(Handle::new(h)).unwrap() {
        EntityType::Text(t) => t.value.clone(),
        _ => panic!("text"),
    }
}

#[test]
fn decisions_and_notes_follow_changes_undo_redo_and_dwg_dxf_roundtrips() {
    for extension in ["dwg", "dxf"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("decisions.{extension}"));
        let mut s = Session::new_blank(&path, 6);
        set(&mut s, decision("height", json!(3)));
        let h = added(
            &mut s,
            json!({"type":"field","at":[0,0],"height":0.2,"field":{"kind":"decision","name":"height"}}),
        );
        assert_eq!(text(&s, h), "3 m");
        set(&mut s, decision("height", json!(4)));
        assert_eq!(text(&s, h), "4 m");
        assert!(s.undo());
        assert_eq!(text(&s, h), "3 m");
        assert_eq!(s.decisions().unwrap()[0].value, json!(3));
        assert!(s.redo());
        assert_eq!(text(&s, h), "4 m");
        s.save_as_headless(&path).unwrap();
        let reopened = Session::open_embedded(&path).unwrap();
        assert_eq!(reopened.decisions().unwrap()[0].value, json!(4));
        assert_eq!(text(&reopened, h), "4 m");
    }
}

#[test]
fn bulk_decisions_validate_before_writing_and_questions_never_invent_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("questions.dxf"), 6);
    let mut asked = decision("programme", Value::Null);
    asked["status"] = json!("asked");
    asked["question"] = json!("What uses are required? / ما الاستخدامات المطلوبة؟");
    asked["unit"] = json!("");
    set(&mut s, json!([decision("height", json!(3)), asked]));
    let h = added(
        &mut s,
        json!({"type":"field","at":[0,0],"field":{"kind":"decision","name":"programme"}}),
    );
    assert_eq!(text(&s, h), "####");
    let before = s.decisions().unwrap();
    let mut bad = decision("width", json!(2));
    bad["reason"] = json!("");
    assert!(s
        .run_input(
            &json!({"type":"decision","kind":"set","entries":[decision("height",json!(7)),bad]}),
            Source::Ai
        )
        .is_err());
    assert_eq!(s.decisions().unwrap(), before);
    assert!(s.run_input(&json!({"type":"set_knowledge","kind":"decisions","name":"height","data":decision("height",json!(9))}),Source::Ai).is_err());
    let duplicate = json!([decision("height", json!(3)), decision("HEIGHT", json!(4))]);
    assert!(s
        .run_input(
            &json!({"type":"decision","kind":"set","entries":duplicate}),
            Source::Ai
        )
        .is_err());
}

#[test]
fn cited_properties_convert_units_follow_updates_and_refuse_contradictions() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("properties.dxf"), 6);
    let mut entry = decision("rise", json!(3000));
    entry["unit"] = json!("mm");
    set(&mut s, entry.clone());
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"TestRun","category":"test","geometry":"path",
        "properties":{"rise":{"kind":"number","unit":"length","default":1}},"quantities":{"face":"length * rise"}}}),
    );
    let h = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"TestRun","points":[[0,0],[2,0]],"props":{"rise":3},"sources":{"rise":{"decision":"rise"}}}]}),
    );
    let prop = |s: &Session| {
        s.objects(&Default::default())
            .unwrap()
            .into_iter()
            .find(|o| o.instance.id == h)
            .unwrap()
            .instance
            .props["rise"]
            .as_f64()
            .unwrap()
    };
    assert_eq!(prop(&s), 3.0);
    assert!(s.run_input(&json!({"type":"set_object_props","targets":[h],"props":{"rise":7},"sources":{"rise":{"decision":"rise"}}}),Source::Ai).is_err());
    entry["value"] = json!(4000);
    set(&mut s, entry);
    assert_eq!(prop(&s), 4.0);
    assert!(s.undo());
    assert_eq!(prop(&s), 3.0);
    assert_eq!(s.decisions().unwrap()[0].value, json!(3000));
}

#[test]
fn new_commands_have_registry_examples_correct_effects_and_bounded_lossless_reads() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("registry.dxf"), 6);
    for (name, effect) in [
        ("decision.set", registry::Effect::Edit),
        ("decision.list", registry::Effect::Read),
        ("outside_check", registry::Effect::Read),
    ] {
        let info = registry::registry().get(name).unwrap();
        assert_eq!(info.effect, effect);
        assert!(!info.examples.is_empty());
        for example in &info.examples {
            registry::parse_command(example.clone()).unwrap();
        }
    }
    set(
        &mut s,
        json!((0..45)
            .map(|i| decision(&format!("d{i:02}"), json!(i)))
            .collect::<Vec<_>>()),
    );
    let before = knowledge::list(&s.drawing.doc, knowledge::Kind::Decisions).unwrap();
    let mut offset = 0;
    let mut count = 0;
    loop {
        let Outcome::Measurement { result } = run(
            &mut s,
            json!({"type":"decision","kind":"list","offset":offset,"limit":3}),
        ) else {
            panic!("list")
        };
        count += result["entries"].as_array().unwrap().len();
        assert_eq!(result["total_entries"], 45);
        let Some(next) = result["next_offset"].as_u64() else {
            break;
        };
        assert!(next > offset);
        offset = next;
    }
    assert_eq!(count, 45);
    assert_eq!(
        knowledge::list(&s.drawing.doc, knowledge::Kind::Decisions).unwrap(),
        before
    );
}

#[test]
fn linked_boq_operands_and_live_quantity_follow_the_same_decision() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("boq.dxf"), 6);
    let (host, _) = crate::measurement_fixture::fixture(&mut s);
    set(&mut s, decision("rise", json!(2.8)));
    run(
        &mut s,
        json!({"type":"set_object_props","targets":[host],"props":{"rise":2.8},"sources":{"rise":{"kind":"engineer","decision":"rise"}}}),
    );
    crate::measurement_fixture::apply(&mut s, crate::measurement_fixture::method("test", false));
    let before = s.measurement_report().unwrap();
    assert!(before
        .items
        .iter()
        .flat_map(|i| &i.lines)
        .flat_map(|l| &l.operands)
        .flat_map(|o| &o.sources)
        .any(|source| source.source.decision.as_deref() == Some("rise")));
    set(&mut s, decision("rise", json!(3.8)));
    let after = s.measurement_report().unwrap();
    assert!((after.items[0].quantity - before.items[0].quantity - 12.6).abs() < 1e-7);
    assert!(s.undo());
    assert_eq!(
        s.measurement_report().unwrap().items[0].quantity,
        before.items[0].quantity
    );
}

#[test]
fn locked_linked_property_refuses_the_whole_decision_update() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("locked.dxf"), 6);
    set(&mut s, decision("rise", json!(3)));
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"P","category":"test","geometry":"point",
        "properties":{"rise":{"kind":"number","unit":"length","default":1}}}}),
    );
    let h = added(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"P","points":[[0,0]],"props":{"rise":3},"sources":{"rise":{"decision":"rise"}}}]}),
    );
    let layer = s
        .drawing
        .doc
        .get_entity(Handle::new(h))
        .unwrap()
        .common()
        .layer
        .clone();
    s.drawing.doc.layers.get_mut(&layer).unwrap().flags.locked = true;
    assert!(s
        .run_input(
            &json!({"type":"decision","kind":"set","entries":decision("rise",json!(4))}),
            Source::Ai
        )
        .is_err());
    assert_eq!(s.decisions().unwrap()[0].value, json!(3));
}

#[test]
fn decision_fields_respect_existing_units_and_arabic_digits() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("notes.dxf"), 6);
    set(&mut s, decision("rise", json!(3)));
    let english = added(
        &mut s,
        json!({"type":"text","at":[0,0],"value":r#"Height %<\AcVar QiyasDecision "rise">% m"#}),
    );
    let arabic = added(
        &mut s,
        json!({"type":"text","at":[0,1],"value":r#"الارتفاع %<\AcVar QiyasDecision "rise">% m"#}),
    );
    assert_eq!(text(&s, english), "Height 3 m");
    assert_eq!(text(&s, arabic), "الارتفاع ٣ m");
    set(&mut s, decision("rise", json!(4)));
    assert_eq!(text(&s, english), "Height 4 m");
    assert_eq!(text(&s, arabic), "الارتفاع ٤ m");
}
