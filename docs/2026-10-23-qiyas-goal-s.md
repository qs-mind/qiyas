# Goal S: subscription providers

Worktree: `.local/wt/goal-s`, branch `goal/s`. No commit or push. The reviewer supplied the
verified terms and exact SIWC wire specification on 23 October 2026. No pages were fetched
again, and no real account, real Claude executable, real token or paid call was used.

## 1. Changes by requested item

| Item | Before this completion | After |
| --- | --- | --- |
| ChatGPT plan | `reviewed_protocol()` always refused; the generic adapter was not enabled | Official authorization/token/discovery/Responses endpoints, exact resource/scopes, browser PKCE/state/nonce, `127.0.0.1` callback and issued client IDs. ID-token signature, issuer, audience, expiry, nonce and identity are checked. |
| Consent | Missing plan scope rejected sign-in | Identity stays signed in; model discovery and inference refuse without `chatgpt.tokens.use.direct`. Consent denial validates state and stops. |
| Registration and storage | One token record; host UUID in a separate file | One OS keychain vault (`qiyas` / `chatgpt-plan`) holds the stable `urn:uuid:...` host ID, separate registrations, active account and tokens. Sign-out keeps host ID and issued client IDs. Account identity and client ID must both match before replacing credentials. Distinct earlier registrations remain available for sign-out. |
| Refresh and recovery | Generic refresh without the final wire contract | Resource included, scope omitted, refreshes serialized, new tokens/expiry/scopes saved together. Verification keys are fetched before rotation. Rotation and keychain writes retain the lock through cancellation; queued requests cannot move to another account. Temporary failures retain credentials; terminal refresh errors clear only tokens and permit one OAuth recovery with the saved registration. Invalid client is a setup error. |
| Sign-out | Local deletion only | Cancels active requests, discovers revocation endpoint, posts the refresh token/type/client ID, accepts empty 200, retries transient failures twice, clears local tokens even if unconfirmed, and tells the user how to disconnect in ChatGPT settings. |
| Responses | Generic API retry handling; incomplete output could count as success | Only completed responses succeed. SIWC-specific English/Arabic errors, bounded transient backoff, persistent plan-limit pause, and changed-body retry for named optional unsupported features. Admission 401 never erases credentials or loops OAuth. Tools, reasoning and usage retain existing Mizan events. |
| Settings | Disabled ChatGPT button and manually typed model | Exact **Continue with ChatGPT** / **المتابعة باستخدام ChatGPT**, account picker, Sign out, plan note and limits link. Models are fetched for the active account, filtered by `visibility=list`, labelled by `display_name` and sent by `slug`. Switching accounts clears the list and stops requests. |
| Claude Code | Already implemented in the starting point | Kept unchanged: installed unmodified CLI only, isolated private MCP and tools, sanitized environment, no credential access or scripted login, individual turns, no Qiyas retries/evals, runtime help checks and Unix cancellation. |
| API keys | Existing Anthropic and OpenAI-compatible providers | Their transports, key storage, defaults and error policy are unchanged. No subscription fallback to paid API calls. |
| Policy/docs | Policy verified, exact SIWC details pending | Exact supplied protocol documented; OpenAI paid/hosted interest form and Anthropic Commercial Terms remain release gates. Plan usage remains free to the user and limited to Mizan's own requests. No general proxy or token pass-through. |

The old test expecting missing scope to reject the whole sign-in was corrected because
that encoded the wrong behavior under the supplied specification. It now proves that
identity is retained and inference is refused. Refresh tests now require the returned
rotated token and replace, rather than inherit, granted scopes. Security assertions were
not removed. Socket tests remain explicit acceptance requirements for the reviewer.

## 2. Files touched in this completion

- `CLAUDE.md`
- `crates/qiyas-ai/src/chatgpt_plan.rs`
- `crates/qiyas-ai/src/chatgpt_plan/oauth.rs`
- `crates/qiyas-ai/src/chatgpt_plan/responses.rs`
- `crates/qiyas-ai/src/chatgpt_plan/tests.rs`
- `crates/qiyas-desktop/src/lib.rs`
- `crates/qiyas-desktop/src/subscriptions.rs`
- `src/api.ts`
- `src/shell/SettingsDialog.tsx`
- `src/shell/SubscriptionSettings.tsx`
- `src/shell/SubscriptionSettings.test.tsx`
- `src/i18n/en.json`
- `src/i18n/ar.json`
- `docs/2026-10-23-qiyas-subscriptions.md`
- `docs/2026-10-23-qiyas-goal-s.md`

No dependencies were added; LICENSES and third-party inventories are unchanged.
Worktree-local MCP acceptance helpers and evidence remain under `.local/`.

## 3. Checks and real results

All Cargo commands use this completion's isolated build prefix:

```sh
CARGO_TARGET_DIR=/home/kareem/Work/qs-mind/qiyas/.local/wt/goal-s/target \
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
```

- `cargo fmt --all`: passed; `.local/goal-s-fmt-final.log`.
- `node_modules/.bin/tsc --noEmit`: passed; `.local/goal-s-tsc-final.log`.
- `node_modules/.bin/vitest run --maxWorkers=2`: 118 files, 877 tests passed;
  `.local/goal-s-vitest-final.log`. Includes account switching, slug selection, retained
  sign-in without scope, unconfirmed sign-out, explicit limit resume and the Arabic button.
- Production ChatGPT source storage audit: passed; no file I/O, Claude credential access or
  backend-api path in the reviewed modules. `.local/goal-run/subscription-chatgpt-storage-audit.json`.
- `cargo check --offline -p qiyas-ai -p qiyas-desktop --all-targets`: passed;
  `.local/goal-s-check-targets.log`. The initial check found a form-serializer lifetime
  across the revocation request; encoding before the async request fixed it.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed;
  `.local/goal-s-clippy-final.log`.
- `cargo build --offline -p qiyas-cli`: passed; `.local/goal-s-build-cli-final.log`.
  The isolated cold build took 19 minutes; the final source rebuild took 27 seconds.
  `.local/goal-run/subscription-build-provenance.json` records the accepted binary hash.
- Final formatting verification: `cargo fmt --all -- --check` passed after the last edits;
  `.local/goal-s-fmt-check-final.log`. Workspace Clippy was also rerun after the cancellation
  fixes and passed for the final source.
- `cargo test --offline --no-fail-fast -p qiyas-ai -p qiyas-desktop -p qiyas-cli`:
  exit **101**; **437 passed, 44 failed with socket permission errors, 13 ignored**.
  All 44 failure traces report `Operation not permitted` / `PermissionDenied`; none is
  a failed ChatGPT assertion. Two subprocess result lines are excluded from the totals
  because their parent tests are already counted. `.local/goal-s-rust-tests-final.log` and
  `.local/goal-run/subscription-rust-test-summary.json` contain the evidence.

  | Crate | Passed | Socket failures | Ignored |
  | --- | ---: | ---: | ---: |
  | qiyas-ai | 220 | 42 | 13 |
  | qiyas-cli | 94 | 0 | 0 |
  | qiyas-desktop | 123 | 2 | 0 |

  The required AI `--lib`, `--test surface` and `--test agent` targets are included:
  together **166 passed, 32 socket failures, 12 deferred**. Surface passed all 57 tests.
  All **15 ChatGPT tests that need no socket passed**, covering OIDC validation, exact
  authorization parameters, consent/client binding, scope refusal, stable host/account
  separation, errors, models, streams, token redaction, limit pause and cancellation safety.
  All **12 ChatGPT HTTP/loopback mocks** are explicitly deferred. The thirteenth ignored
  test is the pre-existing takeoff acceptance case.
  The fake Claude argv/environment/cwd/resume, cancellation, credential sentinel/source
  checks and CLI evaluation refusal passed. Its private HTTP MCP integration needs sockets.
  The desktop mock-keychain test passed.

The current sandbox rejects creation of an IPv4 TCP socket with `EPERM` (errno 1);
`.local/goal-run/subscription-socket-probe.json` records the local-only probe. The 12
socket-dependent ChatGPT tests have `Requires a socket-capable runner` annotations and
are not counted as passed here. The reviewer must run:

```sh
CARGO_TARGET_DIR=/home/kareem/Work/qs-mind/qiyas/.local/wt/goal-s/target \
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
cargo test --offline -p qiyas-ai --lib chatgpt_plan:: -- --ignored --test-threads=1
```

Use the same Cargo prefix. These tests exercise local mock OAuth, discovery, JWKS,
revocation, models and Responses servers, including concurrent refresh, terminal refresh
recovery, limit pause, changed-body retry, broken streams and request cancellation.
No command in this report should be pointed at a live account for automated verification.

## 4. End-to-end evidence

The fresh isolated `target/debug/qiyas-cli mcp --drawing … --workdir …` ran the stdlib
Python acceptance helpers. These are drawing/tool tests, not claims of live subscription
verification or substitutes for the mock HTTP acceptance above.

| Run | Read-back evidence | Artifacts |
| --- | --- | --- |
| Four unit cases | Each starts with one line, adds a 4 m × 3 m outline and bilingual labels, and measures **12 m²** in mm, cm, m and in. Every mutation has entity/object/layer readbacks. Save adds the expected measurement outline; reopening preserves geometry and labels. The saved sidecar contains the 12 m² measurement. | `.local/goal-run/subscription-drawing-v4-{mm,cm,m,in}/transcript.jsonl`, each `reopened/transcript.jsonl`, and `.local/goal-run/subscription-drawing-summary.json` |
| Sample BOQ | Six measurements: 42 m, 14 m, 58.29 m², 22.09 m², 3 doors and 5 windows. Comparison produces the two expected conflicts (A.4 and B.2). Exported workbooks are read back with the stdlib inspector. | `.local/goal-run/subscriptions-sample-boq-v3/transcript.jsonl`, exports and summary in that directory; `.local/goal-s-mcp-boq-final.log` |
| Claude evaluation refusal | `claude`, `CLAUDE`, and `claude` with a custom command are refused before a client starts. The custom-command marker is never created. | `.local/goal-run/subscription-cli-refusal/transcript.json`; `.local/goal-s-cli-refusal-final.log` |

Visual inspection completed for the before image, all four unit-case after images and the
sample BOQ capture. The measured outline and English/Arabic 12 m² labels are visible in
all four cases; the sample drawing shows the two comparison callouts.

- Before: `.local/goal-run/subscription-drawing-v4-mm/0004-before-0.png`
- After: `.local/goal-run/subscription-drawing-v4-{mm,cm,m,in}/0025-after-0.png`
- Sample BOQ: `.local/goal-run/subscriptions-sample-boq-v3/0125-sample-plan-0.png`
- Drawing run log: `.local/goal-s-mcp-drawings-final.log`

The headless MCP contract reopens a standalone drawing without loading its project sidecar;
its reopened takeoff count is therefore zero. The separate sidecar readback proves the
measurement was saved. This behavior is unchanged. Asking for a window screenshot in
headless mode returns `needs_window` without changing drawing contents.

## 5. Remaining acceptance and founder decisions

- The reviewer runs socket-dependent mocks on a socket-capable runner. The founder alone
  tests live sign-in, model availability, plan usage and sign-out on his own accounts.
- Before paid/hosted ChatGPT distribution, complete OpenAI's interest form and resulting
  requirements. Products running Claude Code need the applicable Anthropic Commercial Terms.
- Existing Claude limitations remain: Unix cancellation only, and **Work autonomously**
  mode only. Per-job/per-phase approvals and other platforms are refused, never bypassed.
- Provider terms and account eligibility remain controlled by the providers. The supplied
  verification is dated 23 October 2026; no unsupported credential route has been added.

## Socket-dependent failures for the reviewer

These existing test assertions were retained. Each failed before completing its local
server setup in this sandbox; rerun them on a socket-capable runner.

```text
models::tests::an_unreachable_service_falls_back_to_the_builtin_list
claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo
a_marked_region_lists_what_is_in_it
attached_pictures_go_before_the_words
an_answer_that_keeps_running_out_stops_at_the_cap
an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room
all_server_failures_retry_with_a_visible_wait_and_identical_request
a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user
a_selection_is_listed_and_shown
a_question_ends_the_turn_cleanly_and_the_answer_carries_on
api_key_absent_from_logs_and_errors
billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report
cancel_stops_within_one_second
cancel_between_tool_calls_runs_no_more_of_them
file_tools_without_a_host_say_to_use_the_app
invalid_output_value_keeps_the_cap_and_does_not_retry
history_is_append_only_and_system_prompt_is_stable
malformed_tool_input_still_stops_after_40_calls
invalid_tool_json_is_answered_with_an_error_and_not_run
loop_stops_after_40_tool_calls
partial_usage_chunks_retain_prior_counts_through_the_http_adapter
openai_compatible_drops_a_refused_field_and_asks_again
openai_compatible_streams_text_and_tool_calls
rejected_request_after_find_entities_is_logged_without_retry
text_stream_is_forwarded_in_order
refusal_stop_reason_surfaces_category
tool_use_round_trip_executes_against_the_session
mid_stream_overload_discards_partial_text_and_keeps_a_retry_log
old_pictures_give_way_to_a_line
gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn
view_image_stops_when_the_image_budget_is_used
removals_and_resolutions_stop_after_10_in_one_turn
network_tests::model_discovery_never_follows_a_redirect_with_credentials
other_origins_are_refused_even_with_the_token
execute_request_ids_replay_without_another_mutation_or_ui_event
concurrent_counts_both_land
tool_errors_are_error_results
lists_the_catalogue_and_measures_as_mcp
a_restart_on_the_same_port_binds_again
sheet_layer_and_zoom_changes_reach_the_window
a_wrong_token_is_refused_with_401
view_image_is_an_image_block
commands::tests::the_mcp_server_follows_the_settings
credentials_follow_the_saved_address_and_never_a_draft
```
