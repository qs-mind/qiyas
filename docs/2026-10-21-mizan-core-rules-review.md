# Mizan findings: shared rules review

This change implements the founder's rules in the core, registry and shared automation
surface. It does not rely on model prompting for geometry, input defaults or validation.
Changes are left uncommitted.

## Changes by requested item

1. **Point runs.** `PointRun<CLOSE>` is the shared command input for point paths and
   boundaries. It uses the geometry library's absolute same-point tolerance converted
   through drawing units, plus its survey-coordinate relative tolerance. Boundaries drop
   a repeated return vertex; open paths retain it. Explicitly closed polylines remove the
   repeated vertex and corresponding redundant bulge. The policy appears in field schemas.
   Coverage includes draw paths, wall runs, measurements, comments, markups, leaders,
   dimension and constraint picks, verification and polygon picking. Rooms accept seeds
   and rectangular bounds, not a point-run field.
2. **Own-output guard.** Registry drawing edits run in fresh mm, cm, m and inch drawings,
   with example coordinates converted to equivalent physical sizes. Commands requiring
   existing geometry get explicit fixtures. Added geometry is checked with the general
   pack; errors and warnings fail the guard. Harder cases cover wall loops in both
   directions, explicit and repeated closure, L/T/X junctions, doors and windows near
   ends/corners, small curves and title blocks. The final guard executes all 147 examples
   for 135 commands/kinds in each unit: 588 executions, with 86 commands/kinds producing
   geometry. Every example succeeds; none is skipped or allowed to fail. New entities are
   checked in every owning block, including dimension and table definitions.
   See the findings below.
3. **Visible defaults.** Registry schemas retain every serde default and describe it.
   Added conventions include window sill/head, 3 m wall height, zero text rotation,
   unwrapped MText width, inscribed polygons, solid hatch fill, custom hatch rotation/scale,
   image brightness/contrast/fade, revision-cloud arc length and empty measurement
   lists/notes. Overkill's omitted tolerance now represents the same physical length in
   every drawing unit. Shared outcome metadata reports omitted defaults, with effective
   drawing-unit lengths for contextual sizes. Batch results report the values each step
   actually used, including after an earlier scale or sheet change.
   `Session::run_input(&Value, Source)` owns raw-input validation and reporting. Its separate
   `Reported` struct holds the plain outcome and metadata; no command or outcome variant
   hides another one. Native Rust callers keep `run`/`run_as`. Batch steps retain their raw
   JSON so actual defaults reflect the session at that step.
   Read-only calculations shared with execution resolve grid extents, room labels,
   viewport fitting, clipboard bases, dimensional constraint expressions, dimension spacing,
   verification tolerances, text styles and pasted text height.
4. **All input errors.** A dependency-free schema walker reports missing fields, wrong
   types, const/enum/range violations and unknown-field warnings with JSON pointers.
   Record creation checks nested entity payloads; record replacement checks all updates
   against the actual record type and serde-required fields before mutation. Existing
   serde diagnostics, schema fragments, examples and spelling suggestions remain.
   Stable automation error codes are listed centrally and exposed by capabilities.
5. **Validate.** `qiyas_read` accepts `op: "validate"` with one command or commands.
   It checks input and earlier-step references without executing and returns `valid`,
   `errors`, `warnings`, `defaults_used` and `not_checked`. The prompts contain only the
   requested short advice to validate a large batch before running it.
6. **Operation needs.** One operation table declares drawing, window and host needs.
   Registry reads, capabilities, record schemas and validation work without a drawing.
   Missing environments return `no_drawing`, `needs_window` or `needs_host`.
7. **Safe MCP retries.** Optional execute `request_id` caches the first result, including
   failures, for the connection's latest 256 ids. Identical retries neither mutate nor
   emit another UI event. Reusing an id for different arguments returns
   `request_id_reused`; concurrent retries wait for the original.
8. **Provider profiles.** Settings store separate models and compatible-service addresses
   per provider, with migration from the old flat structure. Switching providers restores
   its profile; fresh defaults are `claude-opus-5-5` and an empty compatible model.
   A missing model in the service list shows a nonblocking warning. Both providers' model
   errors direct the user to Settings, with English and Arabic UI strings.
9. **File dialogs.** Every frontend file picker goes through `src/fileDialogs.ts`.
   Folder order is the visible saved drawing, the last picked folder, then Tauri's Documents
   folder. Offered filenames remain; cancellation does not overwrite folder history.
   A source guard prevents bypassing the helper, including dynamic and side-effect imports.
10. **Usage and effort.** Reasoning usage accepts both confirmed OpenAI-compatible paths,
    `usage.completion_tokens_details.reasoning_tokens` and `usage.reasoning_tokens`,
    without summing duplicate counters or losing them in a later partial usage event.
    Each actual request, including retries after optional-field rejection, logs its sent
    effort/output controls.

`CLAUDE.md` describes the affected core, drawing, check, layouts, records, AI/MCP, settings
and file-dialog contracts. `CHANGELOG.md` records the user-visible changes under Unreleased.

## Guard findings and fixes

- **Walls:** repeated return vertices previously left an open run at its start. Shared
  normalization now closes the centreline and joined wall outline before wall construction.
- **Grid:** perpendicular corner bubbles collided. Axes now extend one bubble radius past
  their extent before the adjoining bubble, separating the circles.
- **Title block:** its bottom/right edges duplicated the enclosing sheet frame. The title
  grid now shares those edges.
- **Text in grids/title blocks:** generic geometry bounds ignored text alignment and treated
  MText's top attachment like a baseline. Bounds now follow alignment, attachment and rotation.
- **`edit.offset`:** nearby parallel lines were incorrectly called dangling ends. A gap
  requires the endpoint's forward continuation to meet the nearby edge.
- **Small circle, arc, ellipse and donut cases:** display tessellation chords are not CAD
  segments. `tiny_segment` measures the actual curve pieces.
- **Tangent circles/axes:** `overlap` now compares actual straight CAD edges, not sampled
  curve chords that happen to fall within the collinearity tolerance.
- **Commands using layer 0:** at review, `layer_zero` stays a warning (the founder's first
  villa had everything on layer 0). It now applies to entities on a sheet only: inside a block
  definition layer 0 is correct. The guard draws on its own layer, as a careful user does.
- **Dimensions and tables inside definitions:** the checker now follows the owning layout
  to distinguish model-unit geometry from paper millimetres. Text collision checks use
  oriented text boxes. A dimension's intentional extension beyond its dimension line is
  distinguished from a changed endpoint that genuinely overshoots.
- **`set_records` registry example:** its layer lineweight used the nonexistent enum value
  `"W50"`. The example now uses the codec's real representation, `{"Value":50}`; the guard
  executes it without rewriting that payload.

The actual command output fixes are walls, grid and title block. The other findings were
incorrect interpretations by the checker or generic text bounds. These registry entries
triggered those findings:

| Rule or shared calculation | Registry commands/kinds caught |
| --- | --- |
| `dangling_end` | `edit`, `edit.offset` |
| `overlap` | `grid`, `title_block`; small-curve cases also exercise sampled-chord false positives |
| Text bounds and `text_over_lines` | `grid`, `title_block`, `dimension.aligned`, `dimension.angular`, `dimension.diameter` |
| `overshoot` | `add_dim_constraint`, `dimension`, `dimension.aligned`, `dimension.angular`, `dimension.arc_length`, `dimension.baseline`, `dimension.continue`, `dimension.linear`, `dimension.quick` |
| `text_height_vs_scale` and block/layout context | `add_dim_constraint`, `dimension` and every dimension kind, `table`, `edit_cell`, `insert_col`, `insert_row`, `delete_col`, `delete_row`, `merge_cells` |
| `tiny_segment` | Harder circle, arc, ellipse and donut cases |
| `layer_zero` | `att_def`, `create_block`, `dimension` and every dimension kind; `draw`, `draw.arc`, `draw.circle`, `draw.donut`, `draw.ellipse`, `draw.hatch`, `draw.line`, `draw.mline`, `draw.point`, `draw.polygon`, `draw.polyline`, `draw.ray`, `draw.rectangle`, `draw.revcloud`, `draw.sketch`, `draw.solid2d`, `draw.spline`, `draw.three_point_arc`, `draw.trace`, `draw.wipeout`, `draw.xline`; `edit`, `edit.blend`, `edit.break`, `edit.chamfer`, `edit.fillet`, `edit.offset`; `field`, `insert_block`, `leader`, `mleader`, `modify.array_polar`, `modify.array_rect`, `modify.copy`, `modify.explode`, `modify.mirror`, `mtext`, `paste_from_clipboard`, `table`, `text`, `title_block`, `tolerance` |

The schema guard also found that inlining a referenced `WallSpec` replaced its properties
with the sibling `type` property. Inlining now merges properties and required lists, so
the published wall schema includes all inputs and their defaults.

## Limits and assumptions

- Defaults depending on geometry, existing records, or earlier batch changes to sheet/scale
  cannot all be predicted without execution; validation names those limits in `not_checked`.
  Execution validates resolved batch inputs and reports actual defaults.
- Wall plan dimensions use drawing units; vertical wall/opening dimensions retain the
  existing metres contract. The new omitted wall-height convention is 3 m.
- Text width remains the existing font-independent estimate; attachment and placement are
  corrected. Live model behaviour and external service token accounting were not exercised.
- No API keys or real user settings were read or written, no dependencies were added, and
  no window/browser, release build, commit, staging or push was used.

## Initial verification before review corrections

All Cargo checks used `CARGO_BUILD_JOBS=1 nice -n 19`, and compilation/tests ran one
heavy command at a time. Vitest used `--maxWorkers=1`. Passing checks were not repeated
without a subsequent relevant change.

| Requested check | Actual result |
| --- | --- |
| `cargo fmt --all` | Passed, including final formatting after all Rust edits. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed after fixing an unnecessary unwrap in the new guard. Repeated after the final fixture/diagnostic edits: passed, no warnings. |
| `cargo test --offline -p qiyas-geom -p qiyas-core` | Full run with `--no-fail-fast`: 709 passed, 3 failed. After fixing two lost record diagnostics and unwrapping input metadata in the command-line test, the failed command-line test and all 9 record tests passed. The changed record-validation file passed all 8 tests. At that stage, 713 core/geometry tests were covered by passing results across those runs. |
| `cargo test --offline -p qiyas-ai --lib --test surface --test drafting_eval` | Library: 55 passed, 1 sandbox port failure. The two remaining targets were then run separately: 3 drafting evaluation tests passed; surface initially had 30 pass and 1 fail. Fixing warnings on a failed referenced batch step made that exact test pass. |
| `cargo test --offline -p qiyas-desktop` | With `--no-fail-fast`: 98 passed, 1 sandbox port failure. Settings migration tests use a mock keychain. |
| `node_modules/.bin/tsc --noEmit` | Passed. |
| `node_modules/.bin/vitest run --maxWorkers=1` | Initial run: 805 passed, 5 failed across 108 files. App/plot fixtures were updated for the shared dialog's drawing lookup; the toast timing failure was retried once. Only the two failed files were rerun: all 50 tests passed. After strengthening the import guard, its 7 tests passed separately. |

The expanded strict registry guard passed after removing every fixture refusal and fixing
the lineweight example. The new contradictory-input error-code unit test also passed.
Together the AI checks cover 90 passing tests and the one blocked port test. The frontend's
810 tests are covered by passing results across the initial run and the two-file retry;
the original full Vitest command itself exited with failures, not zero.
Four focused CLI tests also passed: script execution/export, project save behaviour, script
errors before execution, and refusal of colliding output paths. `git diff --check` passed.
After retaining the old MCP message assertions alongside the new code assertions, formatting
and `cargo clippy --offline -p qiyas-ai --test mcp -- -D warnings` both passed. Its port-based
runtime test remains with the reviewer.

The sandbox failures were `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
in qiyas-ai and `commands::tests::the_mcp_server_follows_the_settings` in qiyas-desktop:
both attempted to bind a local port and received permission denied. The other MCP/provider
integration tests requiring local ports were left for the reviewer, as requested. MCP retry
cache unit tests do not need ports and passed.

Earlier compile attempts caught Rust type inference, wrapper pattern matches and a generic
serde recursion issue; these were fixed before the complete test run. Intermediate guard
failures led to the fixes listed above and corrections to physical scaling and fixture setup.
They are not presented as passing runs.

Existing tests retain their assertions. Point-run constructors use `PointRun`; native command
and outcome matches are direct. Expectations that deliberately changed are the grid's
extended endpoints (to separate corner bubbles), file-dialog starting folders and extra drawing
lookup, and the automation error envelope's new stable code. Real gap, overlap and dimension
overrun regressions remain checked.

Specifically, `grid_defaults_use_the_sheet_and_scale_and_allow_a_custom_bubble_radius`
now expects the axis to extend by one bubble radius; the untitled-save test in
`src/app/useDrawings.test.tsx` expects an absolute Documents path instead of only a filename.
App open/plot/script tests include the helper's `drawing_info` lookup. AI/MCP error tests
read the same messages inside the new JSON envelope and additionally assert stable codes.


## Review corrections

- Removed the hidden input and outcome enum variants and custom command deserialization.
  JSON commands deserialize to their actual variants, including `Command::Batch`.
- `input.rs` defines the separate `Reported { outcome, defaults_used, warnings }` result.
  `Session::run_input` validates raw JSON, keeps serde field diagnostics, resolves omitted
  defaults against the current session and executes the command. Its JSON representation
  preserves outcome fields and step metadata without changing the native enums.
- JSON execution in the AI/MCP surface, desktop IPC, script lines and typed commands uses
  that boundary. The command-line machine retains completed raw input through `take_input`;
  script steps keep raw input separately from their plain command. Native Rust callers
  continue to use `run`/`run_as` and directly match outcomes.
- Batch input is checked before changes, then each step resolves defaults from its original
  JSON when it runs. Errors still roll back the transaction. Added regressions cover direct
  enum matching, omitted versus explicit defaults in four units, and invalid inputs leaving
  entities, undo/redo history and revision unchanged.
- Agent tests now check the actual `REQUEST …; sent: …` log format and structured budget
  code/message. Authoring tests accept structured input errors while retaining field-name
  assertions. The formerly required revision-cloud arc length test now checks its accepted
  default and still refuses a supplied invalid value.
- Reviewer changes are retained: layer 0 remains a warning on sheets, block contents are
  exempt, the drawing guard/layout fixtures use their own layers, and the agent's up-front
  need check never locks the drawing on the async event thread. Drawing availability is
  checked with the session lock off that thread.

All correction checks use one heavy process at a time; every Cargo invocation has
`CARGO_BUILD_JOBS=1 nice -n 19`. No passing test target was rerun.

| Correction check | Actual result |
| --- | --- |
| `cargo fmt --all` | Passed; formatting also passed after the final CLI test fixture correction. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed. Two earlier attempts caught a collapsible CLI match and a leftover test dereference; both were corrected. The later CLI fixture-only correction compiled and passed its focused test; the passing workspace Clippy check was not repeated. |
| `cargo test --offline -p qiyas-core` | Passed: 646 tests, no failures or ignored tests, including the own-output guard and new boundary regressions. |
| `cargo test --offline -p qiyas-ai --lib --test surface --test drafting_eval --test authoring` | With `--no-fail-fast`: 106 passed, one local-port sandbox failure. Authoring: 16 passed, including `a_batch_is_one_step_and_fails_whole`; surface: 31 passed; drafting evaluation: 3 passed; library: 56 passed, 1 failed. |
| `cargo test --offline -p qiyas-desktop -p qiyas-cli` | With `--no-fail-fast`: 117 passed, 2 failed. Desktop had 100 passes and one local-port sandbox failure. CLI had 17 passes and a new-test fixture mismatch; the fixture was corrected without changing any assertion. Its focused retry passed (1 test, 17 filtered out). Across these runs, 118 tests passed and one sandbox port test remains blocked; the original full command exited with failures. |
| `node_modules/.bin/tsc --noEmit` | Passed. |

The sandbox failures are `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
in AI and `commands::tests::the_mcp_server_follows_the_settings` in desktop: their local-port
binds returned `PermissionDenied` / `Operation not permitted`. Neither was retried. The port-opening agent and MCP integration suites remain for the reviewer;
Clippy compiled their updated assertions.

The new CLI regression originally reused a bare codec drawing with annotation scale 1:1,
which correctly reported a 0.0025 m text height. The test expected the new-drawing template's
1:100 convention (0.25 m). Its fixture now explicitly creates and saves that template;
production code and all assertions stayed unchanged. The exact-test retry passed; it rebuilt
the selected target's dependency configuration and took 17 minutes, with no other heavy
command running alongside it. The slow existing CLI damaged-drawing
test passed in the full run and was not retried.

A repository search found **zero** `.bare()` / `into_bare()` calls, zero corresponding helper
methods and zero hidden command/outcome variants. `git diff --check` passed. The specified
command JSON boundaries have no unresolved caller. Caller decision: legacy tool adapters in
`execute.rs`, and the `check`/`rooms` query adapters, build native Rust `Command` values and
keep `run_as`; they return their existing reports without omitted-input metadata. Registry
command JSON uses `run_input`. Validation-only reads use the same schema walker without
executing.

## Touched files

Files below include the original implementation and the review corrections. Mechanical caller/test
adaptations now concern `PointRun`; commands and outcomes remain directly matchable enums.
No dependency manifest or lockfile changed.

```text
CHANGELOG.md
CLAUDE.md
crates/qiyas-ai/src/agent.rs
crates/qiyas-ai/src/anthropic.rs
crates/qiyas-ai/src/errors.rs
crates/qiyas-ai/src/execute.rs
crates/qiyas-ai/src/host.rs
crates/qiyas-ai/src/lib.rs
crates/qiyas-ai/src/mcp.rs
crates/qiyas-ai/src/openai.rs
crates/qiyas-ai/src/prompt_ar.txt
crates/qiyas-ai/src/prompt_en.txt
crates/qiyas-ai/src/provider.rs
crates/qiyas-ai/src/surface.rs
crates/qiyas-ai/src/window.rs
crates/qiyas-ai/tests/agent.rs
crates/qiyas-ai/tests/authoring.rs
crates/qiyas-ai/tests/mcp.rs
crates/qiyas-ai/tests/surface.rs
crates/qiyas-ai/tests/vision.rs
crates/qiyas-cli/src/main.rs
crates/qiyas-cli/tests/cli.rs
crates/qiyas-core/src/check.rs
crates/qiyas-core/src/clipboard.rs
crates/qiyas-core/src/cmdline/mod.rs
crates/qiyas-core/src/commands.rs
crates/qiyas-core/src/comment.rs
crates/qiyas-core/src/conventions.rs
crates/qiyas-core/src/dimension.rs
crates/qiyas-core/src/draw.rs
crates/qiyas-core/src/error.rs
crates/qiyas-core/src/input.rs
crates/qiyas-core/src/input_context_defaults.rs
crates/qiyas-core/src/layouts.rs
crates/qiyas-core/src/lib.rs
crates/qiyas-core/src/modify2.rs
crates/qiyas-core/src/param.rs
crates/qiyas-core/src/point_run.rs
crates/qiyas-core/src/record_edit.rs
crates/qiyas-core/src/registry.rs
crates/qiyas-core/src/rooms.rs
crates/qiyas-core/src/script.rs
crates/qiyas-core/src/session.rs
crates/qiyas-core/src/table.rs
crates/qiyas-core/src/validation.rs
crates/qiyas-core/src/verify.rs
crates/qiyas-core/src/viewports.rs
crates/qiyas-core/src/walls.rs
crates/qiyas-core/tests/autosave.rs
crates/qiyas-core/tests/autosave_recovery.rs
crates/qiyas-core/tests/batch.rs
crates/qiyas-core/tests/blocks.rs
crates/qiyas-core/tests/boq.rs
crates/qiyas-core/tests/check.rs
crates/qiyas-core/tests/cmdline.rs
crates/qiyas-core/tests/comment.rs
crates/qiyas-core/tests/context_defaults.rs
crates/qiyas-core/tests/conventions.rs
crates/qiyas-core/tests/dimension.rs
crates/qiyas-core/tests/draw.rs
crates/qiyas-core/tests/edit.rs
crates/qiyas-core/tests/entity_geometry.rs
crates/qiyas-core/tests/image.rs
crates/qiyas-core/tests/layouts.rs
crates/qiyas-core/tests/measure.rs
crates/qiyas-core/tests/modify2.rs
crates/qiyas-core/tests/own_output.rs
crates/qiyas-core/tests/param.rs
crates/qiyas-core/tests/pdf.rs
crates/qiyas-core/tests/point_runs.rs
crates/qiyas-core/tests/project.rs
crates/qiyas-core/tests/record_validation.rs
crates/qiyas-core/tests/records.rs
crates/qiyas-core/tests/save.rs
crates/qiyas-core/tests/script.rs
crates/qiyas-core/tests/session.rs
crates/qiyas-core/tests/support/own_output_records.rs
crates/qiyas-core/tests/units.rs
crates/qiyas-core/tests/validation.rs
crates/qiyas-core/tests/verify.rs
crates/qiyas-core/tests/xref.rs
crates/qiyas-desktop/src/ai.rs
crates/qiyas-desktop/src/cmdline.rs
crates/qiyas-desktop/src/commands.rs
crates/qiyas-desktop/src/drawing_host.rs
crates/qiyas-desktop/src/mcp.rs
crates/qiyas-desktop/src/script.rs
crates/qiyas-desktop/src/settings.rs
crates/qiyas-desktop/tests/settings.rs
crates/qiyas-geom/src/flatten.rs
crates/qiyas-geom/src/lib.rs
crates/qiyas-geom/src/path.rs
crates/qiyas-plot/examples/phase4_check.rs
crates/qiyas-plot/tests/plot.rs
docs/2026-10-21-mizan-core-rules-review.md
src/ai/AssistantPanel.test.tsx
src/ai/AssistantPanel.tsx
src/ai/useAssistant.ts
src/api.ts
src/app/App.test.tsx
src/app/Dialogs.tsx
src/app/useBlocks.ts
src/app/useDrawings.test.tsx
src/app/useDrawings.ts
src/app/useLayers.ts
src/app/usePlot.test.ts
src/app/usePlot.ts
src/app/useTakeoff.ts
src/fileDialogs.test.ts
src/fileDialogs.ts
src/i18n/ar.json
src/i18n/en.json
src/shell/AliasEditor.test.tsx
src/shell/CustomizeUi.test.tsx
src/shell/ModelPicker.test.tsx
src/shell/ModelPicker.tsx
src/shell/OptionsDialog.test.tsx
src/shell/SettingsDialog.test.tsx
src/shell/SettingsDialog.tsx
```
