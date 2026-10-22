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


## Third completed run: scan framing and element metadata

The described run is `eval-agent-set-20261022T192248424770994-956050`. The reviewer’s
`3b8ae84` scoring implementation is untouched. Exact-basis, basis-category and any-basis
handle tiers never inspect quantities to select correspondence; ambiguous candidates remain
unmatched. Quantity accuracy and geometric reproducibility remain separate checks.

The scan renderer previously put outer strokes directly on page edges. It now adds 4% of
the longest drawing dimension on every side and fits the raster without exceeding its height
limit. `bench regenerate-scans` refreshed all 60 existing generated scans with their original
seeds and transformed prepared vision points/calibration anchors into the new page frame.
Only scanned `drawing.pdf`, `recognition.json` and the `pdf` field in `truth.json` changed.
Every byte of truth outside that field, including quantity literals and seeds, is preserved.
All other case files are unchanged. All 60 raster borders have four white pixels on every
edge; the apartment image was also inspected visually. Images are retained at
`.local/agent-third-fixes/scan-before.png` and `scan-after.png`.

The shared submission writer and benchmark recipe writer now obtain positions and labels
from the drawing itself. Closed geometry uses its centroid; paths use their length-weighted
centre; point/insert drivers use insertion. Coordinates remain in drawing units, including
PDF calibration and insertion already applied to the driver. Explicit owned labels, element
label/name/description properties, or one unambiguous enclosed text supply the verbatim
per-object description. Multiple enclosed texts fall back to rule text. No synonyms, truth
labels or quantity-based correspondence were introduced. Totals keep their BOQ descriptions
and use an unweighted centre of distinct sources. PDF citations cannot masquerade as CAD
handles. Replay reads the same metadata without loading external underlays or sidecars.

### Saved replay results

Times below identify the UTC timestamps in the saved directory names. Precision is item
precision; H = `handles`, C = `handles_category`, A = `handles_any`, D =
`description_position`. Counts not shown are zero.

| Saved run | Coverage before → after | Precision before → after | Matching tiers before → after |
|---|---:|---:|---|
| 16:59, approval-blocked | 0% → 0% | undefined → undefined | none → none |
| 17:03, first successful | 24% → 52% | 41.38% → 37.68% | H12 → H24, C1, D1 |
| 19:22, described run | 48% → 60% | 40% → 50% | H24 → H24, C3, A3 |
| 20:31, concurrently completed | 44% → 44% | 38.60% → 38.60% | H18, C1, A3 → unchanged |

Replays are in `.local/benchmark/runs/rescore-third-fixes-final-<original timestamp and PID>/report.json`;
exact paths and metrics are in `.local/agent-third-fixes/replay-summary.json`.
The older first-successful comparison also includes the earlier dimension-itemisation work;
it is not an isolated measure of this patch. Its 26 matches include 25 passing quantities.
The described run has 30/50 matches, all 30 exact and passing, 30 extras, and 24 independently
traceable items. Its six additional matches come solely from the reviewer’s fallback handle
tiers. All 246 replayed dimension quantities are numerically identical to their saved reports;
citations and dimension lines also agree. Every reconstructed item, including totals, has a
position. No agent or prediction code opened truth.

The 20:31 report was still being written by another process during this work and completed
before replay. It was not launched here. Hash checks show the three earlier completed reports
and their case artifacts remain unchanged. The newer combined report changed during its own
run; it was only read here.

### Checks and limits

All Cargo commands ran one at a time with `CARGO_BUILD_JOBS=4` for builds/tests. The four
requested full commands are each run once; no helper agents or background builds were used.

| Check | Actual result |
|---|---|
| `cargo fmt --all` | Passed. Later corrective edits were formatted with direct rustfmt. |
| `CARGO_BUILD_JOBS=4 cargo clippy --offline --workspace --all-targets -- -D warnings` | Failed on four compile errors from two API names in the new metadata helper (`entity`/`get_entity`, `insertion_point`/`insert_point`). Fixed; subsequent test builds compile them. Clippy was not repeated. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-core -p qiyas-cli` | CLI units: 47 passed, one failed because rewriting truth JSON rounded existing numeric literals. Fixed the writer to preserve all bytes outside PDF metadata; the original assertion was retained and its targeted rerun passed. Cargo stopped before integrations/core. |
| Completion of unrun tests | All 24 CLI integrations passed, including three offline agent/MCP tests. All 79 already-built core test binaries ran sequentially: 755 passed, zero failures or ignored tests. This includes the new metadata tests and calibrated PDF regression. |
| `./target/debug/qiyas-cli bench run --set quick --mode autonomy` | Completed once, exit 0: 120 cases in 165.95 seconds, 19.50% coverage, 98.76% precision, 716 matches and 9 extras. Report: `.local/benchmark/runs/baseline-autonomy-20261022T205653465114932-3/`. This run predates the final shared vector-PDF adapter correction described below. |
| Additional verification | All 60 scan border checks passed; scan truth bytes outside PDF metadata and unrelated-case hashes are unchanged. Final direct rustfmt and whitespace checks passed. |

Detailed logs are in `.local/agent-third-fixes/`. The first full test build took substantial
memory; one separate log-reading shell command was killed with exit 137, but Cargo completed
and reported the regression normally. No test was weakened, removed or ignored.

Recording positions cannot reconstruct missing scanned-room interpretations, invent labels,
or align an old PDF page origin with a CAD origin. Existing scan submissions therefore remain
unmatched; regeneration improves future input, not saved decisions. Clippy still needs a clean
reviewer run because the requested once-only invocation failed before linting completed.
No real agent evaluation, paid call, API-key/settings access, new crate, staging, commit or push
was performed.

### Files touched

- `CLAUDE.md`
- `docs/2026-10-22-agent-eval.md`
- `docs/2026-10-22-agent-eval-fixes.md`
- `crates/qiyas-core/src/lib.rs`
- `crates/qiyas-core/src/element_metadata.rs`
- `crates/qiyas-core/tests/element_metadata.rs`
- `crates/qiyas-core/tests/recognition.rs`
- `crates/qiyas-cli/src/bench/generate.rs`
- `crates/qiyas-cli/src/bench/generate/pdf.rs`
- `crates/qiyas-cli/src/bench/mod.rs`
- `crates/qiyas-cli/src/bench/recognition.rs`
- `crates/qiyas-cli/src/bench/rescore.rs`
- `crates/qiyas-cli/src/bench/submission.rs`
- `crates/qiyas-cli/src/bench/tests.rs`
- Local-only scanned variant PDFs, recipes and PDF metadata, plus replay/check artifacts above.

A final audit found that the shared vector-PDF adapter constructs its own submission rows.
It now uses the same element metadata, including positions. Imported PDF glyph fragments are
joined with the existing calibrated PDF reader’s baseline/gap rules before selecting one
unambiguous enclosed label; ordinary drawing notes stay separate. The PDF regression now
requires the complete `1 Alpha ` / `2 Beta` labels (including the first label’s extracted trailing space) and the calibrated 4.5 m centroid separation,
while retaining its original quantity and page-evidence assertions. This is a stronger identity
check, not a relaxed matching rule. The first focused run failed because the test expected the first label without its
extracted trailing space; a diagnostic rerun confirmed the exact text. The fixture now requires
that complete source string, with every numeric and evidence assertion retained. All 20 final
submission/scoring unit regressions passed. Final CLI build: passed with `CARGO_BUILD_JOBS=4 cargo build --offline -p qiyas-cli`. All four saved reports were replayed again with that final binary; the table above is unchanged.

The single quick run completed before this adapter correction and was not repeated. Its
scores therefore do not establish final vector-PDF matching performance. Actual coordinates
and verbatim labels can expose inconsistencies with older truth positions/descriptions;
no benchmark-specific alignment or label rewriting is performed.

Additional files touched by this last adapter correction:

- `crates/qiyas-cli/src/bench/general.rs`
- `crates/qiyas-core/src/recognition.rs`
- `crates/qiyas-core/src/recognition/pdf_text.rs`
