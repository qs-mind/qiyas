# Optional domain knowledge, 29 October 2026

## Changes by task item

1. `space_check` defaults to facts. `profile` and `building_type` must explicitly select a compatible packaged profile before practice checks run. Reports name the selected profile, thresholds and practice basis; absent, unknown or mismatched selections return facts and available profile names. Geometry facts use actual room polygons, including holes: area, perimeter, interior clear-width chords, shared collinear boundaries, boundary openings and contained fixture insertion points. Circulation uses explicit classifications; unresolved roles produce a null share. Missing access data no longer implies an ensuite relationship, and area divided by width no longer invents corridor length.
2. Explicit room kinds accept arbitrary strings and override name suggestions. Matching returns only `suggested_kind` and `uncertain: true`; broad English `store` and `hall` matches were removed. A name no longer selects the master bedroom.
3. Omitted project type does not select villa. Unknown types return a successful bilingual no-packaged-template response. Omitted storeys remain unknown and conditional sheets are listed separately. Villa and residential-unit packages remain available data. Project aliases match whole aliases rather than arbitrary substrings.
4. Results retain the requested jurisdiction, including Dubai, Abu Dhabi and UK. Aliases live in the register. Unsupported names return no borrowed country values. UAE alternatives already documented in the register's notes now have separate entries with jurisdiction and building-class scopes. Public limits results return all scoped alternatives. The older single-value helper never chooses one scoped alternative arbitrarily.
5. Wall height and opening sill/head defaults retain their drawing values but gain `type_default` provenance. Defaults appear in wall/opening outcomes, survive rebuilds and saved drawings, and can be explicitly confirmed even without changing their numeric value. Source actors are preserved. Door-symbol leaf defaults are stored on the element and reported. `takeoff_check` includes default, unknown and uncited Mizan values and affected generated items. Quantity provenance follows property recipes, geometric dependencies, hosted cuts, rule decisions and BOQ grouping fields; generated items carry `assumed`.
6. Only `Mapping.informational` excludes an informational item. English and Arabic gross-floor-area words no longer change classification.
7. Public display names are Wall/حائط, Door/باب, Window/شباك and Room/فراغ. Historical type identifiers remain unchanged for saved drawings and rules. Schedule recipes use explicit `kind`, and generated descriptions and labels use display names. Existing saved schedule expressions can still resolve historical type IDs.
8. Comments, grids and all drafting symbols accept optional `layer`, retaining their existing defaults. The existing native Rust `Comment` constructor remains compatible; the public JSON `comment` variant exposes the layer.

## Files touched

Under `crates/qiyas-core/`:

- `data/code-register.json`, `data/drafting-standards.json`, `data/legacy-object-schedules.json`, `data/legacy-object-types.json`.
- `src/drafting_standards.rs`, `src/space_planning.rs`, `src/walls.rs`, `src/draft_symbols.rs`, `src/comment.rs`, `src/conventions.rs`, `src/objects.rs`, `src/measurement/evaluate.rs`, `src/digest.rs`.
- Necessary additional wiring: `src/commands.rs` for schemas, examples, dispatch and actor forwarding; `src/input.rs` for batched opening actor forwarding; `src/input_context_defaults.rs` for door-leaf default reporting; `src/point_run.rs` for the compatible comment variant; `src/measurement.rs` only for the additive serialized `GeneratedItem.assumed` field. No rolled-up logic changed.
- `tests/w5_drafting.rs`, `tests/w3_output.rs`, `tests/walls.rs`, `tests/comment.rs`, `tests/conventions.rs`, `tests/opening_object_recipes.rs`, `tests/registry.rs`.

Also this note. No edits to CLAUDE.md, rooms.rs, draw/room_boundary.rs, second_reading.rs or qiyas-ai. Other runs' working-tree changes are not part of this task.

## Tests

All integration additions use files already included in `tests/all.rs`; no new integration binary or registration was needed.

Added:

- `space_planning.rs`: `polygon_facts_exclude_notches_and_corner_contacts`, `clear_chords_stop_at_islands_and_follow_rotated_edges` cover real polygon edges, corner-only contacts, holes and rotated geometry.
- `w5_drafting.rs`: `missing_and_incompatible_profiles_return_no_judgements`, `packages_do_not_invent_a_project_type_or_storeys`, `explicit_space_kinds_win_and_names_are_only_uncertain_suggestions`, `concave_space_facts_use_polygons_without_retaining_or_mutating_rooms`, `each_symbol_can_use_a_caller_layer_and_default_leaves_are_reported`.
- `walls.rs`: `physical_defaults_reach_elements_boq_and_takeoff_check_and_explicit_values_clear_them`, `default_sources_survive_opening_rebuilds_and_same_value_confirmation`, `explicit_ai_inputs_keep_their_actor_and_size_groups_keep_default_sources`. These cover provenance, BOQ flags, save/reopen, undo, same-value confirmation, actor identity and dimension grouping.
- `comment.rs`: `comment_accepts_a_caller_layer`.
- `conventions.rs`: `grid_accepts_a_caller_layer_for_axes_bubbles_and_text`.
- `opening_object_recipes.rs`: `public_names_are_bilingual_and_schedule_recipes_use_explicit_kinds` covers actual English/Arabic schedule cells as well as type metadata and recipes.
- `registry.rs`: `domain_options_are_optional_and_registered_with_open_room_classification` covers the public schemas.

Changed or replaced:

- `corridor_villa_reports_the_founders_complaints`, `a_coherent_villa_has_no_findings_in_each_market`, `unknown_bathroom_access_is_not_a_failed_ensuite_check`, and `practice_defaults_are_labelled_and_never_report_code_errors` now explicitly select villa and explicitly classify rooms.
- Replaced `egypt_code_findings_cite_the_article_its_source_and_the_edition_caveat` with `selected_profiles_are_practice_preferences_in_every_jurisdiction`: optional profile checks are preferences, not automatic code enforcement.
- Replaced `gulf_code_findings_cite_their_document_and_the_rest_stay_practice_defaults` with `jurisdiction_results_keep_the_request_and_every_scoped_alternative`: register facts retain original jurisdiction and all scopes instead of selecting a single country number.
- Renamed/revised `a_gross_floor_area_is_information_not_a_boq_item` to `gross_floor_area_words_do_not_reclassify_a_boq_item`.
- Renamed/revised `a_total_area_line_is_a_billable_item_and_only_the_flag_or_a_gross_floor_area_is_information` to `only_the_explicit_mapping_flag_makes_an_item_informational`; English and Arabic phrases stay billable unless flagged.
- The synthetic code-citation unit test now initializes the additional context fields; its citation behaviour is unchanged.
- Native `Grid` literals now specify `layer: None` in `labelled_grids_have_paper_sized_bubbles_and_text_in_every_unit`, `conventions_reuse_existing_linetypes_and_refuse_locked_layers_without_partial_changes`, `invalid_targets_distances_and_grids_make_no_partial_symbols_or_tables`, and `grid_defaults_use_the_sheet_and_scale_and_allow_a_custom_bubble_radius`.
- The command registry includes an explicit-profile example alongside the facts-only example. Existing registry example round-trip tests remain in place. Formatting-only changes in these owned files do not alter other tests.

## Verification and limits

No builds, tests or check commands were run, as requested. Rust sources were formatted and changed signatures, constructors, schema examples and relevant assertions were reviewed by reading. Compilation and execution remain for the founder's combined check. Work is uncommitted.

Clear widths are actual interior chord samples at projected vertex-interval midpoints in polygon-edge directions, not bounding-box widths or a claim of an exact global minimum clearance. Curves are flattened at the reported drawing tolerance; perimeter uses exact curve length. Fixture containment uses insertion points. Explicit caller-only plan data cannot supply a polygon that was not provided, so unavailable perimeter/width facts remain null/empty. Unknown circulation roles are not guessed.

Scoped UAE values were separated from existing register notes and citations, without new external code verification. Historical object `name` remains the stable identifier; `display_name_en` and `display_name_ar` carry the public names. An uncited AI-supplied value remains Mizan provenance rather than being promoted to engineer-supplied evidence. No additional founder decision was required.
