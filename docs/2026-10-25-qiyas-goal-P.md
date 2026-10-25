# Goal P: engine speed (25 October 2026)

The final 85-call clinic replay is **329.31× faster in debug**: 2,319.288 →
7.043 seconds, with a maximum call of 0.789 seconds. Release is **58.55× faster**:
395.481 → 6.754 seconds, maximum 0.815 seconds. Both absolute timing gates pass.
Both profiles have zero differences across 171 result/state files, four PNGs and
two exports, apart from the explicitly normalized identifiers and timestamps below.

Goal P is **not fully complete**. P5/P6 general batch deferral remains unfinished:
a prototype changed intermediate results and allocated handles and was removed.
The new CPU-relative clinic guard also fails (4.5106 against 2.0), although the
absolute release gate passes. The existing villa guard passes. Three performance
correction rounds are complete; the remaining profile is recorded below.

The founder narrowed acceptance to the clinic speed replay, targeted tests and
checks of touched crates. Other end-to-end gates are **left to the reviewer's
combined run**. Earlier MCP evidence is retained below, but describes an earlier
build. The founder confirmed that interrupted jobs were stopped by connectivity
loss and laptop restarts. They are not counted as failed engine tests.

All edits and artifacts are in `/home/kareem/Work/.qiyas-wt/goal-p`. The founder
checkout was read-only. No commits, pushes, paid calls, dependencies or licence
changes were made.

## Changes by item

| Item | Implementation and result |
| --- | --- |
| P1 | `objects_for(ids)` and `object_quantity(id)` select drivers before quantity evaluation. Fields, tags, explicit schedule IDs, object record selectors and explanations use narrow reads. Host and union dependencies retain the original ordered prefix allocation. The complete debug clinic has no quantity, record or result differences. |
| P2 | Generation-bound caches cover decoded types, expressions, object contexts, records and derived band geometry/facts. Generations compare entity snapshots and exact serialized knowledge, units and table inputs, including writes inside transactions. Streaming byte comparison avoids allocating a JSON tree on every read. Rollback, undo, redo and external-reference reload invalidate caches. Unchanged metadata writes preserve recorder first-touch order. |
| P3 | Source graphs, uncut wall footprints, exact-tolerance arrangements and seeded rooms are cached. Opening changes reuse uncut footprints. Topology changes reuse only faces with unchanged ordered source support and effective tolerance; no fixed-distance invalidation rule is used. Boundary policy and island semantics are unchanged. |
| P4 | Expensive check geometry and rules are scoped before evaluation. Current and former positions, removals, generated parts, fields and other reverse dependencies select candidates. Ordinary room names follow connected source components. Exact rule predicates remain authoritative. Full phase/final checks remain full, and response filtering is unchanged. |
| P5 | Existing ordered execution and one-step transactional undo/rollback remain. General deferred derivation is unfinished because the attempted mixed grouping changed per-step results and allocator state. Read-barrier and exact intermediate-result regression tests were added. |
| P6 | Existing Goal K all-addition grouping remains. The mixed opening addition/removal/edit prototype was removed after differential failure. Grouping all wall edits across batch steps is unfinished; no incorrect shared-final-result implementation remains. |
| P7 | Generation-bound indexes cover assemblies, host openings, generated parts, object fields/schedules and source-to-region candidates. Derivation consults previous and current indexes, including removals. Source-to-region candidates conservatively cover accepted sources on a sheet because a distant source can change effective weld tolerance; exact face reuse narrows actual tracing. Unused type definitions no longer force derivation of existing objects. |
| P8 | Dev `qiyas-core` and `qiyas-param` use opt-level 3 with debug assertions and normal floating-point semantics. The debug speed gates pass. Build and memory observations are below. |
| P9 | No incremental wall-regeneration change. The remaining initial profile bottleneck was generation comparison, not wall booleans; the debug gate passes after fixing that. The release absolute gate also passes; the remaining relative-guard profile is dominated by generation checks and repeated derivation, not wall booleans. |

## Replay and measured gates

The new repository fixture includes all 85 completed calls from `clinic.log`, whose
SHA-256 is `820355a5a5ab06fca2a281aa50bf269fbf9092d982bf95f9432b79db32a99918`.
The supplied log ends at 23:03:18. The later 487-second move and work through 23:58
mentioned in the notes are absent as completed calls. Three clipped inputs were
recovered by the supplied research harness; the fixture notes identify them.
The name-only Settings method `POMI (RICS)` is applied first, reproducing live handles.
Host tab listing is emulated, Save As is omitted, and export paths are isolated.
Every failed call checks exact rollback of entities, structure, history and revision.
Every call saves full entities, quantities, measurement report and take-off.

| Gate | Before | After | Result |
| --- | ---: | ---: | --- |
| Debug, 85 calls total | 2,319.288 s | 7.043 s | 329.31×; passes 10× gate |
| Debug, largest call | 507.840 s | 0.789 s | All calls below 3 s |
| Release, 85 calls total | 395.481 s | 6.754 s | 58.55× |
| Release, largest call | 87.096 s | 0.815 s | All calls below 1 s |
| Exact debug comparison | 171 JSON files, four PNGs, PDF and workbook | Zero mismatches | Pass |
| Exact release comparison | 171 JSON files, four PNGs, PDF and workbook | Zero mismatches | Pass |
| Clinic relative guard | Limit 2.0 × fixed CPU reference | 0.847003 / 0.187779 s = 4.5106 | Fail; threshold unchanged |
| Villa relative guard | Limit 2.0 × fixed CPU reference | 0.252122 / 0.178631 s = 1.4114 | Pass |
| Four-unit villa replay | mm, cm, m, in | 3.19 s test runtime | Pass; 44 openings and failed-batch checks |

An additional final-code debug sample during this worktree's full test compilation
is retained: 22.903 seconds total, maximum 3.504 seconds at call 67 (one failure of
the 3-second gate). The same binary repeated after that build ended produced the
7.043-second result above. Other goals and the founder's app shared the machine;
these measurements are not an isolated-machine benchmark.

Only random session UUIDs and isolated evidence paths are rebound by the JSON comparator.
The PDF is byte-identical, and all workbook members match except the two expected
creation/modification timestamps in `docProps/core.xml`. The comparator normalizes
only those timestamp values; worksheet values, formulas and styles remain exact.
No quantity, entity handle, revision counter or finding is excluded. The four final release
PNGs were inspected and match the baseline byte for byte: geometry matches; existing crowded labels remain visible.
The frozen baseline production source was audited against HEAD `5953604`: all 410
Rust/Cargo files matched. Only its replay harness had output-path fixes.

The first complete optimized profile took 31.824 timed seconds, maximum 4.232 s;
calls 67, 72, 75 and 76 exceeded three seconds. Nested timers identified repeated
JSON allocation during generation comparison as the main remaining cost. Correction
1 streams exact bytes, preserves unchanged-driver touch order, narrows check
geometry, indexes source/region candidates, caches measurement expressions and
avoids deriving unused definitions. Its full debug and release comparisons pass.
Release calls 41, 67, 75, 76, 83 and 84 still exceed one second. At the slowest call,
1.318 s is input comparison and 0.324 s is dependency-index rebuilding. Correction
2 preserves exact unchanged hosted-driver snapshots and groups consecutive immutable
reads under one generation check. It took 9.578 timed seconds, maximum 1.073 s,
with exact outputs. Calls 67 (1.043 s) and 76 (1.073 s) still miss the release gate.
At call 76, input serialization alone takes 0.545 s. Correction 3 uses an exact,
temporary binary cache encoding: type tags, lengths and float bits replace JSON
escaping of embedded knowledge strings. It changes no drawing or API format. This
is the third and final performance correction round. Its first debug replay ran
alongside this worktree's full test compilation: 22.903 timed seconds, maximum
3.504 s at call 67. All 171 JSON files, four PNGs and both exports match. That
sample is retained. The repeat after this worktree's compilation finished passes
both debug timing gates. Final release also passes the absolute gate and exact
comparison. The stricter relative clinic guard still fails; no threshold was relaxed.

The final release profile's slowest call is 67, at 0.814564 s. Exclusive costs include
input-generation comparison 0.275558 s (3,077 calls), entity/index capture 0.136091 s
(142), object contexts 0.084437 s, and derivation capture 0.065875 s. Eleven derivations
consume 0.554184 s inclusive; these nested figures must not be added. Band derivation
is 0.004590 s, and room boundary work is 0.025281 s inclusive across 208 queries.
This points to remaining repeated derivation/generation work, rather than a P9 wall
boolean rewrite. The profile is in correction 3 release `calls.jsonl`.
Instrumentation is compiled only for tests/the existing fixtures feature and is
explicitly enabled by `CLINIC_PROFILE`; it is absent from normal shipped builds.

## Build cost and checks

Use `CARGO_BUILD_JOBS=1` on this 7.5 GB shared laptop, always through the shared
`cargo` queue with `CARGO_TARGET_DIR=/home/kareem/Work/.qiyas-wt/goal-p/target`.
The shared wrapper sets incremental compilation and disables dev/test debug symbols;
do not bypass it. Two concurrent compiler children can exceed 4 GiB before the app
and other work are counted. Debug assertions remain enabled.

| Build | Elapsed including queue | Largest child RSS | Sampled sum of owned-process RSS |
| --- | ---: | ---: | ---: |
| Initial cold baseline debug example | 1,814.02 s | 1,643,864 KiB | Not sampled |
| Baseline release example, resumed build | 1,349.44 s | 2,130,516 KiB | 2,136,748 KiB |
| Correction 1 debug CLI and example | 600.52 s | 2,241,356 KiB | 2,274,632 KiB |
| Correction 1 release example | 830.60 s | 1,929,512 KiB | 1,941,036 KiB |
| Correction 2 release example | 624.42 s | 2,249,060 KiB | 2,355,048 KiB |
| Correction 3 release example and both guard targets | 264.01 s | 2,071,280 KiB | 2,116,552 KiB |
| Final full touched-crate tests, including compilation | 2,225.06 s | 2,428,892 KiB | 2,421,352 KiB |

Sampled summed RSS can double-count shared pages and miss short peaks. These builds
have different warmed dependencies and feature sets under shared load; they are
observed costs, not a controlled opt-level-only comparison. Debug replay maximum
child RSS increased from 134,480 to 160,824 KiB (about 26 MiB) while timed work fell
from 2,319.288 to 7.043 seconds; these measurements include saved snapshots. Earlier core fingerprint
observations were 151.99 s for a baseline fixture library and 652.97 s for its first
optimized build. See `.local/goal-run/core-build-fingerprint-times.json`.

All Cargo commands used the required target directory and `CARGO_BUILD_JOBS=1`.

| Check | Actual result |
| --- | --- |
| `cargo fmt --all`, followed by final `cargo fmt --all -- --check` | Pass |
| `cargo clippy --offline -p qiyas-core -p qiyas-ai -p qiyas-param --all-targets -- -D warnings` | Pass, final run 53.39 s |
| `cargo test --offline -p qiyas-core -p qiyas-ai -p qiyas-param --no-fail-fast` | Exit 101: 1,129 passed, 44 local-port permission failures, 17 ignored |
| Core tests | 834 passed; no failures or ignored tests |
| Param tests | 22 passed; no failures or ignored tests |
| AI tests | 273 passed, 44 failed, 17 ignored; surface 69/69 passed |
| Explicit four-unit `logged_villa_in_every_unit` | Pass, 3.19 s; existing compiled debug test binary |
| Release clinic and villa speed guards | Exit 101: clinic fails ratio 4.5106; villa passes ratio 1.4114 |
| `git diff --check` and Python comparator syntax | Pass |
| Frontend checks | Not applicable; no frontend source changed |

All 44 AI failures are sandbox denials when binding local mock-server ports
(`PermissionDenied` / `Operation not permitted`), not paid service calls. The failed
targets are AI `--lib` (4), `--test agent` (30), `--test key_safety` (1), and `--test mcp`
(9). Every test name, panic location and denial classification is listed in
`.local/goal-run/final-tests-summary.json`; the complete log is `final-tests.log`.
No successful run is claimed for those targets. The explicit AI library, surface
and agent requirements are included in the full crate run.

Final logs: `.local/goal-run/final-clippy-correction3.{log,time}`,
`final-tests.{log,time}`, `final-villa-units.{log,time}`,
`final-release-guards.{log,time}` and `final-fmt-check.log`.
The new tests cover narrow queries, cache generations, exact encoding, mixed opening
read barriers and ordered validity. Existing full core tests cover units, undo/redo,
rollback and injected panic recovery. No existing tests were loosened.
Earlier tests exposed a cache-guard RefCell panic and an incorrect input shape in a
new test; both were corrected before final validation. Initial Clippy cleanup
warnings were fixed without changing behavior. Stale binaries were removed only
from this worktree, with removal inventories in `.local/goal-run/`.

## Evidence paths

All paths below are relative to this worktree.

- Final debug: `.local/goal-run/baseline-debug/correction3-debug-final/`; `comparison.json` records 329.31× and zero mismatches; `calls.jsonl` has every call and profile. Runtime including snapshots: 13.598 s, maximum child RSS 160,824 KiB.
- Final release: `.local/goal-run/baseline-debug/correction3-release/`; `comparison.json` records 58.55× and zero mismatches; `calls.jsonl` has every call and profile. Runtime including snapshots: 9.543 s, maximum child RSS 156,284 KiB. Inspected PNGs: `014.png`, `055.png`, `056.png`, `062.png`.
- Final replay transcripts: `.local/goal-run/replay-correction3-debug-final.{log,time}` and `replay-correction3-release.{log,time}`. Source hashes: `engine-correction3-sources.json`; preserved executables: `bin/correction3-{debug,release}-clinic`.
- Before debug: `.local/goal-run/baseline-debug/baseline-final/`; timings and memory in `.local/goal-run/replay-baseline-debug-final.{log,time}`.
- Correction 3 debug during compilation: `.local/goal-run/baseline-debug/correction3-debug/`; zero exact mismatches, but call 67 exceeded three seconds.
- After debug correction 1: `.local/goal-run/baseline-debug/correction1-debug/`; `comparison.json` records zero differences, and `calls.jsonl` has each timing. Inspected images: `014.png`, `055.png`, `056.png`, `062.png`.
- After release correction 2: `.local/goal-run/baseline-debug/correction2-release/`; full comparison has zero mismatches, with the two timing failures above.
- After release correction 1: `.local/goal-run/baseline-debug/correction1-release/`; full comparison has zero mismatches. Timers and per-call profile are in `calls.jsonl`.
- Before release: `.local/goal-run/baseline-debug/baseline-release/`; `.local/goal-run/replay-baseline-release.{log,time}` (85 calls, all rollback checks pass, 413.636 s including snapshots).
- After debug runtime: `.local/goal-run/replay-correction1-debug.{log,time}` (31.565 s including snapshots, 160,112 KiB maximum child RSS).
- Initial optimized profile: `.local/goal-run/baseline-debug/profile-round1/`; source/build hashes and preserved binaries are in `.local/goal-run/bin/` and `.local/goal-run/engine-correction1-sources.json`.
- Frozen source audit: `.local/goal-run/baseline-source-audit.json`.
- Replay source: `crates/qiyas-ai/examples/clinic_performance.rs`, `tests/clinic_performance.rs`, `tests/fixtures/clinic-performance.{json,md}`; comparator: `scripts/compare-clinic.py`.
- Mixed-edit failure reproduction: `.local/goal-run/{before-mixed,after-round1-mixed}/`, with transcripts, result/state JSON and two inspected PNGs. Baseline allocated opening 107; the removed prototype allocated 103 and returned final wall state for earlier steps.
- Earlier four-unit MCP runs: `.local/goal-run/before-{mm,cm,m,in}/` and `after-round1-{mm,cm,m,in}/`; all 44 openings, undo/redo and failed-batch checks passed. All 4,871 paged records matched, and 16 PNGs matched exactly.
- Earlier room MCP runs: `.local/goal-run/{before,after-round1}-rooms-{mm,cm,m,in}/`; all 20 phase snapshots and 12 PNGs matched. Opening edits preserved areas; partition moves changed the two affected room quantities. Comparison summaries: `mcp-after-round1-comparison.json`, `villa-after-round1-records-comparison.json`, `villa-after-round1-image-comparison.json` under `.local/goal-run/`.

Extra MCP jobs, extra captures/transcripts, and the combined post-merge unit/history,
undo/redo and panic end-to-end gates are **left to the reviewer's combined run**.
The local targeted tests still exercise four units, rollback, undo/redo and panic safety.

## Files and ownership

Core implementation: `evaluation.rs`, `check_scope.rs`, `objects.rs`,
`object_schedules.rs`, `object_tags.rs`, `records.rs`, `field.rs`, `derived.rs`,
`draw/room_boundary.rs`, `check.rs`, `check_layout.rs`, `ink.rs`, `session.rs`,
`transaction.rs`, `measurement/evaluate.rs`, `trust.rs`, `walls.rs`. Other changed core source files: `draw.rs`, `fixtures.rs`, `fixtures/profile.rs`, `evaluation/encoding.rs`,
`lib.rs`, `measurement.rs`, `net_effect.rs`, `object_bands.rs`, `object_regions.rs`,
`objects_validate.rs`, `opening_schedule.rs`, `placement.rs`, `room_objects.rs`,
`rooms.rs` and `validation.rs`. These add profiling spans, cache guards or scope
propagation as appropriate. New targeted tests are
`quantity_queries.rs` and `opening_batch_barriers.rs`. Also changed: root Cargo dev
profiles, `scripts/check.sh`, the clinic fixture/harness/comparator, this report and
the single appended Goal P section in `CLAUDE.md`.

Touches another goal:

- C: only new offline `qiyas-ai` fixture/example/tests. No production AI source, provider request, prompt, history or result-envelope change remains.
- A: `validation.rs` holds an immutable evaluation guard while resolving defaults; normalization and schemas are unchanged.
- U: `room_objects.rs` scopes room checks and `placement.rs` passes the scope predicate. Island semantics are unchanged.

## Remaining work and decisions

P5/P6 broad deferral conflicts with exact intermediate results and handle allocation
in the reproduced mixed edit. The prototype was removed. A founder question about
that result contract is pending; the existing exact-result requirement remains in
force. This is an implementation limitation of the attempted grouping, not proof
that exact deferral is impossible. A future implementation must preserve ordered
per-step snapshots and allocator effects, or obtain an explicit change to that
contract before grouping more operations.

The clinic relative guard remains red after the permitted three correction rounds;
its threshold has not been weakened. `scripts/check.sh` will therefore fail there
on this measured machine until that remaining work is resolved. The absolute clinic
gates and exact comparisons pass. Local-port tests need an environment permitting
mock-server binds. Additional post-merge MCP acceptance is **left to the reviewer's
combined run**. No commands, builds or MCP servers remain running for this goal.
