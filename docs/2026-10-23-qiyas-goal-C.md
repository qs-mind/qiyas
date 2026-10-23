# Goal C: reliable inputs and edit speed

Worktree: `.local/wt/goal-c`, branch `goal/c`. No commit or push. All evidence paths below
are relative to this worktree. No new dependencies, rates, paid calls, frontend changes,
font changes or changes to the founder's checkout.

Implementation and final headless acceptance are complete. Format, workspace clippy and all
798 core tests pass. The AI suite has 189 passes, 36 local-port failures caused by the sandbox,
and one ignored private-drawing test. There are no other test failures.

| Approved item | Result |
| --- | --- |
| C11, M4, X-B8: records | Projections and conditions accept summary names such as `layer`, `length`, `area` and `text`, alongside raw data pointers. `in` and `not_in` require arrays. Malformed or unknown paths return useful errors. Missing properties on mixed records are omitted with warnings; an explicit stored null remains null. Empty scopes say paths could not be checked. `exists` can test absence. Ordered comparisons reject unlike types. |
| X-A1, C22: text queries | `bounds` aliases `within`; `layer` filters effective layers, including block inheritance and external drawing prefixes. Filtering happens before paging. |
| C20–C22: shortcuts | Several layer properties can be changed atomically. Named colors and layer color indices work. `delete_entities {handles}` uses Goal B's handle normalization and protection rules. Run accepts temporary layer/color through the existing batch context, creates a missing layer, and restores the previous drawing properties. Conflicting input forms are refused. |
| M5, M6: validation and schemas | Scalar enum branches report all choices. Codec enum variants are explicitly traced; the test requires a complete registry without the unchecked fallback. Dimension paths include their variant and base, matching actual record data. |
| X-C6, C28: discovery and limits | `commands` discovers operation schemas, including query and finish. Qualified names distinguish overlapping operations. Documentation states 20/200 records and a 12,000-byte record-and-warning budget, excluding the page envelope and retaining the single oversized-record exception. Warnings from a record deferred to the next page do not leak into the current page. |
| M8: speed and change reports | Edits refresh their dependencies. The matched four-layer benchmark improved from a median **17.473 seconds to 0.0457 seconds**, about **383 times faster**. All three optimized batches report empty entity change lists and preserve every checked entity/object record and schedule. |

Derivation compares immutable entity snapshots and source data from before the edit. It
follows object hosts, joined assemblies and block references; enclosures on affected sheets
and layers; selected schedules; measurement rules; and linked fields. Joined assemblies and
shared definition/lookup changes use conservative invalidation. There is no persistent cache
that can become stale after undo or rollback. Goal B's final-effect checks remain in place.

Opening a drawing establishes derived state once, including drawings containing only linked
fields. Reloading a linked drawing also refreshes its derived quantities immediately, outside
command undo history. On a damaged drawing, the original Goal A room-validity refresh still
runs if a full refresh fails. Generated label defaults use the driver's own sheet, so changing
the viewed sheet cannot resize unrelated labels.

The exact comparison caught a new measured-room row keeping a different explanatory note
from full refresh. New or edited take-off rows now refresh retained-region metadata without
retracing unchanged enclosures. Numeric values, validity, notes and generated geometry are
all included in the equality check.

The existing color parser test used to require `red` to be rejected. It now requires ACI 1
and still rejects an unknown color because named colors are an intentional new shortcut.
Three existing integration tests now use `not-a-color` for their refusal probes; their
rollback and history assertions remain intact. An imported-region metadata test now makes
a boundary-source edit instead of creating an unrelated layer to trigger a retrace. Its
metadata and invalid-quantity assertions remain intact.

The full suite also exposed a real wall-copy regression: legacy migration replaced part
indices in an untouched wall group while editing a copy. Migration now preserves the role
and index of parts already converted for the same owner. The existing wall-copy test was
left unchanged, and a new four-unit test compares every edit with full derivation.

The realistic timing fixture contains six wall runs, five openings, three rooms, grids,
a dimension and three live schedules: **153 stored entities**, including block contents,
with 60 model-space entities and 14 objects. Its room areas are 22.18, 10.81 and 10.81 m²,
totaling 43.80 m². It was built through the baseline CLI and read back before timing.

| Timing evidence | Four-layer times, seconds | Median |
| --- | --- | --- |
| Founder audit M8 | 27.3 on its original drawing | — |
| Earlier fresh-fixture run during parallel builds, `before-fresh/speed.json` | 29.397, 34.476, 39.278 | 34.476 |
| Final matched baseline, `before-final/speed.json` | 15.693, 17.473, 29.652 | 17.473 |
| Final optimized run, `after-final/speed.json` | 0.0857, 0.0457, 0.0438 | 0.0457 |

The final pair uses the same DXF and Python client, with three batches of four new layers.
Only the mutation call is timed; layer readback, complete model-space/object pagination,
schedule comparisons and captures follow it. These are debug builds on a shared laptop;
background load explains variation between runs. Wall-group edits and shared-definition
changes still refresh their dependents conservatively.

Startup is reported separately: initialization took 3.583 seconds before and 3.852 seconds
after. Initial normalization therefore adds some opening work. The old first layer batch
reported six delayed entity changes; the optimized run reports none, including its first batch.
The final before/after apartment PNGs are byte-identical, SHA-256
`9beb3cdaec4a226088d0fd2396d76483cd5b09198a0040f3c833e8e534b2116f`.

The baseline executable was built after the input changes and before the speed changes.
Both CLI executables were built in the required shared target and copied unchanged into
`.local/goal-run/bin/` so another agent's build cannot change them during a run. The final
executable hash is in `.local/goal-run/cli-after.sha256`; each rerun records its binary hash
in `.local/goal-run/final-evidence-results.json`. Timing data is consolidated in
`.local/goal-run/timing-summary.json`.

The seven exact-derivation tests compare the resulting entities, exact handles, document
objects, layers, blocks and take-off against an independent full refresh. They cover:

- Walls, openings, retained rooms, live schedules, linked fields, layer creation and rename,
  failed-batch rollback, undo/redo and DXF/DWG reopening in mm/cm/m/in.
- Independent objects, selected schedules, lookup tables, levels and measurement rules.
  Untouched generated parts must retain the same shared entity storage.
- Linked geometry fields, source deletion/restoration and stale saved fields in all four units.
- Enclosure deletion and quantity invalidation, and external-boundary reloads in all four units.
- Generated label sizes when switching between model and paper space in all four units.
- Independent copied walls, preserving source part metadata while editing a copy in all four units.

All seven passed on the final implementation in `.local/goal-run/core-regressions.log`,
alongside the unchanged wall-copy test and the corrected input fixtures: 126 tests passed,
zero failed.
Earlier fixture mistakes were corrected: room labels alone do not create take-off rows,
legacy wall height uses `wall_height`, and opening removal uses `remove_wall_openings`.
Unsupported partial wall deletion remains a refusal with complete rollback.

| End-to-end evidence | Actual result |
| --- | --- |
| `.local/goal-run/verified-{mm,cm,m,in}/transcript.jsonl` | All four units pass, 72 exchanges each. Every mutation has entity/object/layer readback plus assertions of its intended effect. |
| `.local/goal-run/derived-{mm,cm,m,in}/transcript.jsonl` and `derived-e2e-summary.json` | All four units pass, 64 exchanges each: area 12 → 48 m², perimeter field 14 → 28 m, stable table/field handles, unchanged quantities after four layers, and invalid quantity/schedule/field after source deletion. |
| `.local/goal-run/copy-after-fixed/transcript.jsonl` | 22 exchanges, passes. Editing one copied wall adds its window and preserves the source and other copy exactly; `differences.json` is empty. |
| `.local/goal-run/verified-summary.json` | The same 4 m line reads as 4000 mm, 400 cm, 4 m and 157.48031496 in. Projections, enum edits, membership, shortcuts and text filters pass. |
| `.local/goal-run/{before-final,after-final}/transcript.jsonl` | 34 exchanges per timing job. The optimized run preserves all 60 model-space records, 14 object records and three schedules after each four-layer batch. |
| `.local/audit/core-more-probes/transcript.jsonl` | Original audit replay, 44 exchanges. The malformed `common/layer` projection is an explicit error. Goal B's live-cell/row protections remain intact. |
| `.local/audit/flat/transcript.jsonl` | Original audit replay, 147 exchanges. Text bounds/layer filters work; invalid property names are explicit, mixed missing properties have warnings, and the opening batch remains successful. |
| `.local/goal-run/replayed-summary.json` | All seven Goal C replay assertions pass. The original broad scripts also contain intentional failure probes and findings owned by Goals D/G; this is not a claim that every broad-audit finding is fixed. |
| `.local/goal-run/evidence-scan.json` | The dedicated input, quantity and optimized timing jobs contain 87 successful edits, zero successful no-op edits, and no failed effect assertions. The broader audit's deliberate repetitions are separate. |

Saved and visually inspected images:

- `.local/goal-run/copy-after-fixed/0022-independent-wall-copies-0.png`: the middle copy
  has the added window; the source and other copy retain their original openings.

- `.local/goal-run/before-final/0034-drawing-after-0.png` and
  `.local/goal-run/after-final/0034-drawing-after-0.png`: identical apartment geometry,
  quantities and populated schedules. Existing room-label/grid overlaps remain Goal G work.
- `.local/goal-run/verified-mm/0072-english-arabic-input-results-0.png` and the matching
  `verified-in` image: readable English and Arabic, red Notes text, another layer and the
  4 m line. The centimetre/metre images are byte-identical to the inches image.
- `.local/goal-run/derived-mm/0036-before-resize-0.png`, `0054-after-resize-0.png` and
  `0064-after-boundary-removal-0.png`: the room changes from 12 to 48 m², then its quantities
  and linked field show `####` after source deletion.
- `.local/audit/flat/0128-flat-annotated-0.png`: the original audit job retains D1/W2/etc.,
  populated schedules and 43.80 m² total; the pre-existing annotation overlaps remain.
- `.local/audit/core-more-probes/0044-schedule-closeup-0.png`: the selected schedule still
  shows “Legacy wall”, count 1.000, after refused direct edits.

| Required check | Actual result |
| --- | --- |
| `cargo fmt --all` | Passed on the final implementation, exit 0; `fmt-final.log`. |
| `cargo build --offline -p qiyas-cli` | Passed, exit 0; `build-final.log`, `build-final.exit`. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed, exit 0; `clippy-final.log`. |
| Targeted core regressions | Passed: 126 tests, zero failures; `core-regressions.log`. Includes all seven exact-derivation comparisons. |
| Full `qiyas-core` tests | Passed: 798 tests, zero failures, exit 0; `core-final.log`. Counts exclude duplicate output from nested child tests. |
| Full `qiyas-ai` tests | 189 passed, 36 failed because local-port binds were denied, 1 ignored; exit 101, `ai-final.log`. All 58 surface tests passed. |
| Required AI `--lib --test surface --test agent` | 135 passed, 26 failed because local-port binds were denied; exit 101, `ai-required-final.log`. These failures are a subset of the same 36. |
| Four-unit input and quantity MCP jobs, matched benchmark, two audit replays | Passed, exit 0; the corresponding `.exit` files and assertion logs are under `.local/goal-run/`. |
| `git diff --check` | Passed. |
| Frontend TypeScript/Vitest | Not needed: no frontend files changed. |

Cargo uses the founder's shared target, offline dependencies, no debug information and no
incremental compilation. Earlier runs used the original ten-job instruction; every new build
after resumption uses four jobs. The final test runner also limits test threads to four.
Build-lock waits are left to finish. No other agent’s build was interrupted and no `cargo clean`
was run. The interrupted regression run was restarted after resumption.
Final commands, exit codes and elapsed times are recorded in
`.local/goal-run/final-check-results.json`.

Files changed (27 review files):

- `crates/qiyas-core/src/`: `commands.rs`, `derived.rs`, `derived_tests.rs`, `draw.rs`,
  `field.rs`, `layers.rs`, `object_migration.rs`, `object_regions.rs`, `object_schedules.rs`, `objects.rs`,
  `query.rs`, `record_edit.rs`, `records.rs`, `registry.rs`, `session.rs`, `validation.rs`.
- `crates/qiyas-core/tests/`: `goal_c_inputs.rs`, `draw.rs`, `layers.rs`,
  `object_regions.rs`, `properties.rs`.
- `crates/qiyas-ai/src/execute.rs`, `surface.rs`, `surface_args.rs`, and
  `crates/qiyas-ai/tests/surface.rs`.
- `CLAUDE.md` and this report. The exact list is also in
  `.local/goal-run/changed-files.txt`.
- Reproducible local clients, timing jobs, assertions and the check runner are in
  `.local/audit/goal_c_*.py`, alongside copies of the original audit clients.

Remaining verification and limits: 36 AI tests need an environment that permits local-port
binds. They fail during test-server setup with `PermissionDenied` / `Operation not permitted`,
before exercising their network assertions. The ignored test,
`two_private_drawing_reference_answers`, requires private DWG files and
`QIYAS_REAL_EVAL_REFERENCES`. The full list of blocked tests follows and is also saved in
`.local/goal-run/port-blocked-tests.txt`; machine-readable totals are in
`.local/goal-run/test-results.json`. The founder's running app was not changed, and no live
provider or in-app chat was exercised; these headless runs use the real stdio MCP surface
without a model. Existing annotation overlaps in the broad audit remain Goal G work.
No Goal C implementation item or founder decision remains.


<details>
<summary>The 36 AI tests blocked by local-port restrictions</summary>

```text
tests/agent.rs: a_marked_region_lists_what_is_in_it
tests/agent.rs: a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user
tests/agent.rs: a_question_ends_the_turn_cleanly_and_the_answer_carries_on
tests/agent.rs: a_selection_is_listed_and_shown
tests/agent.rs: an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room
tests/agent.rs: an_answer_that_keeps_running_out_stops_at_the_cap
tests/agent.rs: api_key_absent_from_logs_and_errors
tests/agent.rs: attached_pictures_go_before_the_words
tests/agent.rs: cancel_between_tool_calls_runs_no_more_of_them
tests/agent.rs: cancel_stops_within_one_second
tests/agent.rs: file_tools_without_a_host_say_to_use_the_app
tests/agent.rs: gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn
tests/agent.rs: history_is_append_only_and_system_prompt_is_stable
tests/agent.rs: invalid_tool_json_is_answered_with_an_error_and_not_run
tests/agent.rs: loop_stops_after_40_tool_calls
tests/agent.rs: malformed_tool_input_still_stops_after_40_calls
tests/agent.rs: old_pictures_give_way_to_a_line
tests/agent.rs: openai_compatible_drops_a_refused_field_and_asks_again
tests/agent.rs: openai_compatible_streams_text_and_tool_calls
tests/agent.rs: refusal_stop_reason_surfaces_category
tests/agent.rs: rejected_request_after_find_entities_is_logged_without_retry
tests/agent.rs: removals_and_resolutions_stop_after_10_in_one_turn
tests/agent.rs: text_stream_is_forwarded_in_order
tests/agent.rs: tool_use_round_trip_executes_against_the_session
tests/agent.rs: view_image_stops_when_the_image_budget_is_used
tests/key_safety.rs: network_tests::model_discovery_never_follows_a_redirect_with_credentials
tests/mcp.rs: a_restart_on_the_same_port_binds_again
tests/mcp.rs: a_wrong_token_is_refused_with_401
tests/mcp.rs: concurrent_counts_both_land
tests/mcp.rs: execute_request_ids_replay_without_another_mutation_or_ui_event
tests/mcp.rs: lists_the_catalogue_and_measures_as_mcp
tests/mcp.rs: other_origins_are_refused_even_with_the_token
tests/mcp.rs: sheet_layer_and_zoom_changes_reach_the_window
tests/mcp.rs: tool_errors_are_error_results
tests/mcp.rs: view_image_is_an_image_block
unittests src/lib.rs: models::tests::an_unreachable_service_falls_back_to_the_builtin_list
```

</details>
