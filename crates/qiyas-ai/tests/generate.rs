//! Generic tool discovery and execution of computed drawing recipes.
use qiyas_ai::surface::{self, EXECUTE, READ};
use qiyas_ai::ToolOutput;
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn call(s: &mut Session, tool: &str, input: Value) -> Value {
    let surface::Route::Session { tool, op, args } = surface::route(tool, &input).unwrap() else {
        panic!("session route");
    };
    let ToolOutput::Json(value) = surface::run_session(s, &tool, &op, &args, Source::Ai).unwrap()
    else {
        panic!("JSON result");
    };
    value
}

#[test]
fn generic_tools_discover_and_execute_generate_with_one_undo() {
    let mut s = Session::new_blank(std::path::Path::new("/tmp/generated-tool.dwg"), 6);
    let info = call(
        &mut s,
        READ,
        json!({"op":"commands","args":{"name":"generate","full":true}}),
    );
    assert!(info["schema"].is_object());
    let kit = call(&mut s, READ, json!({"op":"takeoff_kit","args":{}}));
    assert!(!kit.to_string().contains("generate"));
    assert!(serde_json::to_vec(&kit).unwrap().len() < qiyas_core::takeoff_kit::MAX_BYTES);
    let example = include_str!("../src/drawing_kit.txt")
        .lines()
        .find(|line| line.starts_with("generate{"))
        .unwrap()
        .split_once(" => ")
        .unwrap()
        .1;
    let command: Value = serde_json::from_str(example).unwrap();
    let result = call(
        &mut s,
        EXECUTE,
        json!({"op":"run","args":{"command":command}}),
    );
    assert_eq!(result["outcome"]["iterations"], 24);
    assert_eq!(result["added"].as_array().unwrap().len(), 48);
    assert!(s.undo_if(Source::Ai));
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
}

#[test]
fn generated_destructive_templates_reach_the_existing_approval_count() {
    let request = json!({"op":"run","args":{"command":{"type":"generate","loops":[{"var":"i","count":2}],
        "commands":[{"type":"delete_entities","targets":[{"handle":"=i+100"}]}]}}});
    assert!(surface::destructive_count(EXECUTE, &request) > 0);
}

fn block(name: &str) -> Value {
    json!({"type":"define_block","name":name,"base":[0,0],"entities":[
        {"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":0.7,"y":0,"z":0}}}
    ]})
}

#[test]
fn office_definitions_and_two_recipes_run_on_the_batch_layer_as_one_undo() {
    let mut s = Session::new_blank(std::path::Path::new("/tmp/office-recipes.dwg"), 6);
    call(
        &mut s,
        EXECUTE,
        json!({"op":"batch","args":{"layer":"A-FURN","commands":[
            block("Desk-equipped"), block("Task-chair"), block("Power-data"),
            {"type":"generate","loops":[{"var":"r","count":3},{"var":"c","count":2}],"commands":[
                {"type":"insert_block","name":"Desk-equipped","at":["=1+c*2","=2+r*2.3"]},
                {"type":"insert_block","name":"Task-chair","at":["=1.92+c*2","=1.55+r*2.3"]},
                {"type":"insert_block","name":"Power-data","at":["=2.4+c*2","=2.51+r*2.3"]}
            ]},
            {"type":"generate","loops":[{"var":"r","count":3}],"commands":[
                {"type":"insert_block","name":"Task-chair","at":[8.92,"=6.2+r*0.72"],"rotation":270},
                {"type":"insert_block","name":"Task-chair","at":[10.98,"=6.2+r*0.72"],"rotation":90}
            ]}
        ]}}),
    );
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 24);
    assert!(s
        .drawing
        .entities_in("*Model_Space")
        .all(|e| e.common().layer == "A-FURN"));
    assert_eq!(s.current.layer, "0");
    assert!(s.undo_if(Source::Ai));
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
    assert!(s.drawing.doc.block_records.get("Desk-equipped").is_none());
    assert!(s.drawing.doc.layers.get("A-FURN").is_none());
    assert!(!s.undo());
    assert!(s.redo());
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 24);
}

#[test]
fn tower_conditional_grid_runs_in_batch_and_layer_wrapped_run() {
    for (op, when) in [
        ("batch", "not(i>=2 and i<=4 and j==2)"),
        ("run", "i<2 or i>4 or j!=2"),
    ] {
        let mut s = Session::new_blank(std::path::Path::new("/tmp/tower-columns.dwg"), 6);
        call(
            &mut s,
            EXECUTE,
            json!({"op":"run","args":{"command":block("Column700")}}),
        );
        let recipe = json!({"type":"generate","vars":{},"loops":[{"var":"i","count":7},{"var":"j","count":5}],"when":when,
            "commands":[{"type":"insert_block","name":"Column700",
                "at":["=if(i==0,0.6,if(i==6,47.4,7.2+(i-1)*8.4))","=if(j==0,0.6,if(j==4,31.4,7.6+(j-1)*8.4))"]}]});
        let mut args = json!({"layer":"S-COLUMN"});
        args[if op == "batch" { "commands" } else { "command" }] = if op == "batch" {
            json!([recipe])
        } else {
            recipe
        };
        call(&mut s, EXECUTE, json!({"op":op,"args":args}));
        assert_eq!(s.drawing.entities_in("*Model_Space").count(), 32);
        assert!(s
            .drawing
            .entities_in("*Model_Space")
            .all(|e| e.common().layer == "S-COLUMN"));
        assert!(s.undo_if(Source::Ai));
        assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
        assert!(s.drawing.doc.block_records.get("Column700").is_some());
    }
}

#[test]
fn office_layer_wrapped_recipe_and_late_failure_keep_transaction_boundaries() {
    let mut s = Session::new_blank(std::path::Path::new("/tmp/office-chairs.dwg"), 6);
    call(
        &mut s,
        EXECUTE,
        json!({"op":"run","args":{"command":block("Task-chair")}}),
    );
    let recipe = json!({"type":"generate","loops":[{"var":"r","count":3},{"var":"c","count":2}],
        "commands":[{"type":"insert_block","name":"Task-chair","at":["=8.92+c*2.06","=6.2+r*0.72"],"rotation":"=270-180*c"}]});
    call(
        &mut s,
        EXECUTE,
        json!({"op":"run","args":{"layer":"A-FURN","command":recipe}}),
    );
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 6);
    assert!(s.undo_if(Source::Ai));
    let revision = s.revision();
    let request = json!({"op":"batch","args":{"layer":"A-FURN","commands":[recipe,
        {"type":"generate","loops":[{"var":"i","count":2}],"commands":[
            {"type":"insert_block","name":"Missing","at":["=i",0]}
        ]}
    ]}});
    let surface::Route::Session { tool, op, args } = surface::route(EXECUTE, &request).unwrap()
    else {
        panic!("session route")
    };
    let error = surface::run_session(&mut s, &tool, &op, &args, Source::Ai)
        .unwrap_err()
        .json()
        .to_string();
    assert!(
        error.contains("/commands/1/commands/0") && error.contains("iteration 1"),
        "{error}"
    );
    assert_eq!(s.revision(), revision);
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0);
    assert!(s.redo());
    assert_eq!(s.drawing.entities_in("*Model_Space").count(), 6);
}
