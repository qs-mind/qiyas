//! `cargo run -p qiyas-ai --example phase2_walkthrough`: the Phase 2 check without a model.
//! Opens a copy of `samples/test-plan.dwg`, measures the rooms and walls and counts the blocks
//! through the AI tools, imports `samples/test-boq.xlsx` (copied beside the drawing: the tools
//! only open files in the drawing's folder), links every measurement, compares, and
//! prints the conflicts. Expected: two quantity mismatches, A.4 and B.2.
//! With `-- --no-link` nothing is linked, to see what the matching rules find on their own.

use qiyas_ai::{surface, ToolOutput};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn call(session: &mut Session, tool: &str, op: &str, args: Value) -> Value {
    match surface::run_session(session, tool, op, &args, Source::Ai) {
        Ok(ToolOutput::Json(v)) => v,
        Ok(ToolOutput::Image { .. }) => panic!("{op}: unexpected image"),
        Err(e) => panic!("{op} {args}: {e}"),
    }
}

fn run(session: &mut Session, command: Value) -> Value {
    call(session, surface::EXECUTE, "run", json!({"command":command}))
}

/// The handle of the entity on `layer` of `kind` whose bounds start at (x, y).
fn handle_at(session: &mut Session, layer: &str, kind: &str, x: f64, y: f64) -> u64 {
    let rows = call(
        session,
        surface::READ,
        "query",
        json!({"kind":"window", "layer":layer, "type":kind}),
    );
    let rows = rows["rows"].as_array().unwrap();
    let row = rows.iter().find(|r| {
        let min = &r["bounds"][0];
        (min[0].as_f64().unwrap() - x).abs() < 1e-6 && (min[1].as_f64().unwrap() - y).abs() < 1e-6
    });
    row.unwrap_or_else(|| panic!("no {kind} on {layer} at ({x}, {y})"))["handle"]
        .as_u64()
        .unwrap()
}

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples");
    let dir = tempfile::tempdir().unwrap();
    let dwg = dir.path().join("test-plan.dwg");
    std::fs::copy(format!("{root}/test-plan.dwg"), &dwg).unwrap();
    std::fs::copy(
        format!("{root}/test-boq.xlsx"),
        dir.path().join("test-boq.xlsx"),
    )
    .unwrap();
    let mut s = Session::open(&dwg).unwrap();

    let living = handle_at(&mut s, "A-ROOM", "LwPolyline", 0.15, 0.15);
    let bedroom = handle_at(&mut s, "A-ROOM", "LwPolyline", 7.15, 0.15);
    let ring = handle_at(&mut s, "A-WALL", "LwPolyline", 0.0, 0.0);
    let part_x = handle_at(&mut s, "A-WALL", "Line", 7.0, 0.0);
    let part_y = handle_at(&mut s, "A-WALL", "Line", 7.0, 5.0);
    let steps = [
        (
            "A.3",
            json!({"type":"add_measurement","kind":"area","handles":[living],"description":"Living room"}),
        ),
        (
            "A.4",
            json!({"type":"add_measurement","kind":"area","handles":[bedroom],"description":"Bedroom"}),
        ),
        (
            "A.1",
            json!({"type":"add_measurement","kind":"length","handles":[ring],"description":"Exterior walls"}),
        ),
        (
            "A.2",
            json!({"type":"add_measurement","kind":"length","handles":[part_x],"description":"Partition"}),
        ),
        (
            "A.2",
            json!({"type":"add_measurement","kind":"length","handles":[part_y],"description":"Partition"}),
        ),
        ("B.1", json!({"type":"count_block","block_name":"DOOR"})),
        ("B.2", json!({"type":"count_block","block_name":"WINDOW"})),
    ];
    let mut measured = Vec::new();
    for (code, input) in steps {
        let m = run(&mut s, input)["outcome"]["measurement"].clone();
        println!("{} {} -> {code}", m["shown"], m["description"]);
        measured.push((m["id"].clone(), code));
    }
    println!(
        "import_boq      {}",
        call(
            &mut s,
            surface::EXECUTE,
            "import_boq",
            json!({ "path": "test-boq.xlsx" })
        )
    );
    if std::env::args().all(|a| a != "--no-link") {
        for (id, code) in measured {
            run(&mut s, json!({"type":"link_boq", "id": id, "code": code }));
        }
    }
    println!(
        "compare_boq     {}",
        call(&mut s, surface::EXECUTE, "compare", json!({}))
    );
    let conflicts = call(
        &mut s,
        surface::READ,
        "records",
        json!({"collection":"conflicts"}),
    );
    println!("{}", serde_json::to_string_pretty(&conflicts).unwrap());
}
