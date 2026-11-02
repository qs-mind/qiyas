use opencadcodec::EntityType;
use qiyas_core::{Command, Session, Source};
use serde_json::{json, Value};

fn session(units: i16) -> Session {
    Session::new_blank(std::path::Path::new("/tmp/generated.dwg"), units)
}

fn run(s: &mut Session, value: Value) -> Value {
    serde_json::to_value(s.run_input(&value, Source::Ai).unwrap()).unwrap()
}

#[test]
fn nested_grid_has_24_columns_labels_and_one_undo_step() {
    let mut s = session(6);
    let result = run(
        &mut s,
        json!({"type":"generate",
        "vars":{"pitch":"8 * unit"},
        "loops":[{"var":"col","count":6},{"var":"row","count":4}],
        "commands":[
            {"type":"draw","kind":"rectangle","a":["=col*pitch","=row*pitch"],"b":["=col*pitch+0.6*unit","=row*pitch+0.6*unit"]},
            {"type":"text","at":["=col*pitch","=row*pitch+unit"],"height":"=0.2*unit","value":"C{col+1}-{row+1}"}
        ]}),
    );
    assert_eq!(result["iterations"], 24);
    assert_eq!(result["templates"][0]["count"], 24);
    assert_eq!(result["templates"][1]["count"], 24);
    assert_eq!(result["steps"].as_array().unwrap().len(), 48);
    assert!(result["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["handles"].as_array().unwrap().len() == 1));
    let texts: Vec<_> = s
        .drawing
        .entities_in("*Model_Space")
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t.value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 24);
    assert!(texts.contains(&"C6-4"));
    assert!(s.undo());
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
    assert!(!s.undo());
    assert!(s.redo());
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 48);
}

#[test]
fn when_false_skips_and_expressions_keep_number_text_and_boolean_types() {
    let mut s = session(6);
    let result = run(
        &mut s,
        json!({"type":"generate",
        "vars":{"distance":"max(2, ceil(1.1))"},
        "loops":[{"var":"i","from":0,"to":5,"step":1}], "when":"mod(i,2)==0",
        "commands":[{"type":"text","at":["=i*distance","=sqrt(9)+sin(30)+cos(60)+tan(0)"],
            "value":"=if(i == 0, 'START', 'COLUMN')","height":"=round(pi,2)/10"}]}),
    );
    assert_eq!(result["iterations"], 6);
    assert_eq!(result["skipped"], 3);
    assert_eq!(result["steps"].as_array().unwrap().len(), 3);
    let positions: Vec<_> = s
        .drawing
        .entities_in("*Model_Space")
        .filter_map(|e| match e {
            EntityType::Text(t) => {
                Some((t.insertion_point.x, t.insertion_point.y, t.value.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(positions[0].2, "START");
    assert_eq!(positions[2].0, 8.0);
    assert!((positions[2].1 - 4.0).abs() < 1e-10);
}

#[test]
fn along_points_and_arc_use_length_and_tangent_in_degrees() {
    let mut s = session(6);
    let result = run(
        &mut s,
        json!({"type":"generate","loops":[{"var":"station","along":{
        "points":[[0,0],[3,0],[3,4]],"spacing":2}}],
        "commands":[{"type":"text","at":["=x","=y"],"rotation":"=angle","value":"S{station}","height":0.2}]}),
    );
    assert_eq!(result["iterations"], 4);
    let texts: Vec<_> = s
        .drawing
        .entities_in("*Model_Space")
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!((texts[2].insertion_point.x - 3.0).abs() < 1e-9);
    assert!((texts[2].insertion_point.y - 1.0).abs() < 1e-9);
    let arc = run(
        &mut s,
        json!({"type":"draw","kind":"arc","centre":[0,0],"radius":10,"start":0,"end":90}),
    );
    let result = run(
        &mut s,
        json!({"type":"generate","loops":[{"var":"i","along":{"path":arc["handles"][0],"count":3}}],
        "commands":[{"type":"draw","kind":"point","p":["=x","=y"]}]}),
    );
    let mid = &result["steps"][1]["values"];
    assert!((mid["x"].as_f64().unwrap() - 50f64.sqrt()).abs() < 1e-8);
    assert!((mid["y"].as_f64().unwrap() - 50f64.sqrt()).abs() < 1e-8);
    assert!((mid["angle"].as_f64().unwrap() - 135.0).abs() < 1e-5);
}

#[test]
fn iteration_37_runtime_failure_rolls_back_and_names_template_and_values() {
    let mut s = session(6);
    let before = s.revision();
    let error = s
        .run_input(
            &json!({"type":"generate","loops":[{"var":"i","from":1,"to":40,"step":1}],
        "commands":[{"type":"draw","kind":"circle","centre":["=i",0],"radius":"=if(i==37,-1,1)"}]}),
            Source::Ai,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("/commands/0"), "{error}");
    assert!(error.contains("iteration 37"), "{error}");
    assert!(error.contains("\"i\":37"), "{error}");
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
    assert_eq!(s.revision(), before);
    assert!(!s.undo());
}

#[test]
fn numeric_validation_names_the_exact_template_pointer() {
    let mut s = session(6);
    let error = s
        .run_input(
            &json!({"type":"generate","loops":[{"var":"i","count":40}],
        "commands":[{"type":"draw","kind":"circle","centre":[0,0],"radius":"=if(i==36,'bad',1)"}]}),
            Source::Ai,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("/commands/0/radius"), "{error}");
    assert!(error.contains("iteration 37"), "{error}");
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
}

#[test]
fn limits_refuse_even_when_all_iterations_skip() {
    for (extra, limit) in [
        (json!({"loops":[{"var":"i","count":20001}]}), "20000"),
        (
            json!({"loops":[{"var":"i","count":100001}],"when":"false"}),
            "100000",
        ),
        (
            json!({"vars":{"deep":format!("{}1{}", "(".repeat(150), ")".repeat(150))}}),
            "128",
        ),
    ] {
        let mut s = session(6);
        let mut input =
            json!({"type":"generate","commands":[{"type":"draw","kind":"point","p":[0,0]}]});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let error = s.run_input(&input, Source::Ai).unwrap_err().to_string();
        assert!(error.contains(limit), "{error}");
        assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
    }
}

#[test]
fn units_produce_the_same_real_geometry_for_native_and_wire_calls() {
    for units in [4, 6] {
        let mut s = session(units);
        let recipe = json!({"type":"generate","vars":{"width":"1.5*unit"},
            "commands":[{"type":"draw","kind":"line","a":[0,0],"b":["=width",0]}]});
        s.run(serde_json::from_value::<Command>(recipe).unwrap())
            .unwrap();
        let line = s
            .drawing
            .entities_in("*Model_Space")
            .find_map(|e| match e {
                EntityType::Line(l) => Some(l),
                _ => None,
            })
            .unwrap();
        assert!((line.end.x * s.drawing.units.to_metres() - 1.5).abs() < 1e-10);
    }
}

#[test]
fn registry_exposes_generate_without_growing_the_takeoff_kit() {
    let registry = qiyas_core::registry::registry();
    let command = registry.get("generate").unwrap();
    assert_eq!(command.category, "draw");
    assert!(!command.batchable);
    assert!(command.examples.len() >= 5);
    for example in &command.examples {
        let report = qiyas_core::validation::validate_command(example, None);
        assert!(report.valid, "{}", report.message());
        serde_json::from_value::<Command>(example.clone()).unwrap();
    }
    let kit = qiyas_core::takeoff_kit::kit();
    assert!(!kit.to_string().contains("generate"));
    assert!(serde_json::to_vec(&kit).unwrap().len() < qiyas_core::takeoff_kit::MAX_BYTES - 256);
}

#[test]
fn inner_bounds_and_variables_use_outer_values() {
    let mut s = session(6);
    let result = run(
        &mut s,
        json!({"type":"generate",
        "vars":{"width":"base*unit","base":2,"offset":"row*width"},
        "loops":[{"var":"row","count":3},{"var":"col","count":"row+1"}],
        "commands":[{"type":"draw","kind":"point","p":["=offset","=col*unit"]}]}),
    );
    assert_eq!(result["iterations"], 6);
    assert_eq!(result["steps"][5]["values"]["row"], 2.0);
    assert_eq!(result["steps"][5]["values"]["col"], 2.0);
}

#[test]
fn no_file_imports_nested_batches_or_invalid_ranges() {
    for extra in [
        json!({"commands":[{"type":"insert_block","name":"X","at":[0,0],"from_path":"/tmp/file.dwg"}]}),
        json!({"commands":[{"type":"batch","commands":[]}]}),
        json!({"commands":[{"type":"generate","commands":[]}]}),
        json!({"loops":[{"var":"i","from":0,"to":1,"step":0}]}),
        json!({"loops":[{"var":"i","from":0,"to":1,"step":-1}]}),
        json!({"loops":[{"var":"i","count":0.5}]}),
        json!({"vars":{"a":"b+1","b":"a+1"}}),
        json!({"vars":{"unit":1000}}),
    ] {
        let mut s = session(6);
        let mut input =
            json!({"type":"generate","commands":[{"type":"draw","kind":"point","p":[0,0]}]});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(s.run_input(&input, Source::Ai).is_err(), "{input}");
        assert!(!s.undo());
    }
}

#[test]
fn registry_examples_draw_desks_stairs_grid_arc_seats_and_room_hatches() {
    for example in &qiyas_core::registry::registry()
        .get("generate")
        .unwrap()
        .examples
    {
        let mut s = session(6);
        let arc = run(
            &mut s,
            json!({"type":"draw","kind":"arc","centre":[0,0],"radius":30,"start":0,"end":120}),
        );
        let mut example = example.clone();
        if example["loops"][0]["along"].is_object() {
            example["loops"][0]["along"]["path"] = arc["handles"][0].clone();
        }
        let result = run(&mut s, example);
        assert!(!result["added"].as_array().unwrap().is_empty());
        assert!(s.undo());
        assert_eq!(s.drawing.entities_in("*Model_Space").count(), 1);
    }
}

#[test]
fn a_template_condition_places_one_stair_arrow_after_the_last_tread() {
    let mut s = session(6);
    let result = run(
        &mut s,
        json!({"type":"generate","loops":[{"var":"i","count":20}],
        "commands":[{"type":"draw","kind":"line","a":[0,"=i*0.28"],"b":[1.5,"=i*0.28"]},
            {"when":"i==19","type":"text","at":[0.75,5.6],"height":0.2,"value":"UP"}]}),
    );
    assert_eq!(result["templates"][0]["count"], 20);
    assert_eq!(result["templates"][1]["count"], 1);
    assert_eq!(result["steps"][20]["iteration"], 20);
}

#[test]
fn expression_results_deserialize_as_integer_fields_and_keep_escaped_braces() {
    let mut s = session(6);
    run(
        &mut s,
        json!({"type":"generate","vars":{"sides":"2+4"},"commands":[
        {"type":"draw","kind":"polygon","centre":[0,0],"radius":1,"sides":"=sides"},
        {"type":"text","at":[0,2],"value":"{{C}} {sides}","height":0.2}]}),
    );
    assert!(s
        .drawing
        .entities_in("*Model_Space")
        .any(|e| matches!(e, EntityType::Text(t) if t.value == "{C} 6")));
}

#[test]
fn when_guards_variables_that_are_undefined_on_skipped_iterations() {
    let mut s = session(6);
    let result = run(
        &mut s,
        json!({"type":"generate",
        "vars":{"radius":"sqrt(i)"},"loops":[{"var":"i","from":-2,"to":2,"step":1}],
        "when":"i>0","commands":[{"type":"draw","kind":"circle","centre":["=i",0],"radius":"=radius"}]}),
    );
    assert_eq!(result["skipped"], 3);
    assert_eq!(result["templates"][0]["count"], 2);
}
