# Drawing craft, part B: sheets

1. `make_sheet` compares orientation using scale fit and plan aspect, retaining explicit orientation and scale. It reports the basis. Requested ratios retain their precision. Paper sizes and zone rectangles are paper millimetres; model geometry and core polygons are drawing units.
2. An oversized plan is partitioned on supplied horizontal/vertical lines or tagged grid axes. Supplied core interiors are protected, with no building-type or room-name inference. Each continuation has reciprocal match lines naming the other drawing number (layout name if no number was supplied), a miniature viewport of the whole drawing, and a bold extent rectangle for its part. The report includes all sheets, bounds, split geometry, bilingual reasons and whether core regions were supplied. The whole operation is one undo step.
3. Generated sheets retain zones in viewport xdata: numbered general notes beside the title block, schedules at the opposite edge and plans in the main field. Optional `zones` accepts the designer's rectangles. Table insertion uses the schedule zone. `arrange` keeps generated annotations grouped, preserves view scales, keeps the key plan and scale bar fixed, and reports remaining collisions and zone overflow. Oversized notes are returned intact as `unplaced_notes`, with `notes_fit: false`, rather than plotted across other zones. Existing loose notes retain their wording when arranged.
4. Both sheet and standalone title-block tools add client, consultant, project number, drawing number, revision, checked by, approved by and a revision table (rev/date/description). Captions are English/Arabic. Missing fields, including revision cells, stay empty and are listed. Authorship, date and revision are no longer invented; status stays For review / للمراجعة.
5. The initial empty Layout1 is reused only in a drawing created in this session, including after Save As. Opened drawings and nonempty layouts are preserved. Undo restores the default layout.

## Files and scope

- `crates/qiyas-core/src/layouts.rs`, `sheet_setup.rs`, `arrange.rs`, `check_layout.rs`.
- `crates/qiyas-core/src/commands.rs`: one contiguous registry block for make_sheet/arrange/title_block, plus the minimal title_block execution arm needed for its new arguments.
- Additional minimal changes: `placement.rs` reads the schedule zone during automatic table placement; `session.rs` adds and initializes a `created_here` provenance flag (four lines plus documentation).
- Tests: `crates/qiyas-core/tests/layouts.rs`, `arrange.rs`, and only the sheet tests in the concurrently edited `w3_output.rs`. Existing modules are already registered in `tests/all.rs`; no new test file or dependency.
- Part A files, CLAUDE.md and other runs' edits are untouched.

## Tests added

- Orientation/aspect, requested-scale fit, full denominator precision and explicit override.
- Tagged grid splitting around a supplied core in five unit systems and both languages; reciprocal references, true scales, key plans and undo/redo.
- Supplied split lines without grids, plus failed split rollback.
- Reuse after saving versus preservation after reopening and preservation of nonempty default layouts.
- Literal title metadata, bilingual captions, missing authorship/date/approval and revision history.
- Notes overflow returns all content; registry discovery and unknown revision field validation.
- Standalone Arabic title block and empty revision-cell reporting.
- Arrangement zones, retained viewport camera/scale and undo; schedule insertion into its edge zone after DXF save/reopen; oversized schedule reports; match-line grouping and fixed key plans.

## Existing tests changed or removed

- `the_scale_is_the_largest_standard_one_that_fits`: accounts for the larger title block and reserved schedule zone.
- `ratios_read_and_numbers_trim`: retained, with the number formatter now preserving significant digits.
- Removed `the_default_date_is_a_whole_local_date`: sheets no longer supply a default date. Date omission is covered by the new metadata tests.
- `title_blocks_report_empty_fields_and_own_their_layer_in_the_same_step`: expanded missing-field list.
- `title_block_writes_only_supplied_values_and_undo_removes_it_all` and `empty_title_block_never_fills_in_date_project_or_author`: allow captions and the fixed review status, forbid invented values.
- `title_block_shares_its_frame_edges_on_small_and_large_paper`: supplies the new optional Rust fields; its existing geometry assertions remain.
- `make_sheet_makes_a_real_sheet_at_a_true_scale_in_every_unit`: revised fit scale and blank metadata instead of Qiyas/P01 defaults.
- `make_sheet_speaks_the_requested_language_and_takes_the_user_language_by_default`: revised Arabic scale and expanded missing-field list.
- Renamed `make_sheet_translates_explicit_review_status_only_on_arabic_sheets` to `make_sheet_keeps_review_status_in_the_sheet_language`: an Approved input no longer becomes a sheet approval claim.

## Limits and validation

No cargo, pnpm, tsc, vitest, build or test was run. Work was reviewed by reading call sites and source; rustfmt was used only to format the sheet files. Compilation, regression execution and visual plotting remain for the founder's single final check. Work is uncommitted.

Splitting currently supports horizontal/vertical lines and tagged axes; diagonal splits and rotated-grid viewport clipping are not implemented. Core identification is supplied by the designer through `core_regions`; an omitted list is explicitly reported and is not a claim that no cores exist. The key plan uses a miniature view of the actual whole model, not an inferred building outline. Default zone sizes are generic drafting geometry and can be supplied explicitly; notes that do not fit are returned for the model to lay out. No paid tools, external services, windows or browser were used.
