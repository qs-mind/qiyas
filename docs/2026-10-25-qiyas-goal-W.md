# Goal W — one workspace per drawing

25 October 2026. Revised founder scope: finish code and fast tests. End-to-end jobs, captures,
replays, the 60-second timing gate and memory acceptance are left to the reviewer's combined run.
Code and fast validation are complete, with local-socket tests blocked by the sandbox as listed
below. Earlier evidence is retained, not rerun after the scope change.
No commits, paid model calls or changes to the founder's running checkout.

The implementation replaces session swapping with stable drawing workspaces. The design
and lock order are in `docs/2026-10-25-qiyas-goal-W-design.md`.

| Item | Change | Fast test evidence |
| --- | --- | --- |
| W1 | Stable session, undo, Mizan, provider settings, window requests and published data per drawing. IPC resolves an optional workspace once. Autosave and file locks cover hidden workspaces. | Desktop closed-id, autosave, recovery and host routing tests pass. `workspaces.test.tsx` also checks History resuming in the drawing it opened before React rerenders. |
| W2 | Switching and opening retain existing turns. Busy close asks to stop first; waiting for a committing tool leaves the tab-switch queue free. File tools retain their originating binding. | `workspace_turns_survive_switch_and_reload_with_independent_providers` passes, including separate drawing effects and undo. `useDrawings.test.tsx` proves confirmation and tab access during a simulated 60-second close wait. |
| W3 | Provider/model/effort picker per drawing; Settings supplies defaults. Credentials remain addressed by provider and service address. Shared autonomy and measurement rules remain current. | `WorkspacePicker.test.tsx` (three tests), `workspaces.test.tsx`, the native concurrent-provider test and `workspace_choice_keeps_shared_work_rules_current` pass. |
| W4 | Default three-turn admission queue, one Claude turn at a time, cancellable waiting. Available memory below 1 GiB reduces admission to one without cancelling work. The process semaphore also waits for a cancelled Claude child to be reaped. | All three `turn_queue` tests, `claude_workspaces_queue_and_keep_their_provider_choices` and AI's `queued_fake_claude_waits_until_cancelled_process_is_reaped` pass. |
| W5 | Drawing IPC uses try-lock. Busy picks retry without blocking other tabs. The window can read committed metadata and render buffers while a tool owns a session. Long commands use blocking workers. External sheet authority has its own guard, keeping status readable. | `workspace_busy_tool_keeps_other_drawing_ipc_interactive` passed: 8 interaction groups in 103.7 ms, worst group 3.22 ms (`ipc-fast-timing.json`). `workspaceIpc.test.ts` and busy-close tests pass, including simulated 60 seconds. Real 60-second gate left to reviewer. |
| W6 | Subscribe before reading the native transcript snapshot; workspace ids and cursors reject wrong-drawing or duplicate events. Reload restores running cards, progress and provider choices. | Three reattachment regressions fail against the original hook and pass with the changes. The native concurrent-provider test also reattaches to both running turns without cancelling them. |
| W7 | Tabs show working, queued, waiting, done and failed status. Hidden completion retains a short summary. Composer drafts and attachments belong to their drawing. | `FileTabs.test.tsx`, `AssistantPanel.test.tsx` and the native concurrent-provider/away-summary test pass. |
| W8 | `qiyas_read events {after,session?}` exposes the existing bounded commit journal, changed handles and explicit resync. Atomic batches produce one commit; failures produce none. Window progress has a separate bounded cursor journal. | Both `qiyas-core --test events` tests pass within the full core run: atomic commits, failure/undo, foreign-session resync in mm/cm/m/in, and expiry after 128 retained commits. The desktop cursor-loss test passes too. |

Before/after evidence currently retained under `.local/goal-run/`:

- `reattach-before.log`: three regression failures against the original frontend hook.
- `reattach-after.log`: the same three tests pass with native snapshot attachment.
- `assistant-suite.log`: 12 files, 78 tests passed.
- `frontend-step9.log`: 3 files, 51 tests passed, including close confirmation and tab access
  throughout a simulated 60-second wait.
- `tsc-step10.log`: TypeScript passed at that step; the final check also passes below.
- `build-before.log`: despite its early filename, this builds the changed CLI, not an original
  baseline binary. No before-build claim is made from that log.

Actual CLI evidence:

- `build-before.log`: changed CLI build passed (33m 03s compiling, after the shared queue).
- `.local/goal-run/events-job-final.log` and `events/{4,5,6,1}/transcript.jsonl`: four real
  `target/debug/qiyas-cli mcp --drawing … --workdir …` processes, 23 requests each. All passed:
  one atomic event with two added handles, no event or entity change after a rejected batch,
  undo removing the two handles, and resync for a foreign drawing cursor. Total logged request
  time was 0.546 s (mm), 0.500 s (cm), 0.321 s (m), 0.412 s (in).
- `events/{4,5,6,1}/0009-before-commit-0.png` and `0016-after-commit-0.png`: empty before,
  rectangle and diagonal after. Captures are 1000 pixels wide; all represent the same 5 m × 3 m
  geometry. Inspected the images. These compare drawing effects, not an old desktop executable.
- The first handwritten empty DXF was rejected by the reader. The corrected fixture starts
  with a valid line, removes it through MCP, and verifies the empty drawing before the job.
- `frontend-integration-3.log`: 3 files, 57 tests passed, including the complete App wiring
  tests, delayed Save As routing, outgoing view routing after a native switch, and subscription
  choice discovery. Existing exact payload checks now assert workspace ids; native event mocks
  allow multiple listeners and use the addressed event channel. No drawing-effect assertions
  were removed. `fileDialogs` caught and corrected a direct plugin import in a new test.

Fast native tests cover concurrent turns, switching/reload, Claude queueing, a held drawing lock,
event cursors and fake child lifetime. The combined end-to-end and memory gates are left to the
reviewer's combined run. For the optional long native timing test, set `QIYAS_W_HOLD_MS=60000`
and run `commands::tests::workspace_busy_tool_keeps_other_drawing_ipc_interactive` with
`QIYAS_W_TIMING` pointing to a JSON output path. The default hold is 100 ms for fast validation.
Checks completed under the revised scope:

- `cargo fmt --all`: passed; `.local/goal-run/fmt-final.log`.
- `tsc --noEmit`: passed; `.local/goal-run/tsc-final.log`.
- Full Vitest, two workers: **124 files, 926 tests passed**, 118.96 seconds;
  `.local/goal-run/vitest-final.log`.
- Desktop: **112 unit tests, 10 options tests and 19 settings tests passed**. The first full
  command found a missing stream event in the new mock provider; corrected without removing
  the summary assertion. The recheck excluded only the first known port test. Two unchanged
  tests cannot bind `127.0.0.1:0` in this sandbox (`PermissionDenied`, code 1):
  `commands::tests::the_mcp_server_follows_the_settings` and
  `settings::credentials_follow_the_saved_address_and_never_a_draft`.
  Logs: `desktop-final.log`, `desktop-final-2.log`. Desktop test command exits 101 because of
  those environment failures; this is not an unqualified full-suite pass.
- `cargo clippy --offline -p qiyas-desktop -p qiyas-ai -p qiyas-core --all-targets -- -D warnings`:
  **passed**, `.local/goal-run/clippy-final-3.log`. Earlier attempts caught a test-only import,
  the expanded PDF IPC argument count, and a missing `u32` annotation in the new routing test.
  The PDF IPC retains its existing fields with a narrowly explained argument-count exception.
- `cargo test --offline -p qiyas-core`: **823 tests passed**, no failures or ignored tests;
  `.local/goal-run/core-final.log` and `core-test-summary.json`. Build plus tests took 580.30 s.
- `cargo test --offline -p qiyas-ai`: **138 unit tests passed, four failed because loopback
  sockets are denied, 12 existing tests ignored**; exit 101. Both new AI tests pass:
  `workspace_routing_keeps_sessions_and_retry_ids_independent` and
  `queued_fake_claude_waits_until_cancelled_process_is_reaped`. Log: `ai-final.log`.
  The four socket failures are:
  `chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back`,
  `chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn`,
  `claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo`, and
  `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`.
  Cargo stopped at the unit-test failures, so all integration targets were run separately.
- AI `surface`: **69 passed**, exit 0; `ai-surface-final.log`.
  AI `agent`: **3 passed, 30 failed at the mock server's socket bind**, exit 101;
  `ai-agent-final.log`. All 30 fail at `tests/agent.rs:70` with `PermissionDenied`, code 1.
- Remaining AI integration targets: **63 passed, 10 socket failures, three existing ignored
  tests**, exit 101; `ai-rest-final.log` and `ai-rest-summary.json`. Passing counts:
  `authoring` 19, `drafting_eval` 4, `goal_j` 6, `goal_l` 1, `key_safety` 1,
  `takeoff_eval` 1, `tools` 24, `villa_openings` 2 and `vision` 5.
  The failures are one `key_safety` mock-server bind and all nine `mcp` server binds.
  No socket-dependent assertion was weakened or marked ignored. All 40 blocked integration
  names are listed below; together with the four AI unit tests and two desktop tests above,
  **46 tests need a rerun where local sockets are allowed**.
- `git diff --check`: passed. No whole-workspace clippy or new standalone headless acceptance
  job was run under the revised scope. Every cargo command used the worktree's target directory
  and shared cargo wrapper. Completed test executables were removed only from this worktree
  when its target directory exceeded 15 GiB; evidence logs remain.

Socket-blocked AI integration tests (the test target prefixes identify where to rerun them):

```text
agent::a_marked_region_lists_what_is_in_it
agent::a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user
agent::a_question_ends_the_turn_cleanly_and_the_answer_carries_on
agent::a_selection_is_listed_and_shown
agent::all_server_failures_retry_with_a_visible_wait_and_identical_request
agent::an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room
agent::an_answer_that_keeps_running_out_stops_at_the_cap
agent::api_key_absent_from_logs_and_errors
agent::attached_pictures_go_before_the_words
agent::billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report
agent::cancel_between_tool_calls_runs_no_more_of_them
agent::cancel_stops_within_one_second
agent::file_tools_without_a_host_say_to_use_the_app
agent::gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn
agent::history_is_append_only_and_system_prompt_is_stable
agent::identical_malformed_input_stops_on_sixth_attempt
agent::invalid_output_value_keeps_the_cap_and_does_not_retry
agent::invalid_tool_json_is_answered_with_an_error_and_not_run
agent::mid_stream_overload_discards_partial_text_and_keeps_a_retry_log
agent::old_pictures_give_way_to_a_line
agent::openai_compatible_drops_a_refused_field_and_asks_again
agent::openai_compatible_streams_text_and_tool_calls
agent::partial_usage_chunks_retain_prior_counts_through_the_http_adapter
agent::refusal_stop_reason_surfaces_category
agent::rejected_request_after_find_entities_is_logged_without_retry
agent::removals_and_resolutions_report_counts_without_blocking
agent::successful_reads_continue_beyond_40_calls
agent::text_stream_is_forwarded_in_order
agent::tool_use_round_trip_executes_against_the_session
agent::view_image_stops_when_the_image_budget_is_used
key_safety::network_tests::model_discovery_never_follows_a_redirect_with_credentials
mcp::a_restart_on_the_same_port_binds_again
mcp::a_wrong_token_is_refused_with_401
mcp::concurrent_counts_both_land
mcp::execute_request_ids_replay_without_another_mutation_or_ui_event
mcp::lists_the_catalogue_and_measures_as_mcp
mcp::other_origins_are_refused_even_with_the_token
mcp::sheet_layer_and_zoom_changes_reach_the_window
mcp::tool_errors_are_error_results
mcp::view_image_is_an_image_block
```

The memory example reports sessions and the production tool surface separately from WebKit and
provider history; it must not be described as a measurement of the whole desktop.

Touches another goal:

- `qiyas-core/src/read_revision.rs`: read-only event access to the existing revision journal;
  no quantity, tracing, derivation, transaction or batching algorithm changed.
- `qiyas-ai/src/mcp.rs`: desktop workspace addressing and per-connection routing/retry state.
- `qiyas-ai/src/surface.rs` and `surface_args.rs`: the `events` read operation only.
- `qiyas-ai/src/claude_code.rs`: wait for the existing single-process semaphore, including reaping.
- `qiyas-ai/examples/workspace_memory.rs`: offline acceptance harness adapted from the supplied
  clinic research replay; no provider is instantiated.

The combined gates are **left to the reviewer's combined run**:

| Gate | What the reviewer should check |
| --- | --- |
| W1 routing and persistence | Open several real drawings; edit and undo each independently. Exercise autosave, locks, recovery and explicitly addressed MCP requests, including a closed id. |
| W2 switching and closing | Run two different scripted providers concurrently. Open and switch drawings mid-turn; both must finish. Close a busy tab, decline once, then stop and close. File tools must retain their original binding. |
| W3 provider isolation | Choose different providers, models and efforts in two panels. Change defaults in Settings and verify each drawing keeps its answerer; credentials must follow its saved service address. |
| W4 queue and memory | Queue a fourth job behind three; cancel a waiting job. Run two fake Claude jobs and verify child lifetimes never overlap, including cancellation. Measure three full clinic replays, including desktop/history overhead, and exercise reduced admission under memory pressure. |
| W5 long step | Hold one drawing for 60 real seconds while switching, selecting, drawing and undoing in another. Inspect the painted canvas and IPC timings; no partial batch may appear. |
| W6 reload | Reload the webview during running and queued turns. Restore the correct drawing, view, selection, transcript and progress without restarting or cancelling. |
| W7 status | Verify each tab's status, the hidden completion summary, and independent composer drafts/attachments in English and Arabic. |
| W8 events | Through MCP, read a cursor, commit an atomic batch, reject a batch and undo. Verify changed handles by readback in mm/cm/m/in; force cursor expiry and foreign-session resync. |

No new standalone end-to-end acceptance run was started after the scope change. Existing default
crate integration tests were run as required. No real model service was called.
There is no new dependency or licence change. The actual WebKit painting, full-memory budget
and combined integration remain unverified here by the founder's revised acceptance scope.
No founder decision is needed before the combined review.

Files touched (53):

| Folder | Files |
| --- | --- |
| `.` | `CLAUDE.md` |
| `crates/qiyas-ai/examples` | `workspace_memory.rs` |
| `crates/qiyas-ai/src` | `claude_code.rs`, `mcp.rs`, `surface.rs`, `surface_args.rs` |
| `crates/qiyas-core/src` | `read_revision.rs` |
| `crates/qiyas-core/tests` | `events.rs` |
| `crates/qiyas-desktop/src` | `ai.rs`, `ai_window.rs`, `blocks.rs`, `cmdline.rs`, `commands.rs`, `conversations.rs`, `drawing_host.rs`, `files.rs`, `lib.rs`, `mcp.rs`, `pdf.rs`, `plot.rs`, `projects.rs`, `script.rs`, `settings.rs`, `state.rs`, `subscriptions.rs`, `tabs.rs`, `turn_queue.rs`, `workspace_view.rs` |
| `docs` | `2026-10-25-qiyas-goal-W-design.md`, `2026-10-25-qiyas-goal-W.md` |
| `src` | `api.ts`, `workspaceIpc.test.ts`, `workspaceIpc.ts` |
| `src/ai` | `AssistantPanel.test.tsx`, `AssistantPanel.tsx`, `WorkspacePicker.test.tsx`, `WorkspacePicker.tsx`, `messages.ts`, `useAssistant.ts`, `workspaces.test.tsx` |
| `src/app` | `App.test.tsx`, `App.tsx`, `useAssistantWiring.ts`, `useDrawings.test.tsx`, `useDrawings.ts`, `useSheets.ts` |
| `src/i18n` | `ar.json`, `en.json`, `errors.ts` |
| `src/shell` | `FileTabs.test.tsx`, `FileTabs.tsx`, `SettingsDialog.tsx`, `theme.css` |
