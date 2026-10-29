# Drawing craft, part A: annotation

Implemented on 29 October 2026. No build, test, formatter or check was run, as requested.

1. `dimension.auto_chain` accepts `grids` (line targets), `wall_groups` (group IDs), and `side` (`top`, `bottom`, `left`, `right`). It creates ordinary styled Dimension entities, adjacent chains and an overall row, in one undo step. Wall chains use actual exterior corners and opening jambs. `offset_mm` and `spacing_mm` default to 10 and 7 paper mm at the annotation scale. This is an orthogonal drafting tool; oblique grids are refused, not projected silently. Selected geometry defines the extent.
2. `object_tags` assigns missing opening marks in `selection`, `left_to_right`, or `top_to_bottom` order, skips existing marks, and records the numbering basis. Category selection defaults to ascending handles. Existing explicit marks survive. Automatic size labels are no longer tag text. Doors default to circles and windows to hexagons; `shapes` overrides a kind or category. Placement reserves the entire symbol against plotted ink. Marks are object properties, so existing and future schedules use the same marks. Linked outlines follow their labels and sources and resize when properties change. Returned handles remain the text handles for compatibility. Symbol provenance prevents annotations becoming room boundaries.
3. `variation_report` accepts optional `revision`. It persists the supplied label; an initially omitted label uses the neutral delta symbol rather than inventing a revision number. Each changed element's before and after bounds gets a 2 mm paper margin. Only overlapping footprints merge, using polygon union rather than a bounding rectangle; disconnected moves and unchanged islands stay separate. Clouds use 3 mm paper arcs, with a labelled triangle on the same layer. Inner exclusion contours belong to the outer cloud and do not mark an unchanged island as a separate revision. Replacement removes all linked parts, respects locked layers, and remains one undo step.
4. The existing Unicode bidi engine now resolves the entire paragraph before reordering wrapped lines. Paragraph direction comes from the first strong character. Leading numbered-note markers are isolated LTR without changing stored text or glyph cluster offsets. Explicit legacy visual-order text remains supported. Literal malformed source text such as `.5 ملاحظة:` is not rewritten.

## Files touched by this task

- `crates/qiyas-core/src/dimension.rs`
- `crates/qiyas-core/src/object_tags.rs`
- `crates/qiyas-core/src/revisions.rs` (cloud generation and label metadata only)
- `crates/qiyas-core/src/comment.rs` (shared cloud provenance, triangle construction and replacement)
- `crates/qiyas-core/src/commands.rs` (existing annotation registry entries and dispatch)
- `crates/qiyas-render/src/text.rs` (bidi, wrapping and renderer unit tests)
- `crates/qiyas-core/tests/dimension.rs`
- `crates/qiyas-core/tests/drawing_quality.rs`
- `crates/qiyas-core/tests/revisions.rs`
- `crates/qiyas-core/tests/w3_output.rs` (only the three tag-related assertions/tests below)
- `crates/qiyas-core/tests/own_output.rs` (fixture for the new dimension kind)
- This note.

Minimal supporting changes outside the named files:

- `crates/qiyas-geom/src/walls.rs`: expose a small polygon-union helper using the existing geometry implementation, preserving concavity and holes without a new dependency.
- `crates/qiyas-cli/src/main.rs` and `crates/qiyas-cli/src/bench/revisions.rs`: update the two native VariationReport constructors for the optional revision label.
- `crates/qiyas-core/src/derived.rs` and `crates/qiyas-core/src/draw/room_boundary.rs`: exclude tagged symbols and comment/revision geometry from enclosure sources, preventing annotation from changing measured areas.

Part B's layout, sheet setup, arrangement, layout checks and title-block files were not edited. Other concurrent changes in shared files were preserved. Nothing was staged or committed.

## Tests

Added to existing test modules (no new test file, so no `tests/all.rs` change):

- `automatic_grid_chains_use_paper_offsets_normal_dimensions_and_one_undo`: four sides, four unit systems, measurements, paper offsets, generated blocks, undo and redo.
- `automatic_facade_chain_measures_actual_corners_and_opening_jambs`: true wall corners, jamb widths, overall dimension and rollback of invalid offsets.
- `auto_chain_registry_contract_has_a_valid_example_and_edit_effect`: registry schema/example/effect.
- `opening_symbols_number_in_requested_order_and_share_schedule_marks`: ordering, explicit marks, default/overridden shapes, schedule consistency, clear text, idempotence and undo across unit systems.
- `tag_outlines_and_revision_annotations_never_become_room_enclosures`: annotation outlines cannot reduce a room's measured area.
- `moved_elements_have_local_clouds_with_revision_triangles_and_replace_together`: separated footprints, unchanged middle geometry, paper sizing, label persistence, layer ownership and replacement undo.
- `a_ring_of_changes_excludes_its_unchanged_centre_without_an_extra_triangle`: integrated cloud generation keeps an inner exclusion contour, one triangle and complete replacement.
- `overlapping_cloud_footprints_keep_concave_boundaries_and_unchanged_islands`: union area, disjoint components, concavity and holes.
- Four renderer unit tests in `annotation_bidi_tests`: the shop label, wrapped paragraph direction, numbered Arabic notes, and full MTEXT layout with logical text protected from the legacy reversal toggle. Includes `Shop 04 / متجر`, `5. ملاحظة`, and `.5 ملاحظة:`.

Updated existing tests:

- `doors_and_windows_without_a_tag_are_a_finding_until_object_tags_tags_them`: expects D01/D02/W01 instead of hyphenated size labels.
- `opening_tags_follow_properties_position_undo_and_reopen_in_every_unit`: a size edit now preserves its numbered mark; position, undo and reopen assertions remain.
- Renamed `door_lines_come_from_the_size_whatever_was_made_first_and_roll_up_for_a_client_boq` to `numbered_door_marks_preserve_quantities_and_roll_up_for_a_client_boq`: numbering may split rows by explicit mark, so it asserts preserved measured quantities and retains client-BOQ roll-up checks instead of identical size-based row codes.
- The `registry_drawing_examples_pass_the_general_check_in_every_unit` fixture now supplies real perpendicular grid lines to `dimension.auto_chain`.

No tests were deleted. Compilation, formatting, runtime assertions and visual output remain unverified until the founder's combined check. No engineering brief values or jurisdictional rules were introduced.
