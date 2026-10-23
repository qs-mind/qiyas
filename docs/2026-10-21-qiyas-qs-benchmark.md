# Phase 15: the QS benchmark

The benchmark measures quantity accuracy, coverage, source reproducibility, discrepancy
detection, elapsed time and repeatability before later QS phases change the engines.
The roadmap follows the founder's plan: benchmark 15, foundation 16, recognition 17,
measurement rules 18, discrepancies 19, revisions 20, projects 21, deliverables 22, trust 23.

All development data stays in the git-ignored `.local/benchmark/`. Nothing is copied into
`~/.qiyas`. No downloaded benchmark material is committed. The Rust generator and harness
ship as CLI code; conversion scripts and their Python environment are development tools.

## Reproduce

```sh
CARGO_BUILD_JOBS=2 cargo run --offline -p qiyas-cli -- bench generate
CARGO_BUILD_JOBS=2 cargo run --offline -p qiyas-cli -- bench run
target/debug/qiyas-cli bench list
target/debug/qiyas-cli bench run --case office-15000 --repeats 3
target/debug/qiyas-cli bench run --clean-only
target/debug/qiyas-cli bench score --case office-15000 --submission takeoff.json
target/debug/qiyas-cli bench score --case office-15000 --submission takeoff.xlsx
```

`--root` defaults to `.local/benchmark`. The generator uses ten seeds per family by default;
`--seeds N` accepts 1–100. Six families: office fit-out, warehouse, apartment floor, MEP,
external works, and a non-building part. Geometry, symbols, notes and tables are drawn with
`Session::run_input`, the same validated commands as the window. Truth is calculated from
the input dimensions while drawing, independently of Qiyas's measurement results.
Dimensions, counts and shapes vary deterministically using SplitMix64.

Each clean case has exploded symbols, wrong layers, merged layers, overlaps, gaps, line text,
inches, missing schedule, seeded BOQ/schedule errors, revision B, vector PDF and scanned PDF
variants. The inches variant changes coordinates and header together; it is a valid unusual
unit drawing, not a deliberately false INSUNITS header. Mutations and revision deltas are
recorded in truth metadata. Vector PDFs use Qiyas's plot engine; scanned PDFs contain a
rasterized drawing. They are synthetic scans, not samples from a physical scanner.
Every drawing has a visible scale bar (1 m, or 0.1 m for the part), drawn with ordinary Qiyas
commands and retained in the PDFs. Clean cases have a `clean-boq.json` counterpart for manual
review; it is not passed to the live agent. Only seeded-error cases supply `input-boq.json`.

```text
.local/benchmark/
  sources/                 provenance, licences, verified upstream downloads
  cases/<id>/              drawing.dxf and truth.json
    variants/<variant>/    drawing, truth, mutation metadata and visible inputs
  holdout/                 sealed independent cases; never opened by dev commands/tests
  holdout-manifest.json     only SHA-256 seals, no quantities or project identities
  runs/baseline-*/          each repetition's submission plus report.json/report.md
  runs/eval-*/              live evaluation outputs
  runs/holdout-exam-*/      deliberate external-agent final exam, report.json and case folders
  holdout-log.jsonl         append-only opening and outcome events
  baseline.json            latest baseline report
  baseline.md
  .venv/                   optional converters, never shipped
```

Development commands never read or regenerate existing holdout contents. On Unix the generator removes
directory permissions after sealing. This is an operational separation, not a cryptographic
defence against the machine owner: generator code and seeds are public. A blind human-run
evaluation should create new private seeds and keep its truth outside developer access.
`bench list` reads only the external seal manifest. Case selection accepts a clean ID or
`<id>/variants/<variant>`; traversal, absolute paths, `holdout` and symlink aliases are refused.

## Deliberate holdout final exam

Only the reviewer runs this command, once, after development is finished:

```sh
./target/debug/qiyas-cli eval-agent --holdout --i-understand-this-opens-the-holdout --agent codex --model gpt-6.1-sol --effort medium --timeout-min 20
```

The long acknowledgement is mandatory; without it the command refuses before accessing the
holdout. `--holdout` cannot be combined with `--case`, `--cases` or `--set`. The existing
`--root` and `--agent-cmd` options also work, including for offline temporary-fixture tests.
This path makes no model API calls and never reads Qiyas settings or API keys. The external
client uses the existing subscription login, as described in `2026-10-22-agent-eval.md`.

The loader accepts the published version-1 seal format, SHA-256 of the exact bytes of
`drawing.dxf || truth.json`. It requires every manifest case, rejects unlisted cases and
symlink inputs, and verifies every hash before any agent starts. A mismatch aborts the whole
exam. On Unix it temporarily grants the owner read/traverse access to the holdout directory,
then restores its original permissions before agents start, including on verification failure.
Verified drawing bytes and truth stay in the outer harness's memory. Each case workspace
receives only the drawing and `preamble.md` when there are clauses: specification wording only,
without rule parameters, truth quantities, recipes, sidecars or visible BOQ/schedule files.
The scorer retains the saved drawing and its embedded measurement knowledge for numerical tracing.

The report and all case runs are under `.local/benchmark/runs/holdout-exam-<timestamp>/`.
`report.json` retains the shared scorer's metrics and per-case scores, plus `north_star`:

| Field | Definition / target |
|---|---|
| `within_2_percent_all_items` | Items within ±2% / all truth items; target at least 95%. Misses count against it. |
| `within_2_percent_matched_items` | The existing P@2% among matched items, shown separately. |
| `coverage` | Matched / truth items; target strictly over 90%. |
| `traceability_submitted` | Reproducibly cited items / submitted non-aggregate items, including extras. |
| `seeded_error_recall` | Correctly located and classified findings / seeded errors; null when none exist. |
| `precision` | Item precision: matched / (matched + extras). |
| `seeded_error_precision` | Correct seeded-error findings / all submitted findings; null when no findings exist. |

All rates use 0–1. Zero denominators are null, never perfect. Existing scorer unit, basis,
matching and traceability rules apply. The same logged-action contamination guard excludes
contaminated cases from metrics; `complete: false` prevents that subset from looking like a
complete exam. Failed clean agents still contribute zero-submission scores when appropriate.
Seeded-error recall remains visible even though extra BOQ/schedule input files are not supplied.

`holdout-log.jsonl` is append-only. An `opening` event is flushed before sealed contents are
accessed, followed by a `completed`, `incomplete` or `failed` event. Both carry time, agent,
model, git commit, coverage, accuracy (±2% over all truth items), precision (item precision)
and the report directory; metrics are null until available. Outcome events also include the
north-star metrics or an error. The shared report directory correlates both events. A crash
therefore leaves an opening record even without an outcome. Partial reports remain available.
An exclusive `holdout-exam.lock` prevents concurrent exams. After an abrupt termination, inspect
the log, saved reports and directory permissions before manually resolving a stale lock; do
not automatically retry. A subsequent deliberate invocation creates another logged opening.

Offline tests use a fake external agent and newly generated temporary sealed cases. They cover
mandatory acknowledgement, drawing/truth hash mismatches before any agent starts, append-only
logging, permission restoration, allowed inputs, north-star denominators and contamination.
Implementation and tests never enumerate or open the real holdout. No real final exam was run.

## Version 1 truth schema

`truth.json` has `schema_version: 1`, a stable `id` equal to its path relative to `cases/`,
`project_type`, a relative `drawing`, nonempty `items`, optional `rules` and `seeded_errors`.
Additional provenance fields (licence, source, seed, mutations, model references) are retained
in the data files. Unsupported schema versions fail rather than being interpreted silently.

```json
{
  "schema_version": 1,
  "id": "example",
  "project_type": "office",
  "drawing": "drawing.dxf",
  "items": [{
    "id": "slab",
    "description": "Ground floor slab",
    "kind": "volume",
    "unit": "m3",
    "quantity": 6.0,
    "source_objects": ["2A"],
    "basis": "rule:slab-depth",
    "tolerance_class": "rule",
    "tolerance_percent": 2.0,
    "position": [2.0, 3.0]
  }],
  "rules": {
    "slab-depth": {
      "operation": "area_times_height",
      "height_m": 0.2,
      "clause": "Project preamble: slab thickness 200 mm"
    }
  },
  "seeded_errors": []
}
```

| Item field | Meaning |
|---|---|
| `id` | Unique stable item code within the case |
| `kind` | `count`, `length`, `area`, `volume` |
| `unit` | Explicit unit; normal physical units are `pcs`, `m`, `m2`, `m3` |
| `quantity` | Finite nonnegative number, in the stated unit |
| `source_objects` | Nonempty list of uppercase hexadecimal CAD handles or namespaced model/PDF IDs |
| `basis` | `geometric` or `rule:<name>` referencing the case's declared rule |
| `tolerance_class` | `count`, `geometry`, `rule`, `judgement`; counts always use `count` |
| `description` | Human-readable description for deterministic matching |
| `position` | Optional `[x,y]` in metres, used to disambiguate descriptions within 50 mm |
| `height_m` | Explicit height/depth for geometric volume reproduction |
| `tolerance_percent` | Optional rule tolerance, 2–5 inclusive; default 2 |

Rule operations supported by the scorer are `area_times_height` (`height_m`),
`length_times_factor` (`factor`) and `area_net` (`deduction`, in m²). Each names a `clause`.
These are case preambles, not an implementation or redistribution of POMI/NRM2/SMM7/CESMM4.
Judgement quantities explicitly declare their allowance; no standard is invented.

`seeded_errors` is an exhaustive list of `{item_id, kind}` with optional descriptive metadata
such as `magnitude_percent`. The generated classes are `missing`, `wrong_quantity`,
`wrong_unit`, `schedule_mismatch`. The visible `input-boq.json` contains the corrupted BOQ;
the visible `schedule.json` and drawing table contain the schedule count. Gold errors are
never handed to the baseline or the live model. Revision B records before/after quantities
and added handles in `revision.deltas`.

## Submissions and scoring

The portable JSON form is `{items: [...], findings: [{item_id, kind}], elapsed_ms: 123}`.
Items use the truth item fields, without requiring `tolerance_class`; only truth controls
tolerances. Native Qiyas `Takeoff` JSON also works: `shown` becomes `quantity`, `handles`
(integer handles) become hexadecimal source objects, and `boq_code` takes priority over its
measurement UUID. Native `height` uses drawing units, so include explicit `height_m` for a
portable volume submission. JSON BOQ `{items:[{code,description,unit,quantity}]}` also works.
Bare item arrays are accepted.

The reader supports Qiyas's `Take-off` Excel sheet (stops before its totals) and BOQ workbooks
through the existing BOQ importer. The current Qiyas Excel format lacks handles; numbers may
score correctly but cannot earn traceability. Use JSON to retain citations. Unknown BOQ units
need a JSON `kind`; wrong units are not silently converted into correct ones.

Matching is deterministic, without an LLM or quantity-dependent guesses: unique exact ID,
then exact source-object set with the same kind and basis, then normalized description and
position. With no submitted position, a description must be unique in truth. Ambiguous
matches remain unmatched; duplicate submitted IDs cannot earn two matches. Extra rows are
reported separately, including duplicate automatic room/region observations.

| Band | Pass |
|---|---|
| Count | Exact |
| Geometry | Absolute percentage error ≤0.5% |
| Rule | ≤declared 2–5%, default 2% |
| Judgement | ≤10% |
| Comparison bands | P@1%, P@2% (roadmap target), P@5%, P@10%, P@25% also reported |

Coverage = matched / all truth items. `pass_rate_all` includes missed items;
`pass_rate_matched` and comparison bands use matched items, including wrong-unit failures.
Zero truth passes only at exactly zero. No single blended headline score hides coverage.
The median absolute percentage error uses compatible matched nonzero truth quantities
(exact zero/zero contributes zero); incompatible units and nonzero/zero are failures and
do not have a finite percentage error.

Traceability is independent of truth quantities and rule names. Agent scoring opens the saved
run's drawing with embedded knowledge only, without sidecars or external-resource loading.
The scorer freshly evaluates its measurement method against geometry, then locates each
submitted dimension recipe by ID or source handles and rule. Sources and evidence handles
must exist; rule-derived drawing-note citations must name existing note handles. Dimension
sources, expressions, operations, units, citations and inclusion decisions must agree, and
fresh numerical values must reproduce the submission within 0.5%, or exactly for counts.
Plain geometric submissions retain direct handle remeasurement as a second path. Cached
measurement results alone cannot establish traceability. Calibrated PDF driver handles can
be checked in the saved drawing without opening the original PDF.

A complete aggregate traces only when all its component dimension recipes trace. Reports
retain `submission_traces` for every submitted row, including extras and aggregates, with a
failure reason. `traceability_submitted` counts traceable matched rows and extras over the
existing matched-plus-extras denominator; redundant aggregates and consumed components are
not counted twice. `traceability_all` counts traceable matched items over all truth items;
missing items therefore remain failures. The two rates have separate numerators.

`bench rescore-agent` preserves saved submissions and writes a new report directory. For a
saved holdout report it updates only traceability, preserves correctness and correspondence,
and appends a `rescore` event to `holdout-log.jsonl`; it never opens sealed inputs. New scores
retain submitted row indices. For legacy reports without those indices, the scored set can
be recovered from saved extras and complete aggregate memberships when that partition is
unique. No quantity chooses a correspondence. In mixed legacy results the per-truth-row
trace fields are omitted because the individual links were not saved; `submission_traces`
identifies each exact failure and the recovered scored set supplies the totals. Ambiguous
partitions are refused. If every submitted row reproduces, all previously matched rows
necessarily reproduce without needing the missing links.

Detection requires both the correct item/location and error class. Precision, recall and F1
are reported, with TP/FP/FN by class and false alarms per clean case. Repeated alerts count
as extra false positives. A zero denominator is `null` (not applicable), never perfect.

The baseline discovers entities without receiving gold quantities: `CountBlock`,
`MeasureEntity` for closed areas, and `rooms` for candidate room areas. It clears embedded
take-off first. It does not infer length, height, rule deductions or discrepancy findings;
these omissions lower coverage and error recall. PDF imports/calibration and semantic
recognition are future baseline work, so unsupported PDFs count as fully missed cases,
with an explicit warning. Runs repeat three times; fingerprints omit random measurement
UUIDs and timestamps but retain quantities, descriptions and citations. Each report records
per-run elapsed times and hashes, item failures, extras, project/tolerance breakdowns and
what phases 16–20 must improve. Failed/unsupported repeated attempts are not evidence of
successful model repeatability.

## Source pipeline

Research: `.local/research/reports/QS benchmark sources and methods.md` and its six notes.
The four approved external sources are buildingSMART Clinic and Duplex, ResPlan and Swiss
Dwellings (CC BY 4.0). See `scripts/benchmark/README.md` for fetch/convert commands and
checks. The fetch allowlist excludes restricted, non-commercial, academic-only and unclear
sources. Candidates are metadata only; there is no permission-request messaging or download.

The report provides source URLs but no literal checksums. Download tooling obtains Git LFS
SHA-256 or official Zenodo checksums where published, records source revisions and local
SHA-256, and refuses unverified data rather than inventing a checksum. A local download plan
records all four sources and their licences before the network step. Conversion needs a
local venv; those dependencies are neither bundled nor Rust dependencies.

IFC quantities come from model mesh geometry, with storey sections exported to DXF. Dataset
geometry is exported in metres. Model quantities need a COBie/hand sample cross-check before
claiming validated source truth; Swiss geometry is exact relative to that dataset, not to
an original plan whose research reports up to 5% deviation. No downloaded formats or actual
IFC conversion could be validated in the network-restricted development environment.
Synthetic scores do not establish performance on external consultant drawings.

## Live Mizan evaluation

Every live run requires Kareem's explicit approval. After that approval:

```sh
target/debug/qiyas-cli eval --case office-15000 --provider-from-settings --approve-live-run
```

The approved runtime reads the app's selected provider profile and retrieves its key through
the OS keychain (`qiyas`, provider account). No key is printed, logged, stored in artifacts,
or passed on a command line. Development and tests do not read actual settings or credentials.
The agent receives a copied drawing and visible BOQ inputs, not truth. It uses the production
agent/tool surface headlessly. Original drawings are preserved. Output includes the resulting
drawing, take-off JSON/Excel, usage, conflicts and scored report; the scorer compares against
the original geometry, so changing geometry to match a guessed quantity cannot earn credit.
CAD drawings open without sidecars or external resources, preventing indirect reads into
sealed data. PDF cases use the existing app importer: copy `source.pdf`, attach page 1 as an
underlay in a new metre `drawing.dxf`, and import available vectors. The agent sees the
underlay through its picture tool. Paper coordinates are explicitly uncalibrated; it must
calibrate from a visible dimension or scale bar and scale vectors consistently, or report
that it cannot measure. It never receives scale inferred from truth. Reports disclose that
only page 1 was processed. PDF preparation needs the app's locally installed PDFium; an
unavailable library or malformed PDF fails before settings/keychain access. Reproducible
PDF region citations remain a scoring gap even when an agent reports correct quantities.

`.local/autopilot/budget.json` defaults to `{"daily_usd":5}`. Add verified per-million-token
prices for the exact provider/model before a paid run; no prices are fabricated or inferred
from a model name. Compatible providers also require the exact saved `endpoint` in the price
entry because relays can charge differently. Example shape (values below are placeholders,
not model prices):

```json
{
  "daily_usd": 5,
  "prices": {
    "anthropic:EXACT-MODEL-ID": {
      "input_per_million": 1,
      "output_per_million": 2,
      "cache_read_per_million": 0.1,
      "cache_write_per_million": 1.25
    }
  }
}
```

Before each provider request a conservative maximum charge is reserved under an atomic
ledger lock. A request that would exceed the daily budget or `--max-cost-usd` is refused
before network activity. `--max-tokens` and `--max-requests` bound the run. Daily accounting
uses UTC dates; costs are estimates from the configured prices, not provider invoices.
Actual usage settles the reservation; missing usage, a failed request or a crashed process
retains it. A stale lock requires manual inspection. Strict provider mode prevents retries
from dropping the output/usage controls or silently selecting a different model. The
conservative reservation may reject a request whose eventual actual bill would be lower.
Per-run tokens (including cache and reasoning) and estimated USD are written to
`.local/autopilot/costs.md`; request reservations live in `budget-ledger.json`.

All tests use a scripted provider and local temporary directories. No live run is included
in Phase 15 verification.

## Dependencies and validation

No new third-party crate is introduced to the workspace. The CLI reuses already approved
crates: SHA-256 via `sha2` (MIT OR Apache-2.0), `calamine` (MIT) for exported workbooks,
`keyring`/`keyring-core` (MIT OR Apache-2.0) for the app's OS credential store,
`tokio`/`tokio-util` (MIT), `chrono` (MIT OR Apache-2.0), and `tiny-skia` (BSD-3-Clause)
for the local PDF raster variant. Existing Qiyas crates supply plotting and the real agent.

Validation on 21 October 2026 (no live model run):

| Check | Actual result |
|---|---|
| `CARGO_BUILD_JOBS=2 cargo fmt --all` | Passed; later edits formatted individually; final `rustfmt --check` passed |
| `CARGO_BUILD_JOBS=2 cargo clippy --offline --workspace --all-targets -- -D warnings` | Initial run failed on one `chunks_exact_to_as_chunks` lint in the PDF generator; fixed |
| Clippy completion after that fix | `-p qiyas-cli --all-targets` and `--workspace --exclude qiyas-cli --all-targets`, both with `--offline` and `-D warnings`, passed; together cover the workspace |
| `CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-cli` | 42 passed, 0 failed (24 unit + 18 integration) |
| `CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-core` | 676 passed, 0 failed, 0 ignored; three ignored-Result warnings in the new test were fixed and the final Clippy completion passed |
| Core standalone loader after the assertion fix | `CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-core --test standalone`: two passed |
| `target/debug/qiyas-cli bench generate` | 60 clean cases, 720 variants, six sealed holdout cases |
| `target/debug/qiyas-cli bench run` | Passed on all 780 dev entries, three repetitions; 12.674 seconds total wall time |
| `corepack pnpm exec vitest run src/shell/PlotDialog.test.tsx src/shell/SettingsDialog.test.tsx --repeats 20 --maxWorkers=2` | Both files passed: 32 logical tests, each repeated 20 times, 16.59 seconds |
| TypeScript `tsc --noEmit` | Passed |
| Offline Python converter/downloader tests | Eight passed; Python syntax and shell syntax passed |
| Final formatting and `git diff --check` | Passed |

The requested full commands were each invoked once. The Clippy correction was verified by
the two scoped completion commands, rather than repeating the whole workspace invocation.
No `scripts/check.sh`, git staging, commit or push was performed. The licence inventory
regeneration found no changed dependency entries; its locale-only sorting changes were removed.
Old incremental compiler caches predating this task were cleared when linking the core tests
reduced disk headroom; source and benchmark files were preserved.

### Baseline result

The complete report is development data at `.local/benchmark/baseline.json` and `baseline.md`,
also retained in `runs/baseline-20261021T191324812793070-3/` with all 2,340 submissions.

| Measure | All 780 dev entries | Clean 60 cases |
|---|---:|---:|
| Truth items | 6,370 | 490 |
| Matched, passed and reproducibly cited items | 2,630 | 250 |
| Coverage / pass rate over all truth | 41.29% | 51.02% |
| Pass rate among matched items | 100% | 100% |
| P@2% and P@25% among matched items | 100% | 100% |
| Extra submitted rows | 1,900 | 190 |
| Traceability over all submitted rows, including extras | 58.06% | 56.82% |
| Seeded discrepancies found | 0 / 240 | Not applicable |
| False alarms on cases without seeded errors | 0 | 0 |

The median compatible matched-item percentage error was zero. Counts covered 600/780
truth items; geometric lengths/areas/volumes covered 2,030/4,030; declared rule and judgement
classes covered 0/780 each. Every repetition produced identical quantities/citations.
That includes 120 unsupported PDF baseline entries returning empty submissions; repeatability
is not successful PDF recognition. Error precision is undefined because the baseline emits
no findings; recall and F1 are zero.

The immediate priorities are coverage and duplicate/semantic region handling, then typed
length/volume derivation, rule execution and discrepancy checks. Correct matched measurements
do not compensate for the 3,740 missed truth items or the extra room/region rows. These are
synthetic development results; no claim about real consultant drawings or human QS accuracy
is made until the external sources and independent spot checks are available.

### Repository file inventory

| Files | Change |
|---|---|
| `crates/qiyas-cli/src/bench/{mod,model,score,submission,baseline,tests}.rs` | CLI, schema validation, safe dev enumeration, matching/scoring, importers, automatic baseline and tests |
| `crates/qiyas-cli/src/bench/generate.rs`, `bench/generate/pdf.rs` | Seeded command-driven projects, variants, PDFs and holdout seals |
| `crates/qiyas-cli/src/eval.rs` | Approved headless agent runs, CAD/PDF preparation, budgets, costs, artifacts and scripted tests |
| `crates/qiyas-cli/src/main.rs`, `crates/qiyas-cli/Cargo.toml`, `Cargo.lock` | CLI wiring and existing approved dependencies |
| `crates/qiyas-ai/src/{anthropic,openai,provider}.rs` | Strict request mode with no unpriced fallback, optional-field or rate-limit retries |
| `crates/qiyas-core/src/session.rs`, `crates/qiyas-core/tests/standalone.rs` | Standalone loading without sidecars or external asset reads |
| `scripts/benchmark/{fetch.sh,fetch.py,sources.json,convert.py,requirements.txt,test_tools.py,README.md}` | Verified-source acquisition, conversion and offline tests |
| `src/shell/PlotDialog.test.tsx` | Condition-based preview waits and deterministic rejection; Settings test unchanged |
| `docs/2026-10-21-qiyas-qs-roadmap.md` | Synced from the founder's plan, benchmark phase 15 and subsequent phases shifted |
| `CLAUDE.md`, this document | Commands, schema, workflow, limits and results |

The local source folder currently contains generator provenance/licence plus pending source
licence notices, catalogue, provenance log and restricted-candidate metadata. Clinic, Duplex,
ResPlan and Swiss Dwellings downloads remain pending: the network probe failed DNS (curl exit
6). The reviewer can run `scripts/benchmark/fetch.sh`; conversion still requires its local
venv and upstream-format/COBie/hand checks. Restricted sources were not downloaded. Decisions
remaining are verified model/service prices and approval for each live run, permission for
any restricted candidate, and a human QS spot-check/ceiling study.
