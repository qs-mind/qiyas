//! Record payloads use their traced serde shapes before any command changes a drawing.

use qiyas_core::record_edit::record_input_schema;
use qiyas_core::validation::{validate_command, validate_schema};
use qiyas_core::{registry::parse_command, Session, Source, Units};
use serde_json::json;

#[test]
fn record_payload_schema_checks_nested_values_without_requiring_defaulted_fields() {
    let schema = record_input_schema("line").unwrap();
    assert!(validate_schema(&schema, &json!({})).valid);
    assert!(validate_schema(&schema, &json!({"start":{"x":4}})).valid);
    let report = validate_schema(
        &schema,
        &json!({
            "start":{"x":"left"}, "end":{"y":false}, "extra":true,
            "common":{"layer":42, "color":{"Index":"red"}}
        }),
    );
    for pointer in ["/start/x", "/end/y", "/common/layer", "/common/color/Index"] {
        assert!(
            report.errors.iter().any(|issue| issue.pointer == pointer),
            "{report:?}"
        );
    }
    assert!(report
        .warnings
        .iter()
        .any(|issue| issue.pointer == "/extra"));
}

#[test]
fn every_entity_payload_is_checked_before_create_or_define_block() {
    for command_type in ["create_entities", "define_block"] {
        let mut command = json!({
            "type":command_type,
            "entities":[
                {"type":"Line", "data":{"start":{"x":"left"},"end":{"y":false},"typo":1}},
                {"type":"Circle", "data":{"radius":"wide","center":{"x":false}}}
            ]
        });
        if command_type == "define_block" {
            command["name"] = json!("BAD");
            command["base"] = json!([0, 0]);
        }
        let report = validate_command(&command, Some(Units::Metres));
        for pointer in [
            "/entities/0/data/start/x",
            "/entities/0/data/end/y",
            "/entities/1/data/radius",
            "/entities/1/data/center/x",
        ] {
            assert!(
                report.errors.iter().any(|issue| issue.pointer == pointer),
                "{report:?}"
            );
        }
        assert!(report
            .errors
            .iter()
            .any(|issue| issue.pointer == "/entities/0/data/typo"));
        assert!(parse_command(command).is_err());
    }
}

#[test]
fn creation_refusals_keep_the_command_hint_in_validation() {
    for command_type in ["create_entities", "define_block"] {
        let mut command = json!({
            "type":command_type,
            "entities":[
                {"type":"Dimension", "data":{}},
                {"type":"Viewport", "data":{}}
            ]
        });
        if command_type == "define_block" {
            command["name"] = json!("BAD");
            command["base"] = json!([0, 0]);
        }
        let report = validate_command(&command, Some(Units::Metres));
        for (index, via) in [(0, "dimension"), (1, "mview")] {
            assert!(
                report.errors.iter().any(|issue| {
                    issue.code == "refused"
                        && issue.pointer == format!("/entities/{index}/type")
                        && issue.message.contains(via)
                }),
                "{report:?}"
            );
        }
    }
}

#[test]
fn unknown_record_fields_refuse_the_edit_without_changing_the_drawing() {
    let mut session = Session::new_blank(std::path::Path::new("records.dxf"), 6);
    let command = json!({
        "type":"create_entities",
        "entities":[{"type":"Line","data":{
            "start":{"x":0,"y":0},"end":{"x":5,"y":0},"spelling_mistake":10
        }}]
    });
    let before = session.drawing.doc.entities().count();
    let history = session.history_steps();
    let report = session.validate_input(&command);
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "unknown_field"
            && issue.pointer == "/entities/0/data/spelling_mistake"));
    assert!(session.run_input(&command, Source::User).is_err());
    assert_eq!(session.drawing.doc.entities().count(), before);
    assert_eq!(session.history_steps(), history);
}

#[test]
fn record_updates_collect_independent_errors_before_mutation() {
    let mut session = Session::new_blank(std::path::Path::new("record-updates.dxf"), 6);
    let made = session
        .run(
            parse_command(json!({
                "type":"draw","kind":"line","a":[0,0],"b":[5,0]
            }))
            .unwrap(),
        )
        .unwrap();
    let handle = serde_json::to_value(made).unwrap()["handles"][0].clone();
    let input = json!({
        "type":"set_records","collection":"entities","updates":[
            {"key":handle,"path":"/start/x","value":"left"},
            {"key":handle,"path":"/end/y","value":false},
            {"key":handle,"path":"/missing","value":1},
            {"key":999999,"path":"/radius","value":1}
        ]
    });
    let before = serde_json::to_value(session.drawing.doc.entities().next().unwrap()).unwrap();
    let report = session.validate_input(&input);
    for pointer in [
        "/updates/0/value",
        "/updates/1/value",
        "/updates/2/path",
        "/updates/3/key",
    ] {
        assert!(
            report.errors.iter().any(|issue| issue.pointer == pointer),
            "{report:?}"
        );
    }
    assert!(
        report.errors.iter().any(|issue| {
            issue.pointer == "/updates/0/value"
                && issue.message.contains("start.x")
                && issue.message.contains("invalid type")
        }),
        "{report:?}"
    );
    assert!(
        report.errors.iter().any(|issue| {
            issue.pointer == "/updates/1/value"
                && issue.message.contains("end.y")
                && issue.message.contains("invalid type")
        }),
        "{report:?}"
    );
    assert!(session.run(parse_command(input).unwrap()).is_err());
    let after = serde_json::to_value(session.drawing.doc.entities().next().unwrap()).unwrap();
    assert_eq!(before, after);
}

#[test]
fn sequential_record_expectations_follow_the_staged_values() {
    let mut session = Session::new_blank(std::path::Path::new("record-updates.dxf"), 6);
    let made = session
        .run(
            parse_command(json!({
                "type":"draw","kind":"line","a":[0,0],"b":[5,0]
            }))
            .unwrap(),
        )
        .unwrap();
    let handle = serde_json::to_value(made).unwrap()["handles"][0].clone();
    let input = json!({
        "type":"set_records","collection":"entities","updates":[
            {"key":handle,"path":"/start/x","value":1.0,"expected":0.0},
            {"key":handle,"path":"/start/x","value":2.0,"expected":1.0}
        ]
    });
    let report = session.validate_input(&input);
    assert!(report.valid, "{report:?}");
    session.run(parse_command(input).unwrap()).unwrap();
}

#[test]
fn record_replacements_report_all_missing_required_fields() {
    let mut session = Session::new_blank(std::path::Path::new("record-required.dxf"), 6);
    let made = session
        .run(
            parse_command(json!({
                "type":"draw","kind":"line","a":[0,0],"b":[5,0]
            }))
            .unwrap(),
        )
        .unwrap();
    let handle = serde_json::to_value(made).unwrap()["handles"][0].clone();
    let input = json!({
        "type":"set_records","collection":"entities","updates":[
            {"key":handle,"path":"/start","value":{"x":1.0}},
            {"key":handle,"path":"/end","value":{"z":0.0}}
        ]
    });
    let report = session.validate_input(&input);
    for pointer in [
        "/updates/0/value/y",
        "/updates/0/value/z",
        "/updates/1/value/x",
        "/updates/1/value/y",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|issue| issue.pointer == pointer && issue.code == "missing_field"),
            "{report:?}"
        );
    }
    assert!(session.run(parse_command(input).unwrap()).is_err());
}

#[test]
fn record_replacements_keep_serde_defaults_and_optional_fields_optional() {
    let mut session = Session::new_blank(std::path::Path::new("record-required.dxf"), 6);
    let made = session
        .run(
            parse_command(json!({
                "type":"draw","kind":"line","a":[0,0],"b":[5,0]
            }))
            .unwrap(),
        )
        .unwrap();
    let handle = serde_json::to_value(made).unwrap()["handles"][0].clone();
    let entity = session.drawing.doc.entities().next().unwrap();
    let mut common = qiyas_core::records::entity_data(entity)["common"].clone();
    let fields = common.as_object_mut().unwrap();
    // Transparency has serde(default); full_visual_style_handle is an Option without it.
    for field in ["transparency", "color_name", "full_visual_style_handle"] {
        assert!(fields.remove(field).is_some());
    }
    let input = json!({
        "type":"set_records","collection":"entities",
        "updates":[{"key":handle,"path":"/common","value":common}]
    });
    let report = session.validate_input(&input);
    assert!(report.valid, "{report:?}");
    session.run(parse_command(input).unwrap()).unwrap();
}
