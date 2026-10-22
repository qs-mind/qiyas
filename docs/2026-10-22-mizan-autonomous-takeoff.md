# Mizan autonomous take-off — 22 October 2026

This phase precedes roadmap Phases 19–23. The headline is **MIZAN ALONE**, measured by
live `qiyas-cli eval`. Engine runs using prepared or shared development rules are diagnostics.
No paid evaluation was run during implementation. No provider settings or API keys were opened.

## Behavior

The three generic tools now expose a compact `digest` and `takeoff_check`. The digest pages at
8,192 serialized JSON bytes and covers all drawing sheets, units, annotation scale, layers by
entity kind, referenced blocks and attribute tags, geometric text clusters, table rows, closed
area statistics, hatches and existing knowledge/quantity counts. Text previews retain source
handles for full reads. A picture call is suggested. Annotation scale is explicitly distinguished
from calibration of PDF geometry. Clusters are possible labels/legends/schedules, not semantic
classification. The model interprets them from evidence.

`commands {names:[...]}` fetches several schemas at once. The registry index's take-off section
is generated from `x-takeoff-step` metadata on command and kind variants. General workflow
instructions cover evidence gathering, data authoring in batches, recognition, the selected
Settings method, generated BOQ mappings, verification, correction by handle and a final
uncertainty report. They contain no project names, standard text, trade lists or rates.

Autonomous is the default. Saved per-job and per-phase modes still work. Autonomous completion
reports introduced drawing errors without blocking for approval. The UI reports uncertainty,
including accepted low-confidence elements, with optional correction controls.

The self-check reports explanation shares for layers, block names and inserts, and closed
regions; unresolved handles; unmapped table rows; unmatched schedule-derived recognition rules;
low-confidence evidence; measurement warnings; and explicit verify results. Empty expectations
are reported as unchecked, never as a pass. Evidence coverage is not benchmark recall. Labels
and annotation may remain unexplained, so an incomplete self-check must be disclosed at the
budget limit. General spatial text groups still need Mizan to supply a schedule column mapping.

## Submission precision

Live submissions use generated BOQ mappings and included dimension sources that resolve to
accepted recognised elements. Manual measurements and raw candidates cannot enter the scored
submission. Generated BOQ and dimension worksheets are retained in the workbook.

Prepared recipe mappings likewise require accepted recognised elements. `include_baseline` is
ignored with a warning; it no longer appends every candidate. Shared data mappings can supplement
a prepared recipe, deduplicated by kind, unit, source handles and basis. Shared PDF label patterns
materialise typed recognised enclosures before mapping areas. `block_name` extraction and
property grouping let recognition data aggregate symbol counts without a built-in trade list.

Development mappings in `.local/benchmark/general/recognition.json` were written from visible
DXF layers and geometry. No truth quantities were supplied to their builder. These mappings
are deliberately absent from live eval and the scripted provider. IFC footprints are not mapped
as model quantities. No standards or domain recognition data were added to shipped source.

## Evaluation and cohorts

Every run retains an unrotated `agent.log` and `steps.jsonl`: every provider request/response,
tool use, trimmed result text, tokens, reservations and estimated cost. Images are represented
by placeholders in the structured log. Original source and output drawings, generated BOQ,
dimensions, conflicts, usage and a self-check are saved in the run directory.

Reservations cover one request, including its input context and output token allowance. UTF-8
byte counts and protocol/image allowances replace the former eightfold input multiplier.
Actual usage settles each reservation; missing usage keeps it charged and stops the case.
The agent receives remaining request and cost information. A missing pricing entry or mismatched
provider endpoint still refuses a live call.

`--cases ID ID ...` and `--set live-dev` require a total budget and produce a combined report.
Remaining budget is divided among remaining cases, so unused allocations carry forward; an
optional per-case ceiling and the existing daily ledger also apply. Failed/unrun cases remain in
the combined coverage denominator. A failure with uncertain accounting stops the remaining work.

Frozen manifests live in `.local/benchmark/sets/`:

- `live-dev`: first lexical clean apartment, office, warehouse, MEP and external case, plus first
  lexical PDF case. Current IDs: `apartment-17000`, `office-15000`, `warehouse-16000`, `mep-18000`,
  `external-19000`, `apartment-17000/variants/scanned-pdf`.
- `quick`: round-robin project-type × variant strata in lexical order; SHA-256(case ID) order
  within each stratum; first 120 cases (or all when fewer exist); one repetition.
- Ordinary full `bench run` keeps three repetitions. Quick reports are separate files and retain
  before scores for the identical selected cohort from the existing full reports.

Reviewer command, with configured provider/model prices and available daily budget:

```sh
nice -n 19 ./target/debug/qiyas-cli eval --set live-dev --approve-live-run --provider-from-settings --total-budget-usd 3 --max-requests 40 --max-tokens 4096
```

This authorizes a total estimated $3 ceiling for that reviewer invocation. The existing daily
ledger may impose a smaller remaining allowance. No command here was executed against a provider.

## Offline evidence and validation

The scripted provider receives only the production conversation and tool results. On two newly
generated drawings it starts without types, recognition rules or take-off rows, reads the digest,
authors its own data, applies recognition, resolves pending candidates by handle, authors a
geometric method and mappings, reads the generated BOQ, verifies and reports uncertainties.
The harness scores only after the agent stops; the provider never opens truth files. This proves
the complete orchestration path, not live model recognition quality on arbitrary consultant drawings.

No dependencies were added. Every mutation still uses the existing command/batch undo boundary.
All work remains uncommitted. Validation results and quick-cohort comparison are recorded below.

The scripted office reached 37.5% coverage and the apartment 44.44%, both with 100% item
precision and 100% accuracy on matched items. Each used ten requests, with the uncertainty
summary on the final request. Neither used prepared or case-local recognition data.

### Checks

All Cargo commands ran sequentially with `CARGO_BUILD_JOBS=2 nice -n 19`. No helper agents,
parallel builds, full benchmark or `scripts/check.sh` were used.

| Requested check | Actual result |
|---|---|
| `cargo fmt --all` | Passed once; subsequent fixes formatted with `rustfmt` on the affected files. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Initially failed on two CLI lints. Both fixed. Targeted CLI recheck passed; final all-targets recheck of core, AI and CLI passed. |
| `cargo test --offline -p qiyas-core -p qiyas-takeoff -p qiyas-boq` | Passed. After the registry metadata fix, its 11 tests passed again. |
| `cargo test --offline -p qiyas-ai --lib --test surface --test drafting_eval --test authoring` | Library: 70 passed, one socket test failed with sandbox `PermissionDenied`. Ran the remaining targets separately: authoring 19 passed, drafting 4 passed. Two surface failures were fixed; all 44 surface tests then passed. |
| `cargo test --offline -p qiyas-desktop -p qiyas-cli` | Initially stopped at two CLI unit failures. Both fixed; all 33 CLI unit tests and 18 CLI integration tests passed. Desktop: 94 passed, one socket test failed with sandbox `PermissionDenied`. The strengthened ten-request scripted test also passed. |
| `node_modules/.bin/tsc --noEmit` | Passed. |
| `nice -n 19 node_modules/.bin/vitest run --maxWorkers=1` | 838 tests passed across 112 files; 263.27 seconds. |
| `nice -n 19 ./target/debug/qiyas-cli bench run --set quick --mode prepared` | Passed once: 120 cases, one repetition, 44.38 seconds. |
| `nice -n 19 ./target/debug/qiyas-cli bench run --set quick --mode autonomy` | Passed once: 120 cases, one repetition, 34.91 seconds. |
| `git diff --check` | Passed. |

The reviewer must rerun `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
and `commands::tests::the_mcp_server_follows_the_settings` where local ports are available.
Neither test was loosened or skipped. Raw check logs are in `.local/autonomous-takeoff/`.

### Quick comparison

Before scores were recovered for all 120 selected IDs from the retained pre-change full reports;
the full benchmark was not rerun. Both runs used the identical frozen quick manifest. This
stratified cohort differs from the full 1,099-case cohort behind the earlier 72%/33% figures.

| Engine mode | Coverage before | Coverage after | Item precision before | Item precision after | Extras before → after |
|---|---:|---:|---:|---:|---:|
| Prepared | 30.17% | 30.17% | 19.02% | 69.73% | 4,717 → 481 |
| Autonomy (shared rules only) | 19.50% | 19.50% | 14.43% | 98.76% | 4,245 → 9 |

Prepared retained all 1,108 matches; autonomy retained all 716. Every matched quantity passed
in both modes. The change includes removing raw candidate submissions and replacing the old
implicit baseline with explicit shared development recognition/mapping data. It does not
establish live model performance. The remaining 481 prepared extras are still false positive
submissions and remain in the precision denominator.

Reports: `.local/benchmark/baseline-prepared-quick.json` and
`.local/benchmark/baseline-autonomy-quick.json`, with Markdown counterparts and per-case
submissions in their `report_directory`. Original full baseline files were preserved.

Outstanding validation is the reviewer's paid `live-dev` run and the two local-port tests.
No new dependencies or licensing decisions are needed.

### Files touched

- Core: `crates/qiyas-core/src/{commands.rs,digest.rs,lib.rs,measurement.rs,recognition.rs,registry.rs}`
  and `crates/qiyas-core/src/measurement/evaluate.rs`.
- Agent: `crates/qiyas-ai/src/{agent.rs,log.rs,prompt.rs,surface.rs,surface_args.rs,takeoff_workflow.txt}`;
  `crates/qiyas-ai/tests/surface.rs` and `crates/qiyas-ai/tests/drafting_eval/phases.rs`.
- CLI: `crates/qiyas-cli/src/{eval.rs,eval_workflow_tests.rs}` and
  `crates/qiyas-cli/src/bench/{baseline.rs,general.rs,generate.rs,mod.rs,recognition.rs,sets.rs,submission.rs,tests.rs}`.
- UI: `src/shell/{RecognitionPanel.tsx,RecognitionPanel.test.tsx,SettingsDialog.tsx,SettingsDialog.test.tsx}`
  and `src/i18n/{en.json,ar.json}`.
- Documentation: `CLAUDE.md`, `docs/2026-10-21-qiyas-qs-roadmap.md` and this file.
- Development data only: `.local/benchmark/general/{recognition.json,README-autonomous.md}`,
  `.local/benchmark/sets/{quick.json,live-dev.json}`, quick run artifacts/reports and check logs.
