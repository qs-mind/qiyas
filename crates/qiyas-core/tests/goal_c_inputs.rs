use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use std::path::Path;

fn run(s: &mut Session, input: Value) -> Value {
    serde_json::to_value(s.run_input(&input, Source::User).unwrap()).unwrap()
}
fn read(s: &Session, query: Value) -> Result<Value, qiyas_core::CoreError> {
    s.records(&serde_json::from_value(query).unwrap())
        .map(|v| json!(v))
}

#[test]
fn projections_membership_and_missing_properties_are_explicit() {
    let mut s = Session::new_blank(Path::new("records.dxf"), 6);
    let h = run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
    )["handles"][0]
        .clone();
    run(
        &mut s,
        json!({"type":"draw","kind":"circle","centre":[10,0],"radius":2}),
    );
    let q = json!({"fields":["layer","length","/length","/common/layer","handle"],"where":[{"path":"type","op":"in","value":["line"]}]});
    let r = read(&s, q).unwrap();
    assert_eq!(r["total"], 1);
    assert_eq!(r["records"][0]["fields"]["length"], 5.0);
    assert_eq!(r["records"][0]["fields"]["/length"], 5.0);
    assert_eq!(r["records"][0]["fields"]["layer"], "0");
    assert_eq!(r["records"][0]["fields"]["handle"], h);
    assert_eq!(
        read(
            &s,
            json!({"where":[{"path":"type","op":"not_in","value":["line"]}]})
        )
        .unwrap()["total"],
        1
    );
    for q in [
        json!({"fields":["common/layer"]}),
        json!({"fields":["typo"]}),
        json!({"where":[{"path":"layer","op":"in","value":"0"}]}),
        json!({"where":[{"path":"length","op":"gt","value":"5"}]}),
        json!({"where":[{"path":"wrong","value":3}]}),
    ] {
        assert!(read(&s, q.clone()).is_err(), "{q}");
    }
    let r = read(&s, json!({"fields":["/radius"]})).unwrap();
    assert!(r["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains("omitted")));
    assert!(r["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["fields"].get("/radius").is_none()));
    assert_eq!(
        read(
            &s,
            json!({"where":[{"path":"/radius","op":"exists","value":false}]})
        )
        .unwrap()["total"],
        1
    );
}

#[test]
fn layer_shorthand_is_atomic_and_handles_alias_uses_the_shared_boundary() {
    let mut s = Session::new_blank(Path::new("short.dxf"), 6);
    run(&mut s, json!({"type":"new_layer","name":"Notes"}));
    let history = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"set_layer","name":"Notes","color":"red","lineweight":50,"plot":false}),
    );
    assert_eq!(s.history_steps().0.len(), history + 1);
    let r = read(&s, json!({"collection":"layers","names":["Notes"]})).unwrap();
    assert_eq!(r["records"][0]["color"], json!({"Index":1}));
    assert_eq!(r["records"][0]["plot"], false);
    for q in [
        json!({"type":"set_layer","name":"Notes","color":"green","lineweight":999}),
        json!({"type":"set_layer","name":"Notes","prop":"on","value":false,"color":"green"}),
    ] {
        assert!(s.run_input(&q, Source::User).is_err());
        assert_eq!(
            read(&s, json!({"collection":"layers","names":["Notes"]})).unwrap(),
            r
        );
    }
    assert!(s.undo());
    assert_eq!(
        read(&s, json!({"collection":"layers","names":["Notes"]})).unwrap()["records"][0]["color"],
        json!({"Index":7})
    );
    let h = run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
    )["handles"][0]
        .clone();
    run(
        &mut s,
        json!({"type":"delete_entities","handles":[h.as_u64().unwrap().to_string()]}),
    );
    assert_eq!(read(&s, json!({"handles":[h]})).unwrap()["total"], 0);
}

#[test]
fn every_scalar_enum_choice_is_reported() {
    let report = qiyas_core::validation::validate_command(
        &json!({"type":"records","query":{"collection":"wrong"}}),
        None,
    );
    let message = report.message();
    for value in [
        "entities",
        "blocks",
        "takeoff",
        "boq",
        "knowledge_reports",
        "layers",
    ] {
        assert!(message.contains(value), "{message}");
    }
    let report = qiyas_core::validation::validate_command(
        &json!({"type":"records","query":{"where":[{"path":"layer","op":"wrong"}]}}),
        None,
    );
    for value in [
        "eq", "ne", "lt", "le", "gt", "ge", "in", "not_in", "contains", "exists",
    ] {
        assert!(
            report.message().contains(&format!("\"{value}\"")),
            "{}",
            report.message()
        );
    }
}

#[test]
fn codec_enums_are_complete_and_dimension_paths_match_records() {
    let schema = qiyas_core::record_edit::record_schema("Text").unwrap();
    for value in ["Baseline", "Bottom", "Middle", "Top"] {
        assert!(
            schema["types"]["TextVerticalAlignment"]
                .as_str()
                .unwrap()
                .contains(value),
            "{schema}"
        );
    }
    for value in ["Left", "Center", "Right", "Aligned", "Middle", "Fit"] {
        assert!(
            schema["types"]["TextHorizontalAlignment"]
                .as_str()
                .unwrap()
                .contains(value),
            "{schema}"
        );
    }
    let dimension = qiyas_core::record_edit::record_schema("Dimension").unwrap();
    assert!(
        dimension["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["path"] == "/Linear/base/text_middle_point"),
        "{dimension}"
    );
    let mut s = Session::new_blank(Path::new("text.dxf"), 6);
    let h = run(&mut s, json!({"type":"text","at":[0,0],"value":"Centred"}))["handles"][0].clone();
    run(
        &mut s,
        json!({"type":"set_records","collection":"entities","updates":[{"key":h,"path":"/vertical_alignment","value":"Middle"}]}),
    );
    assert_eq!(
        read(&s, json!({"handles":[h],"fields":["/vertical_alignment"]})).unwrap()["records"][0]
            ["fields"]["/vertical_alignment"],
        "Middle"
    );
}
