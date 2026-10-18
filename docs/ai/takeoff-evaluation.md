# Take-off evaluation

Run `cargo test -p qiyas-ai --test takeoff_eval -- --nocapture`.
The test uses a scripted `Provider`. It runs the production `Agent` and tools without an API key.
It prints each passing case and the total. A failed assertion makes the command fail.
Each case opens a temporary drawing copy. The source drawing and project stay unchanged.

The provider chooses a fixed tool plan. These tests check execution and quantities.
They do not measure model understanding, tool selection, answer wording, or live service acceptance.
The BOQ case uses explicit links. It does not measure automatic item matching.

## Reference answers

These values come from `samples/test-plan.md` and the drawing construction dimensions.
The tolerance is 0.001 in the displayed unit. Units and AI source are also checked.

| Question | Calculation | Answer |
| --- | --- | --- |
| Living room area | 6.7 × 8.7 | 58.29 m² |
| Bedroom area | 4.7 × 4.7 | 22.09 m² |
| Kitchen area | 4.7 × 3.7 | 17.39 m² |
| Outer wall perimeter | 2 × (12 + 9) | 42 m |
| Vertical partition | 9 − 0 | 9 m |
| Horizontal partition | 12 − 7 | 5 m |
| Doors | Three inserted DOOR blocks | 3 pcs |
| Windows | Five inserted WINDOW blocks | 5 pcs |
| Columns | Six inserted COLUMN blocks | 6 pcs |
| BOQ comparison | Seven measurements linked to six BOQ rows | Exactly two quantity mismatches: A.4, 30 vs 22.09 m²; B.2, 4 vs 5 pcs |

## Real drawings

Two real-drawing LINE references have accepted independent endpoint calculations.
The child task is [QSM-414](/QSM/issues/QSM-414). It covers one case from each of the two local office drawings.
Customer files and names must stay outside this public repository.
Synthetic cases must not be reported as real drawing coverage or expert approval.
Live model evaluation remains with Quant's existing access path. No paid service is required for this test.

## HTTP 400 diagnosis

Evidence: local desktop `logs/agent.log`, 12 October 2026, 20:06:10–20:06:13 (log time).
The model returned `find_entities({"layer":"A-WALL"})`, call ID `call_446341`.
The tool returned four rows: a perimeter of 42 m, partitions of 9 m and 5 m, and an arc of 2π m.
Request 2 then failed with HTTP 400, `invalid_request_error`, code `invalid_value`, and no parameter name.
The log does not contain the rejected HTTP request body. It cannot prove the original wire-format cause.

The current serializer sends assistant content as an empty string beside tool calls.
It sends each result as a tool message with the matching call ID and string content.
It also preserves provider tool-call extra data when present.
These are existing safeguards, not a new service fix from this task.

Run `cargo test -p qiyas-ai --test agent rejected_request_after_find_entities_is_logged_without_retry`.
This local HTTP regression reproduces the same failure boundary with the recorded call and error shape.
It checks the current request fields, a single failed attempt, and request logging without the API key.
The existing `gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn` test covers parallel results.
A live retry is still required to confirm service acceptance. The offline test cannot confirm it.

## Verification on 15 October 2026

- `takeoff_eval`: 10/10 cases passed in one integration test; zero failures (0.07 seconds after compilation).
- `rejected_request_after_find_entities_is_logged_without_retry`: passed; zero failures (0.03 seconds after compilation).
- Rust formatting and `git diff --check` passed for the changed test files.
- Tests used the installed Rust 1.97.0 toolchain, the local Cargo cache, offline mode, and two build jobs.
- The full `scripts/check.sh` was not run. The assigned tester owns that check.
- Real drawing endpoint references were accepted by the measurement expert on 15 October 2026. The parent task owns the final technical review. No push or release was made.

## Private real-drawing cases

`scripts/evals/real_references.py` reads two inspection DXFs and calculates one
model-space LINE length per drawing from its two 3D endpoints. The calculation
uses Euclidean distance, INSUNITS=6, metres, and absolute tolerance 0.001 m.
The private manifest stores source SHA-256, handle, layer, endpoints, question,
formula, answer and professional review state. It contains customer paths and
must stay outside this repository. The local manifest is in the operator's
private `~/.local/share/qiyas-evals/qsm-414/references.json`.

Generate inspection DXFs in run scratch with the existing accuracy export
procedure. Pass the private output directory, the two source DWGs (small then
large), and the two matching DXFs to the Python script. This uses only the Python
standard library. The inspection export shares Qiyas' decoder. The independent
answer is the coordinate calculation, not the measurement dump.

Run the assistant tool test with:

```sh
CARGO_BUILD_JOBS=2 QIYAS_REAL_EVAL_REFERENCES="$HOME/.local/share/qiyas-evals/qsm-414/references.json" cargo test --offline -p qiyas-ai --test takeoff_eval -- --include-ignored --nocapture
```

The private test copies only the DWGs to a temporary directory and clears prior
measurements. It runs the Agent with a fixed measure_length call, then checks
quantity, unit and AI source. Default CI reports the private test as ignored.
A passing private test proves two tool executions against independent arithmetic.
Professional interpretation and live model performance are separate gates.

Local verification, 15 October 2026: both integration tests passed, including
10 synthetic cases and two private LINE cases (0.240 m and 0.320 m).
The run used installed Rust 1.97.0, CARGO_HOME=~/.cargo,
CARGO_BUILD_JOBS=2, and offline dependencies. The measurement expert accepted both references on 15 October 2026 (review interaction
`a10761ca-799c-4daf-a0be-e0a40a7d8ab3`). Acceptance covers the LINE elements,
model-space scope, metre units, endpoint arithmetic and 0.001 m tolerance. It does
not establish independent DWG decoding, construction quantities or live model accuracy.
