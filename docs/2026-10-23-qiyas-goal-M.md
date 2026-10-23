# Goal M: select a measurement method by name

Worktree `.local/wt/goal-m`, branch `goal/m`. No new dependencies, paid model calls,
credential access, commits or pushes. No published standard's wording, clauses,
thresholds or measurement rules were added. Only standard names are product data.

## What changed

1. Settings offers None, POMI (RICS), NRM2 (RICS), SMM7, CESMM4, local practice in
   Egypt/KSA/UAE and Other. English and Arabic labels use the existing window fonts.
   Other accepts a free-text name and restores it when reopened. The saved
   `measurement_method` remains a string, or null for None. Existing local rule
   files are additional choices, displayed by name; they are optional.
2. Missing local files now create an empty method envelope instead of refusing the
   selection. Files are resolved by their declared name, never by treating the
   chosen name as a path. Invalid matching files and duplicate names still fail.
   Existing transactional application to open tabs remains in place.
3. Mizan's opening state and the shared state/kit report the effective drawing
   method, with project precedence. A pending name tells Mizan to write rules from
   its knowledge in its own words with clause references. Its kit batch names the
   chosen method and scope with an empty rules array. The workflow directs it to
   apply the standard's units, deductions, conventions and descriptions, write
   rules through `measurement.set_method`, and list uncertain rules/references in
   its final report. It forbids copying standard text or inventing clauses.
4. Reopening with the same name preserves drawing-authored rules and does not add
   an undo step. A kit with saved rules tells Mizan to reuse them and omits the
   example method replacement. Project overrides still win. Explicitly clearing
   Settings leaves an explicit project override in place.
5. `mcp`, `eval` and `eval-agent` accept `--method "POMI (RICS)"`, or any other name.
   Eval's option overrides its saved choice for that run. The agent harness passes
   the name in its MCP configuration and request; custom runner templates also
   support `{method}`. Headless MCP never loads app settings or credentials.
6. The existing generated quantity and dimension provenance retains the method
   name and cited rule. Verification found a presentation gap: default BOQ
   deliverables omitted those columns, and citation objects could render blank.
   Default English/Arabic BOQ exports now show Method and Source / clause;
   dimension exports show Method alongside readable citations. The standard
   take-off workbook adds those BOQ columns without shifting existing columns.
   Custom report layouts retain their chosen columns and lossless records.

The kit's existing size limit is unchanged. Repeated generic guidance was shortened
without removing kind, basis, named-handle scope, exclusions or self-check duties.

## Before and after evidence

The original selector listed only local files. A missing named file caused
`load_at` to return an error. Baseline source excerpts and the founder audit's
state/kit replies are retained in `.local/goal-run/before-method-selector.txt`,
`before-method-loader.txt` and `audit-before-{state,takeoff_kit}.json`.

The acceptance job `.local/goal-run/named_takeoff.py` uses a copy of the audit's
stdlib MCP client. It starts the rebuilt `target/debug/qiyas-cli mcp` with an
isolated method home and a selected name. It checks state, digest, kit, authored
rules, generated quantities, English/Arabic Excel/PDF and save/reopen reuse. Every
mutation is followed by record snapshots and a direct assertion or file readback.
The geometric fixture is repeated in mm, cm, m and inches.

The scripted provider and headless fixtures use visibly fictitious `TEST-ONLY`
clause references and synthetic geometric rules. These prove orchestration and
provenance, not correctness of any standard interpretation. They contain no rules
from a published standard. No model is invoked.

All four named-method jobs passed. Each changes an empty BOQ to one 20 m² item,
checks eight English/Arabic deliverables plus the take-off workbook, and reopens
with the same selected name without losing the rules. The binary hash stayed
unchanged throughout the run. The final repeat is `named-takeoff-final.log`
(exit 0); `transcript-verification.log` independently checks all exchanges. Each unit has 65 original and 6 reopened MCP
exchanges, for 284 exchanges across four units.

- Transcripts: `.local/goal-run/named-{mm,cm,m,in}/transcript.jsonl` and each
  folder's `reopened/transcript.jsonl`.
- Readbacks: `verified-report.json` in each unit folder and the combined
  `.local/goal-run/verified-summary.json`.
- Inspected PDFs rendered as PNGs: `named-mm/boq-en.png`, `boq-ar.png`,
  `dimensions-en.png` and `dimensions-ar.png` beneath `.local/goal-run/`.
  The method and test clause are readable in both languages. The before PDF
  render, `audit-before-boq.png`, has no method or clause columns.
- Drawing captures: `named-{unit}/0012-before-takeoff-0.png`,
  `0065-after-takeoff-0.png`, and `reopened/0006-reopened-0.png`.
  The inches reopen capture was inspected; geometry remains the same boundary.
- A separate local-file compatibility run passed: `local-rule-file/transcript.jsonl`
  loads `Client rule file` from `methods/unrelated-filename.json`, reuses its rules,
  and retains 20 m². `local-method.log` and `verified-summary.json` record it.
- The offline `eval-agent --method` job passed with the repository's fake stdio
  client: one matched, passed and traceable item; no extras. The generated MCP
  configuration carries `--method` and the name. `eval-method-report.json`,
  `eval-method.log` and `eval-bench/runs/` retain the configuration, MCP events,
  drawing and BOQ. This is an orchestration check, not a model benchmark.
- The public sample audit was replayed with the copied `takeoff_jobs.py`.
  Its 30 MCP exchanges completed without tool errors. All eight quantities equal
  the original audit exactly. Independent straight-geometry/count sums also
  agree. Raw analytic circle/arc values differ from the recognized tessellated
  curves by the same existing approximately 0.000006 m² / 0.000001 m as before.
  The added verifier originally demanded analytic equality for those curves;
  it now checks exact baseline equality for curves and analytic equality for
  straight geometry/counts. Product geometry and existing tests were unchanged.
  Evidence: `takeoff-sample/transcript.jsonl`, `verified-summary.json`,
  `audit-replay-first.log`, and `audit-verification.log`. The inspected
  `takeoff-sample/deliverable.png` shows the new columns on the same public sample
  used for `audit-before-boq.png`.

The shared build directory was later overwritten by another goal's CLI. One extra
compatibility attempt therefore failed at startup with an unknown `--method`
argument; its log is `local-method-wrong-binary.log`. The verified Goal M executable
was recovered from the build cache by matching its recorded SHA-256 and copied to
`.local/goal-run/qiyas-cli`. Later compatibility checks use that private snapshot.
The final rebuilt snapshot has SHA-256
`7ae6aa46e9420559441b18e15773715ef86c0ed706ea01427c030821e34d3db6`.
The four-unit, local-file and fake-agent checks all passed again on that snapshot.
No other build was stopped or cleaned.

Two initial acceptance-script launches failed before starting MCP because of a
missing Python path import. The corrected successful run is retained in
`named-takeoff.log`; the earlier script errors remain in `named-takeoff-first.log`
and `named-takeoff-second.log`.

## Checks

| Check | Result |
| --- | --- |
| `cargo fmt --all` | Passed. Later corrections are formatted directly with rustfmt. |
| TypeScript `--noEmit` | Passed. |
| Full Vitest, `--maxWorkers=2` | Passed: 870 tests in 117 files. |
| Initial CLI build | Failed on a missing `json!` qualification in the new state formatter; corrected to `serde_json::json!`. |
| CLI builds | Corrected build passed (`build-final.log`); final snapshot build also passed (`build-snapshot.log`), both exit 0. |
| Workspace Clippy, all targets, warnings denied | Passed; `clippy.log`, exit 0. |
| Full core, BOQ, desktop and CLI tests | Failed before tests: linker killed by signal 9 while linking the core `write_sample` example; `rust-tests.log`. |
| Focused core tests | Passed: 54 library tests and both method loading/persistence tests. |
| Focused BOQ tests | Passed: 5 library tests and the workbook dimensions test. |
| Focused desktop tests | 95 library and 17 settings tests passed; two local listener tests blocked by the sandbox. |
| CLI tests | All 8 agent-eval tests passed. Final binary suite passed all 65 tests, including the unchanged budget regression; `rust-corrected.log`. |
| Full AI suite | Failed before execution: linker killed by signal 9 while linking `mcp`; `ai-all-tests.log`. |
| AI library and agent tests | 84 library and 3 agent tests passed. One library and 30 agent tests could not bind local listeners in the sandbox. |
| AI surface tests | Final suite passed all 60 tests, including the new scripted provider and method tests and the restored count phrase; `rust-corrected.log`. |
| Headless acceptance and audit replay | Final four-unit job passed: 284 MCP exchanges, 36 exports, save/reopen reuse. Local-file and fake eval-agent runs passed. Sample audit completed; corrected independent verification passed. |
| Final formatting and whitespace checks | Direct rustfmt check and `git diff --check` passed. |

All Cargo commands use the required shared target and four build jobs. Focused
corrective checks add `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
`CARGO_INCREMENTAL=0` to reduce compiler/linker memory. The queued default-debug
required-AI run and superseded queued focused runs were canceled before
execution (exit 130), then consolidated into one invocation. No other agent's build was interrupted and no cache was cleaned.

The focused Rust checks cover all five touched crates: core, BOQ, desktop, AI and
CLI. Across those selected targets, 394 distinct tests passed and 33 local-port
tests were blocked. The broader full-suite attempts did not finish linking, so
this is not a claim that every integration test passed.

Logs and command exit files are in `.local/goal-run/`. No failed check is counted
as a pass. Existing tests were not weakened. The former missing-file error
assertion now requires a valid name-only selection, matching the requested new
behavior. Existing state equality includes its new instruction field; local file
choice labels now assert the file's method name instead of its descriptive title.

## Files and limits

The exact file manifest is `.local/goal-run/files-touched.txt`:

- `CLAUDE.md`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-boq/src/dimensions.rs`
- `crates/qiyas-boq/tests/dimensions.rs`
- `crates/qiyas-cli/src/eval.rs`
- `crates/qiyas-cli/src/eval_agent.rs`
- `crates/qiyas-cli/src/headless.rs`
- `crates/qiyas-cli/tests/agent_eval.rs`
- `crates/qiyas-cli/tests/support/fake_agent.py`
- `crates/qiyas-core/src/deliverables.rs`
- `crates/qiyas-core/src/deliverables/defaults.json`
- `crates/qiyas-core/src/measurement.rs`
- `crates/qiyas-core/src/measurement/methods.rs`
- `crates/qiyas-core/src/takeoff_kit.rs`
- `crates/qiyas-core/tests/measurement_methods.rs`
- `crates/qiyas-desktop/src/methods.rs`
- `crates/qiyas-desktop/src/settings.rs`
- `crates/qiyas-desktop/tests/settings.rs`
- `docs/2026-10-23-qiyas-goal-M.md`
- `src/i18n/ar.json`
- `src/i18n/en.json`
- `src/shell/SettingsDialog.test.tsx`
- `src/shell/SettingsDialog.tsx`


The initial focused run also found two regressions: the compact kit no longer
used the existing count-rule phrase, and longer workflow instructions exhausted
a token-budget fixture early. The phrase was restored and guidance compacted
while retaining every obligation. No budget or test threshold was loosened.

The new reopen regression first used Save on a never-saved drawing. It was
corrected to use Save As; all method, quantity, history and persistence assertions
remain unchanged. Its final execution result is recorded above.

Model knowledge and clause accuracy remain model judgments, explicitly disclosed
when uncertain. There is no bundled rulebook or automatic clause verification.
Custom layouts may intentionally omit provenance columns. No founder decision is
required for the implemented selection and data flow.

## Tests blocked by local-port restrictions

These 33 tests failed while binding local listeners with `PermissionDenied: Operation not permitted`.
The exact failures are in `rust-focused-final.log` and `blocked-port-tests.txt`.

```text
models::tests::an_unreachable_service_falls_back_to_the_builtin_list
attached_pictures_go_before_the_words
billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report
a_selection_is_listed_and_shown
api_key_absent_from_logs_and_errors
a_question_ends_the_turn_cleanly_and_the_answer_carries_on
a_marked_region_lists_what_is_in_it
all_server_failures_retry_with_a_visible_wait_and_identical_request
an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room
a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user
an_answer_that_keeps_running_out_stops_at_the_cap
cancel_between_tool_calls_runs_no_more_of_them
cancel_stops_within_one_second
file_tools_without_a_host_say_to_use_the_app
history_is_append_only_and_system_prompt_is_stable
invalid_output_value_keeps_the_cap_and_does_not_retry
invalid_tool_json_is_answered_with_an_error_and_not_run
loop_stops_after_40_tool_calls
malformed_tool_input_still_stops_after_40_calls
openai_compatible_drops_a_refused_field_and_asks_again
openai_compatible_streams_text_and_tool_calls
rejected_request_after_find_entities_is_logged_without_retry
partial_usage_chunks_retain_prior_counts_through_the_http_adapter
tool_use_round_trip_executes_against_the_session
refusal_stop_reason_surfaces_category
text_stream_is_forwarded_in_order
mid_stream_overload_discards_partial_text_and_keeps_a_retry_log
old_pictures_give_way_to_a_line
gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn
view_image_stops_when_the_image_budget_is_used
removals_and_resolutions_stop_after_10_in_one_turn
commands::tests::the_mcp_server_follows_the_settings
credentials_follow_the_saved_address_and_never_a_draft
```
