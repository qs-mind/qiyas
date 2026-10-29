# Blind second reading informs, never deletes

1. Disputed generated items remain in BOQ rows and comparison totals. Their status, first reading and second reading remain attached. The existing second-reading column prints the second value in English or Arabic; the take-off workbook prints both labels. Drawing queries and acceptance remain, and takeoff_check pages every open dispute.
2. Reader evidence no longer passes through log::clip. Large results use cached, lossless JSON pages within 12,000 bytes, with totals, truncated, next_offset and optional JSON-pointer reads. Full object/entity records use the existing lossless projection path.
3. The reader receives rules, formulas, selection, mapping templates, distinct property groups, grid definitions, exclusions, type definitions and recognition predicates. Evaluated dimension operands, first quantities and roll-up totals are absent from its brief.
4. Object records expose properties while stripping result fields and generated quantities. Spatial queries are available; BOQ, measurement and other result collections remain guarded, including result-field filters/projections.
5. Redaction follows entity provenance. Generated schedules, fields, result-like Qiyas text and query artifacts are hidden. Table filtering happens before loose text becomes anonymous grid cells. New direct text, mtext, edited text and tables record engineer/Mizan authorship in their existing QIYAS_TEXT metadata.
6. Stored readings bind to drawing content, rules/mappings/scope and the source set. Equal totals do not rescue a stale reading. Acceptance keys include the evidence fingerprint. Query clouds/labels are excluded from the drawing fingerprint because creating the review itself must not invalidate its evidence.
7. An operation-scoped observer requests readings at the actual comparison/export boundary. It lends the live session to the async reader while retaining the outer lock and transaction, including comparisons after earlier batch edits. Further passes cover remaining items; failed items are attempted once per operation and failures reach tool-result advice.
8. Declared quantity kind controls tolerance: counts are exact even with an unfamiliar unit or a configured percentage. Other kinds use their configured tolerance or existing kind allowance; unknown legacy units default to exact comparison.

## Files

- AI: src/blind_reader.rs, second-reading wiring only in src/agent.rs, export observer hooks/count only in src/surface.rs; tests/w3.rs and tests/surface.rs.
- Core: src/second_reading.rs, src/measurement.rs, src/deliverables.rs, src/discrepancy/mod.rs, second-reading section of src/digest.rs; tests/w3_output.rs.
- BOQ: src/dimensions.rs second-reading filter/column; tests/deliverables.rs. Excel and PDF deliverable writers already consume the shared core report column.
- Minimal additional core files: src/session.rs installs the actual-operation observation point; src/commands.rs records text/table authorship inside existing undo steps; src/tables.rs adds provenance-preserving filtering before table grouping.

Paths above are relative to crates/qiyas-ai, crates/qiyas-core and crates/qiyas-boq respectively. No new test files or dependencies.

## Tests changed or replaced

- Core the_reader_is_shown_each_item_but_never_its_quantity now permits quantity expression definitions while forbidding result fields.
- Core a_second_reading_that_agrees_is_marked_and_one_that_differs_never_reaches_the_boq was renamed to a_second_reading_that_differs_stays_in_the_boq_with_both_values and now asserts retained bilingual rows.
- Core resolving_one_size_disagreement_does_not_accept_another_size now asserts retained rows with independent statuses and evidence-bound query keys.
- Core comparison_excludes_disputed_items_and_labels_partial_opening_totals was replaced by comparison_retains_disputed_items_and_complete_opening_totals.
- AI a_seeded_disagreement_never_reaches_the_boq_and_appears_as_a_query was renamed to a_seeded_disagreement_stays_in_the_boq_and_appears_as_a_query; safe records/spatial reads and retained rows replace the old exclusions.
- AI a_failed_reader_counts_every_unattempted_item additionally checks the retained error.
- AI takeoff_export_reports_manual_generated_and_dimension_rows now counts disputed rows.
- Reader the_guard_refuses_what_could_show_the_first_quantity now permits safe records. only_the_deliverable_boq_compare_and_take_off_export_start_a_pass covers canonical exports and batches.
- Reader a_text_that_prints_the_first_quantity_is_hidden and arabic_quantities_and_integer_counts_are_hidden_without_changing_geometry were replaced by redaction_uses_output_handles_and_preserves_engineer_numbers. Matching English/Arabic numbers alone no longer justify redaction.
- The existing label unit test supplies the new tolerance/evidence fields; its behavioral assertions are unchanged.

## Tests added

- Core: equal_totals_do_not_keep_a_reading_after_drawing_rule_mapping_or_source_changes; full_blind_scope_contains_formulas_selections_and_size_groups; takeoff_check_lists_each_open_reading_and_acceptance_stays_revision_bound; declared_count_kind_is_exact_with_an_unfamiliar_unit_and_a_configured_percentage.
- AI integration: each_compare_including_after_batch_edits_gets_a_current_reading; a_single_operation_reads_more_than_twelve_items_in_further_passes; a_reader_failure_is_visible_in_the_triggering_tool_result; a_same_total_revision_during_the_reader_is_refused_as_stale.
- Reader unit/integration: evidence_pages_are_lossless_and_keep_totals_past_the_old_log_limit; result_projection_and_filter_side_channels_are_refused; drawing_reads_keep_engineer_text_properties_and_spatial_evidence; object_records_keep_properties_but_remove_evaluated_quantities; generated_tables_and_fields_are_hidden_but_engineer_tables_remain.
- BOQ: disputed_second_readings_remain_visible_in_excel_and_pdf_in_both_languages; takeoff_workbook_keeps_disputed_generated_rows_and_their_second_values.

## Validation and limitations

No build, formatter, test or check was run, as requested. The code and call sites were reviewed by reading. Changes remain uncommitted; concurrent work was preserved.

Older QIYAS_TEXT markers do not distinguish engineer authorship from Mizan authorship. Result-like legacy marked text still follows the generated-text rule; newly authored engineer text and unmarked original drawing text are preserved. That historical ambiguity cannot be resolved reliably from the old marker alone.

The existing registry disallows file exports inside batches; that rule is unchanged. Batchable comparisons use the exact operation boundary, and the same observer covers quantity exports whenever they execute. Custom client workbook templates retain their existing column contract.
