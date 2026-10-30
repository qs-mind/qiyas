# Arabic from the roots, 30 October 2026

1. Added a shared glossary and style rules for engineering terms, unvocalized
   wording, numeric labels, punctuation and mixed Arabic/Latin spacing.
2. Reviewed all 2329 Arabic values against English; revised 608. Kept
   already natural labels, key order and every placeholder. Replaced ambiguous
   working labels, inconsistent BOQ/block/window terms and fixed plural counts.
   No English changes were made by this task; existing concurrent English edits remain.
3. Updated authored Arabic messages and deliverable headings in AI, core, desktop
   and BOQ. Added a style paragraph to Mizan's Arabic prompt. The decision boundary,
   commands, engineering rules and user content are unchanged. Input aliases and
   byte-preservation fixtures retain their original wording.
4. Added source-only Vitest and Rust guards using the same JSON. Rust lexes normal,
   escaped, raw and multiline strings and scans both Arabic prompt files. Exact
   exceptions have reasons and fail if obsolete; no file is exempt. No runtime
   cleanup of model output, dependency, window, commit or network/model call.

## Tests

Added `arabicStyle.test.ts`: dictionary keys/order, placeholder multisets, banned
terms, diacritics, digit/Latin/placeholder adjacency, empty/untranslated values,
negative regressions and unrestricted counts. Added `arabic_style.rs`: shared
style examples, lexer coverage, recursive source scan and both prompt contracts.
Registered it in `tests/all.rs` without disturbing the existing progress module.

Updated assertions, without deleting or skipping tests:
- `ToolCard.test.tsx`: Arabic export wording no longer contains shadda.
- `prompt_tests_ar.rs`: both Arabic instruction contracts use the new spelling;
  expert-frame and defect-repair assertions also follow the instructions already
  present at resume. These two tests also run inside the integration binary.
- `prompt.rs`: `save_docs_describe_app_autosave_and_finish_without_a_save`,
  `no_drawing_says_so_in_both_languages`, and
  `both_languages_keep_internal_names_and_paths_out_of_answers`: expected wording.
- `agent.rs`: `continuation_assigns_technical_decisions_and_preserves_idea_questions`:
  expected spelling of the unchanged decision-recording instruction.
- `space_planning.rs`: `code_findings_keep_the_citation_url_and_caveat`: natural
  unvocalized fixture wording; citation assertions unchanged.
- `qiyas-boq/tests/variation.rs`: added visible Excel header assertions to the
  existing bilingual round-trip test, retaining all round-trip checks.

Checks (one at a time, Rust offline with two build jobs and nice 10):
- `pnpm exec vitest run src/i18n/arabicStyle.test.ts`: four runs, 3 tests passed
  each time, including the final dictionary.
- `pnpm exec vitest run src/ai/ToolCard.test.tsx`: 7 passed.
- `pnpm exec vitest run src/i18n/i18n.test.ts`: 5 passed. An earlier invocation
  mistakenly used `.tsx`; it found no tests and exited 0, so it is not a test pass.
- `pnpm exec tsc --noEmit`: both runs passed (exit 0).
- `CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 cargo test -p qiyas-ai --test all arabic_style`:
  first run 1 passed, 1 failed on the intentional literal-backslash streaming fixture;
  after its exact documented exception and adding the prompt contracts, 4 passed.
  Two further reruns each passed all 4 tests: the last wording fixes, then
  portable exception-path handling. The final run used the final test source.
- `CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 cargo test -p qiyas-boq --test all variation`:
  2 passed, including the extended visible-header/round-trip test.
- `rustfmt --edition 2021` on the two new/extended integration test files: passed.
- Scoped `git diff --check`: passed.

Vitest emitted the existing Vite native-config warning about the extensionless
`scripts/live-plugin` import; it did not fail any test.

No live model run or visual rendering check was performed; no window was opened.
Other crate unit tests were not run as a whole. Existing localization assertions
were updated in source, and both Arabic prompt contracts were exercised through
`tests/all.rs`. There is no outstanding terminology decision. The repository had
no physical AGENTS.md, so the supplied AGENTS instructions and CLAUDE.md were used.

## Files changed by this task

- `crates/qiyas-ai/src/advice.rs`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/blind_reader.rs`
- `crates/qiyas-ai/src/brief_check.rs`
- `crates/qiyas-ai/src/churn.rs`
- `crates/qiyas-ai/src/context.rs`
- `crates/qiyas-ai/src/plan_watch.rs`
- `crates/qiyas-ai/src/prompt.rs`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/prompt_tests_ar.rs`
- `crates/qiyas-ai/src/provider_error.rs`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/takeoff_phase.rs`
- `crates/qiyas-ai/src/takeoff_workflow_ar.txt`
- `crates/qiyas-ai/tests/all.rs`
- `crates/qiyas-ai/tests/arabic_style.rs`
- `crates/qiyas-boq/src/variation.rs`
- `crates/qiyas-boq/tests/variation.rs`
- `crates/qiyas-core/src/arrange.rs`
- `crates/qiyas-core/src/blocks.rs`
- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/decisions.rs`
- `crates/qiyas-core/src/deliverables.rs`
- `crates/qiyas-core/src/deliverables/defaults.json`
- `crates/qiyas-core/src/digest.rs`
- `crates/qiyas-core/src/digest/detail_geometry.rs`
- `crates/qiyas-core/src/discrepancy/compare.rs`
- `crates/qiyas-core/src/drafting_standards.rs`
- `crates/qiyas-core/src/draw/room_boundary.rs`
- `crates/qiyas-core/src/layouts.rs`
- `crates/qiyas-core/src/object_tags.rs`
- `crates/qiyas-core/src/objects.rs`
- `crates/qiyas-core/src/outside_check.rs`
- `crates/qiyas-core/src/pack_check.rs`
- `crates/qiyas-core/src/quantity_notes.rs`
- `crates/qiyas-core/src/second_reading.rs`
- `crates/qiyas-core/src/sheet_setup.rs`
- `crates/qiyas-core/src/space_planning.rs`
- `crates/qiyas-desktop/src/drawing_host.rs`
- `crates/qiyas-desktop/src/output_files.rs`
- `crates/qiyas-desktop/src/updater.rs`
- `docs/2026-10-30-arabic-roots.md`
- `docs/arabic-glossary.md`
- `src/ai/ToolCard.test.tsx`
- `src/i18n/ar-glossary.json`
- `src/i18n/ar.json`
- `src/i18n/arabicStyle.test.ts`
