# Goal H: subscriptions work like the API providers

Worktree `.local/wt/goal-h`, branch `goal/h`. No commit or push. All model streams, accounts
and CLI processes used by the checks are fixtures. No real login, credential file, paid
model call, network documentation lookup or new dependency is involved.

## Changes by goal item

| Item | Before | After |
| --- | --- | --- |
| H1: vanished ChatGPT answers and missing calls | The founder's screenshot shows repeated requests without answers; completed events with an empty/partial output discarded streamed content. | Assemble indexed streamed items, text and function arguments. Final snapshots fill gaps, without duplicated text or calls. Only a completed stream can execute tools. An empty/reasoning-only answer gives a plain English/Arabic error and records item types and usage. |
| H2: first ChatGPT model | Empty model caused `chatgpt_model`; Settings did not select the first account entry. | Choose and save the account's first visible model on sign-in/discovery and before the first send/resume. Settings displays the account list. Account changes clear the previous selection. |
| H3: effort and Claude models | Subscription effort was hidden; Claude model was free text and effort never reached argv. | ChatGPT uses only advertised reasoning levels and remembers an explicit unsupported-effort refusal for the conversation. Claude models/levels come from `--help`; Default omits flags, aliases use a list, and Other accepts a full name. English and Arabic controls are covered. |
| H4: invisible Claude work | The live log shows a request followed by cancellation almost four minutes later. | Partial tools become activity before the full assistant summary. Text, calls, results, retries/status and terminal results enter `agent.log` as received, including turns later cancelled. |
| H5: one-message history | Discarded answers left user requests merged together; logs said one message. | Stored assistant answers, tool results and reasoning are included on later requests. Logs distinguish message count from actual Responses input-item count. |
| H6: safety | Existing Goal S constraints and tests. | Retained. Explicit model/effort values use separate argv items and reject characters outside the allowed ASCII set, including spaces, quotes and shell punctuation. No real Claude print invocation or login was used. |

The reviewed SIWC endpoints, scopes, OAuth, keychain storage and account safety policy are
unchanged. No drafting rules, prompts, side panel, table layout or chat rendering was changed.
Legacy global API effort is no longer migrated into subscription profiles: those use Default
until the user chooses a level. Settings writes are verified by reading the saved file back.
No safety assertion was relaxed. The existing Settings save test now checks Claude status
before choosing a model because the requested list must come from help; its save and
no-API-key assertions remain. The private-MCP test retains its assertions and adds the
real Agent, measured readback, logs and all four units.

## Before evidence

- Founder evidence inspected: `/home/kareem/Work/qs-mind/qiyas/.local/audit/live-2026-10-24/`
  (`today.log` and all four PNGs). The relevant image is `chatgpt-missing-replies.png`.
- `.local/goal-run/settings-before.log`: three new regression failures, eight existing tests pass.
- `.local/goal-run/h-before/stream-tests-resumed.log`: both ChatGPT stream regressions fail
  against the old provider (zero passed, two failed).
- `.local/goal-run/h-before/chatgpt-agent-tests.log`: seven new behavior failures and one
  blocked HTTP test; interrupted-stream safety passes. `chatgpt-history.json` and
  `chatgpt-history.log` show the missing assistant history. `chatgpt-stream-4/transcript.json`
  shows the real Agent ending without executing the streamed edit.
- `.local/goal-run/h-before/claude-tests.log`: five existing checks pass; four new regressions
  fail (help choices, effort argv, partial activity, live logging), and private MCP is blocked
  by sockets. `claude-live-before-cancel.log` records the silence while the fake child runs;
  `claude-live-events.json` records the activity that was received but not logged.
- The first cold build and full Vitest run were interrupted by the laptop memory guard; their
  partial logs are retained (`h-before/stream-tests.log`, `h-vitest-interrupted.log`). Checks
  resumed with one local job at a time; these interrupted attempts are not passes.

## Checks and end-to-end evidence

Check logs are under `.local/goal-run/` in this worktree.

- Focused ChatGPT checks: **28 passed, 2 blocked by socket creation, 12 existing ignored**;
  `.local/goal-run/h-after/chatgpt-tests.log`. The real Agent executes an edit, reads it back,
  retains later history and opaque reasoning, and logs text/calls/usage without fake credentials,
  including credentials split across deltas. All four unit cases pass. A final review
  also added scrubbing and a regression for refusal credentials split across deltas.
- Focused Claude checks: **12 passed, 1 blocked by socket creation**;
  `.local/goal-run/h-after/claude-tests.log`. Existing non-socket Goal S checks remain green,
  including the watched fake credential-file sentinel. Help choices, argument validation,
  separate argv, partial activity, cancellation logging and terminal usage pass.
- `cargo fmt --all`: passed (`.local/goal-run/h-fmt-final.log`). Later assembler cleanup and the split-refusal redaction regression were formatted
  directly with rustfmt.
- First workspace clippy attempt: failed on two redundant `String` conversions in the new
  assembler (`h-clippy-first.log`). Both conversions were removed; no lint was suppressed.
- Initial `cargo build --offline -p qiyas-cli`: passed, 7m 19s (`h-build-cli.log`).
  The final source was rebuilt successfully in 28.48s (`h-build-cli-final.log`) and the
  headless checks were repeated on that executable.
- Stdlib headless MCP: passed (`h-headless-run.log`, `h-headless/summary.json`). The
  saved drawing readbacks report exactly four metres in mm, cm, m and inches. The fresh
  mutation job reads one entity after drawing and zero after undo. The final run contains
  **9 sessions / 46 exchanges**. `h-final-evidence-check.json` records its final binary hash,
  four unit checks and equality of all ten PNGs to the inspected run. The final metre
  before/after pair was opened again. Final CLI SHA-256:
  `37343d718c3e61ca431ba4e56117a521cade5b0c426847d5efb3463a28e8f136`.
- All ten before/after PNGs below were opened and inspected: empty before, one horizontal
  line after, with consistent physical scale across the four units.
- Final `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed,
  exit 0 (`h-clippy-final.log`). The corrected first repeat also passed; the final repeat
  includes the refusal redaction change made while that build was finishing.
- Final `node_modules/.bin/tsc --noEmit`: passed, exit 0 (`h-tsc-final.log`).
- Full `node_modules/.bin/vitest run --maxWorkers=2`: passed, **891 tests in 119 files**,
  exit 0, 151.28 seconds (`h-vitest-final.log`).
- Full `cargo test --offline --no-fail-fast -p qiyas-ai -p qiyas-desktop -- --test-threads=2`:
  **386 passed, 46 failed at local socket creation, 13 ignored**, exit 101;
  `h-rust-tests-final.log`, parsed totals in `h-rust-results.json`. Every one of the 46 failure
  blocks contains `Operation not permitted`. No non-socket test failure remains.
  By crate: **qiyas-ai 259 passed / 44 socket failures / 13 ignored**, and **qiyas-desktop
  127 passed / 2 socket failures**. This includes the required AI library, surface and agent targets: respectively
  **133 passed / 4 socket failures / 12 ignored**, **69 passed**, and **3 passed / 30 socket
  failures**. Desktop: **99 library tests passed / 1 socket failure**, **10 options tests
  passed**, **18 settings tests passed / 1 socket failure**. Both crates' documentation
  tests pass (zero examples). The two nested desktop child-test summaries are not double counted.
- Explicit `cargo test --offline -p qiyas-ai --lib chatgpt_plan:: -- --ignored --test-threads=1`:
  all **12 deferred Goal S mock tests fail at socket creation**, exit 101 (`h-siwc-mocks.log`).
  They were attempted, not assumed green. Across both commands this leaves 386 distinct
  passes, 58 socket-blocked tests and one pre-existing ignored take-off acceptance test.
- The final split-refusal redaction test passes in the full library run. Saved settings
  readback, Claude argv/environment/sentinel/cancellation, and all scripted Agent checks
  that do not need a socket also pass.
- Final direct rustfmt check of the later assembler/test edits and `git diff --check`: passed
  (`h-rustfmt-late-check.log`; whitespace check exit 0).

The environment's local socket probe fails with `EPERM`: `.local/goal-run/h-before/socket-probe.json`.
HTTP mock and private-MCP acceptance must therefore be distinguished from direct stream-parser
and fake-child checks through the real Agent; blocked tests are not counted as passes.

### Saved transcripts and pictures

Paths are under `.local/goal-run/` in this worktree:

| Evidence | Before | After |
| --- | --- | --- |
| Dropped ChatGPT edit through the real Agent | `h-before/chatgpt-stream-4/transcript.json` | `h-after/chatgpt-stream-{4,5,6,1}/transcript.json`, `readback.json`, `agent.log`, `drawing.dxf` |
| Conversation retained | `h-before/chatgpt-history.json` and `.log` | `h-after/chatgpt-history.json` and `.log` |
| Claude logs during a running/cancelled turn | `h-before/claude-live-before-cancel.log` | `h-after/claude-live-before-cancel.log`, `claude-cancelled-agent.log`, `claude-live-events.json` |
| Completed fake Claude turn | New acceptance coverage | `h-after/claude-complete-agent.log`, `claude-complete-agent-events.json` |
| CLI views in mm/cm/m/in | `h-headless/{mm,cm,m,in}/before/0004-before-0.png` | `h-headless/{mm,cm,m,in}/after/0004-after-0.png` |
| Fresh CLI mutation and undo | `h-headless/stdio-mutation/0003-before-0.png` | `h-headless/stdio-mutation/0009-after-0.png`, `transcript.jsonl` |

The earlier focused artifacts are preserved in `h-after-focused/`; `h-after/` contains
fresh final Agent transcripts/logs (plus copies of the focused test-summary logs). The first
successful headless run is preserved in `h-headless-first-pass/` and `.log`.

Each headless before/after directory also has `transcript.jsonl` and `stderr.log`. The
before drawings are empty fixture counterparts made from the real writer's tables and
headers; the after drawings are the saved output of the real Agent replay. These PNGs
verify drawing state, not the desktop subscription UI. Settings is covered by Vitest.
The founder's original missing-answer screenshot remains the live UI before evidence.

Two evidence-harness mistakes were caught by readback and corrected: a minimal empty DXF
was rejected, and `run` with a command named `undo` was rejected. Their failed attempts are
kept at `h-headless-initial.log` / `h-headless-initial/` and
`h-headless-harness-undo.log` / `h-headless-harness-undo/`. The final run uses a valid saved
DXF header/tables and the documented `qiyas_execute` operation `undo_own`. No product code
was changed to accommodate the harness.

## Files changed

Exact 22-file manifest: `.local/goal-run/h-files-touched.txt`. The copied stdlib client,
headless job and generated evidence are local artifacts under `.local/`.

- `crates/qiyas-ai/src/chatgpt_plan.rs`, `chatgpt_plan/responses.rs`,
  `chatgpt_plan/responses/streamed.rs`, `chatgpt_plan/responses/acceptance.rs`,
  `chatgpt_plan/tests.rs`: model capabilities, stream assembly and regression/Agent checks.
- `crates/qiyas-ai/src/claude_code.rs`, `tests/fixtures/claude.py`: help-derived options,
  separate arguments, partial activity, fake-child and private-MCP checks.
- `crates/qiyas-ai/src/agent.rs`, `provider.rs`: received provider activity and safe diagnostic
  logging. No agent drafting rules or prompts changed.
- `crates/qiyas-desktop/src/subscriptions.rs`, `commands.rs`, `settings.rs`,
  `tests/settings.rs`: persist the first account model and keep subscription effort explicit.
- `src/api.ts`, `src/shell/SubscriptionSettings.tsx`, `SettingsDialog.tsx`, their two
  `.test.tsx` files, and `src/i18n/en.json`, `ar.json`: choices and translations.
- `CLAUDE.md` and this report document the changed behavior and validation limits.

## Reproduction

All paths below are relative to this worktree. Every Cargo command used:

```sh
CARGO_TARGET_DIR=/home/kareem/Work/qs-mind/qiyas/.local/wt/goal-h/target \
CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
```

The stdlib client is copied from the founder's audit into `.local/mcp_client.py`; its root
and allowed output directory point only to this worktree. `.local/goal-h-headless.py` uses
`target/debug/qiyas-cli mcp --drawing … --workdir .local/goal-run/…`. It opens the Agent's
saved drawings, measures each line, and captures the before/after view. A separate stdio
job creates a line, reads it back, undoes it, and reads back the empty drawing.

The Rust fixture evidence is saved by setting `QIYAS_SUBSCRIPTION_EVIDENCE` to
`.local/goal-run/h-after` (use an absolute path). On a runner that permits loopback sockets,
the blocked new tests can be rerun without a real account using these filters:

```sh
cargo test --offline -p qiyas-ai --lib mock_http_responses_real_agent_edits_and_reads_back
cargo test --offline -p qiyas-ai --lib unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn
cargo test --offline -p qiyas-ai --lib fake_cli_drives_private_mcp_with_readback_and_undo
```

Apply the Cargo prefix above to each command. The HTTP test uses a scripted local Responses
server. The Claude test uses only `tests/fixtures/claude.py`, performs a scaled three-metre
edit in mm/cm/m/in through the private MCP, reads the measured length, undoes the edit and
reads back the empty session. Neither test can fall back to a real provider.

## Remaining limits and decisions

HTTP mock-server acceptance and Claude's private HTTP MCP acceptance are implemented but
**not verified here**: the sandbox rejects creation of their local sockets. The successful
stream replay, fake-child Agent and stdio CLI runs do not substitute for those transports.
The exact blocked test names are below and in `.local/goal-run/h-blocked-tests.json`.
The private-MCP attempt saves its error and transcript in
`h-after/claude-private-mcp-mm/`; it stops at the first unit before the child can connect.

No live subscription account, live model, real Claude print invocation or desktop sign-in
was tested. The founder/reviewer needs a socket-capable runner for the listed mock checks;
no real login is needed. There is no new product-policy decision, dependency, commit or push.
Existing Goal S release requirements remain unchanged.

### Tests blocked by local socket restrictions

Full Rust command (46):

```text
chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back
chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn
claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo
models::tests::an_unreachable_service_falls_back_to_the_builtin_list
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
network_tests::model_discovery_never_follows_a_redirect_with_credentials
a_wrong_token_is_refused_with_401
a_restart_on_the_same_port_binds_again
execute_request_ids_replay_without_another_mutation_or_ui_event
concurrent_counts_both_land
other_origins_are_refused_even_with_the_token
lists_the_catalogue_and_measures_as_mcp
sheet_layer_and_zoom_changes_reach_the_window
tool_errors_are_error_results
view_image_is_an_image_block
commands::tests::the_mcp_server_follows_the_settings
credentials_follow_the_saved_address_and_never_a_draft
```

Explicit deferred SIWC mocks (12):

```text
chatgpt_plan::oauth::tests::mock_loopback_ignores_wrong_state_and_never_echoes_the_code
chatgpt_plan::oauth::tests::mock_token_and_jwks_servers_exchange_without_real_accounts
chatgpt_plan::responses::tests::mock_responses_server_gets_only_mizan_requests_and_no_stored_response
chatgpt_plan::tests::mock_admission_limit_stops_next_request_and_401_keeps_credentials
chatgpt_plan::tests::mock_broken_stream_never_succeeds_or_executes_partial_tools
chatgpt_plan::tests::mock_cancelled_rotation_commits_before_another_account_operation
chatgpt_plan::tests::mock_refresh_is_serial_and_terminal_failure_reauthorizes_once_with_saved_registration
chatgpt_plan::tests::mock_refresh_rotates_atomically_and_omits_scope_and_secret
chatgpt_plan::tests::mock_revocation_accepts_empty_200_and_retries_server_failure
chatgpt_plan::tests::mock_sign_out_cancellation_stops_an_inflight_request
chatgpt_plan::tests::mock_terminal_refresh_clears_only_tokens_and_network_failures_keep_them
chatgpt_plan::tests::mock_unsupported_feature_changes_the_next_body_and_reports_activity
```
