# Mizan discovery waste corrections — 22 October 2026

The second live-dev report was
`.local/benchmark/runs/eval-set-20261022T113924968610301-4158144/report.json`.
Its apartment log contains 24 requests, all reads or validation, and no mutation. The combined
six-case coverage was 4%. This change addresses orchestration and discovery; no paid evaluation,
provider configuration or API key was opened or used.

## Changes

`qiyas_read digest` points to `qiyas_read takeoff_kit`, and the prompt's workflow index names
these reads first. The kit is generated from command/kind `x-takeoff-kit` registry metadata:
required members for each complete example, all schema enum choices, and a registry example for
object types, recognition definition/schedules/application, methods, mappings, reporting,
define_check and check. Optional structures have their complete required-member lists in error
repairs. The kit also gives self-check and final-answer examples. Mapping accepted elements
triggers the existing automatic BOQ derivation; measurement.report returns the result.

The connected batch copies the selected registry examples in metadata order: define type,
define recognition, apply, set method, set mappings, report. Its layer and block placeholders
are replaced by paired names from an actual drawing insert, exposed in the digest. This does
not assign the insert a trade or classify its meaning. Callers adapt the examples to their
evidence and preserve any selected measurement method. No trade lists, case recipes, standards
or quantities were added to the product.

The final kit is 5,899 serialized bytes on the drawing fixture, below the 6,144-byte guard.
Repeated calls produce identical JSON on unchanged evidence. The static contracts, example
selection and batch order come from registry metadata. The tests compare the returned enum
choices against the complete registry schemas and run the bound example batch.

Missing-field validation now returns `repairs` with full required-member paths, enum choices
and a filled registry example. Run and batch failures retain these repairs, including the
failing step's pointer. Nested empty mappings and missing type/rule payloads are tested through
both validate and execute. Existing schema and error details remain available.

Both prompts and the take-off workflow use digest + kit, one picture if needed, one adapted
batch, apply/report, repairs from returned errors, self-check and end. They explicitly forbid
validating stubs or fetching individual schemas already supplied by the kit. The eval task
prompt uses the same order and the eight-request target.

Metered evaluation handles busy/rate-limit rejections with at most three provider attempts.
Fallback delays are one and two seconds; Retry-After overrides them, including HTTP-date
headers. HTTP 429, 503 and 529 are recognized. One reservation covers the attempts; rejected
attempts do not consume successful request allowance or become unknown-usage charges.
Retries are logged and counted separately. Exhaustion returns the provider error and releases
the reservation. Transport failures and missing usage still retain conservative accounting.
Tests cover successful retry, exhaustion, delays, date parsing and the ledger.

## Offline comparison

The provider sees only production conversation/tool results, with no Session, filesystem,
prepared recognition data or benchmark truth access. The harness generates and scores the
cases outside the provider. The old workflow is retained and exercised alongside the new one.
The new provider copies the returned kit batch and adapts it to supported closed-region
geometry, including expressions, descriptions, units and citations.

| Generated case | First reported BOQ before → after | All requests before → after | Coverage | Item precision | Matched accuracy |
|---|---:|---:|---:|---:|---:|
| office, seed 15127 | 8 → 4 | 11 → 6 | 37.5% | 100% | 100% |
| apartment, seed 17139 | 8 → 4 | 11 → 6 | 44.44% | 100% | 100% |

The old workflow applies mappings on request 7 and reads its first BOQ report on request 8.
The kit workflow generates and reports in request 4, self-checks on request 5 and ends on request
6; the harness then scores the retained BOQ. Thus generation moves from request 7 to request 4,
and both complete scored runs fit within eight requests. No quantities change in this comparison.
This proves the tool path and request budget, not live model performance or complete recognition.

Raw evidence is in `.local/mizan-discovery/`: the compact kit, four before/after JSON reports,
and check logs. Existing history/cost replay tests also passed without relaxing their bounds.

## Checks

All Cargo commands ran sequentially, with `CARGO_BUILD_JOBS=2` and `nice -n 19`. No helper agents,
parallel builds, paid evals, new dependencies, git staging/commits/pushes or scripts/check.sh.
Only obsolete duplicate core test executables with fresh replacements were removed from the
build cache to make room during compilation; source files and run evidence were retained.

| Requested check | Actual result |
|---|---|
| cargo fmt --all | Passed once. Later edits were formatted directly with rustfmt; the final rustfmt check passed. |
| Workspace Clippy, offline, all targets, warnings denied | Initially failed on the read-operation array length (17 → 18); fixed, and the workspace rerun passed. Final all-targets refresh of core, AI and CLI passed with warnings denied. |
| cargo test --offline -p qiyas-core | The example guard initially lacked the new registry-defined type. Its fixture now selects the matching registry definition; the failed and 35 skipped targets passed, as did doc tests. The final 13 registry tests also passed. |
| AI library, surface, drafting_eval, authoring | Library 72 passed; one localhost bind failed with PermissionDenied. Authoring 19 and drafting 4 passed. Surface initially rejected the new read-operation index entry; the guard now validates those names against READ_OPS and all 47 passed. |
| Desktop tests and CLI | CLI 37 unit and 18 integration tests passed. Desktop library 94 passed; one localhost bind failed with PermissionDenied. Remaining options 10, settings 15 and the zero-test main target passed. |
| tsc --noEmit | Passed, exit 0. |
| Vitest | Not run: no src/ files changed. |
| Quick autonomy benchmark | Ran once at the end, exit 0: 120 cases, one repetition, 54.24 seconds. 716/3,672 matched (19.4989% coverage), 98.7586% item precision; all 716 matched quantities within 1%. |

Benchmark report: `.local/benchmark/runs/baseline-autonomy-20261022T131326621122674-3/`. This remains the engine diagnostic, not a live Mizan score. The final whitespace audit also passed.

The reviewer must run `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
and `commands::tests::the_mcp_server_follows_the_settings` where localhost binding is permitted.
Neither test was skipped or weakened. No product decision is outstanding.

## Files touched

- `CLAUDE.md` and this document.
- `crates/qiyas-core/src/{commands,digest,lib,measurement,recognition,registry,takeoff_kit,validation}.rs`.
- `crates/qiyas-core/tests/{own_output,registry}.rs`.
- `crates/qiyas-ai/src/{provider,surface,surface_args}.rs`.
- `crates/qiyas-ai/src/{prompt_en,prompt_ar,takeoff_workflow}.txt`.
- `crates/qiyas-ai/tests/surface.rs`.
- `crates/qiyas-cli/src/{eval,eval_workflow_tests}.rs`.
