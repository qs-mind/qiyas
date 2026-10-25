# Goal C — AI context, evidence and cache reuse

Worktree: `goal/c`, based on `5953604`. Code and fast checks are the final scope agreed by the founder. End-to-end runs and the acceptance loop are **left to the reviewer's combined run**. No new manual headless acceptance runs were started after that scope change. Ordinary Cargo integration tests still exercise their existing CLI/mock fixtures. Earlier completed evidence is retained below; it is not a final acceptance claim.

No real model calls, real credentials, new dependencies, commits or pushes. No founder or other-goal checkout was edited. No Open CAD Studio code was used.

## Changes and fast regression coverage

Test names below are in `qiyas-ai` unless a frontend file is named. The listed unit and frontend tests passed; blocked or ignored transport tests are marked explicitly. Full check results appear in the next section.

| Item | What changed | Fast test coverage |
| --- | --- | --- |
| C1 | History appends between trims. Estimated-token thresholds are 100,000 high, 55,000 low, with at least 30,000 cleared per ordinary trim. Four newest detailed results and the first digest survive. Fixed placeholders retain call id, operation, important handles/ids, errors and immutable artifact addresses. | `history::tests::{short_jobs_are_append_only_even_with_duplicate_reads,watermark_trim_keeps_words_pairs_and_signatures_and_clears_a_large_batch,keeps_recent_results_digest_ids_errors_and_protocol_pairs}`. |
| C2 | Workflow, state, budget, cost and image guidance enters newly appended messages. Duplicate reads, old workflow blocks and picture retirement no longer rewrite the prefix on ordinary requests. | `agent::tests::later_turns_append_without_removing_earlier_workflow_or_changing_the_cache_key` (passed); integration `history_is_append_only_and_system_prompt_is_stable` is blocked by loopback permissions. |
| C3 | Agent and MCP use compact envelopes, capped handle samples with counts, grouped check findings and advice once per rule. Empty defaults are omitted. UI events retain full handles. Full evidence is saved before display rounding, with 12,000-byte UTF-8 artifact pages, historical and current revisions, `detail: full` and field selection. Undo also reports removed handles. Full record reads recover long arrays through exact field reads; ordinary summaries retain native pagination. Exports return paths and row counts. | `result::tests::{batch_envelope_preserves_counts_and_full_precision_in_paginated_evidence,full_entity_reads_keep_precision_in_mm_cm_m_and_inches,full_record_geometry_is_not_cut_at_the_native_array_display_limit}`; `artifact::tests::pages_reassemble_exact_quantities_and_arabic_and_reject_paths`. The four-unit test also checks undo, old artifact revisions and stale-edit rejection. |
| C4 | Discovery contracts have content versions and full-evidence pointers. Repeated contracts become references in the new reply; previous replies stay unchanged. Family contracts link qualified names. Command lists and large kits paginate. Native CLI MCP reconnections retain discovery state. | `result::tests::{contracts_are_versioned_without_revision_churn_and_repeated_only_by_reference,discovery_pages_reach_every_contract_and_full_detail_stays_complete}`; `mcp::tests::native_conversation_keeps_discovery_references_and_blocks_unknown_pictures`. |
| C5 | Full compaction starts at 150,000 estimated tokens, retaining a token-sized recent window and complete pairs. The local note preserves exact user requests, including literal generated-looking tags, first digest, open tasks, discovered contracts, failed outcomes and artifact addresses. It makes no extra model request; system prompt and tools remain stable. | `agent::tests::{a_long_conversation_is_compacted_and_goes_on,one_long_turn_loses_its_oldest_steps_and_keeps_the_newest_whole,token_compaction_keeps_recent_pairs_all_requests_and_failed_outcomes,compaction_preserves_user_words_that_look_like_generated_guidance}`. |
| C6 | Anthropic tools, system and stable-history boundary use one hour; the newest automatic boundary uses five minutes, at most four breakpoints. OpenAI-compatible and ChatGPT-plan requests carry a stable conversation cache key. DeepSeek reasoning replays exactly. Argument/result JSON is ordered. Setting and rejected-capability changes log expected rebuilds. | `anthropic::tests::long_lived_breakpoints_precede_the_newest_default_ttl`; `openai::tests::deepseek_reasoning_and_cache_identity_survive_followup`; `chatgpt_plan::responses::tests::cache_key_and_reasoning_prefix_are_stable`; these pure body tests passed. Loopback `mock_openai_keeps_cache_identity_prefix_and_exact_deepseek_reasoning` is sandbox-blocked; `mock_cache_key_and_prefix_survive_plan_followup` is ignored for the reviewer’s socket-capable run. |
| C7 | Logs include total input, cache reads/writes, output and reasoning, per-request and running turn percentages, and below-70% causes. Cumulative cache events persist after cancellation or a later failed request. Retrying an unfinished answer removes its partial text/thinking while preserving cache totals. Technical details shows one cache row per turn, including restored History. | Provider usage-parsing unit tests; `src/ai/messages.test.ts`: `cache percentage includes cold input, cache reads and cache writes`, `cache totals survive a stopped turn and update without duplicate rows`, `retry discards unfinished thinking across cache telemetry and keeps its usage`; Rust `retry_after_only_thinking_is_visible_and_discards_the_unfinished_thought`. |
| C8 | Duplicate PNG hashes return `unchanged`, including when the image budget is exhausted. Unconfirmed/unsupported models receive no image bytes and plain English/Arabic guidance. Capability comes from model metadata or built-in Claude metadata. ChatGPT settings discovery shares capability metadata with conversations; no paid probe runs. | `agent::tests::identical_captures_are_unchanged_and_unknown_models_receive_no_image`; MCP test above; `chatgpt_plan::tests::model_picture_metadata_is_shared_with_the_conversation_without_a_probe`; model metadata parsing tests. |

Ordinary trims preserve recent detail. An explicit provider context refusal is a separate recovery exception: one logged trim may retire even recent evidence into artifacts to make the retry possible. User words, call/result pairing and reasoning signatures remain intact. `context_refusal_shortens_large_results_once_without_losing_call_pairing` protects that behavior.

## Check results

All Cargo commands use `CARGO_TARGET_DIR=/home/kareem/Work/.qiyas-wt/goal-c/target`, the shared Cargo queue, one build job and one command at a time. Rust tests use two test threads.

- `cargo fmt --all`: passed again on the final Rust sources, `.local/goal-run/fmt-verified.log`.
- Preliminary `cargo test --offline -p qiyas-ai --lib`: 149 passed, five failed, 14 ignored, six seconds after compilation. Four failures were blocked loopback sockets; the fifth exposed the context-refusal regression described above and was corrected. Log: `.local/goal-run/ai-lib-fixed.log`.
- `cargo clippy --offline -p qiyas-ai -p qiyas-cli -p qiyas-desktop --all-targets -- -D warnings`: **passed on the final sources**, 30.56 seconds; `.local/goal-run/clippy-verified.log`. The initial pass found one desktop mock missing the new request field; that fixture was corrected. No workspace-wide Clippy run was made under the revised scope.
- Full required touched-crate run, `cargo test --offline -p qiyas-ai -p qiyas-cli -p qiyas-desktop --no-fail-fast`: completed with failures, `.local/goal-run/tests-final.log`. Excluding two child-process fixture summaries printed by desktop tests: **502 passed, 59 failed, 17 ignored**. There were 47 socket/permission failures and 12 actionable failures; corrections are verified by the targeted reruns below.
- AI regression rerun, `cargo test --offline -p qiyas-ai --lib --test drafting_eval --test surface --test goal_l --no-fail-fast`: **226 passed, four blocked by loopback permissions, 14 ignored**. Breakdown: library 152/4/14; drafting 4/0; surface 69/0; villa regression 1/0. No assertion failures remain in that rerun. Log: `.local/goal-run/tests-regressions.log`.
- CLI rerun, `cargo test --offline -p qiyas-cli --bin qiyas-cli --test agent_eval --no-fail-fast`: **74 passed, zero failed** (66 unit tests and eight agent evaluations); `.local/goal-run/tests-cli-regressions.log`.
- The first desktop event rerun exposed missing image metadata on the visual debug mock. It now declares image support, matching its capture behavior. `cargo test --offline -p qiyas-desktop --lib` then passed **103 tests**, with **one blocked MCP port-binding test**; `.local/goal-run/tests-desktop-verified.log` (7.88 seconds after compilation). Two child-process summaries are excluded from that count.
- `node_modules/.bin/tsc --noEmit`: **passed on the final sources**, `.local/goal-run/typescript-final.log`.
- `node_modules/.bin/vitest run --maxWorkers=2`: **121 files and 915 tests passed**, 102.48 seconds; `.local/goal-run/vitest-final.log`.
- `git diff --check`: passed. All build/test commands finished. Stale test binaries were removed only from this worktree; the final target directory is 9.2 GB.

No test is weakened to hide a failure. Existing expectations changed where they described superseded behavior: moving history edits/picture retirement, five-minute stable Anthropic boundaries, message-count compaction, and the old plan reply without an artifact envelope. Drafting and take-off scripts that inspect full geometry/reports now request `detail: full`; the fake visual provider explicitly advertises image input. Their drawing, quantity, rollback and verification assertions remain. The old CLI short-job test required a moving trim and a 21,000-token cap, which directly conflicts with C1: it now asserts unchanged replies below the 100,000-token watermark (75,000 with that harness’s bytes/4 estimate), retains request/cost stopping checks, and finishes before the reserved final 20% of the cost budget. Large-job savings remain a separate deferred clinic gate. The new expectations check the new contract explicitly.

## Reviewer's combined run

Each end-to-end gate is **left to the reviewer's combined run**:

| Gate | What to check |
| --- | --- |
| Clinic offline replay | Use the copied clinic fixtures with ignored `cache_replay::clinic_policy_replay` (`QIYAS_CACHE_REPLAY` points to `/home/kareem/Work/.qiyas-wt/goal-c/.local/goal-run/cache`). Require at least 60% less estimated total input and at least 90% ideal reusable prefix. Inspect its explicit missing-tail and character-to-token limitations. Do not equate this with observed provider cache residency. |
| Intent and protocol retention | Across ordinary requests, trims, full compaction and resume, preserve user requests word for word, open tasks, first digest, call/result pairs and provider reasoning signatures. Confirm every retired quantity remains reachable by artifact id. |
| Provider mock transport | On a runner allowing loopback sockets, run the provider mocks, including the ignored ChatGPT cache-key test. Compare stable prompt content bytes, cache key identity, TTL order and four-breakpoint limit. Anthropic cache-control marker placement is transport metadata and moves as the prefix grows; prompt content must stay identical. |
| Drafting and take-off | Run scripted-agent drafting/take-off workflows and the combined CLI MCP job on the merged code. Read back every edit, verify all-or-nothing batches, undo, full quantities and paginated evidence. Exercise mm, cm, m and in. |
| Discovery and concise results | Read all pages of large kits/contracts; ensure repeats reference previous versions. Compare result sizes and verify full artifacts retain quantities, reports and geometry. Check export paths and row counts. |
| Pictures | Confirm the second identical capture returns `unchanged`; metadata-confirmed models receive images and unknown/unsupported models receive only guidance. Inspect the resulting drawing PNGs. |
| Cache telemetry | Check successful, cancelled and subsequently failed turns in Technical details and support logs. Confirm totals include cache writes and drops below 70% name the known cause. |
| Live warm sessions, Goal X | At least 80% weighted cache hits on every provider used. No live model service was called here. |

## Evidence completed before the scope change

These earlier runs are retained, not repeated under the revised scope. The before binary already included surviving history/provider edits but did not yet wire the result formatter. Thus it compares result/capture behavior; it is not a pristine whole-application baseline. The preserved clinic log and old simulator provide the history baseline of 8,546,240 total input tokens and 3,658,880 cache reads (42.8%).

| Measurement | Before | First updated replay |
| --- | ---: | ---: |
| Discovery reply, first page | 60,134 bytes (all 16 contracts) | 6,935 bytes (four contracts, paginated) |
| Identical discovery repeat | 60,134 bytes | 1,486 bytes |
| Forty-circle batch reply | 15,614 bytes | 2,659 bytes |
| Forty-row take-off batch reply | 67,667 bytes | 7,318 bytes |
| Entity-area absolute error | 1.6815971512484928e-7 | 0 |
| Identical second PNG suppressed | No | Yes |

Both completed runs created/read back 40 circles and 40 take-off rows, verified an XLSX with three worksheets, checked failed-batch rollback and undid both batches. Updated artifacts reconstructed the saved result after undo; using the old artifact's revision for another edit returned `stale_read`. Exact area was `0.04788283184028488`; the old entity query rounded it to `0.047883`. The standalone measure operation was already exact.

Evidence root: `/home/kareem/Work/.qiyas-wt/goal-c/.local/goal-run/`. These files stay in Goal C’s worktree; they are not the founder checkout’s older `.local/goal-run` files. Paths below are relative to this evidence root:

- Before: `before/transcript.jsonl` (55 calls), `before/metrics.json`, `before/replay.log`, `before/build-resume2.log`.
- First update: `after/transcript.jsonl` (69 calls), `after/metrics.json`, `after/replay.log`, `after/build.log`.
- Inspected PNGs: `before/0043-same-view-first-0.png`, `before/0055-after-undo-0.png`, `after/0051-same-view-first-0.png`, `after/0069-after-undo-0.png`. The circles and restored plan were visually consistent.
- Replay tools: `mcp_client.py`, `replay.py`; original simulator/fixtures in `cache/`. Production clinic replay was not run; its gates are deferred.

All owned MCP children exited after their runs. Later changes (undo envelopes, full record geometry, interrupted-turn telemetry and visual-provider metadata) are covered by fast tests, not those older transcripts.

## Files and ownership

`crates/qiyas-ai/src/`: `agent.rs`, `conversation.rs`, `history.rs`, new `artifact.rs`, new `result.rs`, new `cache_replay.rs`, `lib.rs`, `surface.rs`, `surface_args.rs`, `execute.rs`, `revision.rs`, `mcp.rs`, `provider.rs`, `models.rs`, `anthropic.rs`, `openai.rs`, `claude_code.rs`, `chatgpt_plan.rs`, `chatgpt_plan/responses.rs`, `chatgpt_plan/tests.rs`; integration fixtures `crates/qiyas-ai/tests/{agent,surface,drafting_eval}.rs` and `tests/support/scripted.rs`.

Frontend: `src/api.ts`, `src/ai/AssistantPanel.tsx`, `src/ai/messages.ts`, `src/ai/messages.test.ts`, `src/shell/ModelPicker.tsx`, `src/i18n/en.json`, `src/i18n/ar.json`. Documentation: `CLAUDE.md` and this report.

Touches another goal:

- A: `surface_args.rs` adds presentation options and artifact pagination contracts. No prompts or behavior-judgment rules changed.
- W: three `crates/qiyas-desktop/src/subscriptions.rs` test fixtures supply the new image-capability field; one request fixture in `crates/qiyas-desktop/src/mock.rs` supplies the cache key, and that visual debug mock explicitly declares image support. `commands.rs` also checks that the new cache event precedes a tool call. No tab, session or window state restructuring.
- CLI: `crates/qiyas-cli/src/eval.rs` forwards model image capability through the metered provider. `eval_workflow_tests.rs` supplies cache identity and explicit full-detail requests for deterministic authoring, and checks append-only short jobs below the token watermark. `tests/support/fake_agent.py` explicitly requests the full kit its authoring fixture consumes.

No quantity evaluation, derivation, room tracing/island, transaction or Cargo-profile code changed. No dependencies were added, so LICENSES needs no change.

## Limits and decisions

- The unmodified native Claude CLI owns its private inner history and provider cache controls. Qiyas controls its outer conversation and MCP result sizes/discovery/pictures; it cannot enforce or prove the CLI's inner compaction policy. Unknown/default model aliases remain image-unconfirmed unless metadata identifies them.
- Artifacts persist locally to keep old exact evidence reachable. There is no automatic deletion policy. Storage failure preserves the original full result and reports advice rather than disguising a committed edit as a failure.
- Token counts for local trimming/replay are estimates. The live cache-hit gate is unmeasured here.
- Loopback socket restrictions prevent some mock tests in this sandbox. They must run in the reviewer's socket-capable combined run; no production service or billable probe is a substitute.

## Sandbox-blocked tests

These 47 tests reported `Operation not permitted` while opening their local mock services. They are not counted as passing. Run them with loopback access in the reviewer’s combined run; the full failure details are in `.local/goal-run/tests-final.summary.json`.

<details>
<summary>Exact test names</summary>

- `chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back`
- `chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn`
- `claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo`
- `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
- `a_marked_region_lists_what_is_in_it`
- `a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user`
- `a_question_ends_the_turn_cleanly_and_the_answer_carries_on`
- `a_selection_is_listed_and_shown`
- `all_server_failures_retry_with_a_visible_wait_and_identical_request`
- `an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room`
- `an_answer_that_keeps_running_out_stops_at_the_cap`
- `api_key_absent_from_logs_and_errors`
- `attached_pictures_go_before_the_words`
- `billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report`
- `cancel_between_tool_calls_runs_no_more_of_them`
- `cancel_stops_within_one_second`
- `file_tools_without_a_host_say_to_use_the_app`
- `gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn`
- `history_is_append_only_and_system_prompt_is_stable`
- `identical_malformed_input_stops_on_sixth_attempt`
- `identical_pictures_keep_the_prefix_and_do_not_resend_images`
- `invalid_output_value_keeps_the_cap_and_does_not_retry`
- `invalid_tool_json_is_answered_with_an_error_and_not_run`
- `mid_stream_overload_discards_partial_text_and_keeps_a_retry_log`
- `mock_openai_keeps_cache_identity_prefix_and_exact_deepseek_reasoning`
- `openai_compatible_drops_a_refused_field_and_asks_again`
- `openai_compatible_streams_text_and_tool_calls`
- `partial_usage_chunks_retain_prior_counts_through_the_http_adapter`
- `refusal_stop_reason_surfaces_category`
- `rejected_request_after_find_entities_is_logged_without_retry`
- `removals_and_resolutions_report_counts_without_blocking`
- `successful_reads_continue_beyond_40_calls`
- `text_stream_is_forwarded_in_order`
- `tool_use_round_trip_executes_against_the_session`
- `view_image_stops_when_the_image_budget_is_used`
- `network_tests::model_discovery_never_follows_a_redirect_with_credentials`
- `a_wrong_token_is_refused_with_401`
- `a_restart_on_the_same_port_binds_again`
- `concurrent_counts_both_land`
- `execute_request_ids_replay_without_another_mutation_or_ui_event`
- `lists_the_catalogue_and_measures_as_mcp`
- `other_origins_are_refused_even_with_the_token`
- `sheet_layer_and_zoom_changes_reach_the_window`
- `tool_errors_are_error_results`
- `view_image_is_an_image_block`
- `commands::tests::the_mcp_server_follows_the_settings`
- `credentials_follow_the_saved_address_and_never_a_draft`

</details>
