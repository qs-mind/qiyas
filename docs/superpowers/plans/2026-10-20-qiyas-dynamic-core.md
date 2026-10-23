# Qiyas becomes dynamic: a self-describing core that Mizan (and later the whole app) drives

## Context

Kareem tried Mizan on a blank drawing (`~/Work/Untitled-2.dwg`): "a villa: ground floor, first floor, roof, landscape, sections, elevations, details, legends, tables, as a senior architect in Riyadh".

**What came back.** It "draws like a baby":
- boxes without doors, windows or wall openings, and wall lines crossing each other;
- labels showing `\n` and spilling over walls, and room sizes that don't match the drawing;
- a title block drawn in Model, "SBC compliant" claims it made up, and no layouts;
- no self-checking, no layer control, and drawing in bulk rather than live.

Separately, his drawing "vanished" after he changed the QIYAS-AREA layer.

**His ruling:** *"Why do I have static tools? Why don't I work like Open CAD Studio? I don't want hardcoded tools. I want a fully adaptive and dynamic app like Open CAD Studio."* Scope: **the whole app, AI first.**

### Diagnosis (agent.log lines 2397–3155; the DWG converted and rendered)

- **The root cause is the architecture: 57 hand-written AI tools, each wrapping one slice of the core.** Most core commands (layers, properties, dimension styles, break/join, blocks, viewports, ~70 of ~130) are simply unreachable. Each new feature needs new AI code. Conveniences hide the core (layers created white, dimensions forced onto layer 0).
- **How OCS does it:** the AI uses four generic tools over a self-describing core:
  - a live command list with examples;
  - any command run by its command line;
  - every object readable as a record with a generated schema;
  - any property editable through type-checked JSON-pointer edits;
  - a real screen capture.

  Everything a person can do, the AI can do, with no per-feature AI code. (Read for ideas only: OCS is GPL; no code is copied.)
- **The model made it worse:** Gemini 3.8 Flash through Runware.
  - It invented compliance claims and project data, and got its own room sizes wrong.
  - It looked once at unreadable text and declared it done.
  - A Qiyas bug made it worse: `effort` and the output-token limit are never sent to OpenAI-compatible providers (`openai.rs`).
- **The vanished drawing:** QIYAS-AREA is empty, and its **Isolate** button hid every other layer.
  - Isolate isn't undoable and has no un-isolate; Ctrl+Z undoes Mizan's steps instead.
  - "Show all" restores the drawing.
  - Separately, undo after renaming or deleting a layer restores a whole stale hidden set (`session.rs` ~1630, `undo.rs` ViewState).

### What Qiyas already has that makes this feasible

- **One machine form for every core command:** `Command`, ~130 variants, serde-tagged, already used by IPC and scripts.
- **Codec serde already on:** opencadcodec's `serde` feature is enabled in the workspace, so every entity and table record serializes.
- **Schema tooling available:** `schemars` 1.2.2 (MIT) is in the lock file and supports `extend("x-…")` and `example` attributes.
- **No orphan-rule blockers:** no codec type appears inside a `Command` field, so `JsonSchema` derives need no manual impls.

### Decisions (Kareem, 20 Oct 2026)

- Whole app, AI first:
  - **Step 1:** Mizan and MCP use 3 generic tools generated from the core.
  - **Step 2:** one shared command list also drives the command line, ribbon and scripts.
- Smart walls, layouts, self-checking and live drawing come as core features, so the AI gets them automatically.
- Live drawing animates each step's new elements; undo stays one step.
- Compare Claude Opus 5.5 with his Gemini setup, asking before each paid run (he pastes the keys).

## Ground rules

- Branch `feat/dynamic-core` from local `main`, one commit per phase, `scripts/check.sh` before each. On this laptop: `nice`, `CARGO_BUILD_JOBS=1–2`, `corepack pnpm` on PATH.
- Unit-aware everything (mm, cm, m, inches); every drawing change is one undo step; English and Arabic in step.
- New libraries (all MIT/Apache):
  - `schemars` in qiyas-core;
  - `serde_reflection` and `serde_path_to_error` in qiyas-core;
  - `cavalier_contours =0.7.0` in qiyas-geom.

  Run `scripts/third-party.sh` and commit `LICENSES/THIRD-PARTY.md`.
- Write the design doc to `docs/superpowers/plans/2026-10-20-qiyas-dynamic-core.md` in the first commit. Update CLAUDE.md (AI, MCP, Undo, Scripts) and the CHANGELOG each phase.

## Phase 1: quick fixes (independent of the rebuild)

1. **Isolate and the undo bug:**
   - `undo.rs` keeps a `ViewDelta` (only what changed), not the whole view state.
   - Isolate moves into the session: `isolate_layer` is a toggle, `unisolate_layers` (LAYUNISO) puts back only what isolate hid. It is saved as `qiyas_isolated`, with `DrawingInfo.isolated_layer`.
   - Layers panel: a pressed isolate button and an "Isolated: X · Un-isolate" line. Command line LAYISO and LAYUNISO.
   - Tests in `qiyas-core/tests/layers.rs`: `undoing_a_rename_restores_only_that_layers_hidden_entry`, `isolating_twice_restores_the_hidden_layers_as_before`, `unisolate_keeps_layers_the_user_changed_since`.
2. **Provider:** `openai.rs` sends `reasoning_effort`, `max_completion_tokens` and `stream_options.include_usage`.
   - A refused field is dropped and the request resent once; the refusal is remembered.
   - Reasoning and cached tokens are read into `Usage` and the log.
   - Tests: `effort_tokens_and_usage_are_asked_for`, `a_refused_parameter_is_dropped_and_remembered`.
3. **Model drop-down:**
   - `qiyas-ai/src/models.rs` `list_models`: Anthropic `GET /v1/models` (paged with `after_id`, `x-api-key`, `anthropic-version: 2023-06-01`); OpenAI-compatible `GET {base_url}/models`.
   - Built-in list: `claude-opus-5-5`, `claude-fable-5-1`, `claude-sonnet-5-5`, `claude-haiku-5-5`.
   - IPC `ai_models` (key from the keychain, never returned).
   - `src/shell/ModelPicker.tsx` (Base UI Combobox): the service's list, plus the saved model, plus "Other…" for a custom id; a refresh button; a fallback note.
   - Tests in `SettingsDialog.test.tsx`.

## Phase 2: the core describes itself (registry)

- **Derive `JsonSchema`** on `Command`, `Outcome`, `Target` and every nested spec type (DrawSpec, ModifySpec, EditSpec, DimSpec, …, Boq, Measurement, the report types).
- **Per-variant metadata** beside the variant:
  - `#[schemars(extend("x-category"=…, "x-effect"=…, "x-destructive"=…, "x-batch"=false, "x-needs"="clipboard", "x-internal"=…))]` and `#[schemars(example = …)]`;
  - add the doc comments that are missing.
- **New `qiyas-core/src/registry.rs`:**
  - `registry()` (`OnceLock`, built from `schema_for!(Command)`) gives each command its wire name, nested kinds (`draw.line`), summary, doc, schema, examples, effect, undo part (derived from `Command::changes()` on the examples), destructive, batchable and needs.
  - Also `get`, `search` and `index_text(lang, budget)`.
- **Guard tests** (`tests/registry.rs`):
  - every variant has a doc comment, a category and an example;
  - names are unique;
  - examples deserialize;
  - read-effect commands leave the session unchanged.

  Adding a command without these fails CI; adding one with them exposes it everywhere.

## Phase 3: transactions (any commands, one undo step)

- `undo.rs`:
  - `Step::Group(Vec<Step>)`, undone in reverse and redone forward;
  - `History::begin_group` / `commit_group` / `abort_group`, nestable;
  - each step gets a `label`.
- `Session::transaction(source, label, f)`:
  - takes a `TxView` snapshot (current, hidden, sheet, drafting, UCS) first;
  - **on failure:** the failed step rolls itself back, `abort_group` undoes the earlier steps, and the view is restored;
  - `run_as` also restores take-off, BOQ and conflicts on error.
  - Not one outer `record_document_change`: the codec's recorder can't be nested.
- **Generalized `Command::Batch { commands, layer?, color? }`:**
  - any batchable command, all-or-nothing, one undo step;
  - `layer`/`color` replace `author.rs` `run_batch` (make the layer, set it current, restore it afterwards);
  - unbatchable: block edit in place open/save/discard, WriteBlock, clipboard copy, undo/redo, host ops.
- `Session::last_change()` gives added, changed and removed handles for any command.
- **Tests:**
  - a failing third step restores the drawing, take-off, BOQ, current layer and sheet;
  - group undo and redo order;
  - unbatchable commands refused.

## Phase 4: every object as a record, every property editable

- **`qiyas-core/src/records/`:** `Session::records(RecordQuery)`.
  - Collections: entities, layers, linetypes, text and dimension styles, blocks, layouts, viewports, groups, views, header, layer states, xrefs, take-off, BOQ, conflicts, schedules, comments, constraints, parameters.
  - Query: filters by handles, names, type, layer, sheet and bounds; `where` with JSON pointers; projection by `fields`; detail summary, geometry or full; paging.
  - Output limits: default limit 20, at most 200, truncation markers, under the 40 KB cap, deterministic order.
- **`record_schema(type)`** via `serde_reflection`: paths, JSON types, writable or not with a `via` pointer to the right command, and unit notes (codec angles in radians).
- **`Command::SetRecords { collection, updates: [{key, path, value, expected?}] }`:**
  - serialize → JSON-pointer set → deserialize with `serde_path_to_error`;
  - copy back the fields serde skips (`raw_record` cleared, the changed app's `raw_dwg_eed` dropped);
  - refuse identity, owner and QIYAS xdata changes, locked layers, xrefs and freezing the current layer, and redirect (`via`) to `rename_layer` and `edit_text`;
  - run the same follow-ups as the specific commands: take-off, fields, dimension and table rebuild, constraints;
  - one undo step.
- **`Command::CreateEntities { entities: [{type, data}], sheet? }`:**
  - a codec default, plus the current layer, colour and linetype, plus the model's data merged on top;
  - the owner is the sheet; missing layers are made;
  - types with their own command are refused with `via` (Dimension, Table, Viewport, images and underlays, an Insert of a missing block).
- **Tests:**
  - a round trip for every fixture entity type;
  - every codec type traces;
  - `set_records`: one step, exact undo, refusals and redirects;
  - `create_entities`: defaults and layers.

## Phase 5: AI conveniences move into the core, plus checks

- **Defaults:**
  - Text, MText, Field and AttDef `height` default to 2.5 mm on paper × annotation scale;
  - hatch scale defaults;
  - tables get automatic sizes and a headings + rows form;
  - Text and MText turn a literal `\n` into a new line.
- **New commands:**
  - `Command::AddMarkup`;
  - `rooms.rs`: `Session::rooms` (read) and `Command::LabelSpaces`, moved from `author.rs` `find_rooms`, `ring_area` and `looks_like_a_label`;
  - `AddMeasurement` keeps its empty-item guard and gains `candidates`.
- **Read wrappers:** `query_near`, `query_contains_point`, `query_intersections`. `script.rs` `json_paths` becomes the public `resolve_command_paths` (the folder rule).
- **`check.rs` `Session::check_drawing(scope)` → findings `{rule, severity, handles, at, message}`:**

  | Rule | Finds |
  |---|---|
  | `dangling_end` | an end that touches nothing, with another line nearby |
  | `overshoot` | an end that runs a little past a line it crosses |
  | `duplicate` / `overlap` | repeats and collinear overlaps (reusing modify2 `overlap`/`signature`) |
  | `tiny_segment` | zero or tiny segments |
  | `room_open` | a room name with no closed boundary round it |
  | `room_size_text_differs` | a "5.25m x 3.25m" label that doesn't match its room |
  | `text_overlaps_text`, `text_over_lines` | text colliding with text or lines |
  | `text_height_vs_scale` | text too small or too large on paper |
  | `layer_zero` | entities on layer 0 |
  | `colour_not_bylayer` | explicit colours instead of ByLayer |
  | `dim_override_mismatch` | a dimension text that doesn't match its measurement |
  | `literal_newline` | a backslash-n written as two characters |
  | `far_off_extents` | entities far outside the rest of the drawing |
  | `unlinked_takeoff` | take-off rows that lost their entity |

  Plus `check_line()`, at most 200 characters. Tests in `qiyas-core/tests/check.rs`, one per rule, and a speed guard.

## Phase 6: Mizan and MCP use 3 generic tools

**New `crates/qiyas-ai/src/surface/`** (read, execute, capture, host_ops, result, errors).

**`qiyas_read {op, …}`:**

| op | What it gives |
|---|---|
| `state` | units, `metres_per_unit`, sheet, sheets, annotation scale, current, take-off and BOQ status |
| `capabilities` | collections, ops, categories, limits, whether a host and window are present |
| `commands` | `search` / `name` (supports `draw.line`) → doc, schema, two examples; no name → the index |
| `records`, `record_schema` | the record query and schema from Phase 4 |
| `query` | near, window, polygon, contains_point, intersections, boundary, entity, texts, tables, dimensions, blocks, rooms |
| `measure` | numbers only, no take-off row |
| `run` | read-effect commands only |
| `check`, `history` | findings; labelled undo steps |
| `drawings` (host), `selection` (window) | open and recent drawings; what the user points at |

**`qiyas_execute {op, …}`:**
- `run {command}`;
- `batch {commands, atomic, layer?, color?}`, with step bindings `{"$step":0,"pointer":"/handles/0"}`;
- `set_records`, `create_entities`, `undo_own`, `select`, `view`;
- file ops and host ops (`import_boq`, `export_takeoff`, `export_conflicts`, `compare`), each described in `commands` with `kind: "host"`;
- every path goes through the folder rule.

**`qiyas_capture {source: screen|picture, sheet?, bounds?, width?, marks?, highlight?}`** merges `view_screen` and `view_image`.

**Strict mode:**
- `command`, `commands`, `entities` and `value` are JSON strings, so all three tools stay strict and closed.
- Why: Claude's strict mode rejects recursive and open schemas, Gemini rejects empty objects, and one schema keeps every provider and MCP identical.
- Objects are accepted too, for lenient MCP clients.

**Result:** `{ok, command, outcome, added, changed, removed, step{undoable, source, label}, check, ui{action: reveal|show_sheet|redraw|zoom, handles, bounds}}`.
- `check` runs automatically on what was touched.
- Errors give the serde path, a schema fragment, an example, and did-you-mean for unknown types, kinds and fields.

**Agent:**
- Destructive caps come from registry metadata, not tool names.
- 120 calls per message once a turn has drawn something (40 otherwise), with "Budget: n calls left" near the end.
- The ToolCard label comes from the command (`command.<name>`, falling back to the registry summary).

**Prompt:** the existing domain rules (≤ 900 words) plus a generated index (the registry, about 1.5k tokens, deterministic so it caches). Rules:
- discover, then run, then check, then fix by handle;
- one `batch` per related change;
- never guess handles from a picture;
- never claim compliance or invent project data;
- zoom in when text is too small.

**Migration:**
- Port `authoring.rs`, `tools.rs`, `agent.rs`, `mcp.rs`, `takeoff_eval.rs` and `vision.rs`, keeping their numbers (living room 58.28 / 51.211, bedroom 22.085, kitchen 17.385).
- Keep the 57 legacy tools behind a debug flag until `takeoff_eval` passes on both, then delete `tools.rs`, the `execute.rs` dispatch, `execute/author.rs` and `execute/edit.rs`.
- Guard tests: tool schemas strict, index budget, every command name in the prompt exists, MCP and agent parity.

## Phase 7: live drawing (generic, from any change)

- Any mutating result carries `ui: {action: "reveal", sheet, handles}` from `last_change()`.
- **Window:**
  - new `src/viewport/reveal.ts`: `planReveal` gives each step about 1 s; entities grow one after another (120–400 ms each); steps queue; big steps appear at once. `revealFrame` and `subtractSpans` do the per-frame work.
  - `gl.ts`: `setReveal` hides the entities from the normal pass; `setRevealFrame` draws the growing segments; text fades in with `u_alpha`.
  - `Viewport` gets `reveal` and `onRevealed` props.
  - `useAssistantWiring` queues the reveal before the new batch arrives; skips it under reduced motion; clears it on a drawing switch, an undo or a user command.
  - `onMutated` calls `syncInfo()`, so new layers and layouts show.
- The mock draws in two steps.
- Tests: `reveal.test.ts`; App tests "a reveal is queued and layers sync" and "reduced motion reveals nothing".

## Phase 8: drafting features as core commands (exposed automatically)

- **Smart walls:**
  - `qiyas-geom/src/walls.rs` (cavalier `Or`/`Not`, kernel offsets), steps:
    1. normalise to the wall thickness;
    2. build the junction graph (snap ends, extend stems onto the centreline, join collinear runs);
    3. a strip per run;
    4. mitre or bevel wedges at corners;
    5. union per connected group;
    6. cut openings 10% past both faces;
    7. clean up.
  - `Command::Walls(WallSpec)` (one step, tables included) draws:
    - `A-WALL`: outlines;
    - `A-WALL-PATT`: one hatch per region, which is the area to measure;
    - `A-WALL-CNTR`: centrelines carrying `QIYAS_WALL` xdata;
    - door and window blocks on `A-DOOR` / `A-GLAZ`.
  - `Session::define_block` / `Command::DefineBlock` and `insert_block_placed` (mirror and rotate) generate blocks such as `QIYAS_DOOR_0900` and `QIYAS_WIN_1200_200`, built from mm so they are unit-aware.
  - `WallSpec.group` regenerates a group with added openings.
  - The outcome gives centreline length, net and gross plan area, and volume including the lintel and sill part.
  - Tests:
    - geometry, t = 0.2: L corner 1.40; T junction 1.78; X junction 2.36; closed room 4.00; room with a door 3.82;
    - also a curved wall, survey coordinates, an opening at a junction refused, overlapping walls refused;
    - a release speed guard for 500 walls, added to `check.sh`;
    - core: one step, block reuse, every unit, DWG and DXF round trip, regeneration.
- **Layouts:**
  - `qiyas-core/src/layouts.rs` takes `paper_catalog` and layout scale from `qiyas-plot`.
  - `Session::paper_unit()`; a paper-based `annotation_dimscale()` on layouts.
  - `Command::NewLayout {name, paper, orientation}` wraps the codec `add_layout`. It checks the name and that the block name it will pick is free.
  - `RenameLayout`; MView gains `scale` and `centre`; `Command::TitleBlock {fields, margin_mm, binding_mm}` (writes only the values given); `DrawingInfo.paper`.
  - Tests: one undo step and a paper round trip; taken names refused; viewport scale; text and dimensions sized in paper mm on a layout; a title block writes no value it wasn't given.

## Phase 9: one command list for the whole app (Step 2)

- `qiyas-core/src/cmdline/`: words, aliases and prompt sequences live on the variants (`x-words`). A pure `cmdline::Machine` handles points, `@dx,dy`, `@d<a`, options, Arabic digits, UCS and the last point.
- The window's command line uses IPC `cmdline_step` and `command_words` (cmdk suggestions come from the core). `.scr` scripts use the same machine. The AI gets `qiyas_execute {op: "type", line: "PLINE 0,0 10,0 C"}`.
- Ribbon buttons reference registry names (a test checks each exists). TypeScript `Command` types are generated from the schemas.
- Click-based tools that stay in the window (picking, previews) send registry commands.
- A new core command with words then appears in the command line, scripts, ribbon customisation and the AI with no window changes.

## Phase 10: evaluation

- **Offline: `qiyas-ai/tests/drafting_eval.rs`** (the scripted provider, extended to a queue of calls). A villa is built through the generic surface on a blank metres drawing:
  - layers in one batch, walls with doors and windows, room labels, dimensions on their layer, a layout with a viewport and a title block;
  - pass: `check` has no errors, rooms close, wall areas match hand-computed values, the right layers, viewport scale 1:100.
  - Replaying the founder's logged calls must produce findings for their known faults.
- **Live, only with Kareem's approval for each run:** Claude Opus 5.5 and Gemini 3.8 Flash (effort now sent), same villa request.
  - Scored on: check errors, steps, tokens and cost, and pictures.
  - Write-up in `docs/2026-10-20-mizan-drafting-eval.md`, with a recommendation for the default model.

## Verification

- `scripts/check.sh` green before every commit.
- **Mock app** (`QIYAS_AI_MOCK=1 corepack pnpm tauri dev --config crates/qiyas-desktop/tauri.conf.json`):
  - isolate, un-isolate and undo after a rename;
  - the model list loads or falls back;
  - the two-step animation plays;
  - and after Phase 9, typing a core command's words in the command line works.
- **With a real key:** Mizan asks `qiyas_read commands` and runs commands it was never hand-coded for: a layer with a lineweight, a dimension style, `set_records` on a viewport's scale, walls with doors, a layout with a title block. It finishes with `check` clean, and the canvas animates each step.
- **Pictures:** `qiyas_capture` and the scratch DXF renderer (`…/scratchpad/tools/dxfscan.py`) compare before and after on the villa.
