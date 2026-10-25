# Goal A: Mizan's tools and judgement aids

Worktree: `goal-a`, branch `goal/a`, based on `5953604`. No commit, push, model service,
credential access, paid asset or new dependency was used. The founder's checkout was read only.

**A1–A10 are implemented and the fast regression tests pass.** The full Rust run retains
46 environment failures because the sandbox forbids local sockets. The founder revised
acceptance to code, unit/targeted tests and checks for touched crates. All end-to-end gates are **left to the reviewer's combined
run** after merging the five goals. No further headless job or before/after replay is planned.

## Changes by item

| Item | Before | Change and gate |
| --- | --- | --- |
| A1 / Q2 | `set_method` discovery did not suggest its command kind. | Suggestions compare both full names and kind suffixes; expect `measurement.set_method`. Fast tests pass; end-to-end left to the reviewer. |
| A2 / Q4 | The clinic used repeated guesses around a wall corner. | A refusal gives the closest fitting centre for that width and run, in drawing units. It leaves the opening untouched. Tests retry the returned number in mm, cm, m and inches. Fast tests pass; end-to-end left to the reviewer. |
| A3 / Q6, Q24 | Top-level command fields and spatial filters on several query kinds were rejected. | Unambiguous command fields are wrapped, and `bounds` becomes `within`, with warnings. All nine query kinds accept either box. Supplying both is refused. Fast tests pass; end-to-end left to the reviewer. |
| A4 / Q9 | Partial revision tokens were rejected and unrelated changes invalidated reads. | Separate document, geometry, knowledge, sheet and selection parts; compare only supplied non-null fields. Stale results return changed parts, handles, history completeness and the current token. Atomic rollback must leave every part unchanged. Fast tests pass; end-to-end left to the reviewer. |
| A5 / Q10 | The clinic's setup plan remained unchanged through many edits. | Eight mutating calls without a changed plan produce advice and a stale panel marker. Reposting the same plan does not clear it. Final answers list pending, active and skipped phases; state survives restart. Frontend and agent tests pass; MCP acceptance left to the reviewer. |
| A6 / Q8 | Old guidance encouraged pictures before exact checks and quoting an earlier read after one's own edit. | Both prompts require the latest result's revision, exact checks before pictures, brief-based phases and a final self-check. A new general `spatial_query` read exposes containment, nearest points and intersections. Word-count and runtime gates recorded below. |
| A7 / Q28 | Fixture symbols could float away from walls or obstruct doors and clearances without a retained relationship. | An optional cited JSON pack defines WC, basin and sink objects, host placement, back/body/clearance regions and placement checks. Fixtures do not cut their hosts. Check values, dimensions and tolerances remain editable data. Fast tests pass; end-to-end left to the reviewer. |
| A8 / Q29 | Image capability was not explicit and text-only tool-capable models were excluded from discovery. | State distinguishes supported, unsupported and unknown image input; Settings warns and permits an explicit choice. Text-only models remain selectable. Unsupported models receive no new or historical pictures. Frontend and fake-provider tests pass. |
| A9 | The clinic omitted the requested survey position. | `set_site_origin` moves/rotates model geometry, sets UCS and insertion base, and stores site metadata in one undo step. `site_coordinates` reads exact defining points in world/site, metres and local UCS coordinates. Test destination: E 674,250,000 / N 2,734,180,000 mm, plus equivalent cm, m and inch drawings. Fast tests pass; end-to-end left to the reviewer. |
| A10 / M1–M7, M9–M11 | Missing rooms, wrong sizes/thickness/scale/direction and omitted deliverables were not consistently identified at finish. | `brief_check` checks caller-supplied expectations through `verify` or drawing reads. Each item has a measured value and pass/fail. Untied literal arithmetic does not prove a drawing. Final answers include failed items, and edits invalidate earlier checks. Fast tests pass; end-to-end left to the reviewer. |

## Fast-test coverage and reviewer checks

All end-to-end gates below are **left to the reviewer's combined run**. The named Rust
tests are in `qiyas-ai/tests/goal_a.rs` or `qiyas-core/tests/goal_a.rs` unless stated otherwise.
The targeted run passed all 20 tests: 9 AI tests in 0.34 s and 11 core tests in 2.03 s.

| Item | Fast proof | Reviewer should check after merging |
| --- | --- | --- |
| A1 | `discovery_suggests_the_command_kind_for_a_bare_or_mistyped_kind` | Ask for `set_method` and confirm `measurement.set_method` is suggested. |
| A2 | `corner_refusal_recommends_a_position_that_works_in_every_unit`; `recorded_clinic_opening_succeeds_on_the_second_attempt` | Refuse the impossible corner, leave geometry unchanged, then use the returned position in one retry. |
| A3 | `every_query_accepts_both_boxes_and_bounds_warns`; `run_wraps_command_fields_and_partial_revision_checks_only_supplied_parts` | Send clinic-style top-level fields and both box aliases across all query kinds; inspect warnings and returned geometry. |
| A4 | `revision_parts_are_independent_and_partial_stale_reports_identify_changes`; the AI partial-revision test | Mix drawing, knowledge, selection and sheet changes; stale results identify supplied changed parts and current handles; retry once. |
| A5 | `stale_plan_unmet_brief_and_blind_state_reach_the_final_answer_and_resume`; `plan_watch::tests::an_identical_plan_does_not_hide_unfinished_work`; `messages.test.ts` | After eight edits the panel marks the unchanged plan; final answer lists unfinished phases, including after reopen. |
| A6 | English/Arabic prompt tests; `exact_spatial_reads_work_in_all_units_and_at_survey_coordinates` | Mizan uses the latest revision and exact containment/nearest/intersection/room/clearance facts before pictures. |
| A7 | `fixture_pack_values_drive_contact_clearance_and_host_motion_without_a_cut`; `analytic_door_sector_checks_clearance_even_when_the_fixture_body_is_clear`; `optional_pack_install_is_atomic_editable_and_one_undo_step` | Install the optional pack, place against a host, inspect back contact, obstruction and door-swing findings, then move the host and undo. |
| A8 | The blind-agent/resume test; model capability unit test; `ModelPicker.test.tsx` | Choose a text-only model; Settings warns, state says false, images are withheld, and the answer makes no visual-inspection claim. |
| A9 | `site_geometry_basepoint_and_ucs_roundtrip_and_undo_in_all_units` | Place at E 674,250,000 / N 2,734,180,000 mm, read coordinates, undo/redo and reopen; inspect sheet viewports too. |
| A10 | `brief_checks_measure_sizes_site_direction_and_every_stated_deliverable`; `clinic_brief_catches_missing_rooms_wrong_thickness_scale_survey_and_deliverables`; the final-answer/resume test | Use the founder's actual brief, including rooms, sizes, thickness, scale, survey, direction and every requested output. Each unmet item must appear plainly at finish. |

## Using the new data and reads

The interiors pack is optional. Inspect it with `read_object_pack`, install it with
`install_object_pack`, and place a fixture with one `place_objects` entry naming its host and
distance along it. Length properties use metres; geometry command coordinates use drawing units.
`side` is “left” or “right”. Installed checks are readable as `knowledge_placement` records and
editable with `set_knowledge`.

The supplied body sizes are clearly marked planning assumptions. Clearance defaults cite the
US Access Board's 305.3 clear-floor-space dimensions. The door-swing source acknowledges
603.2.3 exceptions; the pack is a conservative project assumption, not a compliance decision.
Tests substitute an invented `TEST-CLAUSE-A7-*` citation and different dimensions to demonstrate
that the engine takes its values from data. Open lines are not treated as enclosing regions.

For a millimetre drawing, the survey command is:

```json
{"type":"set_site_origin","base_point":[0,0],"site_point":[674250000,2734180000],"x_axis":[1,0]}
```

Omitted targets move all model geometry. An explicit empty list changes only UCS/base-point
metadata. JSON geometry commands use world coordinates; typed points use the UCS. The reported
site coordinates already include the placement; do not add the origin twice.

For the brief, supply the founder's actual requirement as `text`, then either a `verify`
expectation or a read with its JSON pointer and expected value. Numeric vectors and `subtract`
support signed changes such as “1.5 m east”. The engine does not decide whether a brief was
interpreted completely. Artifact checks establish only file existence and size, not content.

## Checks and measured results

| Check | Result |
| --- | --- |
| `cargo fmt --all` | Passed, exit 0. |
| TypeScript `--noEmit` | Passed, exit 0. |
| Full frontend `vitest run --maxWorkers=2` | Passed after the final UI change: 121 files, 915 tests, 106.16 seconds. |
| Changed Rust syntax parsing | Passed before compilation; this is not a type check. |
| Python MCP client syntax | Passed. |
| Prompt words | English 654, Arabic 861; both below 900. |
| Frozen baseline CLI build | Passed, exit 0; 1 minute 36 seconds compilation after queue wait. |
| Targeted core and AI `--test goal_a` | Passed: 20 tests (9 AI, 11 core), including all four length units. |
| Full core, geometry, AI, desktop and CLI Rust tests | Ran all five touched crates with `--no-fail-fast -- --test-threads=2`: exit 101, 1,408 passed, 57 failed, 15 ignored (summed result lines). Of the failures, 46 are local-socket restrictions; 11 code/expectation regressions were corrected and their targets rechecked successfully (details below). No final full-suite pass is claimed. |
| AI surface, agent and drafting evaluations | All 4 drafting evaluations passed (1.58 s). Agent: 3 passed, 30 local-socket failures. Final surface recheck: all 69 passed (2.50 s); kit is 6,118 bytes against the unchanged 6,144-byte limit. |
| Touched-crate clippy, all targets, warnings denied | Passed after all regression fixes, exit 0, 1 min 01 s: core, geometry, AI, desktop and CLI. Log: `clippy-recheck-final.log`. No full-workspace clippy under the revised scope. |
| Headless before jobs | Completed, exit 0: 117 gates, 12 already passing and 105 failures reproduced. All ten PNGs inspected. |
| Headless after jobs and acceptance loop | Left to the reviewer's combined run. |

No existing acceptance assertion was loosened. The old table-query test that expected `within`
to be rejected now verifies acceptance and retains rejection of a misspelled field. The old
model-list test excluded text-only models; it now verifies that tool-capable models remain
available and that their image capability is explicit. Prompt tests now expect exact checks
before pictures, matching the founder's ruling. The CLI instructions assertion now requires
every requested deliverable and forbids adding an unrequested BOQ; the old assertion said
drawing work never needs a BOQ. Existing prompt wording assertions and byte/token budget
assertions remain intact.

## Rust regression and environment results

The full-run log is `.local/goal-run/tests-final.log`. Its socket failures report
`PermissionDenied` / `Operation not permitted` when binding local fake servers:

- AI library: 4 (mock Responses acceptance, unsupported-reasoning retry, fake Claude CLI,
  unreachable-service model-discovery fallback).
- AI `agent`: 30; AI `mcp`: all 9; AI `key_safety`: the network redirect test (1).
- Desktop library: `the_mcp_server_follows_the_settings` (1); desktop settings:
  `credentials_follow_the_saved_address_and_never_a_draft` (1).

These 46 tests need a local-port-enabled runner. They were not skipped or weakened.
The 11 remaining failures found kit/index byte limits, a history budget, optional collection
handle allocation, native command examples/schema, and an obsolete CLI instructions assertion.
The fixes retain the existing byte/token limits. Optional placement storage now allocates only
on first use, preserving handles in drawings that never install the pack.

Final rechecks:

| Target | Result | Log under `.local/goal-run/` |
| --- | --- | --- |
| Core registry + validation | 13 + 11 passed | `tests-schema-final.log` |
| Core Goal A | 11 passed, including six new tests across four units | `tests-core-recheck.log` (this earlier run also contains the subsequently fixed schema failure) |
| AI Goal A | 9 passed | `tests-ai-final.log` |
| AI surface | 69 passed | `tests-ai-final.log` |
| AI library | 137 passed, 4 local-socket failures, 12 ignored; all prompt tests pass | `tests-lib-cli-final.log` |
| CLI unit tests | 66 passed | `tests-lib-cli-final.log` |
| CLI agent evaluation | 8 passed | `tests-cli-recheck.log` (earlier budget failure fixed in the final CLI run) |
| Villa Goal L replay and opening regression | 1 + 2 passed; 2 pre-existing expensive opening tests ignored | `tests-ai-recheck.log` |

The local scripted history replay retains its original 21,000-token and $0.27 finish-case
limits. Final peaks: finish 20,587; request cap 20,978; cost cap 20,144 estimated tokens.
Finish cost is **$0.26088 simulated**, with no paid call. The three replay JSON records are
`.local/mizan-live-fixes/tokens-{finish,request-cap,cost-cap}.json`. The request and cost caps
stop as expected and retain the partial report. Prompt shortening removed repeated prose;
it did not change the history compaction policy or loosen the limits.


## Evidence

The original evidence is the founder's `clinic.log`, `clinic-run-notes.md`, screenshots and
research notes under `.local/audit/live-2026-10-24/`. The source used for a genuine before run
was frozen with `git archive 5953604` into this worktree's `.local/goal-run/baseline-src` before
building. A selection of 62 relevant clinic tool intents is retained in
`.local/goal-run/clinic-selected-intents.json`.

The stdlib Python client and job are `.local/audit/mcp_client.py` and
`.local/audit/goal_a_acceptance.py`. Every edit is followed by paged entity/object/layer reads
and state, plus specific measured readbacks. Before transcripts and PNGs are retained under
`.local/goal-run/before/{recorded-clinic,mm,cm,m,in}/`. All before PNGs were inspected: the
recorded failed atomic request leaves only the seed line; the four unit jobs show the same
L-shaped wall without an opening. Gate details are in `before-gates.json`, and the run log
is `before.log`. These before jobs finished before the scope change. After evidence is left
to the reviewer's combined run.

The recorded clinic fixture is `crates/qiyas-ai/tests/fixtures/clinic-corner.json`, from log
lines 169–170. Opening 10 at 1000 mm is refused at the junction on run 3. The old response
has fitting ranges but no nearest placement. Each baseline unit job also refuses the
top-level text command and the partial revision token: three refusals for those three intents.
The final frontend logs are `tsc-final.log` and `vitest-final.log`; both passed after the
capability-persistence fix. Formatting passed in `fmt-final.log`.

## Files and touches another goal

The main changes are prompts, normalisation/discovery, revision tracking, plan and brief
tracking, model capability metadata, new site/spatial reads, optional placement data and tests.
Files changed, relative to the repository root:

- `CLAUDE.md`, `docs/2026-10-25-qiyas-goal-A.md`.
- `crates/qiyas-ai/src/`: `agent.rs`, `brief_check.rs`, `conversation.rs`, `execute.rs`,
  `lib.rs`, `mcp.rs`, `models.rs`, `normalise.rs`, `plan_watch.rs`, `prompt.rs`,
  `prompt_en.txt`, `prompt_ar.txt`, `prompt_tests_ar.rs`, `surface.rs`, `surface_args.rs`,
  `window.rs`; tests: `tests/goal_a.rs`, `tests/fixtures/clinic-corner.json`.
- `crates/qiyas-core/src/`: `blocks.rs`, `check.rs`, `commands.rs`, `knowledge.rs`,
  `lib.rs`, `objects.rs`, `placement_pack.rs`, `query.rs`, `read_revision.rs`,
  `record_edit.rs`, `records.rs`, `registry.rs`, `session.rs`, `site.rs`, `spatial.rs`, `tables.rs`,
  `transaction.rs`, `undo.rs`; `tests/goal_a.rs`; data: `data/interiors-pack.json`,
  `data/legacy-object-types.json`.
- `crates/qiyas-geom/src/walls.rs`; `crates/qiyas-cli/src/eval.rs`, `tests/agent_eval.rs`.
- `crates/qiyas-desktop/src/ai.rs`, `src/settings.rs`, `tests/settings.rs`.
- Frontend: `src/api.ts`, `src/ai/AssistantPanel.tsx`, `src/ai/PlanChecklist.tsx`,
  `src/ai/messages.ts`, `src/ai/messages.test.ts`, `src/i18n/en.json`, `src/i18n/ar.json`,
  `src/shell/ModelPicker.tsx`, `src/shell/ModelPicker.test.tsx`, `src/shell/SettingsDialog.tsx`.

Necessary overlap with another owner:

- **P:** small object-host/zone additions, the optional placement-check hook, UCS undo state,
  and rollback labels for the two new edits. Quantity rules, cache strategy, room-island policy,
  batch deferral and Cargo profiles were not redesigned.
- **C:** the filtered-query output helper, persisted brief state, agent result advice/final
  notes, image withholding, compact kit prose, and the brief's export-metadata read. No provider request format,
  cache implementation, compaction policy or transport was changed.
- **W:** selection revision synchronisation and local settings-to-agent capability wiring;
  no tab/session ownership or desktop state restructuring.
- **U:** no application bug fix was taken over; UI changes are the Goal A stale-plan marker
  and image-capability settings warning.

## Remaining work and decisions

No Goal A code item is left unimplemented. The 46 local-socket tests require a runner with
local ports. Headless after gates and the acceptance loop are
left to the reviewer's combined run. The optional pack's
planning assumptions need project-specific review before use as requirements; its installation
never claims code compliance. External MCP clients that do not identify a model receive
“unknown” image capability, with explicit guidance to avoid claiming visual inspection.
