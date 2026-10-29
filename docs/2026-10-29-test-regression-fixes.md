# Regression fixes, 29 October 2026

All 18 reported failures now pass in targeted runs. Workspace clippy passes with warnings denied, and workspace formatting completed successfully.

Categories: (a) implementation regression, (b) incorrect fixture API, (c) assertion superseded by a named 29 October design note. No test was deleted, disabled or skipped. Existing behavior assertions remain except the documented replacements below.

## The 18 reported failures

| Test | Category | Cause and correction |
| --- | --- | --- |
| `outside_check::a_concave_outline_checks_whole_segments_not_just_their_endpoints` | b | Draw-line fixtures used `p1`/`p2`; use `a`/`b`. Geometry assertions unchanged. |
| `outside_check::comparison_does_not_allocate_cross_group_lines_or_double_negate_deductions` | b | Same incorrect draw-line fields; assertions unchanged. |
| `outside_check::geometry_facts_include_partial_outside_dangling_no_opening_and_overlap_area` | b | Same incorrect draw-line fields; assertions unchanged. |
| `outside_check::paging_keeps_every_handle_and_totals_and_takeoff_check_keeps_existing_fields` | b | Same incorrect draw-line fields; assertions unchanged. |
| `outside_check::raw_recount_is_per_sheet_and_reports_lost_boq_source_handles` | b | Same incorrect draw-line fields; assertions unchanged. |
| `arrange::a_new_schedule_uses_the_reserved_edge_without_arrange_and_zones_survive_save` | b | Plain `table` requires `at`. Create a real retained room and invoke `room_schedule` after reopening, exercising the intended automatic placement API. Retain the assertion that all table geometry lies in the saved schedule zone. |
| `arrange::arranging_a_split_view_keeps_its_match_lines_attached_and_key_plan_fixed` | a | A grouped viewport has no rendered model ink in this fixture. The bounds fallback required a singleton part, although match lines now share the viewport's part. Find the part containing the selected handle, then retain group expansion and fixed key-plan assertions. |
| `goal_a::space_check_uses_the_same_layer_policy_for_area_and_polygon_facts` | b | The error is in the preceding draw commands, which do not accept `layer`, not in `space_check`. Set draw layers through `batch.layer`. `FloorQuery` already accepts and forwards `boundary_layers`/`exclude_layers` for room and polygon reads as specified in [local-room-detection](2026-10-29-local-room-detection.md). Both area and perimeter assertions remain. |
| `honest_commands::unchanged_table_cell_keeps_its_display_handles_and_history` | a | `mark_logical` replaced the new authorship record even on an unchanged cell edit. Preserve an existing logical-text record, allowing table equality and no-op history detection to work. History, last-change and entity/handle assertions remain. |
| `library_adapters::wrapping_preserves_ascii_advances_and_nbsp_glue` | a | The new range-based bidi wrapper retained leading ASCII spaces. Begin wrapping after those spaces while retaining byte offsets, interior/trailing advances and NBSP glue. Assertions unchanged. |
| `drafting_eval::villa_ground_floor_through_mizans_three_tools` | c | [drawing-craft-sheets](2026-10-29-drawing-craft-sheets.md), item 4, adds bilingual field/revision captions and review status. Assert the exact expanded visible-text set, still excluding invented metadata values. |
| `goal_j::draw_log_replay_applies_rebuilds_and_reports_cheaper_edits` | c | [agent-loop-without-steering](2026-10-29-agent-loop-without-steering.md), item 12, removes prescribed edit strategies. Assert factual repeated-rebuild advice, nonempty evidence handles, removal counts and absence of prescribed edit commands. Drawing-result assertions remain. |
| `prompt::tests::tool_recipes_live_in_tool_and_command_docs` | c | The recipe is present. [drawing-craft-sheets](2026-10-29-drawing-craft-sheets.md), item 3, changed its wording to keeping plans, notes and schedules in their zones. Assert that actual recipe in registry documentation; the [mizan-instructions](2026-10-29-mizan-instructions.md) requirement that recipes live in tool/command docs remains. |
| `surface::named_method_state_digest_and_kit_request_authored_rules_without_bundling_them` | a | Longer method guidance and bilingual exclusion prose exceeded the compact kit budget. Shorten the method guidance without removing source/citation, no-invented-clause, ask-for-text, geometric-rule or persistence requirements. Remove redundant `self_check.required: []` and shorten bilingual exclusion prose. Keep the 6,144-byte ceiling. |
| `surface::takeoff_kit_binds_drawing_names_runs_and_fits_one_read` | a | Same compact kit regression and fix. Required command fields, enum contracts, runnable examples and phase bindings remain intact. |
| `surface::goal_d_interrupted_quantity_work_reports_in_the_chat_language` | c + a | [agent-loop-without-steering](2026-10-29-agent-loop-without-steering.md), items 6 and 8, means failed calls no longer interrupt the turn and app reports belong in `Status`. Simulate a real provider budget limit. Fix the overlooked `report_interrupted` emitter to use bilingual status. Assert Arabic status, the real stop reason, no English report leakage and no app summary in assistant text or model history. |
| `w3::the_villa_request_in_arabic_gives_a_clean_client_package` | a | The schedule zone double-counted gaps around a reserved top strip, leaving 30 mm before placement clearance. Use the full strip between the plan field and frame margin, yielding 50 mm with placement still enforcing clearance. Preserve plan zones, scales and designer-supplied zones. Existing villa and sheet-placement assertions remain. |
| `stdio_client_uses_shared_tools_instructions_finish_and_eof` | c | [agent-loop-without-steering](2026-10-29-agent-loop-without-steering.md), item 12, and [agent-approval-corrections](2026-10-29-agent-approval-corrections.md), item 3, replace prescribed deliverable wording with request scope, optional quantity guidance and model choice. Assert those instructions and retain the registry suffix, tool contract, finish and EOF checks. `support/fake_agent.py` already asserts the current no-invented-clause guidance and needed no edit. |

## Files changed by this repair

- `crates/qiyas-core/src/{arrange,sheet_setup,text}.rs`
- `crates/qiyas-core/tests/{arrange,goal_a,outside_check}.rs`
- `crates/qiyas-render/src/text.rs`
- `crates/qiyas-ai/src/{agent,prompt,surface}.rs`
- `crates/qiyas-ai/tests/{drafting_eval,goal_j,surface}.rs`
- `crates/qiyas-cli/tests/agent_eval.rs`
- This report.

Existing parallel work was preserved. `CLAUDE.md` was not edited. No dependency, commit, index staging, browser, window or paid service was used.

## Verification

All cargo commands use `CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=3`, run sequentially. No whole-workspace test run was made.

| Command (after the environment prefix) | Actual result |
| --- | --- |
| `cargo test -p qiyas-core --test all goal_a::space_check_uses_the_same_layer_policy -- --nocapture` | PASS: 1 passed. |
| `cargo test -p qiyas-core --test all -- outside_check:: arrange:: goal_a:: honest_commands:: library_adapters:: w3_output:: layouts:: registry::` | First run: 121 passed, 4 failed in registry tests. All 10 originally failing core tests passed. An attempted kit compression incorrectly omitted required `trade`; it was restored before subsequent checks. |
| `cargo test -p qiyas-ai --test all -- surface:: drafting_eval::villa_ground_floor_through_mizans_three_tools goal_j:: w3::` | First run: 95 passed, 1 failed. All selected tests except the villa passed. A first 40 mm schedule-zone correction remained too small; the correction was expanded to the full 50 mm reserved strip. |
| `cargo test -p qiyas-ai --test all w3::the_villa_request_in_arabic_gives_a_clean_client_package` | PASS: 1 passed with the final 50 mm strip; the normal villa package completes without changing its assertions. |
| `cargo test -p qiyas-core --test all -- outside_check:: arrange:: goal_a:: honest_commands:: library_adapters:: w3_output:: layouts:: registry::` | Final run: PASS, 125 passed, 0 failed. Includes all originally failing core tests, complete registry examples, sheet scales, saved zones, blank title metadata and retained disputed BOQ readings. |
| `cargo test -p qiyas-ai -p qiyas-core -p qiyas-render --lib -- prompt text::tests:: sheet_setup::tests::` | PASS: AI 22, core 7, render 8. Includes the originally failing prompt recipe test. |
| `cargo test -p qiyas-ai -p qiyas-core -p qiyas-render --lib -- text::` | PASS: AI 3, core 5, render 12. The first filter omitted `annotation_bidi_tests`; this run includes its four new wrapping/direction/source-cluster regressions. |
| `cargo test -p qiyas-cli --test agent_eval` | PASS: 8 passed, 0 failed, including stdio instructions, finish/EOF, selected method and Python fake-agent coverage. |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS: exit 0, no warnings. |
| `cargo fmt --all` | PASS: exit 0. Formatting changed only six already-listed repair files: CLI `tests/agent_eval.rs`; AI `tests/{surface,goal_j,drafting_eval}.rs` and `src/prompt.rs`; core `tests/arrange.rs`. |

`git diff --check` also passed (exit 0).

The final base kit measures 6,135 bytes; the named-method kit also passes the unchanged 6,144-byte assertion. No byte ceiling was raised. No required command field was removed.

## Open items

No known unresolved issue remains from the 18 failures. The complete workspace test suite was not run, as requested. Frontend files were unchanged and frontend tests were not rerun. The report above records both unsuccessful intermediate runs and their successful follow-up checks; it does not imply a new whole-workspace test result.
