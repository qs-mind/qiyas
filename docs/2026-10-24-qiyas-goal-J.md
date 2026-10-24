# Goal J — Mizan works step by step (24 October 2026)

Worktree: `.local/wt/goal-j`, branch `goal/j`. No provider, panel or table UI changes,
new dependencies, paid calls, commits or pushes.

## What changed

| Item | Before | After |
| --- | --- | --- |
| J1: rebuilding | The recorded rectangle → undo → walls → undo sequence repeatedly replaced the same work. Texts and dimensions were also deleted and redrawn. | A shared agent/MCP guard permits the first correction, then refuses another removal of the replacement in that phase. It gives the handles and suitable editing commands. Plans require at least two phases; both prompts require one creation batch per phase and capture/check between phases. |
| J1: editing | A dimension label centred on a grid axis could defeat automatic placement, even when clear space existed beside the axis. | Placement first keeps a clear manual text position, then tries small sideways text adjustments. Parallel dimension chains can share extension lines without being forced to the opposite side. The dimension handle and measured endpoints stay fixed. Existing move, spacing and text-edit paths are tested in mm, cm, m and inches. |
| J2 | A fresh design could be added beside the existing plan without asking. | On a populated drawing, an unspecified destination gets one short new-drawing/current-drawing question. The provider is not called until the answer specifies a destination. |
| J3 | An earlier Arabic greeting could dominate later English replies; uppercase English and Arabic digits had detection edge cases. | Each turn supplies language guidance from the latest prose. Arabic defaults to simple Modern Standard Arabic; dialect follows only current dialectal input. Numeric replies keep the current language. |
| J4 | Harmless variations in input shape failed. | Single-condition `where`, one `handles` entry, missing execute `op` with one command, query corners and list-query limits work with named warnings. Ambiguous alternatives remain errors. |
| J5 | The rooms error did not identify usable text layers. Typed quantity notes were accepted with quantity zero. | The error lists candidate layers and counts inside closed regions. AI/MCP quantity notes require a citation to an existing valid measured take-off row, or get guidance to measure first. |
| J6 | An unsupported method version gave no supported value; the kit example omitted it. | The error names version 1 and examples include `version: 1`. |

The fixed prompts are 877 English words and 868 Arabic words, both below 900. Arabic drawing text continues to use the existing
Thmanyah font configuration; this task changes no fonts or UI styles.

## Before evidence

Read the founder's `live-2026-10-24/draw.log` and all four supplied screenshots. The extracted
call fixture is `crates/qiyas-ai/tests/fixtures/goal_j_draw_calls.json`.

All paths below are relative to this worktree. The baseline executable and its SHA-256 are
saved in `.local/goal-run/before/qiyas-cli` and `before/build.json`.

- `.local/goal-run/before/draw-log/transcript.jsonl`: stdlib MCP replay, with readback after
  every mutation. Its top-level entity churn was **241** (added plus removed).
- `.local/goal-run/before/scripted/scripted-events.json`, `scripted-requests.json` and
  `scripted-churn.json`: actual agent loop with a scripted provider. The recorded wall
  sequence plus two labels and one dimension produced **226** additions/removals.
- `.local/goal-run/baseline-scripted.log`: all five new agent/surface regressions failed on
  unchanged production code, as expected. The separate core baseline build was cancelled
  before running tests; the MCP baseline supplies the core before evidence.
- `.local/goal-run/before/inputs/transcript.jsonl`: rejected harmless inputs, unhelpful room
  and version errors, and accepted uncited quantity text.
- `.local/goal-run/before/founder-areas-dimensions/` and `before/founder-doors-walls/`:
  replays of the founder's other two requests on `samples/test-plan.dwg`.
- `.local/goal-run/probe3/flat-m/0073-failed-place-overall-dimension-0.png`: the dimension
  placement failure discovered during the phased flat acceptance run.

Baseline captures were opened and inspected, including `before/draw-log/0057-replay-final-0.png`,
`before/scripted/scripted-20.png`, `scripted-33.png`, the input probe, and both founder-request
captures. The sample's original dimension style was too small to read. The final replay sets
an explicit annotation scale, as the prompts require.

## Acceptance and checks

| Replay | Before churn | After churn | Reduction |
| --- | ---: | ---: | ---: |
| Exact scripted provider through the agent loop | 226 | 83 | 63.3% |
| MCP client following the refusal | 241 | 92 | 61.8% |

Churn counts top-level entity handles added plus removed after each turn's mutation. It does
not count replacement primitives inside anonymous dimension blocks. The two replays use
slightly different text/dimension counts, so compare each row with itself. The guard's
refusals have zero mutation and preserve the drawing revision. The benchmark deliberately
repeats bad calls; its captures are evidence of refusal, not a finished design.

After evidence:

- `.local/goal-run/after/draw-log/transcript.jsonl`, `churn.json` and
  `0100-replay-final-0.png`.
- `.local/goal-run/after/scripted/`: actual agent events, provider requests, language
  conversation, two captures and churn. The conversation includes “هاي”, English, MSA,
  and English again. Tests also cover uppercase English and neutral Arabic digits.
- `.local/goal-run/after/inputs/transcript.jsonl`: input warnings, candidate layer count,
  supported version, refused uncited note and accepted Arabic note citing the 20 m² row.
- `.local/goal-run/after/founder-areas-dimensions/0033-after-0.png`: measured candidate
  areas 51.211417, 22.085 and 17.385 m², and readable 12 m / 9 m dimensions.
  These are candidates, not approved room meanings. The living candidate deducts the
  enclosed source circle; the raw room outline alone is 58.29 m².
- `.local/goal-run/after/founder-doors-walls/`: source records, measured rows, transcript
  and `0042-after-0.png`. Readback asserts **3 doors** and **62.283185307 m** of wall paths,
  including the source arc. The source sample's existing drawing findings remain visible.

The stdlib clients are `.local/goal-j/mcp_client.py`, `replay.py`, `flat.py` and `requests.py`.
They invoke this worktree's `target/debug/qiyas-cli mcp --drawing … --workdir .local/goal-run/…`.
Every mutation is followed by readback. All three clients were rerun successfully with
the final executable (`replay-verified.log`, `flat-verified.log`, `requests-verified.log`). Flat churn snapshots include all sheets so changing
the active sheet cannot be mistaken for deleting its entities.

Both 8 × 7 m flat jobs finished: `.local/goal-run/after/flat-m/` and `flat-mm/`.
Each contains `transcript.jsonl`, `churn.json`, wall readback and six PNGs. Every PNG was
opened and inspected. The final rerun reproduces all twelve flat PNGs byte for byte
(`after/capture-final-comparison.json`); both final sheets were opened again.
The metre and millimetre plans match physically; their dimension text
correctly reads 4 m / 8 m and 4000 mm / 8000 mm respectively.

| Phase | Deliberate mistake and correction | Capture in each flat directory |
| --- | --- | --- |
| Grid | D renamed to B with `edit_text` | `0035-01-grid-0.png` |
| Walls | Partition width 0.18 → 0.15 m with `set_object_props` | `0054-02-walls-0.png` |
| Openings | Entrance swing corrected with `edit_wall_opening` | `0073-03-openings-0.png` |
| Rooms | Bedrom → Bedroom; move and automatic placement preserve handles | `0106-04-rooms-0.png` |
| Dimensions | Tight spacing corrected with `dim_space`, `dim_edit` and `auto_place` | `0139-05-dimensions-0.png` |
| Sheet | Two-bedrom → Two-bedroom with `edit_text` | `0179-06-sheet-0.png` |

Each flat has total churn **78**, including removal of the one-point blank fixture.
Every correction and placement has **zero** additions/removals. Captures show connected
walls, inward door swings, readable room names, external dimension chains and the corrected
sheet title. `.local/goal-run/after/flat-review.json` records the checks and visual review.
Phases 1–5 report complete checks with zero errors and warnings. The sheet also has zero
errors/warnings, but its default viewport record 461 is outside checker coverage
(`complete: false`); this is disclosed rather than called a complete automated check.

| Check | Result and saved log |
| --- | --- |
| `cargo fmt --all`, then final format check | Passed; `fmt.log`, `fmt-final-check.log` |
| `cargo build --offline -p qiyas-cli` | Passed; `build-verified.log`; binary hash in `after/build-final.json` |
| Workspace Clippy, all targets, warnings denied | Passed; `clippy-complete.log` |
| Core/AI full tests | Full run plus focused retests, detailed below |
| Explicit AI lib/surface/agent tests | 113 library passes, 2 socket failures, 12 ignored; 69 surface passes; 3 agent passes, 30 socket failures; `ai-final-tests.log` |
| Final Goal J / recognition / registry / surface tests | Passed, 103 tests: AI Goal J 6, surface 69, core Goal J 6, recognition 9, registry 13; `verified-tests.log` |
| Frontend TypeScript/Vitest | Not applicable: no frontend files changed |
| `git diff --check` | Passed |

All cargo commands used the requested worktree target, two build jobs, no debug information,
no incremental compilation and offline mode. Initial lint failures and the later moved-value
build error were corrected; no warnings were suppressed. The helper was moved before its
test module to satisfy Clippy. Test retries followed those real failures or subsequent fixes.

The full core/AI test command ran all targets with `--no-fail-fast`.
The original failures and retries are retained, not overwritten:
`.local/goal-run/final-tests.log`, `ai-final-tests.log`, `comparison-retest.log` and
`verified-tests.log`. The separate core-only retest was cancelled during compilation
to avoid rebuilding the same dependencies under a different feature set; its tests were
combined with the final AI tests. The original full run had 3 core failures
and 45 AI failures, with 13 AI tests ignored. Of the 48 failures, 42 were
sandbox-denied local socket binds; six were fixed behavior/test-contract regressions.
The comparison retest passes all 24 tool tests, all 4 discrepancy tests and the
10-question take-off evaluation (1 test; the private-reference test remains ignored).
The final 103-test run passes all previously failing recognition tests and the new
quantity-note cases. All six non-sandbox failures from the full run are resolved by
these retests. No full-suite green result is claimed while the 42 socket tests cannot run.

## Practical limits

These are deterministic offline providers and MCP clients. They exercise the real agent,
command, drawing and rendering code, but do not measure the quality of a live model's prose.
No paid provider was called. Existing literal prompt assertions were updated for the new
wording and the deliberate one-correction allowance. Phase-event tests now supply two phases;
drawing-scope tests specify a destination, with separate tests enforcing the question/wait.
The list-query contract now explicitly accepts layer offset and limit, which are supported inputs. No existing assertion was weakened to accept a failed edit.

The full test run found two further regressions: normalization crossed into a different
schedule-input variant, and the typed-note check blocked engine-generated discrepancy
comments. Both were fixed in production code. The existing recognition, comparison and
discrepancy assertions were retained. Generated discrepancy comments use the engine
query’s source references and remain one undo step; typed AI/MCP notes still need citations.

The churn guard compares command families and overlapping bounds;
it is a repeated-rebuild check, not a general duplicate-geometry detector.

The original wall engine can cancel some sequential single-opening additions because its
computed geometry changes during validation. The phased acceptance client groups each wall's
openings into the existing supported array argument. The original cancellation is retained in
`.local/goal-run/before-flat.log`; no geometry-validation rule was relaxed.

## Files touched

- `CLAUDE.md`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/churn.rs`
- `crates/qiyas-ai/src/judgement.rs`
- `crates/qiyas-ai/src/lib.rs`
- `crates/qiyas-ai/src/mcp.rs`
- `crates/qiyas-ai/src/normalise.rs`
- `crates/qiyas-ai/src/prompt.rs`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/prompt_en.txt`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/surface_args.rs`
- `crates/qiyas-ai/tests/fixtures/goal_j_draw_calls.json`
- `crates/qiyas-ai/tests/goal_j.rs`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/dimension.rs`
- `crates/qiyas-core/src/discrepancy/mod.rs`
- `crates/qiyas-core/src/input.rs`
- `crates/qiyas-core/src/lib.rs`
- `crates/qiyas-core/src/measurement.rs`
- `crates/qiyas-core/src/quantity_notes.rs`
- `crates/qiyas-core/src/registry.rs`
- `crates/qiyas-core/src/rooms.rs`
- `crates/qiyas-core/src/transaction.rs`
- `crates/qiyas-core/src/undo.rs`
- `crates/qiyas-core/src/validation.rs`
- `crates/qiyas-core/tests/goal_j.rs`
- `docs/2026-10-24-qiyas-goal-J.md`

## Tests blocked by the sandbox

These 42 tests failed at local socket binding with `PermissionDenied` /
`Operation not permitted`; they were not skipped or reported as passing.
The explicit AI rerun confirms the same 2 library and 30 agent transport failures.
The stdio MCP jobs do not need a local port and all acceptance jobs run that way.

- `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
- `claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo`
- `an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room`
- `a_selection_is_listed_and_shown`
- `a_marked_region_lists_what_is_in_it`
- `attached_pictures_go_before_the_words`
- `all_server_failures_retry_with_a_visible_wait_and_identical_request`
- `api_key_absent_from_logs_and_errors`
- `an_answer_that_keeps_running_out_stops_at_the_cap`
- `billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report`
- `a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user`
- `a_question_ends_the_turn_cleanly_and_the_answer_carries_on`
- `cancel_between_tool_calls_runs_no_more_of_them`
- `cancel_stops_within_one_second`
- `file_tools_without_a_host_say_to_use_the_app`
- `invalid_tool_json_is_answered_with_an_error_and_not_run`
- `history_is_append_only_and_system_prompt_is_stable`
- `invalid_output_value_keeps_the_cap_and_does_not_retry`
- `loop_stops_after_40_tool_calls`
- `openai_compatible_drops_a_refused_field_and_asks_again`
- `rejected_request_after_find_entities_is_logged_without_retry`
- `openai_compatible_streams_text_and_tool_calls`
- `partial_usage_chunks_retain_prior_counts_through_the_http_adapter`
- `refusal_stop_reason_surfaces_category`
- `malformed_tool_input_still_stops_after_40_calls`
- `tool_use_round_trip_executes_against_the_session`
- `text_stream_is_forwarded_in_order`
- `mid_stream_overload_discards_partial_text_and_keeps_a_retry_log`
- `old_pictures_give_way_to_a_line`
- `gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn`
- `view_image_stops_when_the_image_budget_is_used`
- `removals_and_resolutions_stop_after_10_in_one_turn`
- `network_tests::model_discovery_never_follows_a_redirect_with_credentials`
- `execute_request_ids_replay_without_another_mutation_or_ui_event`
- `lists_the_catalogue_and_measures_as_mcp`
- `a_wrong_token_is_refused_with_401`
- `sheet_layer_and_zoom_changes_reach_the_window`
- `other_origins_are_refused_even_with_the_token`
- `concurrent_counts_both_land`
- `view_image_is_an_image_block`
- `a_restart_on_the_same_port_binds_again`
- `tool_errors_are_error_results`
