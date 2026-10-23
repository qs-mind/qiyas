# Goal A: correct quantities and key safety

23 October 2026. This report records implementation and local verification of the founder-approved Goal A. No dependencies were added, and nothing was staged, committed or pushed.

## Changes

1. Rooms use enclosure geometry independently of Hatch. Annotation kinds, Qiyas grid parts and opening symbols are excluded. Joined wall faces close across hosted openings, giving internal wall-face areas. Explicit boundary inclusion/exclusion layers are available to room queries, seed measurements, region sources and recognition. Retained regions keep that policy through drawing files and undo. Grid axes and letters now carry provenance as well as bubbles. Older grids recover their axes and centred letters from stamped bubbles, without layer-name guesses. Saved measurement overlays are excluded too: derived geometry must never become its own enclosure.
2. Broken room measurements have structured validity and null usable values. Totals and comparisons exclude them; Excel says “Unavailable”. Reports count and list the affected rows. Generated object quantities and schedule fields remain unavailable when their source is invalid. Opening a saved drawing refreshes retained region quantities. Invalid embedded measurements use invisible records with validity and the room binding, including without a project sidecar. Cached and new variation quantities become unavailable while any retained enclosure is broken, with a count and handles; a missing quantity cannot become a removal delta.
3. Plan labels use “Area … m²” and “المساحة … م²”, with Arabic digits and decimal separator in Arabic. Schedule headers use “Area (m²)” and “المساحة (م²)”. Approval stays in metadata and take-off notes. Old area wording still counts as a label when reading room names; existing live fields display the approved wording when refreshed. Existing label expectations were updated to the founder’s wording; quantity assertions were not weakened.
4. Extract Data follows every page and displays the number loaded. An error on a later page rejects the extraction, preventing partial exports.
5. Visible deliverable cells use rounded quantities, integer counts, readable references and filenames. Full precision, full paths and structured references remain in the lossless record. English and Arabic Excel/PDF use the same presentation rules.
6. Keychain accounts include the provider and normalized complete base URL. Legacy credentials migrate only to the saved destination. Model discovery at a different draft address asks the user to save first. Remote HTTP and malformed addresses are refused, and authenticated requests do not follow redirects. Tests use only mock credentials and local servers.

## Analytic evidence

The audit’s rectangle control was 20 → 10 → 20 m² as a grid was added and hidden. The Goal A replay is 20 → 20 → 20 m² in mm, cm, m and inches.

The founder’s actual wall definitions were read from an audit-owned copy of the saved 10 × 8 flat. A new drawing was then drafted with those runs and openings, grids, room names, labels, dimensions and schedules in English and Arabic. Hand calculations use only wall positions and thicknesses:

| Room | Clear dimensions (m) | Expected area (m²) | Measured area (m²) |
|---|---:|---:|---:|
| Hall | 4.775 × 3.30 | 15.7575 | 15.7575 |
| Living | 4.775 × 4.30 | 20.5325 | 20.5325 |
| Bedroom | 4.775 × 4.30 | 20.5325 | 20.5325 |
| Bath | 2.325 × 3.30 | 7.6725 | 7.6725 |
| Store | 2.300 × 3.30 | 7.5900 | 7.5900 |

The earlier living-room result was 19.31 m². Changing tag height and grid visibility leaves every new area unchanged. The unrounded total is 72.085 m².

Evidence is stored under `.local/audit/goal-a/`. The original audit artifacts are preserved. The audit script copies change output destinations; the local MCP helper selects the freshly built task-owned CLI. The new drafting job is `draft_flat.py`.

## Verification results

The requested commands were each invoked once, with one Cargo command at a time and `CARGO_BUILD_JOBS=4`:

| Check | Actual result |
|---|---|
| `cargo fmt --all` | Exit 1: introduced let-chain syntax required Rust 2024. Corrected to Rust 2021 expressions. Direct `rustfmt --check --edition 2021` subsequently passed on all 41 changed Rust files. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 101 on the same syntax error, subsequently corrected. The full command was not repeated under the once-only instruction. |
| `cargo test --offline -p qiyas-core -p qiyas-geom -p qiyas-boq -p qiyas-plot -p qiyas-takeoff -p qiyas-cli` | Exit 101: 700 passed, one failed across 71 completed targets. The failing regression caught a broken room becoming available after save/reopen. Fixed the cause: embedded measurement geometry was being accepted as enclosure geometry. Remaining targets were not reached. Focused correction results follow below. |
| `cargo test --offline -p qiyas-ai --lib --test surface --test agent -p qiyas-desktop --tests` | Exit 101 before test execution: linking the AI `tools` integration executable was killed by signal 9. Focused results follow below. |
| `node_modules/.bin/tsc --noEmit` | Passed, exit 0. |
| `node_modules/.bin/vitest run --maxWorkers=3` | Passed, exit 0: 115 files, 847 tests. |

The original logs are `fmt.log`, `clippy.log`, `test-core.log`, `test-ai-desktop.log`, `tsc.log` and `vitest.log` under the evidence directory. `rustfmt-final.log` records the corrected formatting check. No broad failed command is reported as green. A later deliverable test queued behind another build was cancelled with exit 130 (`test-deliverables-final.log`); remaining verification used a task-owned reflink copy of the build cache to avoid that lock, still with one Cargo command at a time. The isolated cache was removed after verification; the final CLI binary and its hash are retained.

### Focused correction checks

- Final debug CLI build passed, exit 0; `build-isolated-final.log` and `qiyas-cli.sha256` identify the final task-owned binary used for the replays (`.local/audit/goal-a/qiyas-cli`).
- Focused room corrections passed, exit 0: 5 Goal A controls and 6 retained-room tests. Includes saved invalid rooms with and without a sidecar in DWG/DXF, undo, existing grid provenance, explicit layers and unchanged Hatch behavior. See `test-room-corrections.log`.
- Clippy passed for every changed Rust crate (`qiyas-core`, `qiyas-ai`, `qiyas-desktop`, `qiyas-boq`, `qiyas-takeoff`), all targets, warnings denied: `clippy-corrections-final.log`, exit 0. Corrective passes also caught and fixed command enum growth and partial-read handling in the mock server. These focused passes are distinct from the single requested workspace run.
- The focused full AI library build also exited 101 after rustc was killed by signal 9 (`test-ai-lib-final.log`).
- Dedicated `qiyas-ai --test key_safety`: 1 passed, 1 failed. Complete-address/HTTPS validation passed. `network_tests::model_discovery_never_follows_a_redirect_with_credentials` failed at loopback bind with OS error 1, PermissionDenied, before any request (`test-key-safety.log`). This network behavior remains unverified in this sandbox.
- Desktop settings target: 15 passed, 1 failed. `credentials_follow_the_saved_address_and_never_a_draft` failed at loopback bind with OS error 1, PermissionDenied (`test-settings-final.log`). Legacy key migration, mock keychain storage and settings tests passed. A separate pure mock test covers address/provider/path isolation without ports.
- The pure mock-keychain isolation test passed, 1/1. While Cargo was waiting on another process’s build lock, the exact test body was compiled with `rustc --test` against a snapshot of the already-built desktop library and mock keyring dependency. It checks provider, host and base-path isolation plus rejection of draft/malformed/remote-HTTP addresses. See `key-scope-standalone.rs` and `test-key-scope-standalone.log`. This does not claim the blocked socket assertions passed.
- Core persistence checks passed, exit 0: 54 library unit tests and 22 save/reopen tests (`test-core-persistence-final.log`). This includes the legacy-label field upgrade and existing embedded measurement compatibility.
- Deliverables passed, exit 0: 4 tests covering drawing/project exports, English/Arabic, Excel/PDF, method precision and lossless round trips (`test-deliverables-isolated.log`).
- Final Clippy passed on all changed Rust crates and all targets with warnings denied after all source changes (`clippy-isolated-final.log`).
- Final direct formatting and `git diff --check` passed (`rustfmt-final.log`, `diff-check.log`).

### Acceptance artifacts

All final acceptance replays used the freshly built debug CLI. `acceptance-verification.log` records successful independent workbook/transcript assertions.

| Evidence | Before → after | Relative path under `.local/audit/goal-a/` |
|---|---|---|
| Rectangle, mm/cm/m/inches | Grid reduced 20 to 10 m² → 20 before grid, 20 with grid, 20 with grid hidden in every unit | `grid-{mm,cm,m,inches}/transcript.jsonl`, `grid-summary.json` |
| L-shaped enclosure with wall/openings/grid/tags/dimension | 20 m² control → exactly 20 before and after in every unit | `l-{mm,cm,m,inches}/transcript.jsonl`, `l-shape-summary.json` |
| Broken enclosure | Stale 20 m² → null usable values, `quantity_valid: false`, “Unavailable” workbook cell, no numeric total | `core-region-probe/transcript.jsonl`, `verified-summary.json`, `workbook-readback.json`, `stale-takeoff.xlsx` |
| Generated BOQ after broken enclosure | 20 m² → no generated items; invalid object listed, no stale quantity in bilingual Excel/PDF | `invalid-generated/transcript.jsonl`, `verified-summary.json`, `unavailable-{en,ar}.{xlsx,pdf}` |
| Variation | Float tails, JSON references, absolute path → 1.32, “Model / added / 98”, `drawing.dxf`; desks 7 → 8, delta +1 | `revision-qs/transcript.jsonl`, `verified-summary.json`, `variation.xlsx`, `variation.pdf` |
| Arabic variation | Same numeric/provenance presentation rules; Arabic report headings and direction | `revision-ar/transcript.jsonl`, `variation-ar.xlsx`, `variation-ar.pdf` |
| Full flat, both languages | Earlier living label 19.31 → exact clear area 20.5325 m²; five-room total 72.085 m² | `flat-en/transcript.jsonl`, `flat-ar/transcript.jsonl`, each folder’s `hand-check.json` and `flat-takeoff.xlsx` |

Both flat runs completed 96 MCP calls without tool errors. The variation replay completed 43 calls without tool errors. Changing tag height and hiding/showing the grid changed no room area. Workbook readbacks verify that visible values are readable while lossless records still contain full numeric precision and structured element provenance.

Inspected final PNGs:

- English labels: `.local/audit/goal-a/flat-en/0094-labels-close-en-0.png`; full plan and schedules: `0093-labels-en-0.png` in the same folder.
- Arabic labels: `.local/audit/goal-a/flat-ar/0094-labels-close-ar-0.png`; full plan and schedules: `0093-labels-ar-0.png`.
- Variation PDF page renders: `.local/audit/goal-a/revision-qs/variation-pdf.png` and `.local/audit/goal-a/revision-ar/variation-pdf.png`.

The labels visibly say “Area” / “المساحة”, use the requested Arabic digits and decimal separator, and contain no approval status. The room schedule uses the approved area header. The oversized D1 text is the deliberate tag-height perturbation, not a recommended drafting size.

## Files touched

The exact source/test/document manifest is `.local/audit/goal-a/files-touched.txt`. Changes are concentrated in:

- `qiyas-core`: room boundary tracing and persisted policy; grid provenance; retained region validity, embedding and reopen; labels/fields/schedules; measurement, totals, variation and deliverable presentation.
- `qiyas-ai`: endpoint validation, redirect handling, model discovery, prompts and take-off workflow.
- `qiyas-desktop`: address-scoped keychain operations, migration and settings/discovery commands.
- `qiyas-boq` and `qiyas-takeoff`: unavailable quantity export, readable workbook cells, and accuracy exclusion.
- Frontend extraction pagination/progress, nullable quantity display, model picker messaging, translations and associated tests.
- `CLAUDE.md`, this report, and focused Rust regressions.

No test was weakened to accept an incorrect area. Existing numeric area assertions are unchanged. The new enclosure controls add the missing grid/opening/annotation cases.

Changes to existing test expectations:

- `qiyas-core/tests/conveniences.rs`: “Unapproved candidate 12.00” becomes “Area 12.00”; transaction assertions remain.
- `qiyas-core/tests/object_regions.rs`: the 22.00 and 11.00 labels adopt “Area”; recipe, quantity and undo assertions remain.
- `qiyas-core/tests/room_objects.rs`: all existing labels adopt “Area” with the same numeric areas. Broken-enclosure checks are strengthened with null values, empty totals, cached variation unavailability and DWG/DXF reopen without sidecars.
- `qiyas-ai/tests/authoring.rs`: English labels adopt “Area”; Arabic labels use “المساحة ٥١٫٢١ م²”. Area values, language selection and unapproved metadata remain checked.
- `qiyas-ai/tests/drafting_eval.rs`: label identification uses the new “Area ” prefix. Geometry and room comparisons remain unchanged.
- `src/shell/ExtractDialog.test.tsx`: the existing filter/field assertions also require a progress callback. Separate pagination tests cover 5,001 rows and failure on a later page.

## Scope and limits

Automatic label fitting and general annotation placement remain Goal G. This job deliberately placed readable labels inside the rooms and inspected the captured images. Provider network tests are restricted by the sandbox; the loopback bind failure and compiler/linker process kills are recorded without substituting claims of success. No real API key was read, printed or used.

No founder decision is pending. The complete requested Rust suites still need a clean reviewer run: one stopped at the subsequently corrected persistence regression, and the AI/desktop build was killed before execution. The two socket tests need an environment that permits loopback listeners. `scripts/check.sh` was not run. No files were staged, committed or pushed.
