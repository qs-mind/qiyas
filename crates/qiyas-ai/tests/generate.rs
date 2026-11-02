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
