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


## Fifth free run: allowance scope and positional correspondence (23 October 2026)

Source: `.local/benchmark/runs/eval-agent-set-20261022T211039320437030-1177463/report.json`.
This is the fifth saved free AGENT ALONE run (Codex gpt-6.1-sol, medium, live-dev).
The original result is 29/50 matches, 24 passing quantities, 53 extras: 58% coverage,
35.37% precision and 82.76% matched accuracy. No contaminated cases are present.

`takeoff_check` now reports `measurement_errors` with error severity and stable codes:
`fractional_count` for a non-whole count, and `global_allowance` when the same cited
percentage scales every rule in a multi-rule method. The latter handles direct multiplication,
constant arithmetic and a separate proportional addition/deduction, including dimensional
base factors. Its message is “allowance applied globally; confirm its scope from the note”.
Checks use cited drawing notes or verbatim note text in citations, including raster evidence;
they do not classify domain terms. Errors make `passed` false. Kit and workflow require fixing
them before completion and state that factors apply only to the items their notes name.
No saved measurement quantity is repaired by replay.

Truth generation and position-only maintenance share the submission geometry helper: area
centroid, half-length point for open paths, insertion point for blocks, and the mean of distinct
sources for grouped rows. Portable positions are in metres. PDF truth uses its calibrated page
origin, including the scan margin or vector plot offset. `bench regenerate-positions` changes
only `items[].position` arrays in generated development truth, preserving every other byte,
including floating-point quantity literals. Changes are logged with before/after SHA-256 hashes
in `.local/benchmark/sources/qiyas-generator/position-provenance.jsonl`. Holdout is excluded.

PDF description/position matching accepts max(0.25 m, 2% of the geometry-derived element
diagonal); CAD stays at 0.05 m. Matching still does not use quantities. Multiple candidate
recipes with the same label, kind and location remain ambiguous even if one has the right
quantity. Regressions demonstrate a scan item can match and a wrong quantity cannot break an
ambiguity. No labels are rewritten or synonyms introduced.

Verification logs, original truth snapshots, original run hashes and replay results are kept in
`.local/agent-fifth-fixes/`. No real agent evaluation, paid call, helper agent, new crate,
settings/key access, staging, commit or push is part of this work.

### Fifth-run verification

Every requested Cargo command was invoked once, sequentially. Builds/tests used
`CARGO_BUILD_JOBS=4`; no background builds or helper agents were started.

| Check | Actual result |
|---|---|
| `cargo fmt --all` | Passed. |
| `CARGO_BUILD_JOBS=4 cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed on the final source. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-core -p qiyas-cli` | Exit 101 during compilation: `rustc` received SIGKILL while linking the `agent_eval` integration test. No full-suite pass is claimed; the command was not repeated. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-ai --lib --test surface` | Library: 73 passed, one failed because the sandbox denied localhost binding in `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`. Cargo stopped before surface. |
| Direct execution of the newly built surface test binary | All 53 passed, including both new self-check errors, error severity/failed check, successful repair, and kit size. |
| Targeted core recovery: `cargo test --offline -p qiyas-core --lib --test element_metadata --test registry` | All 61 passed: 46 library, two metadata, 13 registry. |
| Targeted CLI recovery: `cargo test --offline -p qiyas-cli --bin qiyas-cli` | All 51 passed, including scan matching, positional tolerance, ambiguity, quantity-independent correspondence and position-only/idempotent regeneration. |

The full core/CLI integration suite remains unverified because its requested invocation was
killed during compilation. The localhost-binding test requires execution outside this sandbox.
The narrower recovery commands do not replace those full checks.

### Files for the fifth-run fix

- `CLAUDE.md`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-cli/src/bench/general.rs`
- `crates/qiyas-cli/src/bench/generate.rs`
- `crates/qiyas-cli/src/bench/generate/positions.rs`
- `crates/qiyas-cli/src/bench/mod.rs`
- `crates/qiyas-cli/src/bench/model.rs`
- `crates/qiyas-cli/src/bench/recognition.rs`
- `crates/qiyas-cli/src/bench/score.rs`
- `crates/qiyas-cli/src/bench/submission.rs`
- `crates/qiyas-cli/src/bench/tests.rs`
- `crates/qiyas-core/src/digest.rs`
- `crates/qiyas-core/src/digest/measurement_checks.rs`
- `crates/qiyas-core/src/element_metadata.rs`
- `crates/qiyas-core/src/takeoff_kit.rs`
- `crates/qiyas-core/tests/element_metadata.rs`
- `docs/2026-10-22-agent-eval-fixes.md`

Generated truth, provenance, audit logs and replay artifacts remain local-only.


### Fifth-run saved rescore and position audit

`CARGO_BUILD_JOBS=4 cargo build --offline -p qiyas-cli` passed. The final binary ran
`bench regenerate-positions` and then:

```sh
./target/debug/qiyas-cli bench rescore-agent \
  --report .local/benchmark/runs/eval-agent-set-20261022T211039320437030-1177463/report.json \
  --output .local/benchmark/runs/rescore-fifth-fixes
```

All 780 generated truth files (60 clean cases and 720 variants) had positions refreshed.
An independent audit compared all 1,099 development truth files against their original bytes:
every byte outside position arrays is identical. The other 319 truth files are unchanged.
The provenance log records each changed file's before/after hash. Original combined report
and per-case artifacts are unchanged. All 105 replayed dimension quantities, citations and
complete dimension lines equal their saved measurement reports. No agent was run.

| Case | Coverage before → after | Precision before → after | Passing quantities after |
|---|---:|---:|---:|
| apartment-17000 | 77.78% → 77.78% | 31.82% → 31.82% | 7/9 |
| office-15000 | 87.50% → 87.50% | 46.67% → 46.67% | 7/8 |
| warehouse-16000 | 75.00% → 75.00% | 42.86% → 42.86% | 6/8 |
| mep-18000 | 62.50% → 62.50% | 83.33% → 83.33% | 0/8 |
| external-19000 | 50.00% → 50.00% | 100.00% → 100.00% | 4/8 |
| apartment-17000/variants/scanned-pdf | 0% → 0% | 0% → 0% | 0/9 |
| **Overall** | **58% → 58%** | **35.37% → 35.37%** | **24/50** |

Matched accuracy remains 24/29 = 82.76%; matches remain 29, extras 53, and traceable items 20.
There is no contamination. Coverage measures correspondence, so the five matched items in
the allowance-affected case remain quantity failures; replay intentionally cannot fix them.

The scan remains 0/9 with 21 extras. Its first region is now only 0.218 m from the regenerated
centroid and falls inside the new 0.25 m tolerance. However, its allowance, surface and area
recipes all have the same description/kind/position, so all three remain ambiguous. The other
three region offsets are 0.272, 0.254 and 0.268 m, beyond their 0.25 m tolerance, and each also
has three competing area recipes. The five remaining truth rows have no same-description/kind
candidate. Diagnostics are in `.local/agent-fifth-fixes/scan-correspondence.json`. No nearest
candidate, quantity-based choice, label rewriting or extra tolerance was added to improve the
score. Regression tests prove unambiguous scanned items can now match; the saved scan still
contains interpretation/calibration errors. This replay is not evidence of improved agent
behavior under the new self-check workflow.

Audit summary: `.local/agent-fifth-fixes/audit.json`.
Replay report: `.local/benchmark/runs/rescore-fifth-fixes/report.json`.


### Final quick autonomy check

`./target/debug/qiyas-cli bench run --set quick --mode autonomy` ran once, last, and
completed with exit 0 in 164.64 seconds (120 cases, one repetition).
Report: `.local/benchmark/runs/baseline-autonomy-20261022T214614335136632-3/`.

Coverage is 18.71% (687/3,672), precision 94.76% (687/725), with 38 extras. All 687
matches pass quantity tolerance and are traceable. The comparison checks found all 36 seeded
errors, with nine false positives; those discrepancy metrics equal the previous saved result.

The previous quick result had 716 matches, nine extras, 19.50% coverage and 98.76% precision.
All 29 lost correspondences are vector-PDF items: the previous saved submissions used labels
such as `Living room`, while the existing verbatim-label adapter now emits `1 Living room `.
All 29 current PDF labels differ from truth by their numeric label prefix (plus whitespace).
Their geometry positions are consistent with regenerated truth: maximum separation is
0.00224 m, far below the new tolerance. This exposes the adapter change documented at the
end of the third-run section, whose earlier quick check predated that change. No descriptions
were altered in this task: truth maintenance was explicitly restricted to positions.
Detailed comparison: `.local/agent-fifth-fixes/quick-pdf-diagnostics.json`.

Final whitespace check passed. The requested full core/CLI check and the AI localhost test
remain the verification limitations stated above. The saved scan still has ambiguous recipes,
small calibration errors and unmatched descriptions; no fresh agent run was permitted or made.

## Sixth free run: explicit note handles (23 October 2026)

The six-case source is
`.local/benchmark/runs/eval-agent-set-20261022T220804177100606-1297250/report.json`:
28/50 matches, 26 passing quantities and 35 extras (56% coverage, 44.44% precision,
92.86% matched accuracy). A newer set was being written independently during this work;
its source is `eval-agent-set-20261022T222211526997653-1319503/report.json`.
Its combined report appeared with one case and subsequently grew, so any replay of it is
identified by the number of cases in the saved snapshot.
No agent evaluation was started for this fix.

Digest notes now expose `named_handles`, alongside their existing closest geometry. Parsing
uses complete decimal or explicitly prefixed hexadecimal numbers that resolve to actual
entities in the drawing; it deduplicates references and excludes decimal-factor fragments,
embedded identifiers, overflow and absent handles. External-reference text does not resolve
against local handles. Complete note text is parsed before it is split across digest pages.
No domain vocabulary, label interpretation or benchmark truth is used.

`takeoff_check` reports error-severity `named_handle_omitted` when a named source was rejected
or has no measurement-rule use. Accepted recognition and dimension-source evidence count as
use, including a referenced schedule supplying measured elements. `note_factor_scope` reports
“factor from note N applied to elements the note does not name” when a cited numeric factor
is used on disjoint sources. Factors are checked in separate operands, expressions, timesing,
percentage additions/deductions and unit bands. These errors contribute to `measurement_errors`
and make `passed` false. The disjointness test intentionally cannot decide which of several
handles within one note owns a particular factor; overlapping scope still needs interpretation.

Workflow and kit say named handles are in scope and must be measured with their note's factors.
A deliberate exclusion needs a separate final-answer line:
`Exclude <handle>: <reason> (note <handle>)`. The optional `takeoff_check.end_report` string
checks those lines in a draft answer. Headless finish and the live evaluator check the retained
final answer too. A matching exclusion excuses only that handle's omission for that note;
it never suppresses a disjoint-factor error. A generic unlabelled rejection, empty reason,
wrong handle or wrong note does not excuse it. These checks do not alter measurements or
saved submissions.

Regressions cover parsing, omitted and rejected handles, explicit exclusion reasons, incorrect
citations, disjoint versus intersecting rule sources, unscaled rules and accepted source evidence.
The shared tool test exercises digest output, error counts, failed checks, a draft exclusion
and measuring the named 7.75-unit source with its 2.4 factor to produce 18.6.

Verification logs and original-artifact hashes are in `.local/agent-sixth-fixes/`.

### Sixth-run checks and saved replay

All Cargo commands ran one at a time, with `CARGO_BUILD_JOBS=4` for compilation. Formatting
and workspace Clippy each ran once and passed. The first core/CLI invocation was interrupted
by the system memory kill during compilation, with no test result. Following the user's
instruction to resume, only that unfinished command was retried. A temporary `RUSTC_WRAPPER`
using `flock` serialized compiler processes while retaining the requested Cargo job setting;
the AI check used the same lock. No source or test assertion was changed during recovery.

| Check | Actual result |
|---|---|
| `cargo fmt --all` | Passed. |
| `CARGO_BUILD_JOBS=4 cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed, exit 0. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-core -p qiyas-cli` | Original attempt interrupted by the system; resumed with compiler serialization, exit 0: 839 passed, zero failed. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-ai --lib --test surface` | Exit 101: 73 library tests passed; `models::tests::an_unreachable_service_falls_back_to_the_builtin_list` failed because the sandbox denied localhost binding (`PermissionDenied`). Cargo stopped before surface. |
| Newly built surface executable, run directly | Exit 0: all 54 passed, including the new scope regression and kit-size guard. |
| `bench rescore-agent`, sixth run | Exit 0; all metrics unchanged: 56% coverage, 44.44% precision, 92.86% matched accuracy; 28 matches, 26 passing quantities, 35 extras. |
| `bench rescore-agent`, newest report snapshot | Exit 0; five completed cases at snapshot time, all metrics unchanged: 63.41% coverage and precision, 100% matched accuracy; 26 matches, 26 passing quantities, 15 extras. This is a partial independently running set, not a new evaluation launched here. |
| `git diff --check` | Passed. |

The sixth replay is `.local/benchmark/runs/rescore-sixth-fixes/report.json`. The newer replay
is `.local/benchmark/runs/rescore-sixth-fixes-newest/report.json`, using the exact combined
report snapshot at `.local/agent-sixth-fixes/newest-source-report.json`. Original saved case
artifacts and the sixth combined report retain their hashes. All replayed submitted quantities,
citations, source handles, bases, rule IDs and dimension lines equal their original submissions.
Full audit results are in `.local/agent-sixth-fixes/audit.json`.

The localhost-binding test still needs execution outside this sandbox. Overlapping handles
within a single note still require interpretation; the implemented factor check is the requested
disjointness check. No real agent evaluation, new crate, key/settings access, staging, commit
or push was performed. No benchmark truth was inspected by an agent; only the offline scorer
opens truth after constructing submissions.

Files touched:

- `CLAUDE.md`
- `docs/2026-10-22-agent-eval-fixes.md`
- `crates/qiyas-core/src/digest.rs`
- `crates/qiyas-core/src/digest/note_scope.rs`
- `crates/qiyas-core/src/digest/measurement_checks.rs`
- `crates/qiyas-core/src/takeoff_kit.rs`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/surface_args.rs`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-cli/src/eval.rs`
- `crates/qiyas-cli/src/headless.rs`

Logs, snapshots, audit data and replay outputs above remain local-only.

## Eleventh free run: raster calibration and PDF correspondence (23 October 2026)

Source: `.local/benchmark/runs/eval-agent-set-20261022T234438437979749-1494095/report.json`.
The six saved cases originally matched 39/50 items (78% coverage), with all 39 passing
quantity tolerance; the scan matched 0/9. No agent evaluation was run for this fix.

Raster vision now refines the bar centreline and transverse tick centres using integrated
antialiased ink coverage. Both tick halves must agree with one unambiguous peak; missing
or competing ticks retain the supplied anchors with a warning. Multiple supplied references
are refined independently; the longest verified span sets scale. `dimensions` accepts
verbatim dimension strings and endpoints, including explicit metric/imperial units or a
caller-declared conversion for bare numbers. Independent scales are compared, with the
maximum discrepancy included in reported residual uncertainty. Unresolved dimension text
is reported. Residual uncertainty above 0.5% caps candidate confidence at 0.5; evidence
retains the calibrated anchors and uncertainty after acceptance. This does not claim that
an unverified reference's uncertainty is an upper bound on its error.

`recognition.vision.texts` carries spatial text from vision/OCR; the PDF text layer is also
collected automatically. Closed regions retain the nearest enclosed verbatim label on their
driver, independently of the object type's declared properties. PDF metadata likewise uses
the nearest enclosed text. Equal-distance alternatives remain ambiguous; ordinary CAD text
keeps its existing unique-label rule. Saved drivers retain their labels for export and replay.
There is no bundled OCR engine or automatic transcription of image-only text: the vision
caller supplies it, as the workflow and command schema now request.

After `description_position`, the PDF-only `pdf_position` tier requires the same kind and
unit and a position within max(0.5 m, 3% of the geometry-derived diagonal). One candidate
matches directly. Multiple candidates can use the existing word tie-break only when they
occupy the same position; otherwise they remain unmatched. Quantities never choose a match.
The existing description tolerance and numerical scoring tolerances are unchanged.

`bench calibrate-scans --output FILE [--agent-report REPORT]` is an offline benchmark audit.
It runs production calibration on every generated scan using its existing prepared input
anchors, then opens generated geometry to measure the scale error. Its optional saved-report
path additionally audits the retained agent calibration input against the saved source PDF.
Neither the production calibration function nor prediction receives truth. Cases, recipes,
truth, saved quantities and original agent artefacts are not rewritten.

Regression coverage includes sub-pixel and vertical ticks, ambiguous/missing ticks, longest
span selection, conflicting dimension text, confidence reduction, label persistence through
acceptance/save, nearest/enclosed PDF labels, submission labels without a `name` property,
PDF-only position matching, kind/unit/location constraints, and quantity-independent ties.

Files touched:

- `CLAUDE.md`
- `docs/2026-10-22-agent-eval-fixes.md`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-cli/src/bench/calibration.rs`
- `crates/qiyas-cli/src/bench/mod.rs`
- `crates/qiyas-cli/src/bench/score.rs`
- `crates/qiyas-cli/src/bench/tests.rs`
- `crates/qiyas-core/src/element_metadata.rs`
- `crates/qiyas-core/src/recognition.rs`
- `crates/qiyas-core/src/recognition/raster_calibration.rs`
- `crates/qiyas-core/src/recognition/vision.rs`
- `crates/qiyas-core/tests/element_metadata.rs`
- `crates/qiyas-core/tests/recognition.rs`
- `crates/qiyas-pdf/src/calibration.rs`
- `crates/qiyas-pdf/src/lib.rs`

Logs, source-report snapshot and original-artifact hashes are under
`.local/agent-eleventh-fixes/`. No helper agents, new crates, key/settings access,
staging, commits or pushes were used. Cargo commands run sequentially with four build jobs.

### Eleventh-run saved replay

`bench rescore-agent` completed with exit 0. Output:
`.local/benchmark/runs/rescore-eleventh-scanned-fixes/report.json`.

| Metric | Overall before → after | Scan before → after |
|---|---:|---:|
| Matched coverage | 39/50 (78%) → 46/50 (92%) | 0/9 → 7/9 (77.78%) |
| Passing quantities | 39/50 → 41/50 | 0/9 → 2/9 |
| Matched accuracy | 100% → 89.13% | undefined → 28.57% |
| Item precision | 48.15% → 58.97% | 0% → 46.67% |
| Extras | 42 → 32 | 18 → 8 |

The other five cases retain their original matches and passing quantities: apartment 8/9,
office 8/8, warehouse 8/8, MEP 8/8 and external 7/8. All seven new scan matches use
`pdf_position`. Count and provisional allowance pass their existing tolerances; the five
other matched quantities retain errors of 2.20–4.73%. Bedroom and bathroom positions are
0.533 m and 0.529 m away, outside the required tolerance, and remain unmatched. No tolerance
was extended to recover them. Matching the complete count aggregate consumes its four
components, accounting for three fewer independently scored submissions without removing
any saved rows.

All 152 replayed submission rows retain their original quantities, dimension lines, citations,
source objects, bases and rule IDs. SHA-256 checks confirm 26 original report/drawing/source/
measurement/event artefacts are unchanged. Audit: `.local/agent-eleventh-fixes/replay-audit.json`.
The lower matched accuracy exposes previously unmatched errors; it is not a new agent result
or evidence that old measured quantities were corrected.

### Calibration audit

All 60 generated scanned variants are included, including failed refinements; no outlier is
excluded. The baseline is the existing prepared calibration anchors passed through the old
unrefined scale calculation, not 60 new agent observations.

| Across all 60 scans | Before | After |
|---|---:|---:|
| Median absolute length error | 0.565684% | 0.289864% |
| Cases below 0.5% | 29/60 | 43/60 |
| Maximum absolute length error | 3027.389469% | 3027.389469% |

37 spans refine successfully; every one has error below 0.5% (maximum 0.435725%).
23 supplied references cannot be verified and are retained with warnings and reduced
confidence. Six older prepared references have errors above 75%, including two above 2700%;
their endpoints are misplaced or their spans are far too short. The product does not use
known generated geometry to repair these references. Readable references/dimension evidence
must be supplied for them. The requested median target is met; calibration is not universally
solved for arbitrary bad input anchors.

The saved eleventh-run agent input is audited separately: its length error falls from
**2.084051% to 0.006269%**. Refined tick centres span 38.308105 page points instead of the
agent's 39.125911. Residual uncertainty is still 0.922921%, so confidence is reduced: a short
single reference is not independent evidence of sub-percent certainty. This audit changes
neither the saved drawing nor its quantities. Complete inputs/results for all 60 scans and
the saved agent are in `.local/agent-eleventh-fixes/calibration-final.json`.

### Eleventh-run verification

Each requested command was invoked once, with no simultaneous Cargo commands or helper
agents. The full test command's failure and targeted recovery are reported separately.

| Check | Actual result |
|---|---|
| `cargo fmt --all` | Passed, exit 0. |
| `CARGO_BUILD_JOBS=4 cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed, exit 0. The only later Rust change corrects the new persistence test's save API; production source is unchanged. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-core -p qiyas-cli -p qiyas-pdf` | Exit 101: 619 passed, one new persistence fixture failed because it called a nonexistent `save` command. Compilation completed in 10m 04s. |
| Targeted recognition recovery | The first recovery had eight passes and the fixture failed because a new unsaved session needs Save As. Corrected to `save_as(&drawing)`; the final targeted persistence test passed. No assertion was removed or relaxed. |
| Remaining already-built core/PDF test binaries | All 30 binaries passed: 239 tests, zero failures/ignored. Together with the corrected persistence test and original passing tests, all 859 distinct core/CLI/PDF test cases are verified. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-core -p qiyas-pdf --doc` | Passed, exit 0; both crates have zero doc tests. |
| `CARGO_BUILD_JOBS=4 cargo test --offline -p qiyas-ai --lib --test surface` | Exit 101: 73 library tests passed; `models::tests::an_unreachable_service_falls_back_to_the_builtin_list` failed on sandbox localhost binding with `PermissionDenied`. Cargo stopped before surface. |
| Newly built surface executable, directly | Passed, exit 0: all 54 tests. |
| Final direct rustfmt check of the corrected test, and `git diff --check` | Passed. |
| Calibration audit and saved rescore | Both completed with exit 0; results above. |

The only remaining test failure requires localhost binding outside this sandbox. The requested
full core/CLI/PDF command itself did not pass; the sequential recovery establishes its remaining
test results without repeating that command. The two setup corrections affect only the new test,
which now verifies the real save/reopen path with every original assertion intact.

The final `./target/debug/qiyas-cli bench run --set quick --mode autonomy` ran once,
last, and completed with exit 0 in 165.47 seconds. All 120 cases completed. Coverage is
19.50% (716/3,672), precision 98.76% (716/725), with nine extras; all 716 matches pass
quantity tolerance. These headline item metrics equal the previous quick result. The PDF
subset has 29 matches, all passing, and no extras. Comparison found all 36 seeded errors
with nine false positives. This is the shared-rule engine diagnostic, not an agent evaluation.

Quick report_directory: `.local/benchmark/runs/baseline-autonomy-20261023T002825336674420-3`.
