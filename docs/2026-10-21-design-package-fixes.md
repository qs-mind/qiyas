# Design-package automation corrections — 21 October 2026

Implementation report for the twelve issues from the live Gemini 3.8 Flash conversation
beginning at 17:19:17 in `~/.config/com.qsmind.qiyas/logs/agent.log`, against baseline
`e2afefe`. The log was read only. Changes remain uncommitted. No dependencies were added.
The rules live in shared command, document, validation and desktop ownership code; only the
requested metadata and schedule-authoring instructions were added to the bilingual prompts.

## 1. Annotation circles are not features

`missing_center_mark` now excludes a circle containing a nonempty TEXT or MTEXT insertion
on the same sheet, regardless of the text's layer. Grid writes `QIYAS_GRID` provenance on its
bubbles, so deleting a grid label or changing its layer does not turn the bubble into a hole.
Plain circles remain subject to the rule. This avoids treating tags and reference bubbles as
machined or drilled features without relying on a project-specific layer name.

Coverage: `labelled_circles_and_grid_bubbles_are_annotations_but_plain_holes_need_marks`
checks grid output, removed grid labels, a tag on another layer and a plain hole in mm, cm,
m and inches.

## 2. Surface errors show the accepted form

Every surface operation has an argument contract in `qiyas-ai/src/surface_args.rs`, checked
by the same core schema walker used for commands. Routing validates the tool envelope and
decoded arguments before dispatch, including operations handled by the window or desktop
host. Structured failures include JSON pointers, schema and example. Capabilities expose
the operation contracts.

The reported failures now explain `detail: "summary" | "full"`, nested bounds
`[[x1, y1], [x2, y2]]`, and that several commands use `op: "batch"`. Nested commands keep
registry validation and batch-reference handling.

`record_schema` now covers layouts, viewports, block records, dimension styles, linetypes,
named views, take-off measurements, schedules, BOQ items and conflicts in addition to its
existing entities/layers/styles/header coverage. Unsupported collection schemas, including
groups and layer states, explicitly list supported names and direct callers to
`records` with `detail: "full"` instead of giving an unexplained unknown-type refusal.

Coverage: the contract unit test covers every published operation; surface tests cover
pointer/form/example errors and both supported and explicitly unsupported record schemas.

## 3. Wall-opening defaults are resolved

`add_wall_openings` resolves defaults from the selected wall run before reporting them:
window sill 0.9 m, door sill 0 m, and head `min(2.1 m, wall height)`. `edit_wall_opening`
reports properties retained when omitted, with physical units; removal has no implicit
size defaults. The existing `walls` defaults remain the same. All callers use the shared
context-dependent defaults rather than null placeholders.

Coverage: `opening_commands_report_resolved_or_retained_defaults_in_every_unit` exercises
add, edit and remove in mm, cm, m and inches, including a wall lower than 2.1 m.

## 4. New drawings have sized default layouts

`Drawing::blank` initializes paper layouts from the drawing's unit template: ISO A3
landscape, 420 × 297 mm, for metric/unknown units; ANSI B landscape, 431.8 × 279.4 mm, for
inches and feet. Initialization does not add an undo step.

**Existing-drawing decision:** opening an existing drawing does not silently assign paper
to an unsized layout. The new registry command `set_layout_paper {name, paper, orientation}`
sizes or resizes it explicitly in one undo step without changing the selected sheet or its
entities. Errors requiring sized paper name both `new_layout` and `set_layout_paper`, and
explain selecting the result with `set_sheet`.

Validation's `not_checked` now names the active layout, paper size and whether annotations
or viewport frames fit the paper. Schema validation does not claim to certify these
drawing-dependent conditions.

Coverage: template paper across metric and imperial units; an imported unsized layout
remaining unsized; useful refusal and validation caveat; explicit sizing, undo/redo and
DWG/DXF persistence.

## 5. Generated sheet annotations own their layers

`title_block` defaults to `A-ANNO-TTLB`, accepts an explicit `layer`, and creates a missing
layer in the same undo step as its entities. It preserves the caller's current layer.
New viewports created while current layer is 0 use non-plotting `A-VIEW`; an explicitly
chosen different current layer remains respected. Viewport undo now includes layer-table
changes as well as the viewport entity.

Coverage: default and overridden title layers, current-layer preservation, complete undo
and redo; viewport creation and removal of its automatically created layer in one step.

## 6. Dates and project data are not invented

`qiyas_read state` includes the computer's current local ISO date as `local_date`.
TitleBlock still draws only supplied values. Its result is now
`TitleBlock {handles, empty_fields}`, listing omitted/empty standard fields and supplied
empty extra fields. Script summaries also expose the empty-field list.

Both English and Arabic prompts require title-block/project metadata from the user or
drawing, leaving and listing other fields in the answer. They do not authorize automatically
filling the date just because the state exposes it. The existing prompt-length check remains
in force.

Coverage: state date and paper-unit assertions; empty-field reporting; existing tests that
an empty title block never supplies a date, project or author remain present.

## 7. Literal arithmetic is identified as untied verification

Each verification result includes `tied_to_drawing`. Checks using literal points without
drawn targets are false, even if their arithmetic passes. Entity-target checks and counts
that inspect drawing entities are true. The outcome includes `untied_checks` and a sentence
stating how many checks prove nothing about the drawing. Individual untied results carry
the same explanation.

Coverage: a passing 3–4–5 literal-distance check remains arithmetically valid but is untied;
a drawn-line check and filtered entity count are tied, and the overall untied count is one.

## 8. Rooms have one persistent source of area

`label_spaces` retains each found outer boundary as an ordinary closed polyline on
non-plotting `A-ROOM-BOUNDARY`. `QIYAS_ROOM` xdata stores the seed, name, island policy,
islands and exact bulges. Labels use live `QiyasRoomArea` fields and identify their boundary;
the room report exposes `boundary_handle`.

`room_schedule {at, source_sheet?, labels?, decimals?}` creates a drawing table with live
area fields and a total. It reads the chosen retained rooms, defaults its source to Model,
and uses `A-ANNO-TABL` when the current layer is 0. `measure_room` accepts exactly one live
label/boundary handle or seed point, measures the same canonical boundary, and links the
take-off row to it. A new seed can retain its drawn closed region without a separate label.

Drawing mutations retrace retained rooms from their seeds, excluding retained room outlines
from boundary discovery. Boundary, label, schedule and take-off updates participate in the
originating command's undo step. The construction rule `room_area_mismatch` checks linked
labels, schedule cells/totals and take-off values against the room, including missing or
opened enclosures.

**Room design decisions:** ordinary polylines and xdata provide durable DWG/DXF storage;
there is no new external database or dependency. Repeated labelling reuses the retained
room, and inconsistent island policies are refused. A room schedule retains the set of rooms
selected when it was created; its fields follow their edited boundaries. Room lookup is
seed-based: when an edit leaves that seed without a closed enclosure, fields show `####`,
the take-off is explicitly marked stale, and the check reports it instead of using the old
area as if it were current.

The existing **“Unapproved candidate …”** wording and approval status remain. These are
live candidate quantities, not a claim that a qualified engineer approved the room or its
measurement convention. Previously typed, unlinked text is not silently converted into a
room object.

Coverage: common labels/schedule/take-off values across units; following wall and boundary
edits; undo/redo; islands and DWG/DXF round trips without a project sidecar; deliberate stale
label/cell/row findings; opened enclosure invalidation; multiple-room totals.
The repeated DWG round-trip test also checks the persisted boundary before any command can
refresh it. Changed boundaries discard their old whole-entity DWG payload and cached graphics;
changed room metadata discards only that application's old EED payload, preserving other apps.

## 9. Opening schedules read wall systems

`opening_schedule {at, source_sheet?}` / `OPENINGSCHEDULE` creates an ACAD_TABLE on the
current sheet from wall opening inserts on the source sheet, Model by default. Rows group
by kind, width and actual `head - sill` height. Dimensions are physical millimetres. Size
marks such as `D-900x2100` are persisted as `QIYAS_OPENING_MARK` xdata on the openings and
returned by `wall_groups`, so plan tags can use the assigned marks.

Count cells and the total are `QiyasOpeningCount` fields linked to the source sheet handle.
The schedule retains its source in xdata. Drawing edits refresh size grouping, assigned
marks and counts, including adding rows for newly introduced sizes, within the edit's undo
step. Placement and column widths are retained. Both prompts require drawing-generated
schedules and quantities from measurements/tool results rather than typed quantities.
On layer 0 the table uses `A-ANNO-TABL`, created in its undo step; the caller's current layer
is preserved. Initial cell sizes use displayed values instead of the longer field expressions.

Coverage: 27 openings with actual door/window heights; size edits; added/removed openings;
new groups; undo/redo; saved fields and marks in DWG and DXF; editing after reopen, across
mm, cm, m and inches.

## 10. Layout captions and reported sizes use paper units

Picture and screen captions distinguish model drawing units from layout paper millimetres.
Pixel size on a layout is reported directly in paper millimetres, without multiplying by
the model drawing's metres-per-unit factor. State exposes `sheet_units` and physical
`paper`; its default 2.5 mm annotation height is 2.5 in layout coordinates. Surface measures
and boundary/near queries use the layout's paper conversion while retaining model
`metres_per_unit` for model geometry.

Coverage: the surface state/capture test uses a metre drawing on a paper layout and checks
date, annotation height, paper size, caption units and measured paper distances.

## 11. Viewport frames can be calculated and resized

`mview` accepts either its existing paper corners or a model window:

```json
{
  "type": "mview",
  "model_bounds": [[0, 0], [10, 8]],
  "scale": "1:100",
  "paper_centre": [210, 148.5]
}
```

Model bounds use the drawing's units; `paper_centre` uses paper millimetres. Qiyas computes
the paper rectangle and model centre from the requested scale. Supplying
`target: {"handle": ...}` resizes that existing viewport, preserving its handle and
per-viewport settings instead of requiring erase/redraw. The two input forms are checked
together, and resolved frame defaults are reported. `x-prompts.required` preserves the
existing two-corner questions for the typed `MVIEW` command.

Coverage: physical frame sizes and centres in mm, cm, m and inches at 1:50 and 1:100;
same-handle resizing, entity-count stability, undo/redo, invalid form pointers and typed
command prompts.

## 12. The window cannot take sheet ownership during Mizan's turn

**What the log proves:** the title-block result at 17:42:26 reports `*Paper_Space1`.
The `set_records` result at 17:42:33 already reports `ui.sheet: "*Model_Space"`; the
17:42:39 `mview` refusal is a consequence, not the first evidence of the switch. The
earlier `new_layout`/subsequent state discrepancy is also present. The log does not record
window IPC, so it cannot identify which exact window event caused either historical switch.
More precisely, the 17:42:33 result retains `ui.action: "reveal"`: `run_one` snapshots the
sheet before execution and `ViewState::update_ui` (`qiyas-ai/src/surface.rs:1353`) would
return `show_sheet` if the command changed it. The session
was therefore already on Model before `set_records`, placing the switch between 17:42:26
and 17:42:33. Editing paper-space entities by handle does not require that sheet to be active.

**Proven exposed write path:** `src/app/useSheets.ts:408` (`chooseSheet`) calls
`setSheet(block)` at line 411. Before this change, `crates/qiyas-desktop/src/commands.rs:166`
(`set_sheet`) wrote `Session::set_sheet` without checking the active assistant turn.
Queued or concurrent window sheet selections could therefore overwrite an assistant-selected
sheet. Window `run_command` with SetSheet, nested batches and history operations offered
additional ways to change sheet state.

This is distinct from accusing rendering or the current AI-event handlers of an observed
write. `Session::with_sheet` at `crates/qiyas-core/src/session.rs:1987` takes `&self` and
renders another sheet without assigning `Session::sheet`. `applyAiUi` in
`src/app/useAssistantWiring.ts:107` mirrors show/reveal events locally; it does not call the
sheet-selection IPC. The triggering historical window event remains unobserved.

The fix guards desktop sheet writes with the assistant-turn mutex through the write itself.
Direct sheet IPC, window SetSheet commands (including batches/scripts), undo, redo and
assistant-undo reject window writes while the turn is owned. Mizan's own session calls remain
able to select sheets. `useAssistantWiring` follows refreshed core sheet information locally;
`useSheets.chooseSheet` reads the actual core selection back after an accepted request.
MCP had another unguarded route to the same shared session (`qiyas-ai/src/mcp.rs:283` now
enters the guard; `qiyas-desktop/src/drawing_host.rs:134` holds the real turn mutex).
Its sheet-changing operations
(including nested/typed commands and view/undo operations) and drawing switches now acquire
the host's same turn guard through `DrawingHost::with_sheet_authority`. Read and ordinary
geometry operations remain available. User tab activation/closing takes the turn lock before
tabs/session locks, preserving conversation cancellation and avoiding the opposite lock order.

Coverage: desktop IPC tests reserve a turn, select a layout as the assistant, attempt each
window write path, and assert the layout remains selected; rendering Model also leaves it
selected. Window selection works after the turn is released. The React test asserts
show-sheet/reveal events never send sheet selection back to the core.
MCP tests cover all accepted sheet-changing forms and continued read/geometry access; the
desktop host test checks that its real turn mutex remains held across new/switch/close calls.

## Existing tests changed and why

No existing assertion was dropped merely to accept a failure. The behavioral/API updates are:

- `qiyas-ai/tests/drafting_eval.rs::villa_ground_floor_through_mizans_three_tools` previously
  required every entity added by `label_spaces` to be on `A-ANNO-TEXT`. It now separately
  asserts label handles on that layer and retained boundary handles on `A-ROOM-BOUNDARY`;
  the command intentionally adds both kinds of entity. Its before/after room equality now
  separately validates the newly added `boundary_handle` and still compares every previous
  geometry/quantity field. Sheet entity assertions expect title-block entities on
  `A-ANNO-TTLB`, while preserving the explicitly chosen viewport layer.
- The `added` helper in `qiyas-core/tests/layouts.rs` accepts the new `Outcome::TitleBlock`
  as well as `Added`. Existing title-block content, geometry and undo assertions remain.
  Native TitleBlock constructors supply the new layer field explicitly.
- Native MView constructors in core layout/viewport/record tests, AI vision tests and plot
  tests add `None` for the new optional fitting/target fields; their old behavior assertions
  remain unchanged.
- The verification helper in `qiyas-core/tests/verify.rs` matches `Verified {results, ..}`
  because the outcome now also reports the untied count and explanation.
- `qiyas-core/tests/own_output.rs` scales the new `model_bounds` example coordinates and
  prepares real room geometry/labels for the new room commands. The existing drawing-output
  checks remain enabled for the registry examples.

New regression tests described under each item supplement those existing checks. The
bilingual prompt-length limit remains unchanged.

## Files touched

Documentation:

- `CLAUDE.md`
- `docs/2026-10-21-design-package-fixes.md`

AI implementation and tests:

- `crates/qiyas-ai/src/execute.rs`
- `crates/qiyas-ai/src/host.rs`
- `crates/qiyas-ai/src/lib.rs`
- `crates/qiyas-ai/src/mcp.rs`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/prompt_en.txt`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/src/surface_args.rs` (new)
- `crates/qiyas-ai/src/window.rs`
- `crates/qiyas-ai/tests/drafting_eval.rs`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-ai/tests/vision.rs`

Core implementation:

- `crates/qiyas-core/src/check.rs`
- `crates/qiyas-core/src/cmdline/mod.rs`
- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/conventions.rs`
- `crates/qiyas-core/src/draw.rs`
- `crates/qiyas-core/src/drawing.rs`
- `crates/qiyas-core/src/field.rs`
- `crates/qiyas-core/src/input_context_defaults.rs`
- `crates/qiyas-core/src/layouts.rs`
- `crates/qiyas-core/src/lib.rs`
- `crates/qiyas-core/src/opening_schedule.rs` (new)
- `crates/qiyas-core/src/record_edit.rs`
- `crates/qiyas-core/src/room_label_ar.txt` (new; unchanged Arabic label wording)
- `crates/qiyas-core/src/room_objects.rs` (new)
- `crates/qiyas-core/src/rooms.rs`
- `crates/qiyas-core/src/script.rs`
- `crates/qiyas-core/src/session.rs`
- `crates/qiyas-core/src/validation.rs`
- `crates/qiyas-core/src/verify.rs`
- `crates/qiyas-core/src/viewports.rs`
- `crates/qiyas-core/src/walls.rs`

Core and plot tests:

- `crates/qiyas-core/tests/check.rs`
- `crates/qiyas-core/tests/cmdline.rs`
- `crates/qiyas-core/tests/layouts.rs`
- `crates/qiyas-core/tests/opening_schedule.rs` (new)
- `crates/qiyas-core/tests/own_output.rs`
- `crates/qiyas-core/tests/records.rs`
- `crates/qiyas-core/tests/room_objects.rs` (new)
- `crates/qiyas-core/tests/verify.rs`
- `crates/qiyas-core/tests/viewports.rs`
- `crates/qiyas-core/tests/walls.rs`
- `crates/qiyas-plot/tests/plot.rs`

Desktop and React:

- `crates/qiyas-desktop/src/commands.rs`
- `crates/qiyas-desktop/src/drawing_host.rs`
- `crates/qiyas-desktop/src/script.rs`
- `crates/qiyas-desktop/src/tabs.rs`
- `src/api.ts`
- `src/app/useAssistantWiring.test.tsx`
- `src/app/useAssistantWiring.ts`
- `src/app/useSheets.ts`

## Required checks

Each requested command was invoked once. Every Cargo invocation uses `CARGO_BUILD_JOBS=2`;
Vitest uses `--maxWorkers=2`. `scripts/check.sh` was not run. Targeted recovery checks are
listed separately below; full results are retained in `/tmp/qiyas-package-*.log`.

| Command | Actual result |
| --- | --- |
| `CARGO_BUILD_JOBS=2 cargo fmt --all` | Passed (exit 0); late edits formatted with direct rustfmt. |
| `CARGO_BUILD_JOBS=2 cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed (exit 0). |
| `CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-geom -p qiyas-core` | Exit 101 before tests: linker was killed with signal 9 while linking example `write_test_plan`. Recovery below passed all library/integration tests. |
| `CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-ai --lib --test surface --test drafting_eval --test authoring` | Exit 101: 62 library tests passed; the existing unreachable-service test could not bind its local port (`PermissionDenied`). Cargo stopped before integrations; supplemental results below. |
| `CARGO_BUILD_JOBS=2 cargo test --offline -p qiyas-desktop -p qiyas-cli` | Exit 101: CLI 18 passed; desktop 76 passed, one existing MCP-port test was sandbox-blocked, and the new sheet-ownership test exposed a missing `redo` registration in its mock IPC. The fixture was fixed and that test now passes. |
| `node_modules/.bin/tsc --noEmit` | Passed (exit 0). |
| `node_modules/.bin/vitest run --maxWorkers=2` | Passed: 109 files, 820 tests; exit 0. |

Additional verification:

- Preliminary `cargo check --offline --workspace --all-targets`: passed.
- Core/geometry recovery: `cargo test --offline -p qiyas-geom -p qiyas-core --lib --tests --no-fail-fast`: passed, 754 tests in 69 suites.
- After the linker kill, Cargo kept two build jobs and used a temporary `cc` wrapper passing `-Wl,--threads=2`. No build configuration or dependency was changed. This also applies to subsequent Rust tests.
- Core/geometry example targets, including `write_test_plan`, subsequently built and passed; their doc-test targets passed (no doc tests).
- Clippy of the changed core/AI/desktop packages and all their targets also passed after the final MCP/tab ownership changes (`qiyas-package-clippy-ownership.log`).
- Final room/opening persistence and behavior suites: 7 passed, including edits between two DWG/DXF round trips and inspection before refresh.
- Final core annotation/registry/room/opening check: 21 passed across `context_defaults`, `conveniences`, `opening_schedule`, `own_output` and `room_objects`. A preceding attempt named the nonexistent target `rooms`; it ran no tests and was corrected to the actual target names.
- Final AI run with `--no-fail-fast`: 63 library tests passed, including the MCP authority regression; the only failure remains `models::tests::an_unreachable_service_falls_back_to_the_builtin_list` at its local-port bind. Surface 37, drafting evaluation 4 and authoring 16 all passed (`qiyas-package-ai-final-verified.log`).
- The new desktop sheet-ownership regression passed after registering the production `redo` command in the mock IPC.
- Final desktop library run: 78 passed, including window ownership, actual host-lock lifetime, tab and script behavior. Only `commands::tests::the_mcp_server_follows_the_settings` failed at its local-port bind (`PermissionDenied`; `qiyas-package-desktop-final-verified.log`). CLI's 18 tests passed in the required run.
- Integration recovery also caught and corrected a surface unknown-field warning regression, the expected new room-boundary/title-layer assertions, and a new warning test's missing room seed. No existing failure was hidden by deleting an assertion.
- Final source formatting check passed for all 45 changed Rust files. English/Arabic prompts contain 840/748 words, below the unchanged 900-word limit.
- `git diff --check`: passed.

## Remaining limitations and decisions

- The exact historical window event for item 12 cannot be established from the available
  agent log; the writable boundary and the ownership regression are identified and guarded.
- Existing unsized paper and unlinked room text are preserved on open; migration is explicit.
- Retained rooms remain unapproved candidates and become invalid when their seed loses its
  enclosure. The new schedule/measurement commands do not grant engineering approval.
- Record schemas for groups, layer states and the other explicitly unsupported collections
  still direct callers to full records and name the supported schemas, as allowed by item 2.
- The two existing local-port tests need the reviewer's environment; they were run and their
  sandbox failures are preserved, not removed or changed to pass.
