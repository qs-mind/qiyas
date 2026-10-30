# Combined repository check, 30 October 2026

Resumed with all four feature changes already uncommitted. Read the window UX,
take-off engine, Mizan plan/progress and Arabic roots notes, the supplied AGENTS
instructions (no physical AGENTS.md exists), and CLAUDE.md's autonomy ruling.
No design steering, dependencies, windows, commits or changes to CLAUDE.md.

## Task outcomes

1. Ran `scripts/check.sh` through its blocking failures, then completed its stages
   individually in order. Used two Cargo jobs, offline Cargo and one heavy command
   at a time; capped Vitest at two workers for the 7 GB machine.
2. Fixed formatting, the Clippy argument limit and prompt word budgets (code),
   stale test APIs/handles/selectors (fixtures), and deliberately replaced Arabic
   and prompt expectations (today's named notes). No assertions were weakened,
   tests deleted, new skips added or speed/byte budgets raised by this task.
   The existing 6,144-byte kit guard passes unchanged.
3. Ran `cargo test -p qiyas-desktop --lib`: both requested workspace busy tests
   passed, also passing again in the final workspace run.
4. Final fmt, Clippy, all release guards, real PDF/OCR, TypeScript and all frontend
   tests pass. Final Rust workspace: 1,991 passed, 60 failed, 12 existing ignored.
   Every failure has `Operation not permitted` from this sandbox's socket
   restrictions. A completely green full check remains unverified until those
   tests run on a socket-capable runner. They are left intact.

## Corrections

- Formatting: wrap the Arabic pack-check strings and desktop imports as rustfmt
  requires. No behavior change.
- Code correction: group the routed tool name, operation and arguments in
  `Agent::run_tool`, retaining progress events while satisfying Clippy's argument
  limit. No lint suppression.
- Fixture correction: four note-scope unit tests still called `findings` without
  the exclusions map introduced by `2026-10-30-takeoff-engine.md`. Supply an empty
  map to retain every original assertion. Add a regression that an explicit
  exclusion excuses omission but never a disjoint factor; an unrelated exclusion
  excuses neither error.

- Code correction: the combined English/Arabic prompt additions exceeded the unchanged
  600-word guard (620/627). Tighten phrasing while retaining autonomy, expert-frame,
  repair, evidence and Arabic style instructions; resulting counts are 600/599.
- Replaced-behavior assertions: `verification_reports_numbers_and_keeps_the_engineer_as_approver`
  now requires repair and re-check before finishing, with escalation limited to
  idea-level decisions or faults Mizan cannot fix, as specified in
  `2026-10-30-mizan-plan-progress.md`. `tool_recipes_live_in_tool_and_command_docs`
  now requires structured decision exclusions, their coverage effect and the
  internal report fallback specified in `2026-10-30-takeoff-engine.md`.
  All other assertions and word budgets remain unchanged.

- Fixture correction: the villa recordings used allocation-era wall handles. Resolve
  each recorded run by its original geometry against `wall_groups` readbacks,
  including the scripted agent's own read results. Keep all 44-opening, exact-size,
  overlap-rejection, member-existence, atomic rollback and timing assertions.
  The shared helper is included by the existing goal_l integration module and
  standalone villa binary; it does not add a new standalone test binary.
- Fixture correction: the revision comparison now authors both exports on the
  same explicit `Survey` layer. Default object layers introduced by the take-off
  note otherwise change one drawing's layer during recognition. The test still
  checks matched, changed, added, removed, renumbering, quantities and undo.
- Replaced-wording assertions, following `2026-10-30-arabic-roots.md`: room budget
  diagnostics use البلوكات; title metadata uses مراجعة and اعتماد; the two
  quantity-comparison tests use المقايسة. Numerical and metadata checks remain.

- Fixture correction: rebase the clinic recording's literal entity handles around
  the actual new `Q-LEGACY-*` layer table records introduced by the take-off note.
  Restrict rebasing to handle fields; coordinates, revision values and `$step`
  references remain unchanged. Add a regression for these boundaries. All 85
  calls, recorded failure/rollback assertions, the existing call-83 allowance and
  the 8x speed budget remain unchanged; rebasing runs outside measured intervals.
- Replaced-wording assertions in nine frontend test files follow the Arabic roots
  note: activity counts/verbs, continuation reassurance, explanation label, sheet
  error, ribbon tabs/tooltips, account permissions, quantity warning and cloud
  tooltip. Keep names, numbers, scales, bidi, placeholders and behavior checks.
- Fixture correction per the window UX note: scope Purge's Close button query to
  its dialog, because the new undo toast also exposes an accessible Close button.
  All command, redraw, autosave and audit assertions remain.

## Checks

All heavy commands run sequentially with `CARGO_BUILD_JOBS=2`,
`CARGO_NET_OFFLINE=true` and `nice -n 10`. Full logs for this resumed run are in
`/tmp/qiyas-full-check-resume/`.

| Invocation | Observed result |
| --- | --- |
| `bash scripts/check.sh`, attempt 1 | Failed fmt in pack_check.rs and desktop commands.rs. |
| Scoped `rustfmt --edition 2021 --config skip_children=true` | Passed for those two files. |
| `bash scripts/check.sh`, attempt 2 | Fmt passed; Clippy rejected eight run_tool arguments. |
| `bash scripts/check.sh`, attempt 3 | Fmt passed; Clippy found 11 outdated findings calls in four note-scope unit tests. |
| Scoped rustfmt on note_scope.rs | Passed. |
| `bash scripts/check.sh`, attempt 4 | Fmt and Clippy passed; workspace test build completed in 13m 41s. First binary (AI unit tests): 201 passed, 17 failed, 6 already ignored. Three prompt-contract failures; 14 localhost socket failures (`EPERM`). Later script steps did not run. |
| `cargo test -p qiyas-ai --lib prompt` | 19 passed; 0 failed. |
| `cargo test --workspace --no-fail-fast`, pass 1 | Completed all targets. Seven repository test failures (villa replay x2, revision fixture, four Arabic assertions), plus socket restrictions in five targets. Both requested desktop busy tests passed. |
| Scoped rustfmt on changed test files | Passed. |
| `cargo test --workspace --no-fail-fast`, pass 2 | Core: 95 unit and 935 integration tests passed. Direct villa replay and all 19 take-off comparison tests passed. Scripted villa helper still failed to read the compact tool result; remaining failures were socket restrictions. |
| `git diff --check` | Passed after the initial fixture correction. |
| `cargo test -p qiyas-ai --test all goal_l::villa_replay_finishes_with_all_openings_and_visible_findings`, first focused run | Failed: helper read outcome/groups correctly but received a compact summary. |
| Same focused command after requesting `detail: full` | 1 passed. |
| `cargo test -p qiyas-desktop --lib` | 127 passed, 1 failed (existing MCP settings socket test, EPERM). Both requested workspace_busy tests passed. Build: 8m 45s. |
| `cargo test -p qiyas-pdf --test pdf` | 6 passed, none ignored. |
| `cargo test -p qiyas-ocr --test all -- --nocapture` | 1 passed, real inference, none ignored. |
| `cargo test -p qiyas-pdf --test ocr -- --nocapture` | 1 passed, none ignored. |
| `cargo test -p qiyas-ai --test all document_ocr -- --nocapture` | 1 passed, none ignored. |
| `cargo test -p qiyas-core --test all recognition_ocr -- --nocapture` | 1 passed, none ignored. |
| `cargo test -p qiyas-render --release pick_index_one_million` | 1 matching speed test passed. Other renderer binaries had zero matching tests, as expected for the script's filter. |
| `cargo test -p qiyas-render --release --test fills fill_in_a_block_inserted_500_times` | 1 passed. |
| `cargo test -p qiyas-param --release --test solver_speed` | 1 passed. |
| `cargo test -p qiyas-geom --release walls_speed` | 1 matching speed test passed. |
| `cargo test -p qiyas-ai --release --test villa_openings villa_speed -- --ignored --nocapture` | 1 passed. |
| `cargo test -p qiyas-ai --release --test clinic_performance clinic_speed -- --ignored --nocapture`, first and second runs | Failed at call 25: old insert handle. Second run exposed the incorrect fixture layer names used by the first repair. |
| Same clinic command after reading actual legacy object layer records | 1 passed, all 85 calls. Largest call 1.3275 s / reference 179.9144 ms = 7.3785, below the unchanged 8x limit. |
| `cargo test -p qiyas-ai --release --test clinic_performance recorded_handles_skip_new_layer_records_without_changing_geometry_or_references` | 1 passed. |
| `pnpm typecheck` | Passed. |
| `pnpm test --maxWorkers=2`, first run | 1160 passed, 22 failed in 10 files: 21 obsolete Arabic wording cases and one ambiguous Close selector. |
| Focused Vitest run of those 10 files | 193 passed, 1 failed: the next Arabic activity assertion also needed the new wording. |
| `pnpm test --maxWorkers=2`, final run | All 1182 tests passed in all 157 files. Existing Vite native-config warning only. |
| `cargo fmt --all -- --check`, final run | Passed. |
| `cargo clippy --workspace --all-targets -- -D warnings`, final run | Passed in 9.01 s. |
| `cargo test --workspace --no-fail-fast`, final run | Exit 101: 1991 passed, 60 failed, 12 existing ignored. All 60 failure diagnostics contain socket EPERM. Both busy-workspace tests, villa replays, clinic rebasing, note exclusions and kit byte guard pass. |
| `pnpm typecheck`, final run after test updates | Passed. |
| Final `git diff --check` and baseline comparison | Passed; 26 tracked files changed by this task plus the villa helper and this note. No new U+2014 characters; unrelated tracked changes match the initial baseline. |

## Files touched by this check task

- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/prompt.rs`
- `crates/qiyas-ai/src/prompt_en.txt`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-core/src/digest/note_scope.rs`
- `crates/qiyas-core/src/pack_check.rs`
- `crates/qiyas-desktop/src/commands.rs`
- `crates/qiyas-ai/tests/goal_l.rs`
- `crates/qiyas-ai/tests/villa_openings.rs`
- `crates/qiyas-ai/tests/support/scripted.rs`
- `crates/qiyas-ai/tests/support/villa.rs` (new helper)
- `crates/qiyas-core/tests/goal_a.rs`
- `crates/qiyas-core/tests/layouts.rs`
- `crates/qiyas-core/tests/revisions.rs`
- `crates/qiyas-takeoff/tests/compare.rs`
- `crates/qiyas-ai/examples/clinic_performance.rs`
- `crates/qiyas-ai/tests/clinic_performance.rs`
- `src/ai/messages.test.ts`
- `src/ai/providerFailure.test.ts`
- `src/app/App.test.tsx`
- `src/boq/Explain.test.tsx`
- `src/i18n/errors.goalU.test.ts`
- `src/shell/Ribbon.test.tsx`
- `src/shell/RibbonUi.test.tsx`
- `src/shell/SubscriptionSettings.test.tsx`
- `src/shell/TakeoffPanel.test.tsx`
- `src/shell/ribbonInfo.test.ts`
- `docs/2026-10-30-full-check.md`

## Tests changed

In `crates/qiyas-core/src/digest/note_scope.rs`, setup only, retaining assertions:
- `omission_and_disjoint_factor_are_errors_even_with_one_rule`
- `uncited_notes_and_unscaled_rules_do_not_get_factor_errors`
- `rejection_needs_a_cited_reason_even_if_an_old_quantity_remains`
- `accepted_source_evidence_counts_as_rule_use`

Added in the same existing unit-test module:
`recorded_exclusion_excuses_omission_but_not_a_disjoint_factor`.
No test deleted, skipped or weakened.

Existing tests additionally updated for the corrections above:
- `prompt::tests::verification_reports_numbers_and_keeps_the_engineer_as_approver`
- `prompt::tests::tool_recipes_live_in_tool_and_command_docs`
- `goal_l::villa_replay_finishes_with_all_openings_and_visible_findings`
- `logged_villa_openings_are_atomic_and_keep_their_members` and the shared replay
  used by the unchanged `villa_speed` guard and existing ignored four-unit replay
- `revisions::carries_reapplies_matches_renumbered_exports_and_round_trips_one_undo_step`
- `goal_a::room_refusal_keeps_budget_details_in_the_public_english_and_arabic_report`
- `layouts::title_metadata_is_literal_bilingual_and_missing_fields_are_reported`
- `arabic_reasons_say_qita_for_pieces`
- `quantity_mismatch_uses_the_sum_of_links`

- Clinic replay setup shared by `clinic_speed` and the existing ignored
  `clinic_replay`; new regression
  `recorded_handles_skip_new_layer_records_without_changing_geometry_or_references`
  in the existing clinic test binary.
- `Arabic activities use Arabic word order and keep names and scale intact`
- The 13 provider failure cases of `%s keeps work and offers Continue in both languages and error formats`
- `PURGE (PU) and AUDIT: a dry run first, then one change that the window catches up with`
- `Arabic project explanation shows drawings and highlights only when asked`
- `an edit on another sheet names it in both languages without exposing command syntax`
- `in Arabic the ribbon reads right to left: the same order from the start edge, Arabic names, logical sides`
- `in Arabic the tooltip is Arabic, the alias stays what is typed`
- `Arabic keeps the same account controls and explains missing plan permission`
- `full names and quantity limits remain separate in ar`
- `every command has a description in English and Arabic, and the new ribbon words exist in both`

## Remaining limitation

The five failing targets in the final workspace run are:

| Target | Passed | Failed | Existing ignored |
| --- | ---: | ---: | ---: |
| qiyas-ai lib | 204 | 14 | 6 |
| qiyas-ai integration all | 204 | 41 | 1 |
| qiyas-desktop lib | 127 | 1 | 0 |
| qiyas-desktop settings | 20 | 1 | 0 |
| qiyas-mcp bridge | 1 | 3 | 0 |

All failures are denied socket operations in local HTTP/provider/MCP mocks.
The execution environment prohibits permission escalation, so this task cannot
provide a successful full-script exit. Run `scripts/check.sh` with
`CARGO_BUILD_JOBS=2 CARGO_NET_OFFLINE=true` in a socket-capable environment to
finish that verification. No product decision remains open; no live model or
window was used. The original four changes remain uncommitted alongside these
corrections. No dependencies or scripts/check.sh changes were needed.
