# Mizan drafting evaluation (20 October 2026)

Phase 10 of [the dynamic core plan](superpowers/plans/2026-10-20-qiyas-dynamic-core.md),
offline portion. The executable specification is
[`crates/qiyas-ai/tests/drafting_eval.rs`](../crates/qiyas-ai/tests/drafting_eval.rs).
No live model, provider endpoint, API key or network access is used.

## What the offline evaluation proves

A scripted provider drives the production `Agent` loop with an ordered queue of calls.
The drafting path exposes only `qiyas_read`, `qiyas_execute` and `qiyas_capture`, the same
generic surface offered to Mizan. The script supplies the decisions; the actual registry,
batch execution, geometry, room detection, checking, verification and picture generation
do the work.

The villa starts as a blank drawing in metres. It creates `A-WALL`, `A-DOOR`, `A-GLAZ`,
`A-ANNO-TEXT`, `A-ANNO-DIMS` and `A-ROOM-NAME` in one batch, sets annotation scale 1:100,
and uses the `walls` command for a 12 × 10 m centreline envelope with partitions at x = 6
and y = 5 m. The walls are 0.2 m thick and 3 m high. The four named spaces are Living,
Kitchen, Bedroom and Bath. Five doors are 0.9 m wide with a 2.1 m head; three windows are
1.2 m wide with a 1.2 m vertical opening.

Room names, dimensions and candidate area labels use their intended layers. The drawing
also includes structural grid axes, centre lines, an A1 layout with a 1:100 viewport,
and a title block whose values are supplied explicitly. The independent wall references
are:

| Quantity | Hand calculation | Expected |
| --- | --- | --- |
| Gross wall plan area | 12.2 × 10.2 − 4 × 5.8 × 4.8 | 13.08 m² |
| Total opening width | 5 × 0.9 + 3 × 1.2 | 8.1 m |
| Net wall plan area | 13.08 − 0.2 × 8.1 | 11.46 m² |
| Wall volume | 13.08 × 3 − 0.2 × (4.5 × 2.1 + 3.6 × 1.2) | 36.486 m³ |

The assertions require:

- The general and construction check packs report no errors.
- `rooms` finds all four distinct named rooms as closed candidate regions, both before
  and after the structural grid is added.
- Wall quantities agree with independent arithmetic from the fixture's dimensions.
- `verify` passes the stated dimensional and geometric intent, including overall size,
  closed envelope and wall contours, and parallel walls. The separate `rooms` assertions
  establish the named-room closure result.
- Authored sheet entities use the intended layers, with none on layer 0; the layout
  viewport has the physical paper-to-model scale 1:100.
- The title block contains only the supplied values, and picture capture returns a
  rendered image through the generic surface.

A second, small drawing is a flange with an outer radius of 2 m and a concentric bore of
radius 0.5 m, a centre mark and a diameter dimension. It uses the same generic route and
checks radii, areas, concentricity and closure with `verify`. It establishes that the
commands and geometric expectations do not depend on a building-specific tool. General
checks remain useful without construction rules. The capture calls cover the villa's
model space, its A1 layout and the flange.

The replay fixture deliberately builds the kinds of faults seen in the earlier Mizan
villa: crossing lines on layer 0, a wall endpoint stopping short, text with a literal
backslash followed by `n`, and a room-size label that disagrees with its enclosure. Its
assertions require the corresponding `dangling_end` / `overshoot`, `literal_newline`,
`room_size_text_differs` and `layer_zero` findings. This is a synthetic regression of those
fault classes, not a verbatim replay of a private drawing or a complete logged conversation.

## What it does not prove

There is no model judgement in these tests. The provider does not interpret the villa
request, discover an effective drafting strategy, choose or repair tool calls, read the
captured pictures, or decide whether a design is acceptable. A passing script proves that
the generic surface can execute the specified draft and expose verifiable evidence.
It does not establish that a live Mizan will choose that sequence.

Expected quantities come from fixture arithmetic, not from a model's answer. `verify`
and `check` nevertheless share Qiyas's engine; passing both is not independent validation
of every geometric implementation or file-format decoder. The fixtures do not establish
building-code compliance, design suitability, production documentation quality, or
professional approval. `label_spaces` values remain unapproved candidate areas.

Room finding uses the drawing's linework, including wall centrelines and opening symbols.
Finding the four closed candidate regions does not prove that they are architectural
clear-floor boundaries. In particular, the fixture does not assert 5.8 × 4.8 = 27.84 m²
as each room's reported net area. Wall plan-area and volume references are asserted
separately.

The layer-0 assertion covers authored entities in model space and on the A1 sheet.
It excludes the codec's invisible overall paper-space viewport (viewport number 1) and
the internal geometry of block definitions, where layer 0 with ByBlock inheritance is
intentional. It does not claim that every database object in a valid CAD file avoids
layer 0.

A capture assertion proves that the capture path produces image data. It is not a visual
review of linework, legibility, clipping, layout balance or door swings. The offline run
also does not test provider wire compatibility, model reasoning, token use, service
latency, price or the desktop interaction.

## Verification

Run these checks serially, one heavy process at a time. Do not use a release build or
`scripts/check.sh` for this task. A heat watcher may pause processes; a pause is not itself
a failure.

```sh
CARGO_BUILD_JOBS=1 nice -n 19 cargo fmt --all
CARGO_BUILD_JOBS=1 nice -n 19 cargo clippy --offline --workspace --all-targets -- -D warnings
CARGO_BUILD_JOBS=1 nice -n 19 cargo test --offline -p qiyas-ai --test drafting_eval
CARGO_BUILD_JOBS=1 nice -n 19 cargo test --offline -p qiyas-ai --lib --test surface
```

Results on this change (all Cargo commands used `CARGO_BUILD_JOBS=1 nice -n 19`):

| Check | Actual result |
| --- | --- |
| `cargo fmt --all` | Passed, exit 0. Run once. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed, exit 0. Run once. |
| `cargo test --offline -p qiyas-ai --test drafting_eval` | First run: replay passed; villa and flange failed on test-fixture mistakes, described below. |
| `cargo test --offline -p qiyas-ai --test drafting_eval -- --skip replay::old_mizan_faults_are_reported_through_the_generic_read_surface` | Passed: villa and flange, 2 tests; the already-passing replay was not rerun. All three evaluation cases have passed. |
| `cargo test --offline -p qiyas-ai --lib --test surface` | 50 library tests passed; one existing library test failed because the sandbox refused a local socket bind. Cargo stopped before running `surface`. |
| `cargo test --offline -p qiyas-ai --test surface` | Passed: all 20 tests. This ran the target that the combined command had not reached. |

The blocked library test is
`models::tests::an_unreachable_service_falls_back_to_the_builtin_list`:
`crates/qiyas-ai/src/models.rs:247` calls `TcpListener::bind("127.0.0.1:0")` and receives
`PermissionDenied: Operation not permitted`. It needs the reviewer's environment.
The other integration tests that open local ports were not run.

The fixture corrections kept batch `commands` as an array inside the string-valued
`args`, supplied the required text `rotation: 0`, and checked a dimension's returned
sheet handles rather than its change list (which also includes anonymous block geometry).
Formatting and Clippy passed before these final fixture corrections and were not
repeated, per the requested heat limits. The corrected tests compiled and passed.
No core bug blocked the evaluation and no production core changes were needed.

No network or live provider was used, no dependencies were added, and no release build
or `scripts/check.sh` was run. The changes are left uncommitted.

## Later live comparison

No live comparison was run for this change. Claude Opus 5.5 and Gemini 3.8 Flash will each
receive the same villa request in a fresh conversation on an identical blank metres
drawing. Kareem must approve **each individual run** in advance, including a retry or
another sample. Approval of this offline work is not approval of a live call.

Before requesting that approval, prepare a run record containing:

- The exact shared villa prompt and any reference image; explicit dimensions, rooms,
  openings, layers, annotation scale, sheet and title-block values; the same allowed
  clarifications for both models.
- The Qiyas commit, prompt and tool-schema versions, exact provider and model IDs,
  requested effort, output-token limit and tool-call budget. Record whether the provider
  accepts the effort setting and any fallback; provider effort names need not represent
  equal computation.
- The maximum permitted spend, stop conditions and artifact locations. Existing settings
  and the founder's keychain access supply credentials; keys never belong in the report.

Use a fixed prompt before starting either model and retain it with the results. Do not
give one model the other's transcript or the scripted solution. Keep the same initial
state, tool surface and stopping rules. Record clarifications, failures, retries and
human interventions instead of silently restarting or selecting only a successful result.
One approved run per model is a paired observation, not a reliable estimate of average
model performance; more samples require separate approval.

After each run, preserve the final drawing, transcript and tool results, final `check`
reports for explicit general and construction packs, `rooms` results, `verify` results,
and captures of the plan, representative room/opening details, and the A1 sheet. Use
matching capture bounds and resolution for the comparison. Keep intermediate captures
when they explain a correction. Local logs may truncate content, so confirm that the
retained artifacts contain the evidence needed for scoring.

Score each run with the same rubric:

| Measure | Evidence and interpretation |
| --- | --- |
| Drawing correctness | Final errors by rule and pack, plus warnings and informational findings separately. Report missing rooms, quantity differences, failed verification, wrong layers, layer-0 entities, viewport scale and invented title-block values explicitly. Zero check errors alone is insufficient. |
| Steps | Count provider turns and tool calls separately; also count core commands inside batches, mutations, corrections and failed calls. A batch is one surface call, not one drafting operation. |
| Tokens | Record reported input, output, cached and reasoning tokens with the provider's definitions. Missing usage is unknown, not zero. |
| Cost | Use the approved provider's effective prices at run time and reported usage or billing evidence. State currency, cache treatment and uncertainty; include failed or retried requests. |
| Pictures | Review the same plan/detail/sheet views for readable text and dimensions, sensible layer and lineweight use, openings, grid and centre lines, overlaps, clipping and sheet composition. Record the reviewer's observations rather than treating a generated image as an automatic pass. |
| Completion | Record whether the model finishes within its budget, asks for needed information, leaves errors unresolved or needs a human correction. Record elapsed time and service failures as context. |

Correctness and visual acceptability come before savings in steps or tokens. Publish the
paired results and the limits of the sample before recommending a default model.
There is no model recommendation from the offline evidence.
