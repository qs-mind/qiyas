# Mizan first live-run corrections

This work precedes other roadmap phases. No live provider settings, API keys or paid evaluations
were opened or run. All changes are general production behavior; neither the agent nor the
scripted providers receive benchmark truth. No dependencies were added.

## Findings from the six live cases

Read the combined report at
`.local/benchmark/runs/eval-set-20261022T081243857428764-3740328/report.json`
and all six referenced `agent.log` and `steps.jsonl` files. Five runs exhausted their cost
allocation; the scan ended with zero submitted items. Coverage was 11/50, with 11/12 item
precision and every match within 1%. The capped runs were already saved and scored: the
problem was producing mapped quantities before the cap, not discarding existing submissions.

The office conversation grew from 4,194 input tokens to 50,043 at request 17, including cached
input. Other cases repeatedly requested validation, made circular quantity aliases such as
`count: "count"`, and supplied nearby-text extraction without `max_distance_m`. The office's
reported `Annotation` error really had no earlier definition in that batch. Proper ordered
batches now have explicit semantic validation and regression coverage; genuinely missing
references still fail instead of silently inventing types.

The scanned PDF exposed two concrete failures before interpretation: picture framing rejected
a sheet containing only an underlay, and relative document paths resolved against the process
working directory. The model repeatedly guessed filenames and never obtained the page. Its
claim that no scale was visible was therefore unsupported by a successful capture.

## Changes

- Keep the newest three tool results whole; replace older verbose evidence with summaries
  retaining identities and error codes. Keep the initial digest, system, call ids, arguments
  and provider signatures stable. Existing message compaction remains the long-chat guard.
- Reduce digest previews/page size, cap record pages at 12 KB, omit empty summary-record fields and shorten picture captions. Filtering records no longer forces full output when summaries were requested.
- Stage all knowledge definitions sequentially on a private drawing during batch validation,
  including schedule-created types, rules, methods, mappings, checks and generic knowledge.
  Execution retains one atomic undo step. Tests cover earlier definitions, missing definitions,
  unchanged live state after validation and undo/redo of the entire batch. If a schedule depends on earlier drawing edits or unresolved step values, dry validation explicitly defers to sequential execution rather than rejecting against stale evidence.
- Execute batches directly. Supply complete cited method/mapping examples and explain quantity
  aliases and recognition prerequisites. Publish mappings alongside recognised scope.
- Switch budget guidance to finishing at 80% or when fewer than two conservative reservations
  fit. Hard caps produce a local end report without another provider request. Eval retains an
  end report with the actual answer, current quantities, submitted count and self-check, even
  after an error. Zero-item completion is a failed investigation and is recorded in both logs.
- Include raster/underlay bounds in picture framing; expose document paths/pages in the digest;
  resolve document paths beside the drawing. Digest paths are absolute even when the drawing path is relative, preventing its folder being joined twice. Scans must use capture, visible calibration and
  raster vision hints with review metadata, or report the actual missing evidence/error.

## Offline replay

The scripted provider receives only production conversation/tool evidence. Its sequence follows
live behavior: digest, plan, schemas, picture, records, a batch defining and using a type,
recognition, method/mappings, self-check and repeated evidence reads. It measures requests with
and without retirement of old results using identical calls and outputs. Additional runs stop
at request and cost caps, save the existing generated BOQ and score it after the provider ends.

Token counts are deterministic offline estimates: UTF-8 bytes divided by four, including tool
schemas and protocol structure, plus 1,024 tokens per image. They are not Gemini tokenizer
measurements. The replay uses $0.75/M input/cache input and $3.75/M output, matching the logged
live charges; it does not read pricing configuration. Reservations retain the existing more
conservative byte-based accounting. This establishes orchestration and cost behavior, not live
recognition completeness or accuracy on unseen drawings.

Raw evidence: `.local/mizan-live-fixes/`. The reviewer will run the same paid `live-dev` command. The replay budgets 200 output tokens per response; input estimates come from the complete provider request. This synthetic output allowance is not a live generation measurement.

## Token and cost results

| Replay | Requests | Last input before → after | Peak after | Estimated cost after | Result |
|---|---:|---:|---:|---:|---|
| finish | 21 | 58,841 → 17,086 | 18,188 | $0.231353 | done at 80% request budget |
| request-cap | 25 | 70,823 → 17,678 | 18,188 | $0.286721 | cap retained submission and end report |
| cost-cap | 16 | 43,863 → 16,346 | 18,188 | $0.164743 | cap retained submission and end report |

The ordinary finish replay costs $0.231353 versus $0.496060 with the same outputs retained in full. All three runs submitted supported geometric quantities and were scored: 37.5% coverage, 100% item precision, and every matched item within 1%. This intentionally limited scripted interpretation proves submission/scoring behavior, not full live take-off coverage. The request-cap provider deliberately ignores the finish warning, so its cost exceeds the normal finishing run. The cost-cap provider is stopped before a conservative reservation would exceed its $0.22 cap.

Per-request input estimates for the normal finishing run:

```text
before: 4564, 5315, 5453, 15992, 17338, 18181, 20558, 21936, 22878, 25873, 28869, 31864, 34860, 37855, 40851, 43846, 46842, 49837, 52833, 55828, 58841
after:  4564, 5315, 5453, 15992, 17338, 18188, 10205, 10427, 10658, 11754, 13530, 15737, 15885, 16033, 16181, 16329, 16477, 16625, 16773, 16921, 17086
```

The untrimmed tail grows by about 2,995 tokens per evidence read; the trimmed tail grows by about 148 tokens per request for retained identities and protocol history. Full evidence is limited to the newest three results, with existing message-count compaction bounding long conversations. The initial schemas cause the maximum of 18,188 tokens before they retire.

## Verification

Checks were serial, with `CARGO_BUILD_JOBS=2` and `nice -n 19` for Cargo; no helper agents or parallel builds. The requested command was run once initially; failed targets and targets skipped after a failure were then run separately. No tests were removed, ignored or weakened.

| Requested check | Actual result |
|---|---|
| `cargo fmt --all` | Passed. Later Rust edits were formatted directly with rustfmt; the final rustfmt check and git diff whitespace audit also passed. |
| Workspace Clippy, offline, all targets, warnings denied | Passed after fixing two new compile errors. A later lint refresh hit missing cached dependency metadata (`E0463`) in drafting_eval; that target passed after dependencies rebuilt. The changed core/AI/CLI all-target refresh passed with warnings denied. The later PDF path regression passed on its targeted run, including a relative drawing path and relative underlay path. |
| Core / takeoff / BOQ tests | Passed after correcting the new schedule fixture (table dimensions and generated type name), then completing targets skipped by Cargo's early exit. The final records pagination check passed all 9 tests. |
| AI library + surface + drafting_eval + authoring | 71 library tests passed; 1 cannot bind a localhost port in this sandbox (`models::tests::an_unreachable_service_falls_back_to_the_builtin_list`, PermissionDenied). Surface 45/45, drafting 4/4, authoring 19/19 passed. The Arabic prompt assertion was repaired; replay assertions retain complete first-observed evidence while decisions use actual trimmed requests. |
| `node_modules/.bin/tsc --noEmit` | Passed, exit 0. |
| Desktop tests + CLI | CLI unit tests 36/36 and integration tests 18/18 passed. Desktop library 94/95 passed; `commands::tests::the_mcp_server_follows_the_settings` cannot bind a localhost port (PermissionDenied). Desktop options 10/10, settings 15/15, and the zero-test main target passed. The new replay's first run failed its token bound; reducing record pages to 12 KB made it pass without weakening the assertion. |
| `nice -n 19 node_modules/.bin/vitest run --maxWorkers=1` | Passed: 112 files, 839 tests; exit 0. |
| `nice -n 19 ./target/debug/qiyas-cli bench run --set quick --mode autonomy` | Ran once at the end, exit 0: 120 cases, 716/3,672 matched (19.4989% coverage), 98.7586% item precision, all 716 matches within 1%. This is the engine autonomy diagnostic, not a paid Mizan run; its rates match the previous diagnostic apart from floating-point rounding. |
| CLI refresh after the final PDF path fix | `CARGO_BUILD_JOBS=2 nice -n 19 cargo build --offline -p qiyas-cli` passed; the final benchmark used this binary. |

Final benchmark report: `.local/benchmark/runs/baseline-autonomy-20261022T112418608088922-3/`.

Initial and retry logs are retained in `.local/mizan-live-fixes/`. One mistyped continuation target (`record_edit` instead of `record_validation`) failed before any tests ran and was corrected. Only the reviewer can complete the two port-binding tests outside this sandbox. No new decision is required.

## Files touched

- `CLAUDE.md`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/execute.rs`
- `crates/qiyas-ai/src/history.rs`
- `crates/qiyas-ai/src/lib.rs`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/prompt_en.txt`
- `crates/qiyas-ai/src/provider.rs`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/tests/drafting_eval.rs`
- `crates/qiyas-ai/tests/drafting_eval/phases.rs`
- `crates/qiyas-ai/tests/support/scripted.rs`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-cli/src/eval.rs`
- `crates/qiyas-cli/src/eval_workflow_tests.rs`
- `crates/qiyas-core/src/digest.rs`
- `crates/qiyas-core/src/measurement.rs`
- `crates/qiyas-core/src/recognition.rs`
- `crates/qiyas-core/src/records.rs`
- `crates/qiyas-core/src/validation.rs`
- `crates/qiyas-core/tests/recognition.rs`
- `crates/qiyas-core/tests/records.rs`
- `docs/2026-10-22-mizan-live-fixes.md`
