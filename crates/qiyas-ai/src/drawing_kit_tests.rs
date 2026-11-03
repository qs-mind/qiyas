use crate::surface::{self, EXECUTE, READ};
use serde_json::{json, Value};

#[test]
fn drawing_kit_is_upfront_under_budget_with_complete_valid_examples() {
    let kit = include_str!("drawing_kit.txt");
    assert!(kit.len() <= 9216, "drawing kit is {} bytes", kit.len());
    let tools = surface::tools();
    assert!(tools
        .iter()
        .find(|t| t.name == EXECUTE)
        .unwrap()
        .description
        .contains(kit));
    let mut covered = std::collections::BTreeSet::new();
    for line in kit.lines().filter(|line| line.contains(" => ")) {
        let (form, example) = line.split_once(" => ").unwrap();
        let name = form.split('{').next().unwrap().trim();
        let example: Value = serde_json::from_str(example).unwrap();
        let info = qiyas_core::registry::registry().get(name).unwrap();
        for field in info.schema["properties"].as_object().unwrap().keys() {
            if field == "sheet" && info.json_only_fields.iter().any(|f| f == "sheet") {
                assert!(kit.contains("Shared edit/batch field: sheet?:S"));
                continue;
            }
            if !matches!(field.as_str(), "type" | "kind") {
                assert!(
                    form.contains(&format!("{field}:")) || form.contains(&format!("{field}?:")),
                    "{name} form omits {field}"
                );
            }
        }
        let report =
            qiyas_core::validation::validate_command(&example, Some(qiyas_core::Units::Metres));
        assert!(report.valid, "{name}: {}", report.message());
        let _: qiyas_core::Command = serde_json::from_value(example).unwrap();
        covered.insert(name);
    }
    for name in [
        "draw.line",
        "draw.rectangle",
        "draw.polyline",
        "draw.arc",
        "draw.circle",
        "draw.ellipse",
        "draw.hatch",
        "batch",
        "generate",
        "text",
        "mtext",
        "dimension.linear",
        "dimension.aligned",
        "walls",
        "add_wall_openings",
        "define_block",
        "insert_block",
        "edit_block_in_place",
        "save_block_edits",
        "modify.move",
        "modify.copy",
        "modify.array_rect",
        "modify.array_polar",
        "make_sheet",
        "mview",
        "title_block",
        "set_property",
        "records",
    ] {
        assert!(covered.contains(name), "missing {name}");
    }
}

#[test]
fn tool_guidance_does_not_restore_retired_design_rituals() {
    let tools = surface::tools();
    let execute = tools
        .iter()
        .find(|t| t.name == EXECUTE)
        .unwrap()
        .description;
    assert!(!execute.contains("expert-frame"));
    assert!(!execute.contains("إطار الخبير"));
    assert!(!execute.contains("records each with decision.set"));
    assert!(!execute.contains("يسجل كل قرار وسببه"));
    let capture = tools
        .iter()
        .find(|t| t.name == surface::CAPTURE)
        .unwrap()
        .description;
    assert!(!capture.starts_with("Look after exact checks and outside_check"));
    let plan = surface::plan_info();
    let doc = plan["doc"].as_str().unwrap();
    assert!(!doc.contains("use frame"));
    assert!(!doc.contains("إطار الخبير"));
    assert!(
        plan["schema"]["properties"]["frame"].is_object(),
        "old plans stay readable"
    );
}

#[test]
fn field_mistakes_suggest_the_correct_field_without_silently_changing_edits() {
    for (command, wrong, right) in [
        (
            json!({"type":"set_property","target":{"handle":1},"property":"layer","value":"A-SEATING"}),
            "property",
            "prop",
        ),
        (
            json!({"type":"set_property","targets":[{"handle":1}],"prop":"layer","value":"A-SEATING"}),
            "targets",
            "target",
        ),
        (
            json!({"type":"field","at":[0,0],"source":{"kind":"date"}}),
            "source",
            "field",
        ),
    ] {
        let report = qiyas_core::validation::validate_command(&command, None);
        assert!(!report.valid);
        let issue = report
            .errors
            .iter()
            .find(|e| e.pointer == format!("/{wrong}"))
            .unwrap();
        assert!(
            issue.message.contains(&format!("Did you mean {right}")),
            "{}",
            issue.message
        );
    }
}

#[test]
fn recorded_schema_mistakes_name_the_right_field_or_shape() {
    let cases = [
        (READ, "records", json!({"collection":"entity"}), "entities"),
        (EXECUTE, "batch", json!({"commands":[],"color":4}), "color"),
        (
            READ,
            "records",
            json!({"collection":"objects","where":{"id":97}}),
            "path",
        ),
        (
            READ,
            "records",
            json!({"collection":"entities","where":{"sheet":"*Paper_Space"}}),
            "path",
        ),
        (
            READ,
            "check",
            json!({"rules":["zero_length","opening_outside_wall"]}),
            "tiny_segment",
        ),
    ];
    for (tool, op, args, field) in cases {
        let (args, _) = crate::normalise::args(tool, op, &args).unwrap();
        let error = crate::surface_args::validate(tool, op, &args)
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{op}: {error}");
    }
    let cases = [
        (
            json!({"type":"set_property","targets":[{"handle":479}],"property":"layer","value":"A-ANNO-TAGS"}),
            "prop",
        ),
        (
            json!({"type":"set_property","targets":[{"handle":479}],"property":"layer","value":"A-ANNO-TAGS"}),
            "target",
        ),
        (
            json!({"type":"field","at":[0,0],"source":{"kind":"decision","name":"terminal_width"}}),
            "field",
        ),
        (
            json!({"type":"decision","kind":"set","entries":[{"name":"terminal_width","value":600,"reason":"Drawing dimensions","source":"mizan_design","citation":"Terminal plan","status":"answered"}]}),
            "unit",
        ),
        (
            json!({"type":"set_object_props","targets":[97],"props":{"name":"Terminal"},"sources":{"name":{"kind":"user","text":"Engineer brief"}}}),
            "engineer",
        ),
    ];
    for (command, field) in cases {
        let report = qiyas_core::validation::validate_command(&command, None);
        assert!(!report.valid, "{command}");
        assert!(
            report.message().contains(field),
            "{command}: {}",
            report.message()
        );
    }
}

#[test]
fn recorded_semantic_failures_name_source_sheet_and_sheet_fit_controls() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = qiyas_core::Session::new_blank(&dir.path().join("airport.dxf"), 6);
    for (command, field) in [
        (
            json!({"type":"opening_schedule","at":[0,0],"source_sheet":"Model","language":"en"}),
            "source_sheet",
        ),
        (
            json!({"type":"make_sheet","name":"A-101 Terminal","paper":"ISO A3","scale":"1:50","model_bounds":[[0,0],[600,400]]}),
            "split_lines",
        ),
    ] {
        let error = session
            .run_input(&command, qiyas_core::Source::Ai)
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{error}");
        assert!(
            !session.can_undo(),
            "a rejected command must not add an undo step"
        );
    }
}

#[test]
fn recorded_wrong_artifact_pointers_name_available_evidence_fields() {
    // These were evidence-path errors, not malformed command schemas.
    for (evidence, pointer, correct) in [
        (json!({"commands":[],"phases":{}}), "/examples", "commands"),
        (
            json!({"measurement_summary":[],"outside_check":{}}),
            "/assumed_values",
            "measurement_summary",
        ),
        (
            json!({"call_id":"example-call","content":[],"is_error":false,"operation":"records"}),
            "/outcome/result",
            "content",
        ),
    ] {
        let id = crate::artifact::save(&evidence).unwrap();
        let error = crate::artifact::read(&json!({"id":id,"pointer":pointer}))
            .unwrap_err()
            .to_string();
        assert!(error.contains(pointer), "{error}");
        assert!(error.contains(correct), "{error}");
    }
}

#[test]
fn decision_array_errors_name_missing_fields_instead_of_requesting_an_object() {
    let command = json!({"type":"decision","kind":"set","entries":[{
        "name":"terminal_width","value":600,"reason":"Drawing dimensions",
        "source":"mizan_design","citation":"Terminal plan","status":"answered"
    }]});
    let report = qiyas_core::validation::validate_command(&command, None);
    assert!(!report.valid);
    for field in ["unit", "set_by"] {
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.pointer == format!("/entries/0/{field}") && e.code == "missing_field"),
            "{}",
            report.message()
        );
    }
    assert!(
        !report
            .errors
            .iter()
            .any(|e| e.pointer == "/entries" && e.message.contains("Expected")),
        "{}",
        report.message()
    );
}
