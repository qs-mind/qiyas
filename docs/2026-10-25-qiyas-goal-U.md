# Goal U — clinic app fixes, 25 October 2026

Code is confined to `goal/u` in `/home/kareem/Work/.qiyas-wt/goal-u`.
No commit, push, new dependency, paid model call or change to the founder's running
checkout was made. The founder revised acceptance to fast tests; all separate
clinic/MCP replays and screenshot acceptance are left to the reviewer's combined run.

## Changes and fast proof

Tests named below are in `crates/qiyas-core/tests/goal_u.rs` unless another file is
named. All 17 core Goal U regressions pass, including the nested-void correction.
Final check totals and sandbox limitations are listed below.

| Item | Change | Fast test / evidence |
| --- | --- | --- |
| U1 | Fit the first content of an empty drawing once; frame new revealed handles, including with reduced motion. | `src/app/useSheets.test.tsx`: first content fits once; reveal frames its new bounds. Both failed before the change and passed afterwards. |
| U2 | Reuse an open empty untitled drawing with matching units, including a parked tab; retain drawings with content. | Desktop `drawing_host::tests::u2_new_drawing_reuses_empty_untitled_tabs_with_matching_units`. |
| U3 | Client pages omit diagnostics and internal entity references. Excel retains hidden audit records; PDF retains attached JSON. Sheet names and references are plain. | `u3_u4_client_references_are_plain_and_citations_are_unique`; BOQ `tests/goal_u_exports.rs` verifies visible cells/text and lossless round trips. Before: variation reference printed handle 987654321. |
| U4 | Deduplicate citations, use m²/m³ consistently and wrap workbook prose. Existing PDF pagination/wrapping is retained and exercised with a cell spanning pages. | Three tests in BOQ `tests/goal_u_exports.rs`; a 40-repeat description must retain every occurrence of finish, deductions and checked. |
| U5 | Room-name placement stays inside the room; room names and area labels avoid existing plotted ink. Try wrapping and a quarter-turn while preserving text height. Edit results expose full rule counts alongside sampled findings. | `u5_narrow_room_names_wrap_inside_and_area_labels_avoid_ink` in four units; AI surface `u5_change_reports_all_label_collisions_and_u15_checks_the_edited_sheet` requires at least 48 collisions to be reported. Before: name escaped a 1600 mm room to x=1850. |
| U6 | Untitled desktop exports use Documents. After Save As, exports follow the saved drawing. Host summaries and export results state the location. | `u6_untitled_export_uses_the_host_folder_then_follows_save_as`; desktop `u6_untitled_exports_have_a_visible_documents_destination`. Before: export used the hidden config/untitled folder. |
| U7 | New area labels default to English, including Arabic seed names. A supplied drawing's existing Arabic room-name text may retain Arabic; explicit language wins. Room schedules remain English by default. | Both `u7_*` tests check the default label, schedule headings, supplied Arabic exception and explicit English. Before: an Arabic seed generated Arabic in a new drawing. |
| U8 | Normalize Arabic/Persian/mixed digits, Arabic decimal/thousands separators and numeric field values. Ignore area-unit exponents when reading labels. | `u8_correct_arabic_english_and_mixed_area_labels_do_not_mismatch` in four units, plus `numbers` unit test. Correct labels pass; an incorrect Arabic area still fails. Before: three false mismatches. |
| U9 | Fixture/furniture block outlines do not deduct floor area by default. Explicit geometry or data types marked `room_void:true` can define islands. Room results retain deducted-island lists. | Both `u9_*` tests: four-unit fixtures, explicit geometry, imported block enclosure, explicit void type and undo. Before: a fixture reduced 20 m² to 19.717257 m². |
| U10 | Persist view-only markers on regions/groups. Manual take-off and generated quantities respect them; children follow excluded hosts. Markers survive reopen and undo. Scaled repeated detail geometry is advice until explicitly marked. | Three `u10_*` tests cover four units, totals, group edits, undo/redo and DXF reopen; `src/boq/boqMath.test.ts` excludes view-only rows. Before: no marker command or detail advice. |
| U11 | Rename comparison to `discrepancy_check`, retain old command alias `check`, and direct drawing-check requests to read op `check`. | Core `u11_discrepancy_check_has_a_clear_name_and_legacy_alias`; AI surface `u11_read_check_refusal_points_to_drawing_check_and_the_comparison_name`. |
| U12 | Matching layer, layout, object-type, block-definition and group creates return a note without another undo step. Conflicting definitions still refuse an implicit overwrite. | Three `u12_*` tests verify properties, history, matching definitions and conflicting members/geometry. Before: matching layer/layout creates failed. |
| U13 | Vite and Tauri ignore `.local` and `target`. Document `.local/wt` for nested worktrees. | `src/devWatch.test.ts` failed before and passes after. Founder dev server untouched; apply changes while idle. |
| U14 | Radius/diameter text follows the picked point outside the circle; leader ends clear of the text. Keep the glib alert open with a source guard and documented reachability review. | `u14_radial_dimension_text_follows_the_picked_point_outside_the_circle` in four units; glib guard and negative injected-call check. Before: mm radius text was at (1862.500036,1200), rather than (3000,4000). |
| U15 | Explicit `sheet` targets edits without switching the visible sheet. Refusals name the owner and how to target it. Checks follow the edited sheet; reveal follows the visible sheet. | Core `u15_edit_an_explicit_sheet_without_switching_the_view` tests both directions; two AI surface tests cover atomic rollback and remote-sheet checks; `src/i18n/errors.goalU.test.ts` checks plain English/Arabic. |
| U16 | Connection failures retain the conversation and completed work; panel offers Connection lost / Continue. Continue uses the existing agent history. | `src/ai/AssistantPanel.test.tsx`; AI `tests/goal_u_connection.rs` drops after the first edit, resumes with exactly one completed result, and requires exactly two distinct tool calls for two lines. No real provider is used. |

Geometry regressions use millimetres, centimetres, metres and inches. Tests read
back entities, quantities or history after edits; undo checks verify restoration.
The new narrow-room fixture was corrected to create its door swing before placement,
so the placement routine can avoid it. Its typed-void expected area uses the public
room report's six-decimal rounding. Existing assertions that expected internal handle 98 or `*Model_Space` were
changed to require plain client references. Older layer/layout tests that rejected
matching creates now require success with unchanged properties/history, while
conflicting properties remain refused. These were wrong-behavior expectations,
not relaxed checks. The older authoring expectations now require English by default
and accept an identical object type without an undo step; conflicting type data still
fails. The workbook unit expectation is m². The registry example fixture now creates
a real region for the view-only command. Radius/diameter examples put their actual
text anchor outside the circle; the existing zero-collision example guard is unchanged.
JSON-only sheet routing does not add positional command-line prompts or leak private
metadata into public schemas. A short equivalent take-off instruction saves room for
the longer `discrepancy_check` name without raising the response-size limit.

## Check results

Logs live in `.local/goal-run/checks/`. All cargo commands use
`CARGO_TARGET_DIR=/home/kareem/Work/.qiyas-wt/goal-u/target` and plain queued `cargo`.
Compilation/test runs use `CARGO_BUILD_JOBS=1`; no full-workspace clippy was run.

- Baseline core: 15 intended failures, 0 passes, one then-ignored fixture writer
  (`before-core-restarted.log`; build 2m26s, tests 0.24s). The obsolete writer was
  removed when separate end-to-end acceptance was dropped.
- Core after: all 17 Goal U tests pass (`final-targeted-tests-2.log`, 0.92s), including
  view-only groups, Arabic labels, explicit sheets and nested typed voids. The earlier
  expanded run exposed the nested-void issue; the passing run includes its correction.
- Desktop U2/U6: both failed before (`before-desktop-restarted.log`, 0.01s after
  a 21m58s dependency build); both pass in `final-rust-tests.log`.
- Export regressions: all 3 pass (`final-targeted-tests-2.log`, 0.55s), including
  PDF long-cell word counts and lossless Excel/PDF records.
- U16 mock-provider continuation passes (`final-targeted-tests-2.log`): two intended
  edits, two distinct calls, one retained first-step result after Continue.
- Focused frontend after: U1/U13 and wiring 7 passed (`after-u1-u13.log`);
  Continue panel 1 passed (`after-u16-panel.log`); totals 7 passed
  (`after-view-only-panel.log`); translated errors 3 passed (`after-sheet-errors.log`).
- `cargo fmt --all`: passed (`final-fmt-4.log`).
- Clippy, all targets, `-D warnings`, on core, geom, plot, boq, ai and desktop:
  **passed on the final code** (`final-clippy-4.log`, 1m11s). Earlier runs caught
  one helper-name warning and one documentation-spacing warning; both are fixed.
- Full tests for all six touched crates completed with **1,392 passed, 55 failed,
  16 ignored**, plus two passing isolated child-process repetitions
  (`final-rust-tests.log`; build 9m37s). Of those failures, 46 cannot bind sandbox
  loopback sockets; the other nine were addressed, with focused reruns described below.
  This full run was not green, and it is not represented as such.
- Correction rerun: **141 passed, 0 failed**, across nine named test targets
  (`final-targeted-tests-2.log`; build 57.12s). All nine non-socket full-suite failures
  now pass, as does the subsequently caught schema-extension guard. This includes
  72 AI surface tests, 19 authoring tests, the continuation test, 17 Goal U core
  tests, 13 registry tests, 12 command-line tests, 3 drawing-example tests, 3 export
  tests and the workbook report test.
- `node_modules/.bin/tsc --noEmit`: passed (`final-tsc-2.log`, exit 0).
  The first run caught the watcher test's missing Node ambient type; using Vite's raw
  text import fixed the test without changing application behavior.
- `node_modules/.bin/vitest run --maxWorkers=2`: **124 files, 919 tests passed**
  in 129.53s (`final-vitest.log`). After the test-only raw-import change, its focused
  rerun also passed (`final-watch-test.log`, 1 test, 408ms).
- The glib source guard scanned 18,072 Rust files / 690 resolved packages with no
  external affected-API reference. Its injected-call negative check failed as intended.
- No new end-to-end or screenshot run was started under the revised scope.

The full Rust run's actual counts, before the focused corrections (excluding the
two isolated desktop child-process repetitions):

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| qiyas-core | 839 | 3 | 0 |
| qiyas-geom | 80 | 0 | 0 |
| qiyas-plot | 43 | 0 | 1 |
| qiyas-boq | 26 | 1 | 0 |
| qiyas-ai | 270 | 49 | 15 |
| qiyas-desktop | 134 | 2 | 0 |

The nine non-socket failures covered updated English/idempotent-create/unit expectations,
export confinement error wording, typed command prompts, registry examples and the
kit size budget. The follow-up also caught and corrected a private schema extension.
No assertion, collision rule, response budget or sandbox-port test was weakened.
The 46 socket failures are listed in full below; the reviewer must rerun them where
local binds are permitted. They comprise 44 AI and two desktop tests.

<details>
<summary>Tests blocked by sandbox socket permissions</summary>

```text
qiyas-ai / lib: chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back
qiyas-ai / lib: chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn
qiyas-ai / lib: claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo
qiyas-ai / lib: models::tests::an_unreachable_service_falls_back_to_the_builtin_list
qiyas-ai / agent: a_marked_region_lists_what_is_in_it
qiyas-ai / agent: a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user
qiyas-ai / agent: a_question_ends_the_turn_cleanly_and_the_answer_carries_on
qiyas-ai / agent: a_selection_is_listed_and_shown
qiyas-ai / agent: all_server_failures_retry_with_a_visible_wait_and_identical_request
qiyas-ai / agent: an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room
qiyas-ai / agent: an_answer_that_keeps_running_out_stops_at_the_cap
qiyas-ai / agent: api_key_absent_from_logs_and_errors
qiyas-ai / agent: attached_pictures_go_before_the_words
qiyas-ai / agent: billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report
qiyas-ai / agent: cancel_between_tool_calls_runs_no_more_of_them
qiyas-ai / agent: cancel_stops_within_one_second
qiyas-ai / agent: file_tools_without_a_host_say_to_use_the_app
qiyas-ai / agent: gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn
qiyas-ai / agent: history_is_append_only_and_system_prompt_is_stable
qiyas-ai / agent: identical_malformed_input_stops_on_sixth_attempt
qiyas-ai / agent: invalid_output_value_keeps_the_cap_and_does_not_retry
qiyas-ai / agent: invalid_tool_json_is_answered_with_an_error_and_not_run
qiyas-ai / agent: mid_stream_overload_discards_partial_text_and_keeps_a_retry_log
qiyas-ai / agent: old_pictures_give_way_to_a_line
qiyas-ai / agent: openai_compatible_drops_a_refused_field_and_asks_again
qiyas-ai / agent: openai_compatible_streams_text_and_tool_calls
qiyas-ai / agent: partial_usage_chunks_retain_prior_counts_through_the_http_adapter
qiyas-ai / agent: refusal_stop_reason_surfaces_category
qiyas-ai / agent: rejected_request_after_find_entities_is_logged_without_retry
qiyas-ai / agent: removals_and_resolutions_report_counts_without_blocking
qiyas-ai / agent: successful_reads_continue_beyond_40_calls
qiyas-ai / agent: text_stream_is_forwarded_in_order
qiyas-ai / agent: tool_use_round_trip_executes_against_the_session
qiyas-ai / agent: view_image_stops_when_the_image_budget_is_used
qiyas-ai / key_safety: network_tests::model_discovery_never_follows_a_redirect_with_credentials
qiyas-ai / mcp: a_wrong_token_is_refused_with_401
qiyas-ai / mcp: a_restart_on_the_same_port_binds_again
qiyas-ai / mcp: concurrent_counts_both_land
qiyas-ai / mcp: execute_request_ids_replay_without_another_mutation_or_ui_event
qiyas-ai / mcp: lists_the_catalogue_and_measures_as_mcp
qiyas-ai / mcp: other_origins_are_refused_even_with_the_token
qiyas-ai / mcp: sheet_layer_and_zoom_changes_reach_the_window
qiyas-ai / mcp: tool_errors_are_error_results
qiyas-ai / mcp: view_image_is_an_image_block
qiyas-desktop / lib: commands::tests::the_mcp_server_follows_the_settings
qiyas-desktop / settings: credentials_follow_the_saved_address_and_never_a_draft
```

</details>

After the full suite finished, 9.83 GiB of obsolete/completed test executables were
removed from this worktree's own target, bringing it back to about 13 GB. No other
worktree, shared dependency source or running process was removed.

Interrupted compile logs do not count as completed test runs. Cargo uses the shared
queue, this worktree's target directory and one compiler job. Only one build/test
run from Goal U is active at a time.

## End-to-end gates — left to the reviewer's combined run

No new MCP run, transcript, PNG capture or clinic replay is claimed under the revised
scope. Compare the founder's original screenshots with the merged application's
captures of both floors and the detail, then check:

| Items | Reviewer check |
| --- | --- |
| U1–U2 | From an empty drawing, first user/Mizan content is visible; later edits preserve user zoom; repeated matching new_drawing reuses the tab. |
| U3–U4 | Open and print the merged clinic PDF/Excel: no handles, internal diagnostics or Model_Space; citations once, full long cells and consistent units. |
| U5 | Both floors/detail have readable room names and area labels clear of walls, doors and other labels; full collision counts are visible to Mizan. |
| U6 | Export an untitled drawing, open the reported Documents path, then Save As and verify the next export is beside the saved drawing. |
| U7–U8 | New labels/schedules/titles/tags are English unless requested otherwise; existing supplied Arabic names can match; correct Arabic labels produce no area mismatch. |
| U9 | Clinic fixtures do not deduct room floor area; explicit voids do, and listed islands explain each deduction. |
| U10 | Mark the enlarged detail view only; manual/generated totals exclude it, advice clears, and save/reopen/undo retain the correct marker state. |
| U11–U12 | Discovery distinguishes discrepancy_check from read check; repeated matching named creates succeed without duplicate entities or undo steps. |
| U13 | With the founder idle, apply watcher changes and write files below .local/wt and target; the running app must not reload. |
| U14 | Visually inspect diameter/radius leaders at outside picked points; retain the glib alert and rerun its guard on dependency updates. |
| U15 | With a layout visible, edit Model by explicit sheet, and vice versa; the view stays put and checks target the edited sheet. Merge W's state changes carefully. |
| U16 | Drop a mock connection after a completed edit; Connection lost / Continue is readable in both languages and resumes without repeating that edit. |

## Dependency review and limits

Tauri 2.12.1 → GTK 0.18.2 → glib 0.18.5. The offline source guard found no references
to the affected iterator outside glib in 18,072 Rust files across 690 resolved
packages. An injected call was rejected. This is a reachability assessment, not a
patched dependency; keep the alert open. See [glib-variant-iterator.md](glib-variant-iterator.md).
No new dependency was introduced, so LICENSES is unchanged.

The original founder screenshots were inspected. Earlier isolated browser capture
attempts failed before painting with sandbox socket permission errors; no successful
painted-window evidence is claimed. Such captures are now deferred by the founder.

The revised code and fast-test work is complete. Remaining acceptance is the
reviewer's combined visual/end-to-end run and rerunning the 46 socket-dependent tests
outside this sandbox. The glib dependency remains unpatched for the compatibility
reason above. Detail detection is advice for repeated scaled closed geometry, not a
claim to recognize every detail; explicit view-only markers determine measurement.
Cross-goal merges need the focused regression tests again. No new founder decision
is required to review these changes.

## Files touched and touches another goal

Cross-owner changes are intentionally local:

- P: `objects.rs` adds the void property/idempotent definition; `draw/room_boundary.rs`
  changes island eligibility; `measurement/evaluate.rs`, `derived.rs` and `input.rs`
  add exclusion refresh/filtering and explicit-sheet handling. Merge these with P's caches
  and batch deferral without losing invalidation or all-or-nothing behavior.
- C: `qiyas-ai/src/surface.rs` adds full check counts, checks on edited sheets, visible-sheet
  reveal filtering, explicit batch sheet inheritance and export routing. Preserve C's
  result envelope and artifact formatting when merging.
- A: one equivalent shorter instruction in `qiyas-core/src/takeoff_kit.rs` reserves
  bytes for the renamed discrepancy command; the budget and contracts are unchanged.
- W: the small `drawing_host.rs`, `tabs.rs`, `commands.rs` and `files.rs` changes reuse
  blank tabs and choose Documents. `src/app/useSheets.ts` fits first content/reveals.
  Preserve W's per-drawing session and assistant ownership when adapting these helpers.

No prompt, provider request body or other worktree is changed. Files in the final diff:

```text
.taurignore
CLAUDE.md
crates/qiyas-ai/src/surface.rs
crates/qiyas-ai/tests/authoring.rs
crates/qiyas-ai/tests/goal_u_connection.rs
crates/qiyas-ai/tests/surface.rs
crates/qiyas-boq/src/deliverables.rs
crates/qiyas-boq/src/dimensions.rs
crates/qiyas-boq/src/export.rs
crates/qiyas-boq/src/queries.rs
crates/qiyas-boq/src/report.rs
crates/qiyas-boq/src/variation.rs
crates/qiyas-boq/tests/deliverables.rs
crates/qiyas-boq/tests/dimensions.rs
crates/qiyas-boq/tests/goal_u_exports.rs
crates/qiyas-boq/tests/report.rs
crates/qiyas-core/src/blocks.rs
crates/qiyas-core/src/check.rs
crates/qiyas-core/src/cmdline/mod.rs
crates/qiyas-core/src/commands.rs
crates/qiyas-core/src/deliverables.rs
crates/qiyas-core/src/derived.rs
crates/qiyas-core/src/digest.rs
crates/qiyas-core/src/digest/detail_geometry.rs
crates/qiyas-core/src/dimension.rs
crates/qiyas-core/src/discrepancy/mod.rs
crates/qiyas-core/src/draw/room_boundary.rs
crates/qiyas-core/src/draw/room_islands.rs
crates/qiyas-core/src/embed.rs
crates/qiyas-core/src/groups.rs
crates/qiyas-core/src/input.rs
crates/qiyas-core/src/layers.rs
crates/qiyas-core/src/layouts.rs
crates/qiyas-core/src/lib.rs
crates/qiyas-core/src/measurement/evaluate.rs
crates/qiyas-core/src/measurement_scope.rs
crates/qiyas-core/src/numbers.rs
crates/qiyas-core/src/objects.rs
crates/qiyas-core/src/placement.rs
crates/qiyas-core/src/quantity_notes.rs
crates/qiyas-core/src/record_edit.rs
crates/qiyas-core/src/registry.rs
crates/qiyas-core/src/room_objects.rs
crates/qiyas-core/src/rooms.rs
crates/qiyas-core/src/session.rs
crates/qiyas-core/src/takeoff.rs
crates/qiyas-core/src/takeoff_kit.rs
crates/qiyas-core/tests/goal_u.rs
crates/qiyas-core/tests/layers.rs
crates/qiyas-core/tests/layouts.rs
crates/qiyas-core/tests/own_output.rs
crates/qiyas-desktop/src/commands.rs
crates/qiyas-desktop/src/drawing_host.rs
crates/qiyas-desktop/src/files.rs
crates/qiyas-desktop/src/tabs.rs
crates/qiyas-geom/src/dimension.rs
crates/qiyas-plot/src/report.rs
docs/2026-10-25-qiyas-goal-U.md
docs/glib-variant-iterator.md
scripts/audit-glib-iterator.py
src/ai/AssistantPanel.test.tsx
src/ai/AssistantPanel.tsx
src/api.ts
src/app/useSheets.test.tsx
src/app/useSheets.ts
src/boq/boqMath.test.ts
src/boq/boqMath.ts
src/devWatch.test.ts
src/i18n/ar.json
src/i18n/en.json
src/i18n/errors.goalU.test.ts
src/i18n/errors.ts
src/shell/TakeoffPanel.tsx
vite.config.ts
```
