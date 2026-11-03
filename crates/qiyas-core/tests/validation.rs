use qiyas_core::registry::{parse_command, registry};
use qiyas_core::validation::{validate_batch, validate_command, validate_schema};
use qiyas_core::{Command, DrawSpec, Outcome, Session, Source, Units};
use serde_json::{json, Value};

#[test]
fn every_registry_example_passes_the_shared_input_check() {
    let mut failures = Vec::new();
    for command in registry().commands() {
        for input in &command.examples {
            let report = validate_command(input, Some(Units::Millimetres));
            if !report.valid || !report.warnings.is_empty() {
                failures.push(format!(
                    "{}: {} {:?}",
                    command.name,
                    report.message(),
                    report.warnings
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn serde_commands_and_typed_execution_keep_native_enum_variants() {
    let input = json!({
        "type":"batch",
        "commands":[
            {"type":"draw","kind":"line","a":[0,0],"b":[5,0]},
            {"type":"text","at":[0,1],"value":"Room"}
        ]
    });
    for command in [
        serde_json::from_value::<Command>(input.clone()).unwrap(),
        parse_command(input.clone()).unwrap(),
    ] {
        let Command::Batch { commands, .. } = &command else {
            panic!("JSON must deserialize directly to Command::Batch: {command:?}");
        };
        assert!(matches!(commands[0], Command::Draw(DrawSpec::Line { .. })));
        assert!(matches!(commands[1], Command::Text { .. }));
        let mut session = Session::new_blank(std::path::Path::new("native.dxf"), 6);
        let outcome = session.run(command).unwrap();
        let Outcome::Batch { outcomes, .. } = &outcome else {
            panic!("Typed execution must return Outcome::Batch: {outcome:?}");
        };
        assert!(matches!(outcomes[0], Outcome::Added { .. }));
        assert!(matches!(outcomes[1], Outcome::Added { .. }));
        let wire = serde_json::to_value(outcome).unwrap();
        assert_eq!(wire["type"], "batch");
        for result in std::iter::once(&wire).chain(wire["outcomes"].as_array().unwrap()) {
            assert!(result.get("outcome").is_none(), "{result}");
            assert!(result.get("defaults_used").is_none(), "{result}");
            assert!(result.get("warnings").is_none(), "{result}");
        }
    }
}

#[test]
fn invalid_raw_commands_and_batches_leave_entities_history_and_revision_unchanged() {
    let mut session = Session::new_blank(std::path::Path::new("invalid-input.dxf"), 6);
    for y in [0, 2] {
        session
            .run_input(
                &json!({"type":"draw","kind":"line","a":[0,y],"b":[5,y]}),
                Source::User,
            )
            .unwrap();
    }
    assert!(session.undo());
    let before = serde_json::to_value(session.drawing.doc.entities().collect::<Vec<_>>()).unwrap();
    let history = serde_json::to_value(session.history_steps()).unwrap();
    let revision = session.document_revision();
    for input in [
        json!({"type":"draw","kind":"line","a":"left","typo":true}),
        json!({"type":"batch","commands":[
            {"type":"new_layer","name":"MUST-NOT-EXIST"},
            {"type":"draw","kind":"line","a":"left","typo":true}
        ]}),
    ] {
        let error = session.run_input(&input, Source::User).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("/a") && message.contains("/b"), "{error}");
        assert_eq!(
            serde_json::to_value(session.drawing.doc.entities().collect::<Vec<_>>()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(session.history_steps()).unwrap(),
            history
        );
        assert_eq!(session.document_revision(), revision);
        assert!(session.drawing.doc.layers.get("MUST-NOT-EXIST").is_none());
    }
}

#[test]
fn reports_all_missing_invalid_and_unknown_fields_with_json_pointers() {
    let report = validate_command(
        &json!({"type":"text","rotation":"sideways","typo":true}),
        Some(Units::Metres),
    );
    assert!(!report.valid);
    for pointer in ["/at", "/value", "/rotation"] {
        assert!(
            report.errors.iter().any(|e| e.pointer == pointer),
            "{report:?}"
        );
    }
    assert!(report.warnings.is_empty());
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.pointer == "/typo" && issue.code == "unknown_field"));
    let error = parse_command(json!({"type":"draw","kind":"line"})).unwrap_err();
    assert!(error.contains("/a") && error.contains("/b"), "{error}");
    assert!(
        error.contains("missing field"),
        "Keeps serde's original error: {error}"
    );
}

#[test]
fn walker_checks_types_bounds_consts_enums_and_escaped_pointers() {
    let schema = json!({
        "type":"object","required":["needed"],"properties":{
            "a/b~c":{"type":"number","minimum":2,"maximum":4},
            "choice":{"enum":["one","two"]},
            "tag":{"const":"fixed"},
            "points":{"type":"array","items":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}}
        }
    });
    let report = validate_schema(
        &schema,
        &json!({
            "a/b~c":1,"choice":"three","tag":"wrong","points":[[0,"x"],[1]],"unknown":0
        }),
    );
    for pointer in [
        "/needed",
        "/a~1b~0c",
        "/choice",
        "/tag",
        "/points/0/1",
        "/points/1",
    ] {
        assert!(
            report.errors.iter().any(|e| e.pointer == pointer),
            "{report:?}"
        );
    }
    assert_eq!(report.warnings[0].pointer, "/unknown");
    assert!(!validate_schema(&schema, &json!([])).valid);
}

#[test]
fn flattened_object_parts_share_fields_and_keep_unknown_fields_as_warnings() {
    let schema = json!({
        "allOf":[
            {"type":"object","properties":{"a":{"type":"number"}},"required":["a"],"additionalProperties":false},
            {"type":"object","properties":{"b":{"type":"number","default":2}}}
        ]
    });
    let report = validate_schema(&schema, &json!({"a":1,"b":2,"extra":3}));
    assert!(report.valid, "{report:?}");
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].pointer, "/extra");
    let omitted = validate_schema(&schema, &json!({}));
    assert_eq!(omitted.errors.len(), 1);
    assert_eq!(omitted.errors[0].pointer, "/a");
    assert_eq!(omitted.defaults_used.len(), 1);
    assert_eq!(omitted.defaults_used[0].field, "/b");
}

#[test]
fn batch_checks_all_steps_and_references_without_running() {
    let commands = vec![
        json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
        json!({"type":"delete_entities","targets":[{"handle":{"$step":0,"pointer":"/handles/0"}}]}),
    ];
    assert!(validate_batch(&commands, None).valid);
    let report = validate_batch(
        &[
            json!({"type":"delete_entities","targets":[{"handle":{"$step":1}}]}),
            json!({"type":"draw","kind":"line"}),
            json!({"type":"delete_entities","targets":[{"handle":{"$step":2}}]}),
        ],
        None,
    );
    for pointer in [
        "/commands/0/targets/0/handle/$step",
        "/commands/1/a",
        "/commands/1/b",
        "/commands/2/targets/0/handle/$step",
    ] {
        assert!(
            report.errors.iter().any(|e| e.pointer == pointer),
            "{report:?}"
        );
    }
    assert!(
        !validate_command(
            &json!({"type":"delete_entities","targets":[{"handle":{"$step":0}}]}),
            None
        )
        .valid
    );
}

fn visit_defaults(schema: &Value, count: &mut usize) {
    match schema {
        Value::Object(object) => {
            if object.contains_key("default") {
                *count += 1;
                assert!(
                    object["description"]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains("default")),
                    "{schema}"
                );
            }
            // Keys under `properties` and friends are field names (a field may be called
            // "default"); default, example and enum values are data, not schemas.
            for (key, value) in object {
                match key.as_str() {
                    "properties" | "patternProperties" | "$defs" | "definitions" => {
                        if let Value::Object(fields) = value {
                            for field in fields.values() {
                                visit_defaults(field, count);
                            }
                        }
                    }
                    "default" | "examples" | "example" | "const" | "enum" => {}
                    _ => visit_defaults(value, count),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                visit_defaults(item, count);
            }
        }
        _ => {}
    }
}

fn declared_defaults(raw: &Value, published: &Value, definitions: &Value, depth: usize) {
    if depth >= 8 {
        return;
    }
    if let Some(reference) = raw["$ref"].as_str() {
        if let Some(name) = reference
            .strip_prefix("#/$defs/")
            .filter(|n| *n != "Command")
        {
            declared_defaults(&definitions[name], published, definitions, depth + 1);
        }
    }
    if let Some(default) = raw.get("default") {
        assert_eq!(
            published.get("default"),
            Some(default),
            "A serde default was lost from the published schema: {raw}"
        );
    }
    match raw {
        Value::Object(object) => {
            for key in [
                "properties",
                "items",
                "prefixItems",
                "oneOf",
                "anyOf",
                "allOf",
                "additionalProperties",
            ] {
                if let Some(value) = object.get(key) {
                    if key == "properties" {
                        for (field, schema) in value.as_object().unwrap() {
                            declared_defaults(schema, &published[key][field], definitions, depth);
                        }
                    } else {
                        declared_defaults(value, &published[key], definitions, depth);
                    }
                }
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                declared_defaults(item, &published[i], definitions, depth);
            }
        }
        _ => {}
    }
}

#[test]
fn declared_defaults_are_visible_and_described_in_every_registry_schema() {
    let raw = qiyas_core::registry::command_schema().to_value();
    for variant in raw
        .get("oneOf")
        .or_else(|| raw.get("anyOf"))
        .unwrap()
        .as_array()
        .unwrap()
    {
        let name = variant["properties"]["type"]["const"]
            .as_str()
            .or_else(|| {
                variant["allOf"].as_array().and_then(|parts| {
                    parts
                        .iter()
                        .find_map(|part| part["properties"]["type"]["const"].as_str())
                })
            })
            .unwrap();
        declared_defaults(
            variant,
            &registry().get(name).unwrap().schema,
            &raw["$defs"],
            0,
        );
    }
    let mut count = 0;
    for command in registry().commands() {
        visit_defaults(&command.schema, &mut count);
        // Deserialize an example and compare all omitted fields with their registry declaration.
        for example in &command.examples {
            let parsed: Command = serde_json::from_value(example.clone()).unwrap();
            let complete = serde_json::to_value(parsed).unwrap();
            if let Some(properties) = command.schema["properties"].as_object() {
                for (field, schema) in properties {
                    if let Some(default) = schema.get("default") {
                        if example.get(field).is_none() {
                            assert_eq!(
                                complete.get(field),
                                Some(default),
                                "{} /{field}",
                                command.name
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        count > 100,
        "The guard must cover all command families: {count}"
    );
    for (command, field, default) in [
        ("text", "rotation", json!(0.0)),
        ("text", "height", json!(0.0)),
        ("mtext", "width", json!(0.0)),
        ("walls", "height", json!(3.0)),
    ] {
        assert_eq!(
            registry().get(command).unwrap().schema["properties"][field]["default"],
            default
        );
    }
}

#[test]
fn wire_edits_refuse_unknown_fields_and_valid_inputs_keep_effective_defaults() {
    for (code, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut session = Session::new_blank(std::path::Path::new("defaults.dxf"), code);
        let mut raw = json!({"type":"text","at":[0,0],"value":"Room","typo":10});
        let before = session.history_steps();
        assert!(session
            .run_input(&raw, Source::User)
            .unwrap_err()
            .to_string()
            .contains("unknown_field"));
        assert_eq!(session.history_steps(), before);
        raw.as_object_mut().unwrap().remove("typo");
        let reported = session.run_input(&raw, Source::User).unwrap();
        assert!(matches!(reported.outcome, Outcome::Added { .. }));
        let outcome = serde_json::to_value(reported).unwrap();
        assert_eq!(outcome["type"], "added");
        assert!(outcome.get("outcome").is_none(), "{outcome}");
        assert_eq!(outcome["warnings"], json!([]));
        let height = outcome["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["field"] == "/height")
            .unwrap();
        assert!(
            (height["value"].as_f64().unwrap() * factor - 0.25).abs() < 1e-9,
            "{outcome}"
        );
        assert!(height["unit"].is_string());
        assert!(outcome["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["field"] == "/rotation" && d["value"].as_f64() == Some(0.0)));
    }
}

#[test]
fn batch_reports_defaults_from_the_context_when_each_step_runs() {
    for (code, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut session = Session::new_blank(std::path::Path::new("batch-defaults.dxf"), code);
        let outcome = session
        .run_input(
            &json!({
                "type":"batch",
                "commands":[
                    {"type":"annotation_scale","name":"1:50"},
                    {"type":"text","at":[0,0],"value":"Room"},
                    {"type":"annotation_scale","name":"1:20"},
                    {"type":"text","at":[0,1.0/factor],"value":"Door"},
                    {"type":"text","at":[0,2.0/factor],"value":"Explicit","height":0.4/factor,"rotation":0}
                ]
            }),
            Source::User,
        )
        .unwrap();
        let wire = serde_json::to_value(outcome).unwrap();
        let height = wire["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["field"] == "/commands/1/height")
            .unwrap();
        assert!((height["value"].as_f64().unwrap() * factor - 0.125).abs() < 1e-9);
        assert!(height["unit"].is_string());
        if code == 6 {
            assert_eq!(height["value"], 0.125);
            assert_eq!(height["unit"], "m");
        }
        let second = wire["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["field"] == "/commands/3/height")
            .unwrap();
        assert!((second["value"].as_f64().unwrap() * factor - 0.05).abs() < 1e-9);
        assert!(!wire["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["field"] == "/commands/4/height"
                || value["field"] == "/commands/4/rotation"));
        assert_eq!(wire["warnings"], json!([]));
        assert_eq!(wire["outcomes"][1]["type"], "added");
        assert_eq!(wire["outcomes"][1]["warnings"], json!([]));
        let step_height = wire["outcomes"][1]["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["field"] == "/height")
            .unwrap();
        assert_eq!(step_height["value"], height["value"]);
        let heights: Vec<_> = session
            .drawing
            .doc
            .entities()
            .filter_map(|entity| match entity {
                opencadcodec::EntityType::Text(text) => Some(text.height * factor),
                _ => None,
            })
            .collect();
        for expected in [0.125, 0.05, 0.4] {
            assert!(
                heights
                    .iter()
                    .any(|height| (height - expected).abs() < 1e-9),
                "{heights:?}"
            );
        }
    }
}

#[test]
fn conventional_cloud_tolerance_and_paste_defaults_follow_drawing_units() {
    for (code, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut session = Session::new_blank(std::path::Path::new("conventions.dxf"), code);
        let points = json!([
            [0, 0],
            [4.0 / factor, 0],
            [4.0 / factor, 2.0 / factor],
            [0, 2.0 / factor],
            [0, 0]
        ]);
        for command in [
            json!({"type":"draw","kind":"revcloud","points":points}),
            json!({"type":"comment","points":points,"text":"Review"}),
        ] {
            let wire =
                serde_json::to_value(session.run_input(&command, Source::User).unwrap()).unwrap();
            let length = wire["defaults_used"]
                .as_array()
                .unwrap()
                .iter()
                .find(|value| value["field"] == "/arc_len")
                .unwrap();
            assert!((length["value"].as_f64().unwrap() * factor - 12.0 / 32.0).abs() < 1e-10);
            assert!(length["unit"].is_string());
        }
        let report = session.validate_input(&json!({"type":"edit","kind":"overkill","targets":[]}));
        let tolerance = report
            .defaults_used
            .iter()
            .find(|value| value.field == "/tol")
            .unwrap();
        assert!((tolerance.value.as_f64().unwrap() * factor - 1e-9).abs() < 1e-20);
        let wire = serde_json::to_value(
            session
                .run_input(
                    &json!({
                        "type":"paste_from_clipboard","at":[0,0],"text":"Note"
                    }),
                    Source::User,
                )
                .unwrap(),
        )
        .unwrap();
        let height = wire["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["field"] == "/height")
            .unwrap();
        assert!((height["value"].as_f64().unwrap() * factor - 0.25).abs() < 1e-10);
    }
}

fn natural_input_session() -> Session {
    Session::new_blank(std::path::Path::new("tower-floor.dxf"), 6)
}

#[test]
fn natural_decision_and_records_filters_replay_failed_inputs() {
    let mut s = natural_input_session();
    let entry = json!({"name":"floor_scope","value":"One typical floor of a 30-storey tower","unit":"","source":"engineer","status":"answered","reason":"Requested scope","citation":"Engineer brief","set_by":"engineer"});
    for input in [
        json!({"type":"decision","action":"set","entries":[entry]}),
        json!({"type":"set","entries":[entry]}),
    ] {
        s.run_input(&input, Source::Ai).unwrap();
    }
    for input in [
        json!({"type":"decision","action":"list"}),
        json!({"type":"list"}),
    ] {
        s.run_input(&input, Source::Ai).unwrap();
    }
    s.run_input(&json!({"type":"define_block","name":"Column700","base":[0,0],"entities":[{"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":0.7,"y":0.7,"z":0}}}]}), Source::Ai).unwrap();
    s.run_input(
        &json!({"type":"insert_block","name":"Column700","at":[7.2,7.6]}),
        Source::Ai,
    )
    .unwrap();
    for condition in [
        json!({"path":"block","value":"Column700"}),
        json!({"block":"Column700"}),
        json!({"field":"block_name","value":"Column700"}),
    ] {
        let out = s.run_input(&json!({"type":"records","query":{"collection":"entities","type":"Insert","where":[condition]}}), Source::Ai).unwrap();
        let Outcome::Records { page } = out.outcome else {
            panic!("records")
        };
        assert_eq!(page.total, 1);
    }
}

#[test]
fn omitted_polyline_vertex_fields_work_in_blocks_entities_and_batches() {
    let mut s = natural_input_session();
    let entities = json!([{"type":"LwPolyline","data":{"is_closed":true,"vertices":[
        {"location":{"x":-0.35,"y":-0.35}}, {"location":{"x":0.35,"y":-0.35}},
        {"location":{"x":0.35,"y":0.35}}, {"location":{"x":-0.35,"y":0.35}}
    ]}}]);
    let out = s
        .run_input(
            &json!({"type":"batch","commands":[
                {"type":"define_block","name":"Column700","base":[0,0],"entities":entities},
                {"type":"create_entities","entities":entities}
            ]}),
            Source::Ai,
        )
        .unwrap();
    for command in [0, 1] {
        for field in ["bulge", "start_width", "end_width", "vertex_id"] {
            assert!(
                out.defaults_used.iter().any(|d| d.field
                    == format!("/commands/{command}/entities/0/data/vertices/0/{field}")
                    && d.value == 0.0),
                "{:?}",
                out.defaults_used
            );
        }
    }
    let records = s
        .records(&qiyas_core::records::RecordQuery::default())
        .unwrap();
    assert_eq!(records.total, 1);
    assert!(s.undo());
    assert_eq!(
        s.records(&qiyas_core::records::RecordQuery::default())
            .unwrap()
            .total,
        0
    );
}

#[test]
fn method_defaults_keep_citations_required_and_report_the_chosen_values() {
    let mut s = natural_input_session();
    let citation = json!({"source":"drawing","clause":"Wall paths"});
    let input = json!({"type":"measurement","kind":"set_method","method":{"name":"Tower floor geometry","rules":[{
        "id":"length","unit":"m","terms":[{"operation":"add","factors":["geometry.length"],"citation":citation}],"citation":citation
    }]}});
    let out = s.run_input(&input, Source::Ai).unwrap();
    for (field, value) in [
        ("/scope", json!("project")),
        ("/method/title", json!("Tower floor geometry")),
        ("/method/based_on", json!("none")),
        ("/method/source_note", json!("")),
        ("/method/rules/0/terms/0/label", json!("geometry.length")),
    ] {
        assert!(
            out.defaults_used
                .iter()
                .any(|d| d.field == field && d.value == value),
            "{:?}",
            out.defaults_used
        );
    }
    let mut missing = input;
    missing["method"]["rules"][0]
        .as_object_mut()
        .unwrap()
        .remove("citation");
    let report = s.validate_input(&missing);
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|e| e.pointer == "/method/rules/0/citation"
            && e.message.contains("every quantity must have a source")));
}

#[test]
fn row_data_headers_widths_and_dimension_line_point_replay_failed_batch() {
    let mut s = natural_input_session();
    let dimension = s.run_input(&json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[48,0],"line_point":[24,-1]}), Source::Ai).unwrap();
    let dimension = serde_json::to_value(dimension).unwrap()["handles"][0]
        .as_u64()
        .unwrap();
    let out = s.run_input(&json!({"type":"batch","commands":[
        {"type":"dim_edit","target":{"handle":dimension},"line_point":[24,-3]},
        {"type":"table","at":[60,32],"columns":["Floor / finish","Area m2","Specification"],"rows":[["Typical floor","1536","For review"],["Core","300","Concept"]],"col_widths":[3,1.5,5.3],"row_height":0.42,"text_height":0.14}
    ]}), Source::Ai).unwrap();
    assert!(out
        .defaults_used
        .iter()
        .any(|d| d.field == "/commands/1/rows" && d.value == 3));
    assert!(out
        .defaults_used
        .iter()
        .any(|d| d.field == "/commands/1/cols" && d.value == 3));
    let Outcome::Batch { outcomes, .. } = out.outcome else {
        panic!("batch")
    };
    let Outcome::Added { handles } = &outcomes[1] else {
        panic!("table")
    };
    let info = s.entity_info(handles[0], &[]).unwrap();
    let table = info.table.unwrap();
    assert_eq!(table.widths, vec![3.0, 1.5, 5.3]);
    assert_eq!(
        table.cells[0],
        vec!["Floor / finish", "Area m2", "Specification"]
    );
    let Some(opencadcodec::EntityType::Dimension(d)) = s
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(dimension))
    else {
        panic!("dimension")
    };
    let opencadcodec::entities::Dimension::Linear(d) = d else {
        panic!("linear")
    };
    assert_eq!(d.definition_point.y, -3.0);
    for fields in [json!({}), json!({"line_point":[1,2],"text_override":"<>"})] {
        let mut input = json!({"type":"dim_edit","target":{"handle":dimension}});
        input
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let report = s.validate_input(&input);
        assert!(!report.valid);
        assert!(report.message().contains("move_text") && report.message().contains("flip"));
    }
}

#[test]
fn missing_assignment_layers_are_created_reported_and_undone() {
    let mut s = natural_input_session();
    let out = s
        .run_input(
            &json!({"type":"draw","kind":"line","a":[0,0],"b":[48,0]}),
            Source::Ai,
        )
        .unwrap();
    let Outcome::Added { handles } = out.outcome else {
        panic!("line")
    };
    let handle = handles[0];
    for input in [
        json!({"type":"set_property","target":{"handle":handle},"prop":"layer","value":"S-COLUMN"}),
        json!({"type":"set_records","collection":"entities","updates":[{"key":handle,"path":"/common/layer","value":"S-COLUMN"}]}),
        json!({"type":"set_layer","name":"S-COLUMN","color":"ACI 7"}),
        json!({"type":"set_current","current":{"layer":"S-COLUMN","color":"ByLayer","linetype":"ByLayer","lineweight":-1}}),
    ] {
        let out = s.run_input(&input, Source::Ai).unwrap();
        assert!(s.drawing.doc.layers.contains("S-COLUMN"));
        assert!(
            out.advice
                .iter()
                .any(|a| a["code"] == "layer_created" && a["name"] == "S-COLUMN"),
            "{:?}",
            out.advice
        );
        assert!(s.undo());
        assert!(!s.drawing.doc.layers.contains("S-COLUMN"));
        assert_eq!(s.current.layer, "0");
    }
}

#[test]
fn literal_mapping_totals_name_the_drawing_quantity_form() {
    let report = validate_command(
        &json!({"type":"measurement","kind":"set_mappings","mappings":[{
            "rule":"(6.27375) + (-3.898125)","code":"W-N","description_en":"Net wall area","description_ar":"صافي مساحة الجدار","section":"Walls","trade":"","unit":"m2","citation":{"source":"drawing","clause":"Wall paths"}
        }]}),
        Some(Units::Metres),
    );
    assert!(!report.valid);
    assert!(report.message().contains("measurement.set_mappings"));
    assert!(report.message().contains("geometry.*") && report.message().contains("per-element"));
}

#[test]
fn registry_schemas_describe_the_accepted_natural_forms() {
    for (name, input) in [
        ("decision.list", json!({"type":"decision","action":"list"})),
        (
            "records",
            json!({"type":"records","query":{"where":[{"block":"Column700"}]}}),
        ),
        (
            "table",
            json!({"type":"table","at":[60,30],"columns":["Floor","Area"],"rows":[["Typical","1536"]],"col_widths":[4,2]}),
        ),
        (
            "dim_edit",
            json!({"type":"dim_edit","target":{"handle":405},"line_point":[11.65,3.35]}),
        ),
    ] {
        let info = registry().get(name).unwrap();
        let report = validate_schema(&info.schema, &input);
        assert!(
            report.valid && report.warnings.is_empty(),
            "{name}: {report:?}"
        );
        parse_command(input).unwrap();
    }
}
