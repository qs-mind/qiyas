//! Every integration test of this crate, built as one binary.
//!
//! Each file in `tests/` used to be its own test binary, and each one linked the whole crate
//! again. One binary links once. A new test file must be listed here; the test
//! `every_test_file_is_listed` fails when one is missing.

#[path = "arrange.rs"]
mod arrange;
#[path = "audit.rs"]
mod audit;
#[path = "autosave.rs"]
mod autosave;
#[path = "autosave_recovery.rs"]
mod autosave_recovery;
#[path = "batch.rs"]
mod batch;
#[path = "blank.rs"]
mod blank;
#[path = "blocks.rs"]
mod blocks;
#[path = "boq.rs"]
mod boq;
#[path = "check.rs"]
mod check;
#[path = "clipboard.rs"]
mod clipboard;
#[path = "cmdline.rs"]
mod cmdline;
#[path = "comment.rs"]
mod comment;
#[path = "conflicts.rs"]
mod conflicts;
#[path = "context_defaults.rs"]
mod context_defaults;
#[path = "conveniences.rs"]
mod conveniences;
#[path = "conventions.rs"]
mod conventions;
#[path = "define_block.rs"]
mod define_block;
#[path = "dimension.rs"]
mod dimension;
#[path = "discrepancy.rs"]
mod discrepancy;
#[path = "draw.rs"]
mod draw;
#[path = "drawing_quality.rs"]
mod drawing_quality;
#[path = "edit.rs"]
mod edit;
#[path = "element_metadata.rs"]
mod element_metadata;
#[path = "entity_geometry.rs"]
mod entity_geometry;
#[path = "events.rs"]
mod events;
#[path = "goal_a.rs"]
mod goal_a;
#[path = "goal_c_inputs.rs"]
mod goal_c_inputs;
#[path = "goal_j.rs"]
mod goal_j;
#[path = "goal_u.rs"]
mod goal_u;
#[path = "honest_commands.rs"]
mod honest_commands;
#[path = "image.rs"]
mod image;
#[path = "inquiry.rs"]
mod inquiry;
#[path = "layers.rs"]
mod layers;
#[path = "layouts.rs"]
mod layouts;
#[path = "measure.rs"]
mod measure;
#[path = "measurement.rs"]
mod measurement;
#[path = "support/measurement_fixture.rs"]
mod measurement_fixture;
#[path = "measurement_methods.rs"]
mod measurement_methods;
#[path = "modify.rs"]
mod modify;
#[path = "modify2.rs"]
mod modify2;
#[path = "object_migration.rs"]
mod object_migration;
#[path = "object_regions.rs"]
mod object_regions;
#[path = "object_schedules.rs"]
mod object_schedules;
#[path = "object_union_delete.rs"]
mod object_union_delete;
#[path = "object_vertical.rs"]
mod object_vertical;
#[path = "objects.rs"]
mod objects;
#[path = "open.rs"]
mod open;
#[path = "opening_batch_barriers.rs"]
mod opening_batch_barriers;
#[path = "opening_object_recipes.rs"]
mod opening_object_recipes;
#[path = "opening_schedule.rs"]
mod opening_schedule;
#[path = "own_output.rs"]
mod own_output;
#[path = "param.rs"]
mod param;
#[path = "pdf.rs"]
mod pdf;
#[path = "../../../tests/support/performance.rs"]
mod performance;
#[path = "pick_index_lazy.rs"]
mod pick_index_lazy;
#[path = "pick_polygon.rs"]
mod pick_polygon;
#[path = "point_runs.rs"]
mod point_runs;
#[path = "project.rs"]
mod project;
#[path = "projects.rs"]
mod projects;
#[path = "properties.rs"]
mod properties;
#[path = "purge.rs"]
mod purge;
#[path = "quantity_queries.rs"]
mod quantity_queries;
#[path = "query.rs"]
mod query;
#[path = "read_revision.rs"]
mod read_revision;
#[path = "recognition.rs"]
mod recognition;
#[path = "record_validation.rs"]
mod record_validation;
#[path = "records.rs"]
mod records;
#[path = "registry.rs"]
mod registry;
#[path = "revisions.rs"]
mod revisions;
#[path = "room_objects.rs"]
mod room_objects;
#[path = "save.rs"]
mod save;
#[path = "script.rs"]
mod script;
#[path = "session.rs"]
mod session;
#[path = "sheetset.rs"]
mod sheetset;
#[path = "snap.rs"]
mod snap;
#[path = "standalone.rs"]
mod standalone;
#[path = "symbol_objects.rs"]
mod symbol_objects;
#[path = "table.rs"]
mod table;
#[path = "text.rs"]
mod text;
#[path = "trust.rs"]
mod trust;
#[path = "ucs.rs"]
mod ucs;
#[path = "units.rs"]
mod units;
#[path = "validation.rs"]
mod validation;
#[path = "verify.rs"]
mod verify;
#[path = "viewports.rs"]
mod viewports;
#[path = "views.rs"]
mod views;
#[path = "w3_output.rs"]
mod w3_output;
#[path = "w5_drafting.rs"]
mod w5_drafting;
#[path = "wall_copy_mirror.rs"]
mod wall_copy_mirror;
#[path = "wall_defaults.rs"]
mod wall_defaults;
#[path = "wall_object_recipes.rs"]
mod wall_object_recipes;
#[path = "wall_persistence.rs"]
mod wall_persistence;
#[path = "wall_rollback.rs"]
mod wall_rollback;
#[path = "wall_systems.rs"]
mod wall_systems;
#[path = "walls.rs"]
mod walls;
#[path = "xref.rs"]
mod xref;

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
