# Goal D — Mizan's judgement

Worktree: `.local/wt/goal-d`, branch `goal/d`. No commit or push. No paid provider calls, credentials, new dependencies or licence changes.

## Changes and the evidence they address

| Item | Before | After |
|---|---|---|
| Drawing versus quantity work — M2, X-C2, X-C5 | Every system prompt and 80% notice asked for take-off completion. A drawing-only headless finish failed for having zero BOQ items. Desktop finish called Save. | The request and actual quantity operations determine scope. Generic drawing jobs get drawing checks and remaining-work guidance. Quantity workflow is injected only for quantity jobs. `finish` records completion without saving. Drawing-only completion does not require BOQ rows. Explicit Save and EOF retention remain separate. |
| Repeated edits — M11, P5, X-C1 | Identical ineffective edits could run until the full tool budget was gone. | After two failures or no-effect results, the identical third edit is refused, with a plain explanation. Reads between attempts do not reset the guard. A successful intervening edit permits a fresh attempt. The agent and MCP enforce it. |
| Memory — M9, P9, X-C3, X-C4 | Results older than three were stripped; schemas disappeared. Compaction dropped whole earlier conversations, clipped requests at 2,000 characters, and called failed attempts “done.” | Twenty recent results and successful discovered contracts remain whole. Identical successful reads share their newest full evidence; changed values are never deduplicated. Pictures follow their own retention policy. Compaction keeps the newest 160 messages, rounding to complete call/result pairs, plus a memory note. All request blocks survive without clipping, including corrections merged after interruptions. Call summaries distinguish success, failure and incomplete calls; success is not claimed as verified. |
| Window — M10 | A screen capture showed only the drawing canvas, with no panel state. | `panel_state` reads the existing window request/reply channel. It reports visible panels, the active tab, current layer, flat/folder view, visible groups and expanded state, layer-row selection, open layer controls, drawing selection and dialogs. It changes nothing. Without a window it says so clearly. Prompts require explanation without unsolicited edits. |
| Workflow and language — X-C7, D1, D21 | No redo; workflow was always English; warnings did not stop completion. | `redo_own` and typed `REDO` restore the original next step only for its owner. Workflow and budget guidance follow chat language; a new drawing turn removes prior injected quantity guidance. New errors and warnings prevent clean completion; autonomous work gets two correction requests, then an incomplete result if findings remain. Prompts require readable labels, clear placement, and visual review. |

The fixed rules contain **882 English words and 869 Arabic words**, below the unchanged 900-word limit. The separately requested quantity guides contain 897 English words and 628 Arabic words. The command index is neutral. Full quantity guidance has its own `qiyas_read workflow {language: "en"|"ar"}` read, so the existing 6,144-byte kit limit is preserved.

A numeric read of a drawn entity does not start a take-off. Quantity-row commands and recognition/measurement work do. Room and opening schedules remain drawing work. This is based on command metadata and generic request wording, without villa or layer-name rules.

## End-to-end runs

The stdlib MCP client was copied from the original audit into `.local/goal-d/`. Runs use the CLI built from this worktree. Because other goal builds replace the shared executable, a private copy is taken immediately after building and its hash recorded. No other worktree is modified.

- `.local/goal-d/verify.py`: four 5 × 4 m drawing jobs stored in mm, cm, m and inches. Every mutation is followed by reads. It checks 20 m², undo removal, redo with the original handle, English/Arabic labels, a read-only screen question, finish without a file change, two no-op attempts followed by refusal, and explicit saving.
- `.local/goal-d/flat_job.py`: the original audit flat script with only its input/output paths changed. Its original `redo-typed-command` and `finish-drafting-job` probes are retained. This broad exploratory script also probes issues assigned to other goals; those are reported separately from Goal D.
- `.local/goal-d/takeoff_jobs.py`: the original audit's sample take-off job, confined to this worktree. It reads the source, runs recognition and measurement, reads the generated quantities back, exports reports and records completion.
- Scripted provider runs exercise drawing and quantity requests, Arabic chat in an English interface, the 80% notice, contract retention, the loop guard and warnings. These use canned provider responses, not a paid or subscription model.

The evidence root is `.local/goal-run/`. `verified-evidence.json` records the CLI hash, exact transcript and PNG paths, and original before/after responses. `bin/build.json` identifies the executable copied from the shared target.

| Run | Evidence | Result |
|---|---|---|
| Drawing in mm / cm / m / in | `drawing-<unit>/transcript.jsonl`, 68 requests each | 20 m² each; undo removes the shape; redo restores its original handle; zero drawing warnings; panel read changes nothing; finish changes no file; third identical no-op is refused; explicit Save succeeds. |
| Original flat audit | `audit-flat/transcript.jsonl`, 147 requests | Original step 140 no longer rejects REDO as unknown. Its redo stack is empty, so it correctly answers `redone:false`; the four dedicated jobs prove actual restoration. Original step 144 changes from `failed: done with 0 submitted items` to `job:drawing, status:done`. |
| Original take-off audit, sample case | `takeoff-sample/transcript.jsonl`, 30 requests | Eight quantity totals match independent record sums within 0.00001 absolute/relative tolerance. Largest difference: 0.0000062818 m² for the circle. Excel/PDF exports and take-off completion succeed. This is geometric scope, not a complete measured specification. |
| Scripted Mizan turns | `scripted/drafting-en-requests.json`, `takeoff-en-requests.json`, `takeoff-ar-requests.json`, with corresponding `-events.json` files | Drawing has no injected quantity guide; quantity work has the right language; the 80% notices differ; the first command schema survives more than 33 later reads. |

PNGs inspected: `drawing-mm/0028-before-undo-en-0.png`, `drawing-mm/0063-after-redo-en-0.png`, `drawing-cm/0063-after-redo-ar-0.png`, `takeoff-sample/0030-completed-0.png`, and `audit-flat/0128-flat-annotated-0.png`. The new drawing labels are readable and clear of the outline; Arabic letters join correctly. Corresponding m/in captures are retained. The broad flat audit still has overlapping labels/tables from the original layout script. This work does not claim those other-goal layout defects are fixed. The original comparison image is `/home/kareem/Work/qs-mind/qiyas/.local/audit/flat/0128-flat-annotated-0.png`.

Reproduction clients: `.local/goal-d/verify.py`, `flat_job.py`, `takeoff_jobs.py`, and `check_evidence.py`. They use stdlib Python MCP over stdin/stdout; each drawing mutation is followed by record/state readback. Logs are `headless-final.log`, `audit-flat-final.log`, `audit-takeoff-final.log`, and `evidence-check.log`.

## Tests changed for corrected behaviour

The previous prompt-index tests required unconditional take-off instructions. They now require the neutral index while still checking registry membership. Quantity rules moved out of drafting prompt assertions and into the workflow checks. The old autonomous completion test expected “Done” with unresolved drawing errors; it now requires correction attempts and an incomplete stop. Duplicate-line repair fixtures use a named layer so their intended repair does not leave an unrelated layer-zero warning. Compaction's count assertion allows the retained 160-message tail, the memory/boundary overhead, and the newly appended user/assistant pair; a new test checks the retained tail byte for byte and preserves a long brief and merged correction.

The append-only protocol test now excludes only the exact generated workflow block when comparing old messages: that block must expire when the job changes. Requests, state, tool calls, tool results, tools and the system prompt are still checked unchanged. A scripted test exercises the same boundary without a local port.

The simulated cost-cap replay fixture changes its cap from $0.22 to $0.30 so it reaches the retained evidence window. Its twofold history-saving, 20,000-token peak, correct cost-cap stop, traceability and quantity assertions remain unchanged. The finish case still has its original below-$0.25 assertion. Keeping the old short fixture would require discarding distinct recent evidence again. These figures are offline estimates; no billable requests run. The final replay peaks at 19,848 estimated tokens, below the unchanged 20,000 limit. The normal finish replay falls from 50,704 to 19,382 at its final request; its estimated cost is $0.24625. Replay evidence is copied to `scripted/tokens-*.json`. A full-suite regression also caught a missing temporary-output path check in the Save/Finish split; the original symlink test is unchanged and the path checks were restored before saving and reporting.

## Checks and limits

All cargo commands used the required shared target, 10 jobs, debug information disabled and incremental compilation disabled. Build locks were allowed to finish. No whole-workspace test run or clean was used.

| Check | Real result and log under `.local/goal-run/` |
|---|---|
| `cargo fmt --all` | Passed (`fmt-final.log`). Later touched Rust files were formatted with rustfmt; its final check is in `rustfmt-check.log`. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed (`clippy-after-fix.log`), after correcting one collapsible match reported by the first run (`clippy-final.log`). Final AI-only check after the Arabic fallback also passed (`clippy-ai-final.log`). |
| `cargo test --offline -p qiyas-ai -p qiyas-cli -p qiyas-desktop --no-fail-fast` | Completed with exit 101 (`rust-final.log`): sandbox port failures and two CLI regressions, both corrected and rerun below. |
| Required AI `--lib --test surface --test agent` | Latest library: 81 passed / 1 port failure; agent: 2 passed / 25 port failures (`ai-reviewed.log`). All 64 surface tests passed on the final rerun (`surface-final.log`), including the corrected budget-interruption fixture. The other full-run AI targets passed: authoring 19, drafting 4, tools 24, vision 5, take-off 1 (plus 1 ignored). Key safety had 1 pass / 1 port failure; MCP had 9 port failures. |
| CLI tests after correction | 65 unit tests and 7 agent-evaluation tests passed (`cli-after-fix.log`). The full run also passed all 18 CLI integration and 3 project tests. The unchanged symlink check and all replay assertions pass. |
| Desktop tests | 96 library, 10 options and 16 settings tests passed. One library and one settings test failed to bind local ports (`rust-final.log`). The new visible-panel request/reply bridge test passed. |
| `node_modules/.bin/tsc --noEmit` | Passed (`tsc-final.log`). |
| `node_modules/.bin/vitest run --maxWorkers=2` | 117 files, 862 tests passed (`vitest-final.log`). |
| Final CLI build and MCP acceptance rerun | Passed (`build-last.log`, `headless-final.log`, `audit-flat-final.log`, `audit-takeoff-final.log`, `evidence-check.log`). All four unit-aware jobs and the original audit probes were rerun on the final executable. Its SHA-256 is `753f0b9c332830b56bed1e164664282c100dcbb3f1793a401aa78e56700e7b62` (`bin/build.json`). |

The 38 sandbox failures all report `Operation not permitted` while binding a loopback port. The full list is `sandbox-port-failures.txt`: 1 AI model-list test, 25 agent HTTP-stream tests, 1 key-safety redirect test, 9 MCP HTTP tests, and 2 desktop tests. They were not skipped or changed to hide the restriction. The stdio MCP acceptance jobs run without those ports and pass. The pre-existing ignored take-off test remains ignored; no paid provider run was enabled.

Native window interaction is not claimed by the headless captures. The panel component and desktop request/reply bridge have offline tests; a live model's interpretation of an ambiguous request is not proven by scripted responses. The warning gate covers new findings on Mizan's changed geometry; it does not certify a drawing against every possible visual or specification defect. The direct MCP audit records the client's completion and reports drawing checks; its hardcoded script is not a Mizan conversation. No founder decision is needed to review these changes. A native desktop smoke check remains useful before release.

## Files changed

- `crates/qiyas-ai/src/`: `agent.rs`, `history.rs`, `host.rs`, `judgement.rs` (new), `lib.rs`, `mcp.rs`, `prompt.rs`, `prompt_en.txt`, `prompt_ar.txt`, `prompt_tests_ar.rs`, `surface.rs`, `surface_args.rs`, `takeoff_workflow.txt`, `takeoff_workflow_ar.txt` (new), `window.rs`; tests in `crates/qiyas-ai/tests/surface.rs` and `crates/qiyas-ai/tests/agent.rs`.
- `crates/qiyas-cli/src/headless.rs`, `crates/qiyas-cli/src/eval_workflow_tests.rs`, and `crates/qiyas-cli/tests/agent_eval.rs`.
- `crates/qiyas-desktop/src/ai_window.rs` and `crates/qiyas-desktop/src/drawing_host.rs`.
- `src/ai/AssistantPanel.tsx`, new `src/ai/windowState.ts` and `src/ai/windowState.test.tsx`, `src/app/App.tsx`, `src/app/useAssistantWiring.ts`, `src/shell/LayersPanel.tsx`, and `src/i18n/en.json` / `ar.json`.
- `CLAUDE.md` and this report. Local acceptance clients and transcripts are under `.local/goal-d/` and `.local/goal-run/`; these evidence directories are ignored by git.
