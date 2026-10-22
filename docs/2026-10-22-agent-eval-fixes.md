# Saved subscription-agent run corrections — 22 October 2026

The first successful free AGENT ALONE run retained exact per-object measurements but
submitted only grouped BOQ lines. The submission now retains both, with explicit component
IDs and dimension provenance. Eval, eval-agent/MCP and benchmark recognition use this
representation. The scorer validates complete component accounting before exempting a
reported total from extras, and consumes components when grouped truth matches that total.
Kind, basis, unit, numerical accuracy and traceability requirements remain in force.

Rules can declare `kind` and `basis`; the kit and workflow demonstrate them. Older saved
rules lack these fields. Replay derives kind from unit and uses geometric basis only for
direct geometric recipes or explicit area-times-numeric-height recipes. Other transformations
retain `rule:<their own saved rule ID>` with a warning. Replay never assigns benchmark note
names, changes a quantity, repairs an interpretation or consults truth during submission
construction. This matters when several different square-metre recipes cite the same object.

Digest now retains complete note text in bounded chunks, with the closest geometry handles,
layers and distances, and includes every schedule column, including detected text grids.
Self-check compares explicit numeric units in cited notes against simple geometry/factor
recipes and checks a percentage's dimensional scope when the note gives a unit. These are
warnings requiring interpretation; spatial proximity and dimensional consistency cannot
prove that a note applies to an element. Unitless scope and arbitrary expression semantics
still require model judgement.

`bench rescore-agent --report PATH --output NEW_DIRECTORY` rebuilds from saved measurement
reports and recognition metadata read directly from the saved drawing. It does not follow
external underlay references, load project sidecars, derive new measurements or start agents.
The outer scorer opens truth only after the new submission is written. Original artefacts
remain untouched; contaminated cases stay excluded and missing artefacts stay empty.

The existing Codex MCP approval argument and regression test are retained unchanged.

## Verification

All Cargo checks run sequentially, with `CARGO_BUILD_JOBS=3 nice -n 19` for Clippy/tests.
No helper agents, real agent evaluations, paid calls, new crates, settings/key access,
staging, commits or pushes were used. The requested full check commands are each run once.

- `cargo fmt --all`: passed. Subsequent small corrections use direct rustfmt.
- Workspace Clippy with `--offline --all-targets -- -D warnings`: passed before the later
  note-unit edge-case and replay-underlay corrections. It is not repeated.
- `cargo test --offline -p qiyas-core`: passed. The later Unicode-unit boundary correction
  is exercised by the surface regression below.
- `cargo test --offline -p qiyas-ai --lib --test surface`: library 73 passed, 1 failed
  (`models::tests::an_unreachable_service_falls_back_to_the_builtin_list`) because sandbox
  localhost binding returns `PermissionDenied`. Cargo stopped before surface.
- The already-built surface binary was run directly: 50 passed, 1 failed in the new test
  because it read `handles` instead of the tool response's `added` field. Corrected that test
  and ran `cargo test --offline -p qiyas-ai --test surface
  takeoff_self_check_preserves_unicode_note_unit_dimensions`: 1 passed, 50 filtered out.
  All 51 surface tests therefore passed across those runs; no tests were removed or weakened.
- `cargo test --offline -p qiyas-cli`: 43 unit tests passed; the new handle-basis test
  initially failed because its unchanged description correctly matched through the existing
  description fallback. Made that fixture's description distinct, keeping its zero-handle-match
  assertion. Targeted rerun: 1 passed, 43 filtered out.
- The built agent-eval integration binary initially had 1 pass and 2 failures because the old
  artefact assertion expected one submission row. Updated it to require two rows (dimension
  and total), both quantities and the exact component link. `cargo test --offline -p qiyas-cli
  --test agent_eval`: all 3 passed, including offline replay and contamination checks.
- Remaining built CLI integrations ran directly: `cli` 18 passed; `projects` 3 passed.
  Across the initial and targeted runs, all 44 CLI unit and 24 integration tests passed.
- `nice -n 19 ./target/debug/qiyas-cli bench run --set quick --mode autonomy`: completed
  once, exit 0, 120 cases in 169.10 seconds. Item coverage 19.50%, precision 98.76%, 716
  matches and 9 extras; these item metrics are unchanged from the previous quick run.
  Report: `.local/benchmark/runs/baseline-autonomy-20261022T190847530704533-2/`.
  The run exposed 490 discrepancy false positives: the comparison adapter was consuming
  both dimension rows and totals. Fixed it to use the same validated component accounting
  as scoring and compare at client granularity (or complete totals for missing-scope checks).
  `cargo test --offline -p qiyas-cli --bin qiyas-cli bench::`: all 26 passed, including the
  strengthened unreadable-truth regression: a correct dimension plus total produces no
  alarm, while wrong quantities, wrong units and missing scope still produce findings.
  The full benchmark was not repeated, respecting the once-only instruction; post-fix
  discrepancy rates for the full quick cohort are therefore not claimed.
- Final direct rustfmt and `git diff --check`: passed after all source corrections.
- `CARGO_BUILD_JOBS=3 nice -n 19 cargo build --offline -p qiyas-cli`: passed after the
  comparison correction; `target/debug/qiyas-cli` contains the final implementation.

## Saved-run comparison

The successful combined source report is
`.local/benchmark/runs/eval-agent-set-20261022T170350242878931-649677/report.json`.
Its replay is
`.local/benchmark/runs/rescore-agent-set-20261022T170350242878931-649677-dimensions/report.json`.

| Case | Coverage before | Coverage after | Precision before | Precision after |
|---|---:|---:|---:|---:|
| apartment-17000 | 11.11% | 66.67% | 16.67% | 28.57% |
| office-15000 | 12.50% | 62.50% | 16.67% | 31.25% |
| warehouse-16000 | 25.00% | 75.00% | 33.33% | 42.86% |
| mep-18000 | 50.00% | 50.00% | 66.67% | 66.67% |
| external-19000 | 50.00% | 50.00% | 100.00% | 100.00% |
| apartment-17000/variants/scanned-pdf | 0.00% | 0.00% | 0.00% | 0.00% |
| **Overall** | **24.00%** | **50.00%** | **41.38%** | **38.46%** |

Matched items increased from 12/50 to 25/50. All 25 are exact, pass their tolerances and
are independently traceable. Extras increased from 17 grouped lines to 40 individual
measurements. Itemisation exposes the agent's speculative scope as well as its correct
measurements; precision must reflect those extras. The 29 retained BOQ totals are listed
in `reported_aggregates`, without a second extras charge. The apartment's saved object 52
(`34` in hexadecimal) now supplies both its 43.875 m² area and 9.871875 m³ volume. No note
semantics were repaired during replay, and no truth quantities entered the submission builder.

The earlier approval-blocked batch was also replayed:
`.local/benchmark/runs/rescore-agent-set-20261022T165927007936386-638425-dimensions/report.json`.
It remains 0/50 coverage, no extras and undefined precision, since it has no measurements.
Both batches retain all six cases. Original report and case-folder file hashes were checked
before/after and are unchanged. Every submitted dimension quantity, citation and complete
line list was compared with its saved measurement and is unchanged.

## Files changed

- `CLAUDE.md`
- `docs/2026-10-22-agent-eval.md`
- `docs/2026-10-22-agent-eval-fixes.md`
- `crates/qiyas-core/src/measurement.rs`
- `crates/qiyas-core/src/digest.rs`
- `crates/qiyas-core/src/takeoff_kit.rs`
- `crates/qiyas-core/tests/measurement.rs`
- `crates/qiyas-core/tests/registry.rs`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-cli/src/bench/checks.rs`
- `crates/qiyas-cli/src/bench/mod.rs`
- `crates/qiyas-cli/src/bench/model.rs`
- `crates/qiyas-cli/src/bench/submission.rs`
- `crates/qiyas-cli/src/bench/recognition.rs`
- `crates/qiyas-cli/src/bench/score.rs`
- `crates/qiyas-cli/src/bench/rescore.rs`
- `crates/qiyas-cli/src/bench/tests.rs`
- `crates/qiyas-cli/src/eval.rs`
- `crates/qiyas-cli/src/eval_workflow_tests.rs`
- `crates/qiyas-cli/tests/agent_eval.rs`
- `crates/qiyas-cli/tests/support/fake_agent.py`

The already-present `eval_agent.rs` MCP approval argument and its test remain unchanged.

## Remaining limits

The localhost-binding test needs a reviewer run outside this sandbox. Clippy was not repeated
after the later corrections, as requested. The full quick discrepancy rates after the
comparison correction still need reviewer verification; the once-only benchmark was not repeated. No real agent evaluation was run; judging
whether the clearer evidence changes model decisions remains the reviewer's task.

