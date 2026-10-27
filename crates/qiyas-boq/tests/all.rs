//! Every integration test of this crate, built as one binary.
//!
//! Each file in `tests/` used to be its own test binary, and each one linked the whole crate
//! again. One binary links once. A new test file must be listed here; the test
//! `every_test_file_is_listed` fails when one is missing.

#[path = "deliverables.rs"]
mod deliverables;
#[path = "dimensions.rs"]
mod dimensions;
#[path = "export.rs"]
mod export;
#[path = "../../qiyas-core/tests/support/measurement_fixture.rs"]
mod fixture;
#[path = "goal_u_exports.rs"]
mod goal_u_exports;
#[path = "import.rs"]
mod import;
#[path = "queries.rs"]
mod queries;
#[path = "report.rs"]
mod report;
#[path = "variation.rs"]
mod variation;
#[path = "w3_workbook.rs"]
mod w3_workbook;

/// Files built as their own binary (see `[[test]]` in Cargo.toml).
const OWN_BINARY: &[&str] = &[];

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
