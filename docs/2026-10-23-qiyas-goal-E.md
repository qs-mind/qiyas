# Goal E — providers and clear errors

Worktree `.local/wt/goal-e`, branch `goal/e`. This builds on Goals A and F. No core commands, prompts or normal drafting/compaction policy changed. No real credentials were read, no paid model requests were made, and no dependencies, licences, commits or pushes were added.

The implementation is ready for review. Live HTTP fixtures and painted desktop acceptance remain restricted by the sandbox; those are not claimed as passes.

## Changes and before/after evidence

| Goal item | Before (approved audit) | After |
| --- | --- | --- |
| P2, P10, X-F2: plain errors | Provider English or Goal F's generic fallback; credit, context and transport causes not distinguished | Stable `ai.error.*` codes with English and Arabic messages for billing/quota, authentication, rate limit, missing model, context, output limit, timeout, network, broken stream, service failure and content refusal. Raw diagnostics stay in the support log. |
| P3, P14, M16: retries | A 429 quota failure waited up to two minutes; 500/502/504 and network failures stopped immediately; waits were invisible | Classify the complete scrubbed reply before clipping diagnostics or retrying. Billing/quota never retries, including stream errors and optional-parameter fallback. Transient 429, 5xx, connection failures and interrupted/overloaded streams use bounded waits. `RETRY` logs say why and how long; the window shows the wait. Failed partial text is removed before another answer; no tool from that partial response executes. The existing retry for an answer that used all its room thinking also shows a notice. Metered evaluation disables automatic retries. |
| P4: model selection and keys | Typing a new key did not refresh discovery; custom IDs could be saved despite a readable list rejecting them | A key explicitly typed for the current address refreshes discovery; changing the address clears it. Without a draft key, Goal A's saved-address gate still runs before keychain access. Save checks a readable service list and refuses an unlisted ID. Supported-model-name 400s are recognized; a generic missing endpoint is no longer called a missing model. |
| P12: capabilities | Every model was offered | Entries explicitly lacking tools or image input are hidden using capability flags, input modalities and supported parameters. A typed filtered-out ID also fails the readable-list check. Unknown metadata stays unknown; there is no paid probing. |
| P6: effort | One global high default; compatible xhigh/max silently meant high | Independent profile effort. New Anthropic defaults to medium; compatible to low. None omits the effort parameter and is labelled as the service default. Compatible choices stop at high. Old global settings migrate only into their selected profile. Headless eval reads that profile too. Changing effort keeps the conversation. |
| P7, P8: limits | Context refusals left the same oversized request; naming a token field could remove the output cap | One context recovery attempt shortens large results/pictures while retaining user words and call/result pairing. If still too long, the error asks for a new chat. Only explicit unsupported-parameter errors allow field fallback. A token-range 400 stays a clear failure, and an output cap is always retained. Cap-name fallback is not restored on resume. |
| P13: streaming performance | Every text/thinking delta copied the complete conversation | Deltas append in memory. Full checkpoints occur at response/step boundaries, including interruption. Desktop saves are reduced to opening/completed-step/terminal boundaries. A 1,000-delta regression requires fewer than 10 provider-state snapshots and verifies the complete saved answer. |
| P15: usage | Missing fields in a later usage object reset previous counts to zero | Only supplied counters update. Cached/reasoning tokens and DeepSeek cache aliases are retained. |
| P25: MCP token | Display/copy only | **Replace token on Save** creates a random draft token. Save persists it and uses the existing server restart; Cancel preserves the previous token. The displayed setup snippets follow the draft. |
| P11: drawing failure | A provider error appended an English take-off end report even in drawing chats | Provider errors return the translated error without an automatic take-off narrative. Completed edits remain in the drawing. A socket-free production-agent regression makes a real edit, then fails with quota, and reads the retained edit back. The headless evaluation host retains its own short stopped-run summary. |

The transport also has a ten-minute total request deadline, in addition to its existing connect and inactivity limits. Model-list requests no longer use the two-minute retry schedule.

## Window contract for Goal F

`ai:done.error` remains a string for compatibility. For provider failures it contains JSON:

```json
{"code":"ai.error.billing","message":"Your AI account has no available credit or quota. Check billing with your provider.","message_ar":"لا يوجد رصيد أو حصة متاحة في حساب الذكاء الاصطناعي. راجع الفوترة لدى المزوّد."}
```

Model-list errors use the same envelope. `src/i18n/errors.ts` selects the catalog entry by code before legacy patterns. Retry events are `retrying {seconds, code, discard_partial}`; technical diagnostics are not sent in that event. All error/retry wording lives in `src/i18n/en.json` and `ar.json` under `ai.error.*`.

## Checks

Results are filled from the retained logs, not inferred from the presence of a build binary.

- `cargo fmt --all`: passed. Later changed Rust files were formatted individually.
- Workspace Clippy, offline, all targets, warnings denied: passed; final changed-file recheck recorded below.
- TypeScript `--noEmit`: passed.
- Full Vitest suite with two workers: **867 tests passed across 117 files**.
- Offline `qiyas-cli` build: passed.
- `cargo test --offline -p qiyas-ai -p qiyas-desktop -p qiyas-cli --no-fail-fast`: **405 passed, 43 local-socket failures, 1 real regression, 1 existing ignored live evaluation**. The regression was an empty headless end-report answer after a budget stop; it was fixed in the evaluation host without changing the test.
- Final explicit AI `--lib --test surface --test agent --no-fail-fast`: **139 passed, 31 local-socket failures** (82 library, 54 surface and 3 agent passes). No non-socket failure remained in that run. This includes the two new socket-free output-cap/partial-usage regressions.
- Final TypeScript check and focused frontend rerun: **passed, 34 tests across 4 files**.
- Touched Rust formatting check and `git diff --check`: passed. [Final formatting log](../.local/goal-run/format-final.log).
- Final CLI binary tests: **65 passed, 0 failed**, including the unchanged budget-stop end-report test that caught the regression. [Log](../.local/goal-run/cli-final-tests.log).
- Final changed-crate Clippy (AI, CLI and desktop, all targets): **passed**. Focused rechecks after the last AI and desktop edits also **passed**, with warnings denied. [Three crates](../.local/goal-run/clippy-final.log), [last AI edits](../.local/goal-run/clippy-error-body.log), [last desktop edit](../.local/goal-run/clippy-desktop-last.log).
- The updated desktop effort test initially failed: comparing entire profiles made effort changes reset the conversation. The comparison now uses provider/address/model only. **All 10 desktop AI tests passed** on recheck. [Initial failure](../.local/goal-run/desktop-effort-final.log), [passing rerun](../.local/goal-run/desktop-ai-recheck.log).
- Final AI library rerun: **84 passed, 1 local-socket failure**. Both the long-reply quota regression and the thinking-limit retry notice passed. [Log](../.local/goal-run/ai-final-library.log).
- Final CLI rebuild: **passed**. [Build log](../.local/goal-run/build-cli-final.log). All nine headless jobs then passed again using that binary; saved-file and workbook readbacks passed, and all 13 PNGs were present. [Evidence check](../.local/goal-run/final-evidence-check.log).

Logs: [format](../.local/goal-run/fmt.log), [Clippy](../.local/goal-run/clippy.log), [TypeScript](../.local/goal-run/tsc.log), [Vitest](../.local/goal-run/vitest.log), [CLI build](../.local/goal-run/build-cli.log), [Rust tests](../.local/goal-run/cargo-test.log), [explicit AI checks](../.local/goal-run/cargo-required.log), [final focused frontend](../.local/goal-run/vitest-final-focus.log), [final TypeScript](../.local/goal-run/tsc-final.log).

Every Cargo command used the required shared target, two jobs, disabled debug information and no incremental compilation. Builds waited for the shared lock; no cache was cleaned and no other build was stopped.

Existing tests were updated where they asserted the audited wrong behavior: offering a missing saved model as selectable, a global high default, allowing Save with an invalid readable-list ID, dropping both token-cap fields, and treating every 404 as a missing model. The key-scrubbing test now uses strict-budget adapters to test each failure independently; dedicated tests cover the newly retryable 5xx and stream paths. No socket test was disabled or turned into a passing skip.

## End-to-end evidence

The offline stdlib client is copied from the audit into [.local/mcp_client.py](../.local/mcp_client.py). It runs `qiyas-cli mcp --drawing … --workdir …`, reads records after every edit, and saves the full transcript and returned PNGs. The final rerun uses fresh `final-*` folders. A local copy of the freshly built shared-target binary prevents a later parallel build changing the tested executable; its [SHA-256](../.local/goal-run/qiyas-cli.sha256) is retained.

- [Drawing job](../.local/drawing_job_final.py): rectangle, bilingual label, measurement and readback in mm/cm/m/in. Every area is 12 m². All four saved files were [reopened and checked](../.local/goal-run/final-reopen-summary.json). The screen query correctly returns `needs_window` in headless mode; this does not claim access to live window panels.
- The audit's [sample BOQ job](../.local/sample_boq_job_final.py) passed, including workbook readback: expected lengths 42/14 m, areas 58.29/22.09 m², counts 3/5, and exactly two conflicts. [Verified results](../.local/goal-run/final-sample-boq/verified-results.json), [transcript](../.local/goal-run/final-sample-boq/transcript.jsonl).
- The copied [room/grid audit](../.local/room_grid_units_final.py) still reports 20 m² before a grid, with the grid and after hiding it in all four units. [Readback summary](../.local/goal-run/final-grid-verified.json). This is a Goal A regression check, not a new geometry rule.
- [Provider-error readback](../.local/goal-run/provider-error-after-edit.json): the production agent created one line (7 → 8 entities), then received quota refusal. The retained transcript contains the successful edit and no take-off report; the returned error has the billing code and both languages. This uses a scripted provider without a network socket.
- [Mutation scan](../.local/goal-run/final-mutation-scan.json): 40 execute operations across nine jobs; zero repeated identical edits. Drawing, measurement and workbook readbacks verified their intended effects.

| Units | Transcript | Before | After | Saved/reopened |
| --- | --- | --- | --- | --- |
| mm | [JSONL](../.local/goal-run/final-drawing-mm/transcript.jsonl) | [PNG](../.local/goal-run/final-drawing-mm/0003-before-0.png) | [PNG](../.local/goal-run/final-drawing-mm/0020-after-0.png) | [PNG](../.local/goal-run/final-drawing-mm/reopened/0005-saved-drawing-0.png) |
| cm | [JSONL](../.local/goal-run/final-drawing-cm/transcript.jsonl) | [PNG](../.local/goal-run/final-drawing-cm/0003-before-0.png) | [PNG](../.local/goal-run/final-drawing-cm/0020-after-0.png) | [PNG](../.local/goal-run/final-drawing-cm/reopened/0005-saved-drawing-0.png) |
| m | [JSONL](../.local/goal-run/final-drawing-m/transcript.jsonl) | [PNG](../.local/goal-run/final-drawing-m/0003-before-0.png) | [PNG](../.local/goal-run/final-drawing-m/0020-after-0.png) | [PNG](../.local/goal-run/final-drawing-m/reopened/0005-saved-drawing-0.png) |
| in | [JSONL](../.local/goal-run/final-drawing-in/transcript.jsonl) | [PNG](../.local/goal-run/final-drawing-in/0003-before-0.png) | [PNG](../.local/goal-run/final-drawing-in/0020-after-0.png) | [PNG](../.local/goal-run/final-drawing-in/reopened/0005-saved-drawing-0.png) |

I inspected the initial drawing, final bilingual drawing, reopened inch drawing and [sample BOQ PNG](../.local/goal-run/final-sample-boq/0125-sample-plan-0.png). These exercise the actual CLI and drawing renderer; they do not prove provider networking or desktop font painting.

The [component capture job](../.local/provider-ui.test.tsx) renders the current assistant with each error and a retry in both languages. [English after](../.local/goal-run/ui-after-en.html), [Arabic after](../.local/goal-run/ui-after-ar.html). [English before](../.local/goal-run/ui-before-en.html) and [Arabic before](../.local/goal-run/ui-before-ar.html) use the starting checkout's Goal F error translator on the audited insufficient-credit error; both fall back to the generic unknown-error advice. These are component HTML snapshots, not desktop screenshots. [Capture log](../.local/goal-run/ui-evidence.log).

## Files touched

- `crates/qiyas-ai/src/`: `provider.rs`, new `provider_error.rs`, `anthropic.rs`, `openai.rs`, `models.rs`, `agent.rs`, `conversation.rs`, `lib.rs`; `tests/agent.rs`.
- `crates/qiyas-desktop/src/`: `settings.rs`, `ai.rs`, `commands.rs`; `tests/settings.rs`. `commands.rs` changes only provider discovery, provider-error delivery and assistant persistence boundaries; no core command file changed.
- `crates/qiyas-cli/src/eval.rs`: selected-profile effort reader and host-owned stopped-run summary for the take-off report.
- `src/`: `api.ts`, `i18n/{en.json,ar.json,errors.ts,providerErrors.test.ts}`, `shell/{ModelPicker,SettingsDialog}.tsx` and their tests, `ai/{AssistantPanel.tsx,messages.ts,messages.test.ts}`.
- `CLAUDE.md` and this report. Scripts, logs, transcripts, HTML and PNG evidence stay inside this worktree's ignored `.local/`.

## Limits and remaining acceptance

- Local sockets are refused with `EPERM`, so the real HTTP/SSE adapter and MCP rotation networking tests must be rerun in an environment that allows loopback ports. All 43 affected test names are in the [blocked-test list](../.local/goal-run/sandbox-blocked-tests.json). The tests remain present. No paid calls are needed for them.
- Chromium also fails at its socket setup ([log](../.local/goal-run/browser-probe.log)). Actual English/Arabic desktop screenshots and a live in-app provider-error/retry review remain open. Headless PNGs and component HTML are not substitutes for that acceptance.
- Context recovery is deliberately bounded and reactive. A huge user message, or a conversation still too large after shortening result payloads, gets the new-chat instruction. Normal compaction belongs to Goal D.
- A provider omitting capability metadata cannot be proven to support tools and pictures without invoking it. Such models remain available. If discovery is unavailable, a custom model can still be saved; its eventual provider refusal is translated clearly.
- Output-range failures stop clearly; the implementation does not guess a cap from arbitrary service prose. Existing explicit caps are preserved.
- Disk writes still use the existing AI lock to keep reset/delete and persistence ordered. Per-token copying is fixed; fully asynchronous persistence is deferred because it needs an ordered writer and stale-write protection.
- No new founder decision is required. Remaining acceptance needs a normal local runtime, not permission for paid calls.
