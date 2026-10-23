# Goal G — drawing quality and honest checks

This work is isolated on `goal/g` in `.local/wt/goal-g`. It adds no dependencies and makes no paid calls. It does not change the wall or object engines.

## What changed

| Goal item | Before | After |
| --- | --- | --- |
| Check visible annotations (D2, X-E1, X-E2) | Tables, dimension parts, block contents and hatches could be missed. Text collision boxes used a character-count estimate and only their middle portion. | Checks use the renderer's shaped text and flattened geometry, full text edges, table cells and hatch interiors. Owner handles identify what to fix. Text and cell borders inside an annotation are checked too. Intended dimension arrows and extension-line connections remain valid. |
| New rules and honest coverage | Unknown rules could return a clean result. No clear indication when geometry could not be checked. | Unknown rule and pack names are schema errors. New checks cover annotations crossing grid bubbles, labels outside their linked rooms, overlapping annotations and broken object tags. Reports include `complete`, `not_checked` and `checked_rules`. |
| Large drawings and language (X-E4, X-E5) | Text checks stopped after 2,000 labels; room checks stopped after 200 candidates. English layer fragments selected room checks. | Both caps are removed. Spatial indexing limits collision work. Room candidates and construction geometry work with Arabic and generic layers. Explicit rules run independently of automatic pack selection. A nested-geometry budget reports incomplete coverage instead of a clean result. |
| Free space and placement (D10, D11, D22) | Callers guessed coordinates, and schedules could land over the plan. | General registry commands `free_space` and `auto_place` have schemas and examples. Queries are read-only. Placement moves labels and tables or offsets linear/aligned dimensions while preserving their measured endpoints. Grid bubbles are obstacles. Schedules without an explicit position use clear space beside the drawing. |
| Room labels (D13, X-E3) | A long label could extend outside a small room. | Labels move and, when needed, wrap at the requested readable height. Their entire rendered box must fit the room, including curved edges and islands. Value and unit stay together when wrapped. Failure rolls back rather than shrinking the text silently. |
| Door symbol | The hinge was offset to the face opposite the swing. | The recipe's normal offset puts it on the swing face. Only recipe data changed; the existing wall tests now assert the corrected face. |
| Display (X-E6) | Linetype definitions did not paint dashes. Every point was drawn as a cross. | The shared screen/capture/plot geometry paints simple dash/gap/dot patterns, with ByLayer/ByBlock and entity/global scales. Measuring and hatch/room boundary tracing keep continuous source geometry, including block arrays. Point modes draw a dot, nothing, a plus, a cross, a tick, a circle or a square as requested. |
| Linked tags (D15) | Door/window mark labels were independent text. | `object_tags` links a label to any text/number object property. Tags follow movement and property changes, including source-handle remapping, undo and save/reopen. Missing sources and changed label text are reported. |
| Wording (D14, U8) | Goal A's wording was already approved. | Plan labels keep “Area 19.31 m²” / “المساحة ١٩٫٣١ م²”. Approval status remains in the report. |
| Mizan | Layout warnings could be visible but did not prevent a phase from being presented as ready. | Newly introduced layout findings join the existing repair gate (review modes pause; autonomous mode reports unresolved findings). Mutation results expose layout counts and coverage. Both prompts contain a short repair note; the English prompt is 892 words and its 900-word test passes. |

Dimension generation now uses the actual shaped text width for its line gap and text position. The old estimate could put radius or ordinate text across its own leader. The checker exposed this in the existing registry examples.

## How to use it

`free_space` accepts either a rectangle size or an existing entity, plus an optional preferred position, side, sheet and containing polygon. Coordinates and sizes are drawing units; clearances are paper millimetres. It returns up to eight positions with bounds and movement deltas. `auto_place` applies the same search in one undo step. Explicit creation coordinates remain literal.

For example, through the generic command tools:

```json
{"type":"free_space","entity":1234,"side":"right","clearance_mm":10}
```

```json
{"type":"auto_place","targets":[{"handle":1234}],"clearance_mm":2.5}
```

```json
{"type":"object_tags","targets":[{"handle":5678}],"property":"mark"}
```

## Before and after evidence

Evidence is under this worktree's `.local/goal-run/`. The stdlib Python MCP clients are in `.local/audit/`; every mutation is followed by entity/object/layer readbacks, with specific checks for the changed quantity or position. They run a locally built CLI without a model provider.

Original audit captures are preserved in:

- `.local/goal-run/before/table-dimension-bubble.png`
- `.local/goal-run/before/room-label.png`

The original quality probe is rerun in `.local/goal-run/verified-quality-probe-v2/`. Its colliding fixture previously returned “No findings”. The oversized-label fixture now wraps inside its room and keeps its unapproved state in the report.

All six final headless jobs passed; `.local/goal-run/headless-v2-results.json` records their results. The array regression imports a native DXF array, because raw Insert creation and array-structure edits are correctly refused by the generic tools. The locally built CLI was copied to `.local/goal-run/bin/qiyas-cli` immediately after building so another goal's shared-target build could not replace it during the run. Its SHA-256 is recorded beside it.

| Evidence | Result | Transcript and PNGs |
| --- | --- | --- |
| Original audit probe | The old “No findings” fixture now reports 10 warnings: 4 text overlaps, 4 text/line crossings and 2 unreadable heights. | `verified-quality-probe-v2/transcript.jsonl`, `0032-table-dimension-bubble-0.png`, `0045-oversize-room-label-0.png` |
| mm, cm, m, in | Both table and dimension cross the bubble before placement. After placement and correcting the bubble-label height, every selected layout rule is clear. The dimension value is unchanged. Each area label wraps within its 4 m² room. Moving it outside produces a finding; undo clears it. | `verified-v2/{mm-en,cm-en,m-en,in-en}/transcript.jsonl`; each has `0029-before-placement-0.png`, `0047-after-placement-0.png`, `0063-fitted-label-and-schedule-0.png` |
| Arabic | Label fits, uses Arabic wording/digits, retains `room_type_status: unapproved` in the report; schedule is placed outside the room. | `verified-v2/m-ar/transcript.jsonl`, `0063-fitted-label-and-schedule-0.png` |
| Door/window tags | Door text changes from `D-900x2100` to `D-1000x2100` and moves with the opening. Linked-tag check is clear. | `verified-symbols-v2/transcript.jsonl`, `0021-door-hinge-tags-before-0.png`, `0028-door-hinge-tags-after-0.png` |
| Patterns and point modes | Read back modes 0, 1, 2, 3, 34 and 66. Captures show dashed/centre lines, hidden points and the requested symbols. | `verified-symbols-v2/0059-patterns-point-mode-0-0.png` through `0089-patterns-point-mode-66-0.png` |
| PDF plotting | The compiled plot regression writes and reads back patterned PDFs in all four units; the metre PDF was rasterised and visually checked. | `pdf/patterns-units-{4,5,6,1}.pdf`, `pdf/patterns-units-6.png`, `pdf-artifacts.log` |
| 2,101 texts | Finds the collision at the end of the drawing; placement clears it, with `complete: true`. | `large-v2/texts/transcript.jsonl`, `0004-before-last-label-0.png`, `0011-after-last-label-0.png` |
| 205 rooms | Finds room 205's open boundary on Arabic/generic layers. Closing the gap clears it, with `complete: true`. | `large-v2/rooms/transcript.jsonl`, `0004-last-room-gap-0.png`, `0010-last-room-closed-0.png` |
| Dashed room boundaries | A rectangle, a block and both cells of an imported array retain 12 m² in mm, cm, m and in, before and after switching to dashed display. | `boundaries-v2/{mm,cm,m,in}/transcript.jsonl`, `imported-array/transcript.jsonl`, `imported-array/0014-dashed-closed-array-0.png` |
| Original grid/unit audit | All four units retain 20 m² before/after adding and hiding grids. | `audit-grid-v2-{mm,cm,m,inches}/transcript.jsonl` |

The listed paths are relative to `.local/goal-run/`. Captures of placement, Arabic labels, corrected door hinge/tags, hidden/circled points, large-text repair, the last room and dashed block arrays were opened and visually reviewed.

## Validation

- `cargo fmt --all`: passed (`.local/goal-run/fmt-final.log`); the final `--check` also passed (`fmt-check-final.log`).
- Targeted tests: **62 passed**, none failed (`.local/goal-run/targeted-checks-3.log`). This includes all registry drawing examples in four units, not just the new fixtures.
- Local CLI build: passed (`.local/goal-run/build-cli-boundary.log`).
- Six final headless evidence scripts: passed, including the original room/grid audit and the dashed-boundary regression in all four units.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed (`.local/goal-run/clippy-after-fixes.log`).
- Full CAD-crate test command: **956 passed, 1 failed, 1 ignored** (`final-cad-tests.log`, exit 101). Breakdown: core 779 passed plus the superseded array fixture failure; geometry 80 passed; plot 43 passed/1 ignored; renderer 54 passed. The fixture now imports a native array; its complete rerun passed **8/8** (`drawing-quality-final.log`, exit 0). Across the full run and corrected-suite rerun, all **957 distinct CAD tests pass**, with 1 existing plot test ignored. CAD documentation tests also pass.
- AI full run: 185 passed, 37 failed, 1 ignored (`final-tests-isolated.log`). One failure was the old fixed-offset label assertion, corrected to verify actual containment and clear rendered bounds; all 19 authoring tests then passed (`regression-fixes-retry.log`). The remaining 36 failures require local ports and are listed below. AI documentation tests now pass (`doc-tests-final.log`, exit 0; no doctest examples). The final required `-p qiyas-ai --lib --test surface --test agent` run finished with **132 passed and 26 local-port failures** (`ai-final.log`, exit 101): library 75/1, agent 2/25, surface **55/0**. Every failure reports `Operation not permitted` while binding a test server. The prompt-length and scripted layout-repair tests pass. No functional failure remains in the executed tests.
- Shared-target artifacts from parallel worktrees caused stale-library compile and documentation-test failures in earlier attempts. Final verification uses the temporary, ignored `.local/goal-run/verify-build.toml` (17 codegen units for the internal libraries) to give this worktree distinct artifacts in the same required target folder. All Cargo calls retain the required jobs/debug/incremental settings. No tracked build configuration changes, cache deletion or interrupted builds.
- `git diff --check`: passed. No frontend `src/` files changed; TypeScript/Vitest checks are not applicable.

The test set includes millimetres, centimetres, metres and inches; English and Arabic room labels; table/dimension/grid collisions; text-edge crossings; hatch interiors; non-plotting helpers; block contents; large text sets; linked opening tags; point modes; and PDF line patterns.

Two existing tests encoded the audited display bugs and are corrected: default point mode means a dot, and a door hinge belongs on the swing face. The registry MText example now uses a readable 2.5 mm paper height. The PDF recognition fixture keeps its imported text and is checked at its readable 1:50 sheet scale. Label-placement tests now assert clear, contained rendered labels and matching report/readback positions, rather than a fixed offset under the name. Wording assertions normalise wrapping whitespace so a non-breaking value/unit pair preserves the same wording. Check assertions are not disabled.

## Limits and review notes

- Placement uses a finite search. “No clear position found” is an honest failure, not proof that no possible placement exists. A room too small at the chosen scale needs a scale/layout decision; the command does not silently make the text smaller.
- Automatic dimension placement currently supports linear and aligned dimensions. Radius/ordinate text generation uses measured width; other dimension types can still be placed explicitly and checked.
- Simple linetypes are painted. Complex embedded text/shape linetypes and viewport-specific PSLTSCALE are not added here.
- Negative point size uses a percentage of the sheet extent, so captures and plots agree. It does not change with viewport zoom.
- Unsupported or budget-limited nested geometry is reported as incomplete; the placement helper refuses to claim free space around it. Full `check` is available even when the quick mutation check skips a very large sheet.
- Opening marks come from the existing property/schedule recipes. Custom mark policy remains Goal B's responsibility.
- Live paid-provider behaviour is not tested. Mizan is covered by local agent and surface tests.

## Tests blocked by the sandbox

These 36 tests attempted to bind a local port and received `Operation not permitted`. They were run; none was silently skipped. The stdio headless jobs do not need ports and passed. Their detailed failures are in `final-tests-isolated.log`; the final required subset repeats 26 of them in `ai-final.log`. The complete list is also saved in `port-test-failures.json`. They need rerunning where local test servers are permitted.

`crates/qiyas-ai/src/models.rs`:

- `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`

`crates/qiyas-ai/tests/agent.rs`:

- `a_marked_region_lists_what_is_in_it`
- `a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user`
- `a_question_ends_the_turn_cleanly_and_the_answer_carries_on`
- `a_selection_is_listed_and_shown`
- `an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room`
- `an_answer_that_keeps_running_out_stops_at_the_cap`
- `api_key_absent_from_logs_and_errors`
- `attached_pictures_go_before_the_words`
- `cancel_between_tool_calls_runs_no_more_of_them`
- `cancel_stops_within_one_second`
- `file_tools_without_a_host_say_to_use_the_app`
- `gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn`
- `history_is_append_only_and_system_prompt_is_stable`
- `invalid_tool_json_is_answered_with_an_error_and_not_run`
- `loop_stops_after_40_tool_calls`
- `malformed_tool_input_still_stops_after_40_calls`
- `old_pictures_give_way_to_a_line`
- `openai_compatible_drops_a_refused_field_and_asks_again`
- `openai_compatible_streams_text_and_tool_calls`
- `refusal_stop_reason_surfaces_category`
- `rejected_request_after_find_entities_is_logged_without_retry`
- `removals_and_resolutions_stop_after_10_in_one_turn`
- `text_stream_is_forwarded_in_order`
- `tool_use_round_trip_executes_against_the_session`
- `view_image_stops_when_the_image_budget_is_used`

`crates/qiyas-ai/tests/key_safety.rs`:

- `network_tests::model_discovery_never_follows_a_redirect_with_credentials`

`crates/qiyas-ai/tests/mcp.rs`:

- `a_wrong_token_is_refused_with_401`
- `a_restart_on_the_same_port_binds_again`
- `execute_request_ids_replay_without_another_mutation_or_ui_event`
- `concurrent_counts_both_land`
- `lists_the_catalogue_and_measures_as_mcp`
- `other_origins_are_refused_even_with_the_token`
- `tool_errors_are_error_results`
- `sheet_layer_and_zoom_changes_reach_the_window`
- `view_image_is_an_image_block`

## Files

- `CLAUDE.md`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/prompt_en.txt`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/tests/authoring.rs`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-core/data/legacy-object-types.json`
- `crates/qiyas-core/src/check.rs`
- `crates/qiyas-core/src/check_layout.rs`
- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/dimension.rs`
- `crates/qiyas-core/src/draw.rs`
- `crates/qiyas-core/src/draw/room_boundary.rs`
- `crates/qiyas-core/src/field.rs`
- `crates/qiyas-core/src/ink.rs`
- `crates/qiyas-core/src/lib.rs`
- `crates/qiyas-core/src/object_tags.rs`
- `crates/qiyas-core/src/placement.rs`
- `crates/qiyas-core/src/query.rs`
- `crates/qiyas-core/src/room_objects.rs`
- `crates/qiyas-core/src/rooms.rs`
- `crates/qiyas-core/tests/check.rs`
- `crates/qiyas-core/tests/drawing_quality.rs`
- `crates/qiyas-core/tests/goal_a.rs`
- `crates/qiyas-core/tests/opening_object_recipes.rs`
- `crates/qiyas-core/tests/own_output.rs`
- `crates/qiyas-core/tests/walls.rs`
- `crates/qiyas-geom/src/dimension.rs`
- `crates/qiyas-geom/src/flatten.rs`
- `crates/qiyas-geom/src/lib.rs`
- `crates/qiyas-geom/src/linetype.rs`
- `crates/qiyas-geom/src/paint.rs`
- `crates/qiyas-geom/tests/paint.rs`
- `crates/qiyas-plot/tests/plot.rs`
- `crates/qiyas-render/src/batch.rs`
- `crates/qiyas-render/tests/display_modes.rs`
- `crates/qiyas-render/tests/fills.rs`
- `docs/2026-10-23-qiyas-goal-G.md`
