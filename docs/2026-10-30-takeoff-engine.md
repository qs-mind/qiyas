# Take-off engine findings, 30 October 2026

1. Coverage now honours explicit decision exclusions by handles or object type, and internal `Exclude <handle>: <reason>` report lines. Decisions retain the reason and source; the schema rejects blank reasons and empty scopes. Excluded elements are paged under `section: excluded`, with their reasons, instead of requiring measurement or mapping. They also satisfy note coverage. No prose or type-name inference changes scope, and quantities are not rewritten.
2. The outside check uses Qiyas opening cut footprints as well as symbol geometry, so a leaf drawn on one side of a wall still identifies the opening on the other face. It recognises generated opening inserts independently of layer names. Derived wall outlines and hatches are not additional raw quantities, and a closed wall run's enclosed floor area is not a wall quantity. A raw source measured in a different BOQ dimension is retained with `comparison_applicable: false`. Existing thickness-aware junction and wall-union enclosure logic is covered by a separate-system T/L shop-and-corridor regression, including missing-opening, wall-gap and outside-site faults.
3. Region placement accepts `seed: [x, y]` and optional `boundary_layers`. It binds the same `QIYAS_OBJECT_REGION` source used by retained rooms, including islands and updates after wall edits. Typed outlines remain valid. `region_not_from_boundary` is advisory only, using a 0.5 mm geometric tolerance around the centroid. Region objects do not supply their own enclosure geometry unless explicitly declared room voids. A budget-limited search does not assert a mismatch.
4. New object drivers and generated roles default to `Q-<TYPE-NAME>` when no role layer applies. Region labels use their shaped text bounds and the existing free-space search, fitting inside the region, outside islands and clear of plotted ink. Explicit layers remain supported; existing adopted drivers on named layers are preserved.

The take-off kit and English/Arabic workflows document seed placement and structured exclusions. New diagnostic strings are also in both translation catalogues. No window behaviour, dependencies or CLAUDE.md changed.

Validation is recorded below after the targeted checks complete. All commands run offline with two Cargo build jobs, sequentially. The initial outside-check regression reproduced the false missing-opening finding. A further exploratory test that removed opening metadata showed that a generated symbol alone cannot recover a missing cut footprint; metadata-free imports still need explicit opening geometry/handles when their symbols do not reach the enclosure. That destructive-metadata scenario is outside the live-test fixture.

Changed source files:

- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/decisions.rs`
- `crates/qiyas-core/src/digest.rs`
- `crates/qiyas-core/src/digest/exclusions.rs` (new)
- `crates/qiyas-core/src/digest/note_scope.rs`
- `crates/qiyas-core/src/draw/room_boundary.rs`
- `crates/qiyas-core/src/object_regions.rs`
- `crates/qiyas-core/src/objects.rs`
- `crates/qiyas-core/src/outside_check.rs`
- `crates/qiyas-core/src/placement.rs`
- `crates/qiyas-core/src/takeoff_kit.rs`
- `crates/qiyas-core/tests/object_regions.rs`
- `crates/qiyas-core/tests/outside_check.rs`
- `crates/qiyas-core/tests/registry.rs`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/surface_args.rs`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/src/takeoff_workflow_ar.txt`
- `crates/qiyas-ai/tests/surface.rs`
- `src/i18n/en.json` (five take-off messages only)
- `src/i18n/ar.json` (matching five messages only)
- `docs/2026-10-30-takeoff-engine.md`

Tests added or extended:

- `reasoned_exclusions_close_coverage_by_handle_type_and_report`: rejects blank reasons through schema validation, closes coverage through handle/type decisions and report lines, checks reasons and a passing BOQ, and exercises undo.
- `qiyas_shop_plan_has_no_false_geometric_findings`: independent exterior, shop, partition and core wall systems, custom layers, doors/windows, and T/L junctions.
- `qiyas_shop_plan_still_reports_missing_door_gap_and_outside_element`: genuine missing opening, gap and site-containment faults remain visible.
- `wall_graphics_are_not_extra_quantities_and_different_dimensions_are_not_mismatches`: no duplicate graphics/GFA quantities or invalid comparisons across dimensions.
- `seeded_placement_follows_wall_edits_and_places_clear_labels_on_named_layers`: metre/mm plans, boundary-layer selection, label placement around an obstacle, quantities after edits, undo/redo and DXF reopening.
- `typed_region_mismatch_is_advisory_and_seed_inputs_are_unambiguous`: mismatch advice, matching typed outline, and rejection of conflicting seed/driver/point inputs.
- Extended the existing AI kit test to check the seed, exclusions and compact self-check read operation. Extended the existing kit registry test to validate the seed example and exclusion guidance. Its original byte guard remains unchanged. No pre-existing test was removed or weakened.

Checks run (prefix for every Cargo command: `CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2`):

| Command | Actual results |
| --- | --- |
| `cargo test -p qiyas-core --test all outside_check` | First run: 10 passed, 1 failed, reproducing the opening-contact bug. Second run: 10 passed, 1 failed only in the exploratory metadata-removal assertion described above; the intact plan passed. Final two runs: 12 passed each. |
| `cargo test -p qiyas-core --test all object_regions` | 7 passed, then 7 passed after adding explicit label position/layer assertions. |
| `cargo test -p qiyas-core --test all objects::` | 21 passed. |
| `cargo test -p qiyas-core --test all decisions::` | 7 passed. |
| `cargo test -p qiyas-core --test all registry` | First: 26 passed, 2 failed (kit 6,108 bytes exceeded the reserved-headroom guard; standalone seed example lacked its fixture). Second: 27 passed, 1 failed (kit 5,950 bytes). Final: 28 passed after reducing the kit to 5,887 bytes and keeping the seed example in the kit. Original guards unchanged. |
| `rustfmt --edition 2021 --check --config skip_children=true <all changed Rust files>` | Passed. |
| `git diff --check` on task files | Passed. |
| Python JSON/catalogue and added-text checks | Both JSON catalogues parse; all five new messages have both languages; added task text contains no U+2014. |
| `cargo test -p qiyas-ai --test all takeoff_kit` | First: 1 failed because the bound response was 6,310 bytes. Final: 1 passed after removing duplicated report guidance and the redundant self-check wrapper; actual bound response is 6,136 bytes, below 6,144. |
| `cargo test -p qiyas-ai --test all digest_named_handles_and_scope_checks_share_drawing_evidence` | 1 passed. |
| `cargo test -p qiyas-ai --test all takeoff_check_reports_count_and_global_allowance_errors_until_repaired` | 1 passed. |

The AI kit's self-check is now the compact read operation `{op: "takeoff_check", args: {}}`; report paths remain documented on the read surface. The existing kit test refreshed its ignored `.local/mizan-discovery/takeoff-kit.json` artifact. No other task's source changes were edited. Work remains uncommitted.

Limits: verification used headless fixtures, not the original 190-request model session. The label search places text when its full bounds fit; it does not change the chosen text size or wrap an oversized label. No project/design decision is needed from the engineer for these changes.
