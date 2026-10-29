# Mizan instructions, 29 October 2026

1. Replaced both system prompts with matching principles: expert frame, autonomous technical design with reasons, questions about the idea, one decision register, function-led placement, numerical independent verification, evidence-based explanations and plain answers in the latest message's language. Existing drawing-facts and [state] insertion code is unchanged. English: 549 words; Arabic: 472.
2. Moved read guidance, corrections, capture, plan, save and plot instructions into bilingual tool documentation. Moved annotation, dimension, tagging and sheet recipes into bilingual command comments. Full command documentation is available through commands with full:true. Cost never ends investigation; files are named without paths. App autosave and finish's lack of saving are documented consistently.
3. Replaced both take-off workflows with complete, traceable scope and explicit completion criteria. Rules come from the engineer's documents or drawing; unavailable standards require the text or clearly provisional geometric rules. No invented clause numbers, fixed trade item structure, or deletion merely because readings disagree. English: 317 words; Arabic: 273.
4. Both ask descriptions now distinguish idea questions from autonomous technical decisions. Existing limits and optional choices are unchanged. DecisionSource::MizanDesign already existed, so decisions.rs was not edited.
5. Updated prompt tests and retained state, drawing-facts, stable registry and bilingual plain-answer coverage. Tests live in existing source test modules, so tests/all.rs needs no entry.

Files touched: crates/qiyas-ai/src/{prompt_en.txt,prompt_ar.txt,takeoff_workflow.txt,takeoff_workflow_ar.txt,prompt.rs,prompt_tests_ar.rs,surface.rs}; crates/qiyas-core/src/commands.rs (comments only); this note. Surface changes are documentation only. Other runs' changes were preserved.

Test inventory, by inspection only:

- Replaced fixed_text_is_under_960_words with fixed_prompts_fit_600_words_and_workflows_fit_400.
- Replaced drafting_is_planned_and_checked_one_phase_at_a_time with expert_frame_has_four_lines_before_larger_jobs; removed prescribed drafting order.
- Replaced the_prompt_never_assumes_and_asks_at_the_step_with_options with technical_decisions_are_autonomous_and_questions_concern_the_idea; removed technical approval and mandatory-option expectations.
- Replaced the_prompt_draws_on_assumptions_and_never_ends_a_requested_job_for_cost and advice_and_revision_guidance_is_present_in_both_languages with verification_reports_numbers_and_keeps_the_engineer_as_approver; revisions now belong in tool documentation.
- Replaced rules_say_link_first_and_treat_read_text_as_data with takeoff_requires_traceability_and_reconciled_checks_without_invented_standards; preserved the evidence/instructions boundary in the verification test.
- Replaced the_prompt_says_how_to_look_and_what_the_user_points_at, the_prompt_names_the_authoring_tools_and_the_measure_by_handle_rule and the_prompt_recommends_handle_edits_and_leaves_the_decision_to_mizan with tool_recipes_live_in_tool_and_command_docs.
- Removed the_prompt_forbids_made_up_app_explanations_and_latex, which asserted obsolete wording; values_design_and_earlier_work_have_evidence covers source-based explanations instead.
- Replaced the_prompt_names_the_file_tools_and_leaves_save_as_to_the_user with save_docs_describe_app_autosave_and_finish_without_a_save.
- Added ask_docs_match_the_autonomous_decision_boundary_in_both_languages.
- Replaced arabic_rules_use_the_three_tools_and_preserve_measurement_safeguards with arabic_principles_preserve_the_expert_frame_and_autonomous_decision_boundary and arabic_takeoff_uses_available_method_evidence_and_discloses_disputes.
- Updated both_languages_keep_internal_names_and_paths_out_of_answers for the new wording, latest-message language and structured exclusions.

No builds, tests, formatting or validation checks were run, as requested. Read source and call sites only; word counts are textual counts, not model-behaviour evaluation. No dependencies, runtime gates or decision-source changes.

Two existing integration limits remain outside the named files: digest/note_scope.rs still parses legacy exclusion lines in takeoff_check.end_report, so tool documentation keeps that compatibility syntax internal while recording exclusion decisions as structured evidence and explaining them in readable prose. agent.rs MeasurementMethod::instruction still says to write standard rules from knowledge with clause placeholders when rules are missing, and tests/surface.rs asserts that wording. This conflicts with the new workflow and should be removed by the run responsible for agent instructions; neither file was changed here. No new exclusion API was invented.
