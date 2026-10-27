//! Every integration test of this crate, built as one binary.
//!
//! Each file in `tests/` used to be its own test binary, and each one linked the whole crate
//! again. One binary links once. A new test file must be listed here; the test
//! `every_test_file_is_listed` fails when one is missing.

#[path = "agent.rs"]
mod agent;
#[path = "authoring.rs"]
mod authoring;
#[path = "drafting_eval.rs"]
mod drafting_eval;
#[path = "goal_a.rs"]
mod goal_a;
#[path = "goal_j.rs"]
mod goal_j;
#[path = "goal_l.rs"]
mod goal_l;
#[path = "goal_u_connection.rs"]
mod goal_u_connection;
#[path = "key_safety.rs"]
mod key_safety;
#[path = "mcp.rs"]
mod mcp;
#[path = "support/scripted.rs"]
mod scripted;
#[path = "surface.rs"]
mod surface;
#[path = "takeoff_eval.rs"]
mod takeoff_eval;
#[path = "tools.rs"]
mod tools;
#[path = "vision.rs"]
mod vision;
#[path = "w1.rs"]
mod w1;
#[path = "w3.rs"]
mod w3;

/// Files built as their own binary (see `[[test]]` in Cargo.toml).
const OWN_BINARY: &[&str] = &["clinic_performance", "villa_openings"];

#[test]
fn every_test_file_is_listed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let listed = include_str!("all.rs");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        if stem == "all" || OWN_BINARY.contains(&stem) {
            continue;
        }
        assert!(
            listed.contains(&format!("#[path = \"{stem}.rs\"]")),
            "tests/{stem}.rs is not in tests/all.rs, so it never runs"
        );
    }
}
