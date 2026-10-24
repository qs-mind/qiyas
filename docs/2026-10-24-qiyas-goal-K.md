# Goal K: wall openings on the villa

Worktree `.local/wt/goal-k`, branch `goal/k`. No commit or push. No new dependencies,
provider requests, paid calls or credential access. Builds use the worktree target;
after the memory-guard interruption, the requested four-job environment and one
build at a time. The final desktop build needed a two-job command-line limit
when available RAM fell to 386 MB; its interrupted work was retained and resumed.
All evidence paths below are relative to this worktree unless stated otherwise.

1. What changed

**K1 — valid openings.** The wall command used to create a temporary complete wall
system while the old system was still in the drawing. Creating that temporary
system refreshed the old system too. Its saved member list could already be stale
when handle reuse began. The release diagnostic catches old members 2786 and
2807 disappearing during that temporary regeneration, before reuse. Reusing
handles and regenerating again then changed the
order of cuts, because entity storage uses swap removal. The verifier compared
these two generated shapes and correctly noticed they differed, but no user had
asked for those intermediate outlines or hatches.

An edit now creates only temporary source drivers, establishes surviving handles
and hosts, then lets the object engine generate shared outlines and hatches.
Cuts have a stable order by host and position. The object engine is the sole
owner of those graphics; Goal B's requested-field verifier is unchanged.

**K2 — speed.** Profiling exposed repeated whole-assembly unions, quantity prefix
unions and context evaluation for every member, compounded by full regeneration
for every opening command. Independent opening-only batches now validate in
input order and rebuild each affected assembly once. Whole-wall totals use one
union per assembly. Shared graphics are derived only by their owner. Explicit
assemblies avoid redundant vertical grouping, and a name-only measurement method
reports missing rules without calculating unused quantities. Computed quantity
expressions, symbols and labels still evaluate their required quantities.

Grouped step results describe the final shared wall state, with
`shared_wall_result: true` on the AI/MCP surface. Their change lists describe the
whole grouped edit; the batch's net lists remain authoritative. Mixed batches
and step references retain sequential execution. No villa or layer names occur
in the engine changes.

**K3 — rollback.** The missing-member guard remains. The stale-member source was
inside temporary regeneration, rather than evidence of surviving changed entity
records after a cancelled batch. Failure also now restores document structure,
including handle allocation and block membership. The release baseline proves
this gap: the first cancelled large batch leaves both `next_handle` and the
header handle seed at 968 instead of 273, although entity records match.
Four-unit tests compare every
entity record, structure, wall membership, take-off, history and revision before
and after ordinary failure and injected panic. Every wall member must exist.

**K4 — panic safety.** Document edits and transaction groups catch panics and
restore their changes before returning a plain error. AI tools and desktop
commands catch remaining panics before the session guard leaves its scope.
The baseline injected transaction panic escapes and leaves one drawn entity
with no undo history. Tests now inject a panic after a real mutation, check that the lock is available and
unpoisoned, then edit, undo and redo. The desktop test also saves the project and
checks a panic at the host boundary. The fault injector is thread-local and
available only under the existing test/fixtures configuration, with no tool input.

**Fixture.** `crates/qiyas-ai/tests/fixtures/villa-openings.json` contains 13
original drawing/opening inputs, with no model replies or credentials. The
normal CI test recreates the desktop handles and name-only POMI selection, adds
all 44 distinct requested openings, and checks that the logged retries refuse
only overlaps with fitting ranges. The full mm/cm/m/in replay is an explicit
ignored test; a release guard in `scripts/check.sh` uses the existing fixed CPU
reference. Both print per-call timings.

2. Files touched

- Core implementation: `crates/qiyas-core/src/{walls,objects,input,transaction,session,commands}.rs`
  and `crates/qiyas-core/src/measurement/evaluate.rs`.
- Core regression tests and fault injection: `crates/qiyas-core/src/{derived_tests,fixtures}.rs`
  and `crates/qiyas-core/tests/wall_rollback.rs`.
- AI boundary and replay: `crates/qiyas-ai/src/surface.rs`,
  `crates/qiyas-ai/tests/villa_openings.rs`, and the fixture JSON/Markdown under
  `crates/qiyas-ai/tests/fixtures/`.
- Desktop boundary and recovery test: `crates/qiyas-desktop/src/commands.rs`.
- Release guard: `scripts/check.sh`. Behaviour notes: `CLAUDE.md` and this report.
- Local evidence/client scripts stay under `.local/goal-run/` and `.local/audit/`.
  They are not intended as committed source.

3. Checks

All Cargo commands used the worktree target, the four-job environment, disabled dev/test
debug information and disabled incremental compilation after resuming. Final
checks ran serially with two test threads. The desktop dependency build was
interrupted before the memory guard when available RAM reached 386 MB, then
resumed with `--jobs 2` while retaining `CARGO_BUILD_JOBS=4` in the environment.
Its partial log is `check-desktop-interrupted.log`. No other process was stopped,
no target was cleaned, and no existing test was weakened.

| Check | Real result | Log under `.local/goal-run/` |
| --- | --- | --- |
| `cargo fmt --all` | Passed; final test edits also pass Rust 2021 formatting checks | `check-fmt.log`, `check-final-test-format.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed. Initial run found the release-only guard's constant assertion; its intentional debug rejection now has a documented function-level lint exception. No speed assertion was removed | `check-clippy.log`; initial diagnostic in `check-clippy-initial.log` |
| `cargo test --offline -p qiyas-core --no-fail-fast` | Passed: 821 tests, 0 failed | `check-core.log` |
| `cargo test --offline -p qiyas-ai --no-fail-fast` | 268 passed, 44 blocked by denied loopback sockets, 15 ignored | `check-ai.log` |
| Required AI `--lib --test surface --test agent --no-fail-fast` | 206 passed, 34 blocked by denied loopback sockets, 12 ignored; all 69 surface tests pass | `check-ai-required.log` |
| `cargo test --offline -p qiyas-desktop --no-fail-fast --jobs 2` | 131 passed, 2 blocked by denied loopback sockets; the new panic-recovery test passes | `check-desktop.log` |
| Debug villa replay and AI panic recovery | 2 passed, 2 deliberately ignored variants; final repeat after the lint correction also passed without warnings | `replay-after-final-debug.log`, `check-villa-final.log` |
| Full villa replay in mm, cm, m and inches | Passed, explicitly ran the ignored variant | `replay-after-four-units.log` |
| Release speed guard | Passed, explicitly ran the ignored release guard | `replay-after-release.log` |
| `cargo build --offline -p qiyas-cli` | Passed | `cli-after.log` |
| Headless metres, millimetres and saved-DXF reopening | Passed, 133 complete MCP exchanges including the before job | `headless-summary.json`, `mcp-after-m.log`, `mcp-after-mm.log`, `reopen-check.log` |
| Frontend TypeScript/Vitest | Not applicable: no root `src/` files changed | — |
| `git diff --check` | Passed | — |

The 44 AI failures all stop while creating a local listener, with OS error 1,
“Operation not permitted”; they do not reach their socket-dependent assertions.
They are listed below. The required AI subset repeats 34 of those same failures.
The new villa and panic tests use no sockets and pass. No permission escalation
or live provider call was attempted.

4. Before and after evidence

Baseline source and diagnostic instrumentation are saved in
`.local/goal-run/baseline-source/`. The baseline retains the reviewer's existing
missing-member guard. The original conversation and replay came from the
founder's `.local/audit/live-2026-10-24/`; they were only read.

The first full debug baseline ran during the earlier overlapping builds and
swapping, so its absolute times are not an isolated benchmark. Its profile
counts and false cancellations are reproducible. The release baseline completed
in 98.28 seconds and reproduced all four false cancellations; its largest opening
call took 42.4 seconds. The later baseline headless
run reproduces the same failure without a model: seven openings succeed, then
the 37-opening phase is cancelled and leaves seven. The screenshot was inspected.

The debug replay in `.local/goal-run/replay-after-final-debug.log` completed with
seven openings in 0.482 seconds, then 37 in 1.992 seconds. The ordinary replay
and AI panic test both pass. `.local/goal-run/replay-after-four-units.log` and
`after-four-unit-timings.json` record the full four-unit run: largest successful
batches take 2.009 seconds (mm), 2.005 (cm), 2.267 (m), and 2.082 (in).
All 44 openings survive each replay; every refused retry leaves the exact before
state. The final repeat after the test lint correction also passed: seven openings in
0.455 seconds and 37 in 2.495 seconds (`check-villa-final.log`). All measured
final-engine batches stayed below the three-second debug target. No speed
assertion is imposed on debug hardware; the required release regression guard
uses a CPU-relative threshold.

The baseline timing and count files are `before-debug-timings.json`,
`before-release-timings.json`, and `villa-comparison.json`, under `.local/goal-run/`.
The diagnostic logs are `replay-before-debug.log` and `replay-before-release.log`.
The first seven-opening call performs 360 band derivations and 16,019 context
evaluations. Before cancellation, the next batch performs nine wall rebuilds,
18 object refreshes, 3,617 band derivations and 236,206 context evaluations.
In release those context evaluations account for 14.49 seconds of inclusive
profile time; band derivations account for 5.38 seconds. Inclusive times overlap
and must not be added together. This is repeated engine work, not model latency.

| Opening phase | Before release | After debug | After release | After result |
| --- | ---: | ---: | ---: | --- |
| First 7 openings | 1.1 s | 0.482 s | 0.098 s | Added; read back |
| Next 37 openings | 28.0 s, cancelled at step 8 | 1.992 s | 0.394 s | Added; read back |
| Logged retry of 34 openings | 42.4 s, cancelled at step 13 | 1.138 s | 0.176 s | Correct overlap refusal at step 0 |
| Smaller logged retries | 1.3–10.8 s, mixed outcomes | 0.128–0.284 s | 0.021–0.045 s | Correct overlap refusals |

The release speed guard passed: 393.925 ms for the largest call against a
561.283 ms fixed CPU reference, ratio 0.7018 with a limit of 2. Every opening call
is below the one-second release target. Log: `.local/goal-run/replay-after-release.log`.
The first two phases establish the intended 44 openings, so later original
retries now genuinely overlap instead of rescuing previously cancelled additions.

The actual CLI jobs use `.local/goal-run/villa_mcp.py` and the copied stdlib
client `.local/audit/mcp_client.py`. The command is
`target/debug/qiyas-cli mcp --drawing .local/goal-run/blank-UNIT.dxf --workdir .local/goal-run/CASE --method "POMI (RICS)"`.
A seed line makes the blank DXF readable; it is deleted and read back before
construction. The jobs build the original 21-run villa and four-run site wall,
resolve live source handles, and add openings in two phases. All record pages
are read back after every mutation; positions, widths, kinds and vertical
extents are checked against the requested inputs.

| CLI job | First 7 | Next 37 | Openings retained | Transcript |
| --- | ---: | ---: | ---: | --- |
| Before, metres | 4.004 s | 90.952 s, cancelled | 7 | `.local/goal-run/before-m/transcript.jsonl` |
| After, metres | 0.441 s | 2.128 s | 44 | `.local/goal-run/after-m/transcript.jsonl` |
| After, millimetres | 0.491 s | 2.005 s | 44 | `.local/goal-run/after-mm/transcript.jsonl` |

Each after job includes a mixed batch: add a valid site opening, draw a line,
then request an impossible opening. It fails at step 2 with fitting ranges.
All entity readbacks and wall groups equal the before state. The capture after
that failure is byte-for-byte identical to the successful phase's capture.
Each complete after transcript contains 43 exchanges; `summary.json`,
`mcp-exit.json`, DXF and project files are retained beside it.

Inspected PNGs:

- Before: `.local/goal-run/before-m/0028-after-opening-phase-5-0.png` and
  `0021-after-opening-phase-4-0.png`. Only the seven front openings remain;
  interior walls have no requested openings.
- After metres: `.local/goal-run/after-m/0043-final-villa-0.png`.
- After millimetres: `.local/goal-run/after-mm/0043-final-villa-0.png`.
  Interior door gaps, exterior windows and the requested site gates are visible
  in both after drawings. The concept's large gate swings are preserved as requested.

Both saved DXFs were reopened through fresh MCP sessions. Each still contains
44 openings, every reported member exists, and each new PNG is byte-for-byte
identical to its final capture before closing. The reopened metre PNG was
inspected too. Evidence: `.local/goal-run/reopened-m/transcript.jsonl`,
`.local/goal-run/reopened-mm/transcript.jsonl`, their `0006-saved-villa-0.png`
captures, and `.local/goal-run/reopen-check.log`. These add 12 exchanges to the
121 exchanges in the three primary jobs.

The initial after-metre audit stopped when its one-page readback hit the response
size cap at 73 of 115 entities. That was a client assertion, after the openings
had succeeded. The client now follows `next_offset`; the complete jobs above
were rerun from blank input. The partial diagnostic is retained under
`.local/goal-run/after-m-partial-readback/`, separate from the accepted evidence.

5. Limits and founder decisions

No founder decision is required for the engine changes. The 44 AI socket tests
and two desktop socket tests still need a run where local listeners are allowed;
they are not reported as passes. The two desktop failures are
`commands::tests::the_mcp_server_follows_the_settings` and
`credentials_follow_the_saved_address_and_never_a_draft`, both OS error 1 at
listener creation. The new desktop panic-recovery test passed, including a later
project save and recovery from a separate host-boundary panic. The fixture
preserves the founder's concept; it does not assess architectural quality or compliance.
The existing process panic hook may log that a drawing is locked at the instant
of a panic, before unwinding reaches recovery. Recovery releases that lock and
keeps the session usable. No frontend source or Arabic typography changed.


Socket-blocked AI tests (also saved in `.local/goal-run/blocked-ai-tests.json`):

`qiyas-ai lib`:

```text
chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back
chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn
claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo
models::tests::an_unreachable_service_falls_back_to_the_builtin_list
```

`qiyas-ai agent`:

```text
a_marked_region_lists_what_is_in_it
a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user
a_question_ends_the_turn_cleanly_and_the_answer_carries_on
a_selection_is_listed_and_shown
all_server_failures_retry_with_a_visible_wait_and_identical_request
an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room
an_answer_that_keeps_running_out_stops_at_the_cap
api_key_absent_from_logs_and_errors
attached_pictures_go_before_the_words
billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report
cancel_between_tool_calls_runs_no_more_of_them
cancel_stops_within_one_second
file_tools_without_a_host_say_to_use_the_app
gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn
history_is_append_only_and_system_prompt_is_stable
invalid_output_value_keeps_the_cap_and_does_not_retry
invalid_tool_json_is_answered_with_an_error_and_not_run
loop_stops_after_40_tool_calls
malformed_tool_input_still_stops_after_40_calls
mid_stream_overload_discards_partial_text_and_keeps_a_retry_log
old_pictures_give_way_to_a_line
openai_compatible_drops_a_refused_field_and_asks_again
openai_compatible_streams_text_and_tool_calls
partial_usage_chunks_retain_prior_counts_through_the_http_adapter
refusal_stop_reason_surfaces_category
rejected_request_after_find_entities_is_logged_without_retry
removals_and_resolutions_stop_after_10_in_one_turn
text_stream_is_forwarded_in_order
tool_use_round_trip_executes_against_the_session
view_image_stops_when_the_image_budget_is_used
```

`qiyas-ai key_safety`:

```text
network_tests::model_discovery_never_follows_a_redirect_with_credentials
```

`qiyas-ai mcp`:

```text
a_restart_on_the_same_port_binds_again
a_wrong_token_is_refused_with_401
execute_request_ids_replay_without_another_mutation_or_ui_event
concurrent_counts_both_land
lists_the_catalogue_and_measures_as_mcp
other_origins_are_refused_even_with_the_token
tool_errors_are_error_results
sheet_layer_and_zoom_changes_reach_the_window
view_image_is_an_image_block
```
