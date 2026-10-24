# Goal L: Qiyas reports facts, Mizan decides

Worktree `.local/wt/goal-l`, branch `goal/l`. No commits, pushes, paid model calls or new dependencies. The founder's files are read-only; the villa conversation is tested from a private copy.

Qiyas now applies valid requested changes and reports advice for Mizan to weigh. It still refuses impossible or unsafe edits and stale revisions. Completed answers can carry open findings, visible in the conversation and restored from History.

The refusal inventory groups the individual error sites by the fact they protect. The targeted review is saved as `.local/goal-run/refusal-review.txt`; the broader 855-site scan is `.local/goal-run/refusal-inventory-sites.txt`. Behaviour rules supply advice; they do not decide whether the task is complete.

| Rule and location | Class | Decision |
| --- | --- | --- |
| Requested fields changed by automatic updates (`net_effect`, `derived`) | Behaviour | Apply valid changes; return every requested/actual value, entity, field and update reason. |
| Identical unsuccessful edits (`agent`, `mcp`) | Behaviour / subscription protection | Advice from attempt two. Only five identical failures allow the next identical call to stop. |
| Repeated deletion and rebuilding (`churn`) | Behaviour | Apply; report actual churn, affected handles and cheaper edit commands. |
| New drawing warnings at phase/final completion (`agent`) | Behaviour | Allow the answer and phase; attach open findings to the visible saved turn. |
| Small phases (`surface::read_plan`) | Behaviour | A single phase is allowed with advice. Unique phase IDs and valid status data remain required. |
| Uncited quantities in notes (`quantity_notes`) | Behaviour | Keep the note; advise measuring and citing its source. Quantities in notes do not become measured quantities. |
| Empty unmeasured item (`commands`) | Behaviour | Report that the row has no measurement; do not invent a quantity. |
| Counts restricted to block references (`commands::add_measurement`) | Behaviour | Count the selected objects and advise that each must represent one item. External entities without a retainable count location remain an integrity refusal. |
| New-design destination heuristic (`agent`) | Behaviour | Give the model the context and advice; it decides whether clarification is needed. |
| Destructive-call and tool-call counters (`agent`) | Behaviour | Report counts and cost guidance; no count-based refusal. |
| Invalid JSON, unknown/ambiguous fields, unsupported types or versions, conflicting aliases (`validation`, `registry`, `records`, `surface_args`, `surface`) | Fact/integrity | Keep: the request does not describe an executable operation. Report accepted fields, forms or versions. |
| Missing entities/layers/sheets/blocks, invalid paths, absent window/host, unsupported formats (`core`, `surface`, `host`) | Fact/integrity | Keep: the requested target or capability is unavailable. |
| Impossible dimensions, nonfinite numbers, zero/negative sizes, self-intersections, overlapping openings, no fitting wall range, incompatible constraints (`geom`, `walls`, `param`, draw/edit modules) | Fact/integrity | Keep, including the fitting alternatives and units. |
| Generated parts, hosted openings, retained room boundaries, live schedules, links and ambiguous replacements (`objects`, `walls`, `record_edit`, `handle_refs`) | Fact/integrity | Keep operations that cannot preserve their source/link invariants; name the owner or supported edit. |
| Locked layers, xrefs, wrong sheet/epoch, ownership of undo/redo (`session`, targets, `surface`, `agent`) | Fact/integrity | Keep the actual access/ownership restrictions. |
| Stale record `expected` values and drawing `expected_revision` | Fact/integrity | Keep; no stale mutation. Supply current revision, changes and replacement maps. |
| Transaction failures/panics, invalid batch nesting, corrupt document membership (`transaction`, `session`, Goal K guards) | Fact/integrity | Keep atomic rollback and every integrity regression. |
| Credentials, file confinement, provider permissions, cancellation, user-selected approvals, request-ID conflicts (`agent`, `mcp`, hosts) | Safety/authority | Keep. Advice cannot override the user's permissions or run a different command under the same request ID. |
| Actual memory, schema/payload, image size and provider context limits | Capability/resource integrity | Keep explicit limits; pagination or a smaller request is the available option. |

Before evidence:

- `.local/goal-run/history-before/summary.json`: the copied villa chat has two user turns under `untitled/Untitled-1.dwg`; exact-path filtering finds none for the recovered filename.
- `.local/goal-run/before/transcript.jsonl` and `summary.json`: derivation cancelled a colour/visibility edit; `expected_revision` was unknown; the second label rebuild, one-phase plan and uncited quantity note were blocked. The third identical missing-handle call was stopped.
- `.local/goal-run/before/0008-before-overridden-field-0.png` and `0080-final-0.png`: inspected both. The blocked second deletion left duplicate labels.
- `.local/goal-run/before-units/{mm,cm,m,in}/transcript.jsonl` and `0033-result-0.png`: the same physical wall in all four units; count of its selected centreline was blocked as a non-block reference. Inspected all four pictures.
- `.local/goal-run/scripted-before/scripted-events.json`: the two-bedroom replay had six tool errors and ended at `drawing_checks_unresolved`. Its `scripted-33.png` was inspected: the earlier layout remained, with a room label crossing a door swing.

After evidence already verified:

- `.local/goal-run/scripted-after/`: `scripted-events.json`, `scripted-requests.json`, `scripted-churn.json`, `scripted-20.png`, `scripted-33.png`. The same two-bedroom replay finishes with zero tool errors. Its final picture has walls, openings, both labels and a 9 m dimension. The turn records two text-height findings. The actual churn is 226 entity additions/removals; advice exposes the cost rather than preventing the requested work. Inspected the final picture.
- `.local/goal-run/villa-after/`: `villa-events.json`, `villa-transcript.json`, `villa-final-groups.json`, `villa-final.png`. The Goal K villa inputs run through a scripted Agent with readbacks after every edit. All 44 distinct valid openings remain, every wall-group member resolves, and the turn finishes. Seven impossible overlapping/ill-fitting retries still fail. Inspected the final picture. This verifies the logged drawing operations, not new architectural design decisions by a paid model.
- `.local/goal-run/history-after.json`: actual desktop IPC flow with isolated settings, an untitled drawing, autosave, crash-copy recovery, History, Save As, then store restart. Three source chats (including the copied founder chat with two user turns) plus autosave and crash-copy chats yield five chats in recovered History and five after Save As. The founder's original was never modified. This is a desktop command-flow test, not a manual window recording.

Revision contract:

`revision` contains a session identity, the existing document revision, a monotonic knowledge/history sequence, the effective sheet and a working-context fingerprint. Read and edit results carry it; image results put it in the caption; no open drawing gives null. `expected_revision` is optional for native/MCP callers, and nullable in the strict provider schema. A mismatch returns `stale_read` before mutation, with added/changed/removed handles and replacement maps. Rebuild results always carry `handle_map`, including retained identities and removed handles. History retains 128 revisions and bounds each entry to 32,768 handle/map entries. Expired, truncated or untracked history explicitly says `history_complete: false` and asks for a full reread. Undo, redo, sheet/context and table changes invalidate old reads too. Request-ID retries return the original cached result without another mutation.

The take-off kit's old workflow list named `revision` is now `revision_commands`, so the list cannot be overwritten by the read token. Native typed outcomes remain Rust enums; the wire report adds metadata.

New untitled drawings also skip paths already used by chat history, preventing a fresh drawing from inheriting an earlier drawing's chats after Save As.

Old tests for blocking valid edits were changed to assert actual applied values, advice and readback. The retained-room mixed-move test now requires the unrelated line to move, the derived room area to stay correct, explicit override advice, and one-step undo in all four units. A missing numeric value in a normal user item remains invalid unless the user explicitly asks for an empty item. Invalid geometry, link integrity, locked-layer, xref, atomic rollback, unknown-field and strict-provider-schema tests remain. The strict schema regression caught and corrected the nullable revision envelope; the kit workflow regression caught and corrected the naming collision.

Headless after runs:

- `.local/goal-run/after/transcript.jsonl` has 87 exchanges; `.local/goal-run/after/summary.json` retains the compared results. `0083-final-0.png` was inspected. Colour changes survive; the overridden visibility request reports `requested: true`, `actual: false`, its handle, property and derivation reason. The stale edit changes nothing, lists five added and six changed handles plus replacement maps; the next read and revision-checked edit succeed. Repeated failures one through five run, attempts two through five advise, and only attempt six stops. Rebuilding the label succeeds and the final read contains one label. The single-phase plan and uncited note both persist with advice.
- `.local/goal-run/after-units/{mm,cm,m,in}/transcript.jsonl`: 34 exchanges in each unit. Each stale edit refuses, a fresh read/edit succeeds, counting one selected object gives one with advice, and retrying one request ID leaves exactly one label. Inspected all four `0034-result-0.png` captures. Every tool read/edit/capture in these transcripts carries revision metadata.
- Reproduction clients: `.local/audit/mcp_client.py`, `.local/audit/goal_l.py`, `.local/audit/goal_l_units.py`. They use Python's standard library and the worktree's `target/debug/qiyas-cli mcp --drawing … --workdir …`, saving readbacks after mutations. Baseline executable and SHA are in `.local/goal-run/qiyas-cli-before` and `before-cli.sha256`.

Compatibility and limits:

- Older clients may omit `expected_revision`. They gain metadata, but stale protection requires supplying the token. Array/scalar wire reads now use `{value, revision}`; object result fields are retained. Tool kit consumers must use `revision_commands` for revision-workflow examples.
- The in-app failure count resets each user turn. MCP has no user-turn notification: each connection starts afresh and successful `finish` ends a task and resets the counters. Cached request IDs retain their original result.
- Revision history is deliberately bounded and process-local. A reopened drawing requires a new read. A missing interval is reported as incomplete, never presented as a full change list.
- No paid provider was called. The scripted providers verify tools, retained drawings, completion and recorded advice; they cannot prove how a live model will weigh that advice.
- Recovery was verified through the real desktop IPC commands and isolated conversation store, including the copied real chat. No founder settings or conversation files were changed. No manual GUI recovery session was recorded.

Files touched:

- Documentation: `CLAUDE.md`, `docs/2026-10-24-qiyas-goal-L.md`.
- crates/qiyas-ai: `src/agent.rs`, `src/churn.rs`, `src/errors.rs`, `src/execute.rs`, `src/host.rs`, `src/judgement.rs`, `src/lib.rs`, `src/mcp.rs`, `src/normalise.rs`, `src/prompt.rs`, `src/prompt_ar.txt`, `src/prompt_en.txt`, `src/surface.rs`, `src/surface_args.rs`, `tests/agent.rs`, `tests/authoring.rs`, `tests/drafting_eval.rs`, `tests/goal_j.rs`, `tests/surface.rs`, `tests/tools.rs`, `src/advice.rs`, `src/revision.rs`, `tests/goal_l.rs`.
- crates/qiyas-core: `src/commands.rs`, `src/derived.rs`, `src/error.rs`, `src/input.rs`, `src/lib.rs`, `src/net_effect.rs`, `src/quantity_notes.rs`, `src/session.rs`, `src/takeoff_kit.rs`, `src/transaction.rs`, `src/undo.rs`, `tests/conveniences.rs`, `tests/goal_j.rs`, `tests/honest_commands.rs`, `tests/object_regions.rs`, `src/read_revision.rs`, `tests/read_revision.rs`.
- crates/qiyas-desktop: `src/commands.rs`, `src/conversations.rs`, `src/drawing_host.rs`, `src/files.rs`, `src/mcp.rs`, `src/projects.rs`, `src/script.rs`, `src/tabs.rs`.
- src: `ai/AssistantPanel.test.tsx`, `ai/AssistantPanel.tsx`, `ai/messages.test.ts`, `ai/messages.ts`, `api.ts`, `i18n/ar.json`, `i18n/en.json`.

Check results:

- `cargo fmt --all`: passed; `.local/goal-run/fmt-final.log`. Files subsequently corrected were formatted directly with rustfmt.
- `node_modules/.bin/tsc --noEmit`: passed; `.local/goal-run/tsc-final.log`.
- `node_modules/.bin/vitest run --maxWorkers=2`: passed, 121 files and 912 tests; `.local/goal-run/vitest-final.log`.
- Both fixed prompts meet the limit: English 855 words, Arabic 871.
- Final CLI rebuild and all five MCP jobs passed: `.local/goal-run/build-reviewed.log`, `mcp-reviewed.log`, `mcp-reviewed-units.log`. The final executable checksum is `after-cli.sha256`.
- The first combined final Rust run ended with signal 15 (exit 143), before test results. `.local/goal-run/cargo-test-final.log`. The cause was not established; touched-crate reruns follow below.
- The all-core test build failed when the linker was killed with signal 9 while linking `viewports` (`cargo-test-core.log`). The replacement runner keeps the required Cargo environment but selects one target at a time: `.local/goal-run/run-test-targets.sh`. Per-target logs are under `.local/goal-run/test-targets/`; exit codes are in `test-target-results.tsv`.

The full suite was selected across 107 library, integration, binary and documentation targets. Initial behaviour/metadata assertion failures were corrected and their complete targets rerun. No failing integrity test was disabled, ignored or loosened.

| Check | Result |
| --- | --- |
| Core crate | 821 passed. Full run plus corrected-target reruns; retained-room regression, original integrity checks and unit variants all pass. |
| AI crate | 271 passed, 44 socket-denied, 15 already ignored. Includes all 69 surface tests, the villa replay and the two-bedroom replay. |
| Desktop crate | 132 passed, 2 socket-denied. This includes 103 library passes, 10 options passes and 19 settings passes. Binary/doc targets passed. |
| TypeScript | Passed. |
| Frontend | 912 tests passed in 121 files. |
| Workspace Clippy | Passed with `-D warnings`; `.local/goal-run/clippy-reviewed.log`. |

All socket-denied failures return `Operation not permitted` when creating or binding local test sockets. No paid provider call was made. The complete machine-readable list is `.local/goal-run/sandbox-test-failures.json`. These tests need a run where local sockets are permitted:

- `qiyas-ai-agent`: `a_marked_region_lists_what_is_in_it`.
- `qiyas-ai-agent`: `a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user`.
- `qiyas-ai-agent`: `a_question_ends_the_turn_cleanly_and_the_answer_carries_on`.
- `qiyas-ai-agent`: `a_selection_is_listed_and_shown`.
- `qiyas-ai-agent`: `all_server_failures_retry_with_a_visible_wait_and_identical_request`.
- `qiyas-ai-agent`: `an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room`.
- `qiyas-ai-agent`: `an_answer_that_keeps_running_out_stops_at_the_cap`.
- `qiyas-ai-agent`: `api_key_absent_from_logs_and_errors`.
- `qiyas-ai-agent`: `attached_pictures_go_before_the_words`.
- `qiyas-ai-agent`: `billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report`.
- `qiyas-ai-agent`: `cancel_between_tool_calls_runs_no_more_of_them`.
- `qiyas-ai-agent`: `cancel_stops_within_one_second`.
- `qiyas-ai-agent`: `file_tools_without_a_host_say_to_use_the_app`.
- `qiyas-ai-agent`: `gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn`.
- `qiyas-ai-agent`: `history_is_append_only_and_system_prompt_is_stable`.
- `qiyas-ai-agent`: `identical_malformed_input_stops_on_sixth_attempt`.
- `qiyas-ai-agent`: `invalid_output_value_keeps_the_cap_and_does_not_retry`.
- `qiyas-ai-agent`: `invalid_tool_json_is_answered_with_an_error_and_not_run`.
- `qiyas-ai-agent`: `mid_stream_overload_discards_partial_text_and_keeps_a_retry_log`.
- `qiyas-ai-agent`: `old_pictures_give_way_to_a_line`.
- `qiyas-ai-agent`: `openai_compatible_drops_a_refused_field_and_asks_again`.
- `qiyas-ai-agent`: `openai_compatible_streams_text_and_tool_calls`.
- `qiyas-ai-agent`: `partial_usage_chunks_retain_prior_counts_through_the_http_adapter`.
- `qiyas-ai-agent`: `refusal_stop_reason_surfaces_category`.
- `qiyas-ai-agent`: `rejected_request_after_find_entities_is_logged_without_retry`.
- `qiyas-ai-agent`: `removals_and_resolutions_report_counts_without_blocking`.
- `qiyas-ai-agent`: `successful_reads_continue_beyond_40_calls`.
- `qiyas-ai-agent`: `text_stream_is_forwarded_in_order`.
- `qiyas-ai-agent`: `tool_use_round_trip_executes_against_the_session`.
- `qiyas-ai-agent`: `view_image_stops_when_the_image_budget_is_used`.
- `qiyas-ai-key_safety`: `network_tests::model_discovery_never_follows_a_redirect_with_credentials`.
- `qiyas-ai-lib`: `chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back`.
- `qiyas-ai-lib`: `chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn`.
- `qiyas-ai-lib`: `claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo`.
- `qiyas-ai-lib`: `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`.
- `qiyas-ai-mcp`: `a_wrong_token_is_refused_with_401`.
- `qiyas-ai-mcp`: `a_restart_on_the_same_port_binds_again`.
- `qiyas-ai-mcp`: `concurrent_counts_both_land`.
- `qiyas-ai-mcp`: `execute_request_ids_replay_without_another_mutation_or_ui_event`.
- `qiyas-ai-mcp`: `lists_the_catalogue_and_measures_as_mcp`.
- `qiyas-ai-mcp`: `other_origins_are_refused_even_with_the_token`.
- `qiyas-ai-mcp`: `sheet_layer_and_zoom_changes_reach_the_window`.
- `qiyas-ai-mcp`: `tool_errors_are_error_results`.
- `qiyas-ai-mcp`: `view_image_is_an_image_block`.
- `qiyas-desktop-lib`: `commands::tests::the_mcp_server_follows_the_settings`.
- `qiyas-desktop-settings`: `credentials_follow_the_saved_address_and_never_a_draft`.

Final corrections and checks:

- Focused correction logs: `.local/goal-run/corrected/`. `measure` 19/19, `object_regions-reviewed` 5/5, `conveniences` 9/9, AI `authoring` 19/19, `drafting_eval` 4/4, `tools` 24/24 and `surface` 69/69. The mixed-room regression now verifies applied movement, retained area, exact requested/actual advice and undo; the original curved-island integrity assertion is unchanged.
- Workspace Clippy initially flagged a helper after its test module. Moving it before the tests fixed the lint; the complete workspace/all-targets rerun passed (`clippy-reviewed.log`, status 0).
- Final changed-Rust-file format check passed (`rustfmt-reviewed.log`, status 0). `git diff --check` passed.
- `.local/goal-run/test-summary.json` consolidates the full targets and their corrections: 1,224 Rust passes, 46 socket-denied failures, 15 pre-existing ignored tests. The initial runs remain saved, including the resource failures and corrected assertions.
- The final CLI build passed and all five headless MCP jobs passed again against that executable. The final transcripts contain 223 exchanges across the main case and four unit variants. Every tool result carries a revision. All five final PNGs were inspected again after this run. `.local/goal-run/final-checks.json` records the final results.

No founder decision is needed for the implemented changes. Remaining verification is a socket-enabled run of the 46 listed tests and, if desired, a manual desktop window recording. The compatibility limits above (optional tokens, finite revision history, MCP task boundary and renamed kit field) are explicit; no commits or pushes were made.
