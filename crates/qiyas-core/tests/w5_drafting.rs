//! Job-owned limits and measured geometry, in every drawing unit.
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};
const UNITS: [(i16, f64); 4] = [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)];
fn run(s: &mut Session, value: Value) -> Outcome {
    s.run_input(&value, Source::Ai).unwrap().outcome
}
fn added(s: &mut Session, value: Value) -> Vec<u64> {
    match run(s, value) {
        Outcome::Added { handles } => handles,
        other => panic!("{other:?}"),
    }
}
fn snapshot(s: &Session) -> Value {
    let mut entities: Vec<_> = s.drawing.doc.entities().collect();
    entities.sort_by_key(|e| e.common().handle.value());
    json!({"entities":entities,"layers":s.drawing.doc.layers.iter().collect::<Vec<_>>(),
        "blocks":s.drawing.doc.block_records.iter().collect::<Vec<_>>(),"current":s.current})
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}
fn plan() -> Value {
    json!({"floors":[{"name":"Concourse","rooms":[
    {"name":"Bedroom shop","kind":"retail","area_m2":30,"width_m":3},
    {"name":"Wardrobe","area_m2":20}],"doors":[{"name":"Exit","width_m":1.2}],
    "stairs":[{"name":"East stair","riser_m":0.18,"going_m":0.28,"passage_beside_m":1.1}]}]})
}
fn check(s: &mut Session) -> Value {
    let Outcome::Measurement { result } = run(s, json!({"type":"space_check","plan":plan()}))
    else {
        panic!("measurement")
    };
    result
}
#[test]
fn removed_commands_and_design_data_are_not_shipped() {
    let index = qiyas_core::registry::registry().index_text();
    for name in ["symbol", "drafting_knowledge"] {
        assert!(!index.split_whitespace().any(|word| word == name));
        assert!(!qiyas_core::registry::registry()
            .commands()
            .iter()
            .any(|c| c.name == name));
    }
    for name in ["drafting-standards.json", "code-register.json"] {
        assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join(name)
            .exists());
    }
}
#[test]
fn no_limits_reports_facts_without_judgements_or_word_list_classification() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    let value = check(&mut s);
    assert!(value["findings"].as_array().unwrap().is_empty());
    let facts = value["measurements"].as_array().unwrap();
    assert!(facts
        .iter()
        .any(|f| f["metric"] == "area_m2" && f["measured"] == 30.0));
    assert!(facts
        .iter()
        .any(|f| f["metric"] == "riser_m" && f["measured"] == 0.18));
    assert!(facts
        .iter()
        .all(|f| f["passed"].is_null() && f["message"] == "No limit was given"));
    assert_eq!(value["floors"][0]["spaces"][0]["kind"], "retail");
    assert!(value["floors"][0]["spaces"][1]["kind"].is_null());
    assert!(value["floors"][0]["spaces"][1]
        .get("suggested_kind")
        .is_none());
}
#[test]
fn supplied_limits_and_decisions_are_compared_and_cited() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    run(
        &mut s,
        json!({"type":"decision","kind":"set","entries":{"name":"retail_area","value":40,"unit":"m2","reason":"Terminal programme","source":"document","citation":"Programme section 8","set_by":"mizan","status":"answered"}}),
    );
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"retail area","data":{
        "kind":"measurement_limit","target":"room","classification":"retail","metric":"area_m2","unit":"m2","minimum":{"decision":"retail_area"}}}),
    );
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"exit width","data":{
        "kind":"measurement_limit","target":"door","metric":"width_m","unit":"m","minimum":1.1,
        "citation":{"source":"Engineer brief","clause":"Exit width"}}}),
    );
    let value = check(&mut s);
    let facts = value["measurements"].as_array().unwrap();
    let area = facts.iter().find(|f| f["rule"] == "retail area").unwrap();
    assert_eq!(area["passed"], false);
    assert_eq!(area["minimum"], 40.0);
    assert!(area["citations"]
        .to_string()
        .contains("Programme section 8"));
    let width = facts.iter().find(|f| f["rule"] == "exit width").unwrap();
    assert_eq!(width["passed"], true);
    assert!(width["citations"].to_string().contains("Engineer brief"));
    assert_eq!(value["findings"].as_array().unwrap().len(), 1);
}
#[test]
fn drawing_checks_report_opening_width_and_apply_only_job_limits() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[20,0]]}],"thickness":0.3,"height":4,
        "openings":[{"run":0,"at":5,"width":1.2,"kind":"door","sill":0,"head":3}]}),
    );
    let scope = Default::default();
    let before = serde_json::to_value(s.check_drawing(&scope)).unwrap();
    assert!(before["measurements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["target"] == "door" && f["metric"] == "width_m" && f["passed"].is_null()));
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"door width","data":{
        "kind":"measurement_limit","target":"door","metric":"width_m","unit":"m","minimum":1.5,
        "citation":{"source":"Engineer brief","clause":"Exit width"}}}),
    );
    let after = serde_json::to_value(s.check_drawing(&scope)).unwrap();
    assert!(after["measurements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["rule"] == "door width"
            && f["passed"] == false
            && f["citations"].to_string().contains("Engineer brief")));
}
#[test]
fn concave_space_facts_use_polygons_without_retaining_or_mutating_rooms() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in UNITS {
        let mut s = Session::new_blank(&dir.path().join("facts.dxf"), units);
        let points: Vec<_> = [
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 1.0],
            [1.0, 1.0],
            [1.0, 4.0],
            [0.0, 4.0],
        ]
        .map(|p| [p[0] / k, p[1] / k])
        .into();
        added(
            &mut s,
            json!({"type":"draw","kind":"polyline","closed":true,"points":points}),
        );
        added(
            &mut s,
            json!({"type":"text","at":[0.5/k,2.0/k],"height":0.1/k,"value":"Space A"}),
        );
        let inside = added(
            &mut s,
            json!({"type":"draw","kind":"point","p":[0.5/k,3.0/k]}),
        )[0];
        let outside = added(
            &mut s,
            json!({"type":"draw","kind":"point","p":[2.0/k,2.0/k]}),
        )[0];
        let before = snapshot(&s);
        let Outcome::Measurement { result } = s
            .run_input(
                &json!({"type":"space_check","floors":[{"name":"Floor"}]}),
                Source::Ai,
            )
            .unwrap()
            .outcome
        else {
            panic!("measurement")
        };
        assert_eq!(snapshot(&s), before);
        let space = &result["floors"][0]["spaces"][0];
        near(space["area_m2"].as_f64().unwrap(), 7.0);
        near(space["perimeter_m"].as_f64().unwrap(), 16.0);
        assert!(space["clear_widths_m"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| (w["width_m"].as_f64().unwrap() - 1.0).abs() < 1e-8));
        let fixtures = space["fixtures"].as_array().unwrap();
        assert!(fixtures.iter().any(|f| f["handle"] == inside));
        assert!(!fixtures.iter().any(|f| f["handle"] == outside));
        assert!(result["findings"].as_array().unwrap().is_empty());
        run(
            &mut s,
            json!({"type":"set_knowledge","kind":"rules","name":"Terminal passage","data":{
            "kind":"measurement_limit","target":"room","metric":"width_m","unit":"m","minimum":1.5,
            "citation":{"source":"Terminal brief","clause":"Clear passage"}}}),
        );
        let Outcome::Measurement { result } = run(
            &mut s,
            json!({"type":"space_check","floors":[{"name":"Floor"}]}),
        ) else {
            panic!("measurement")
        };
        let row = result["measurements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rule"] == "Terminal passage")
            .unwrap();
        near(row["measured"].as_f64().unwrap(), 1.0);
        assert_eq!(row["passed"], false);
        assert!(row["citations"].to_string().contains("Terminal brief"));
    }
}

#[test]
fn classifications_are_drawing_owned_and_bad_or_missing_limit_evidence_never_passes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"classification","data":{
        "kind":"space_classification","floor":"Concourse","name":"Wardrobe","classification":"exhibition"}}),
    );
    assert_eq!(
        check(&mut s)["floors"][0]["spaces"][1]["kind"],
        "exhibition"
    );
    assert!(s
        .run_input(
            &json!({"type":"set_knowledge","kind":"rules","name":"uncited","data":{
        "kind":"measurement_limit","target":"room","metric":"width_m","unit":"m","minimum":2}}),
            Source::Ai
        )
        .is_err());
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"unanswered","data":{
        "kind":"measurement_limit","target":"room","metric":"width_m","unit":"m","minimum":{"decision":"missing"}}}),
    );
    let value = check(&mut s);
    let row = value["measurements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == "unanswered")
        .unwrap();
    assert!(row["passed"].is_null());
    assert_eq!(row["measured"], 3.0);
    assert!(row["message"]
        .as_str()
        .unwrap()
        .contains("Missing decision"));
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"unit mismatch","data":{
        "kind":"measurement_limit","target":"room","metric":"width_m","unit":"mm","minimum":2000,
        "citation":{"source":"Engineer brief","clause":"Units stated"}}}),
    );
    let value = check(&mut s);
    assert!(value["measurements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["rule"] == "unit mismatch")
        .all(|r| r["passed"].is_null()));
}

#[test]
fn drawing_size_labels_do_not_silently_allow_five_percent_difference() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    added(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[30.3,10]}),
    );
    added(
        &mut s,
        json!({"type":"text","at":[15,5],"value":"30 x 10","height":0.25}),
    );
    let report = s.check_drawing(&qiyas_core::check::CheckScope {
        rules: vec!["room_size_text_differs".into()],
        ..Default::default()
    });
    assert!(report
        .findings
        .iter()
        .any(|f| f.rule == "room_size_text_differs"));
}

#[test]
fn annotation_only_checks_do_not_evaluate_unrelated_job_limits() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("terminal.dxf"), 6);
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"rules","name":"unavailable stair","data":{
        "kind":"measurement_limit","target":"stair","metric":"width_m","unit":"m","minimum":2,
        "citation":{"source":"Engineer brief","clause":"Stair width"}}}),
    );
    let report = s.check_drawing(&qiyas_core::check::CheckScope {
        rules: vec!["text_height_vs_scale".into()],
        ..Default::default()
    });
    assert!(report.complete, "{report:?}");
    assert!(report.measurements.is_empty());
    let report = s.check_drawing(&qiyas_core::check::CheckScope {
        rules: vec!["job_limit".into()],
        ..Default::default()
    });
    assert!(!report.complete);
    assert!(report
        .measurements
        .iter()
        .any(|row| row["rule"] == "unavailable stair" && row["status"] == "unavailable"));
}
