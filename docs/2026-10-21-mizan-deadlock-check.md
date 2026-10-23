# Mizan session-lock correction — 21 October 2026

The Phase 14 tab-switch deadlock came from checkpoint creation taking the session lock before
sending `ToolCall`. The test's session owner was waiting for that event before releasing the
lock. Computing the first mutation's check baseline on the async thread added another path to
the same stall.

## Changes

- `Agent::snapshot`, checkpoint creation and `Agent::resume` no longer take the session lock.
  They use drawing metadata captured by blocking workers. The opening captures system facts,
  live state, pointed-at picture, drawing path and epoch under one lock.
- Session tools validate cancellation and the expected epoch after acquiring the lock. The
  first mutation's baseline is computed inside that same critical section, before execution;
  cancellation is checked again after the baseline. The tool result carries the phase state
  and drawing metadata back to the async loop.
- Phase-boundary and turn-end checks run on blocking workers with cancellation/epoch validation
  before and after the check. One returned error set drives both the decision and approval text.
  Error identity remains sheet + rule + sorted entity handles; only new errors involving the
  phase's touched entities count. Baselines still survive interruption/resume until repaired.
- Host/window calls remain outside the session lock. Their blocking wrappers validate the turn
  and return drawing metadata; successful host drawing changes advance the expected epoch.
  Save As updates the idle agent cache as well as the saved checkpoint.
- Both crates deny Clippy's `await_holding_lock` for all targets, including integration tests.
  The desktop mock provider's session reads also run on blocking workers.
- All 26 agent integration tests have named 60-second Tokio timeouts. Nine desktop AI turn/IPC
  tests use named 60-second Tokio timeouts driven by an independent runtime. The tab-switch
  regression now enables checkpoints, verifies their original drawing and tool event while the
  session is held, and uses async synchronization. Its turn runs separately so timing out can
  release the holder instead of hanging the timeout driver.
- Blocking-step cancellation keeps tool results as `Not run: cancelled`, while the public
  `Stopped` turn event retains its existing `cancelled` reason. The desktop cancellation test
  caught this distinction during verification; the implementation was corrected without
  changing its assertion.

## Files touched by this correction

- `CLAUDE.md`
- `crates/qiyas-ai/Cargo.toml`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/tests/agent.rs`
- `crates/qiyas-desktop/Cargo.toml`
- `crates/qiyas-desktop/src/ai.rs`
- `crates/qiyas-desktop/src/commands.rs`
- `crates/qiyas-desktop/src/mock.rs`
- `docs/2026-10-21-mizan-deadlock-check.md`

The earlier accepted Phase 14 changes remain in the working tree. No dependencies were added,
no real settings or keys were accessed, and nothing was staged, committed or pushed.

## Checks

All Cargo commands used `CARGO_BUILD_JOBS=2`. Each requested command ran once. Commands used the
normal Cargo test path and production agent; this sandbox cannot bind local TCP ports.

| Command | Actual result |
| --- | --- |
| `cargo fmt --all` | Exit 0. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0; no warnings, including the lock lint. |
| `cargo test --offline -p qiyas-ai --lib --test surface --test drafting_eval --test authoring` | Exit 101: library 70 passed, 1 denied local-port bind. Cargo stopped before integration binaries. |
| `cargo test --offline -p qiyas-ai --test surface --test drafting_eval --test authoring` | Follow-up for those unrun targets only: exit 0; surface 42, drafting_eval 4, authoring 16 passed. |
| `cargo test --offline -p qiyas-ai --test agent` | Exit 101: 1 passed, 25 denied local-port binds; complete in 0.44 seconds, no hang. |
| `cargo test --offline -p qiyas-desktop` | Exit 101: 90 passed, 2 failed. One was the cancellation reason mismatch described above; the other was MCP's denied local-port bind. Cargo stopped before integration binaries. |
| `cargo test --offline -p qiyas-desktop --lib commands::tests::a_cancelled_turn_says_so_and_ends -- --exact` | After correcting the cancellation event: exit 0, 1 passed (0.05 seconds). Its assertion is unchanged. |
| `cargo test --offline -p qiyas-desktop --test options --test settings` | Follow-up for the unrun targets: exit 0; options 10 and settings 15 passed. |
| `rustfmt --edition 2021 --check crates/qiyas-ai/src/agent.rs` | Exit 0 after the final cancellation change. |
| `cargo clippy --offline -p qiyas-ai --all-targets -- -D warnings` | Exit 0 after that final change; no warnings. |

The desktop port-only failure is `commands::tests::the_mcp_server_follows_the_settings`,
at `crates/qiyas-desktop/src/commands.rs:1961`, with OS code 1, `PermissionDenied`,
`Operation not permitted` while binding its local test listener. The initial cancellation
failure was `commands::tests::a_cancelled_turn_says_so_and_ends`.

The library's port-only failure is
`models::tests::an_unreachable_service_falls_back_to_the_builtin_list`, at
`crates/qiyas-ai/src/models.rs:247` (`TcpListener::bind`).

Every failing agent integration test stopped at `crates/qiyas-ai/tests/agent.rs:70`, the common
`TcpListener::bind("127.0.0.1:0")` fixture, with OS code 1, `PermissionDenied`,
`Operation not permitted`. They did not reach provider/agent assertions:

- `a_marked_region_lists_what_is_in_it`
- `a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user`
- `a_question_ends_the_turn_cleanly_and_the_answer_carries_on`
- `a_selection_is_listed_and_shown`
- `an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room`
- `an_answer_that_keeps_running_out_stops_at_the_cap`
- `api_key_absent_from_logs_and_errors`
- `attached_pictures_go_before_the_words`
- `cancel_between_tool_calls_runs_no_more_of_them`
- `cancel_stops_within_one_second`
- `file_tools_without_a_host_say_to_use_the_app`
- `gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn`
- `history_is_append_only_and_system_prompt_is_stable`
- `invalid_tool_json_is_answered_with_an_error_and_not_run`
- `loop_stops_after_40_tool_calls`
- `malformed_tool_input_still_stops_after_40_calls`
- `old_pictures_give_way_to_a_line`
- `openai_compatible_drops_a_refused_field_and_asks_again`
- `openai_compatible_streams_text_and_tool_calls`
- `refusal_stop_reason_surfaces_category`
- `rejected_request_after_find_entities_is_logged_without_retry`
- `removals_and_resolutions_stop_after_10_in_one_turn`
- `text_stream_is_forwarded_in_order`
- `tool_use_round_trip_executes_against_the_session`
- `view_image_stops_when_the_image_budget_is_used`

## Ten tab-switch repetitions

After the full agent file, this command ran ten additional times sequentially, then another ten
on the final code after the cancellation-reason correction:

```sh
CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-ai --test agent \
  a_call_waiting_through_a_tab_switch_is_not_run_on_the_new_drawing -- --exact
```

Both batches passed 10/10, with exit 0, one passed test and 25 filtered out each run. The first
batch took 0.45 seconds for run 1 and 0.44 seconds for runs 2–10 inside the test harness.
Every run in the final batch took 0.44 seconds. Each run covers both cancellation and epoch
rejection without cancellation, with checkpoint assertions enabled. No timeout fired.

| Final run | Exit | Test duration |
| --- | --- | --- |
| 1 | 0 | 0.44 seconds |
| 2 | 0 | 0.44 seconds |
| 3 | 0 | 0.44 seconds |
| 4 | 0 | 0.44 seconds |
| 5 | 0 | 0.44 seconds |
| 6 | 0 | 0.44 seconds |
| 7 | 0 | 0.44 seconds |
| 8 | 0 | 0.44 seconds |
| 9 | 0 | 0.44 seconds |
| 10 | 0 | 0.44 seconds |

The port-dependent tests still need the reviewer's host run. No browser/window, manual app run,
`scripts/check.sh`, frontend checks or live provider request was used for this correction.
