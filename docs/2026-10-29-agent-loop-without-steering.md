# Agent loop without steering, 29 October 2026

Implementation follows the founder's ruling: autonomous execution does not authorize missing facts or engineering decisions. No build, formatter, type check, test, browser, or model run was executed. Changes remain uncommitted.

## Changes by task item

1. The finish gate supplies at most one factual open-phase note per turn, even after plan updates. Questions and explicit blockers bypass it. Skipped phases are excluded. No instruction to assume remains in this gate.
2. Only unqualified continuation activates the whole open plan. Narrow requests stay with the model; continuation guidance preserves unanswered questions and requires permission for assumptions.
3. Ask accepts 1 to 20 questions, up to 600 characters each, with 0 to 6 optional choices. Free text remains available. Documentation asks for missing facts together at planning and again for new gaps, with recommendations only when supported.
4. Country information distinguishes Settings, method-name inference needing confirmation when material, and unset values requiring a question when material.
5. Mixed-family batches, batches above the 12-command advisory threshold, missing plans, and absent phase checks produce information rather than workflow refusals. Multiple active phases are valid. Legacy approval settings no longer pause native or local-provider turns. The actual 200-command batch transaction limit, schema validation, ownership, stale revisions, undo and path confinement remain unchanged.
6. Five identical edit failures suppress only that call on subsequent attempts at the same revision. Its failure count and previous error are returned. Other calls and the turn continue. Failed reads and successful no-change calls are not failed edits.
7. Removed forced language-rewrite requests and the unused mismatch classifier.
8. Open phases, requirements, automatic save success and save failure are separate `status` transcript events, with the latest user-message language. They are never appended to assistant prose or native model history. The window renders a small distinct block after the answer; existing chats still replay. MCP finish returns `statuses` separately and preserves the submitted answer.
9. Brief watchers consume raw structured results before formatting. Requirements retain explicit evidence handles. Known touched evidence needs a recheck; uncertain scope may need one. Unrelated known handles do not invalidate a requirement. Compact reports retain requirement identities and outcomes; full reports retain evidence.
10. Task classification only selects guidance and information; it cannot refuse edits or require a workflow.
11. Qiyas prefixes actual quantity export filenames with `DRAFT ` for outstanding measurement errors, unmatched schedules, unmeasured elements or disputed second readings. Results include the outstanding categories and counts. Filename wording cannot bypass this. Default deliverable exports still avoid collisions, and actual output paths still undergo confinement.
12. Efficiency and churn notes retain observed counts and remove prescribed editing or reading strategies. MCP startup guidance describes these facts without imposing phase order or editing strategy.

## Files touched by this task

- `crates/qiyas-ai/src/`: `agent.rs`, `plan_watch.rs`, `brief_check.rs`, `judgement.rs`, `takeoff_phase.rs`, `surface.rs`, `churn.rs`, `advice.rs`, `mcp.rs`, `result.rs`.
- `src/ai/`: `messages.ts`, `PlanDock.tsx`, `PlanDock.test.tsx`, `AssistantPanel.tsx`; `src/i18n/en.json`, `src/i18n/ar.json`.
- Minimal shared-contract exception: `src/api.ts`, adding the status event and correcting the question comment.
- Existing test exceptions: `crates/qiyas-ai/tests/agent.rs`, `surface.rs`, `w1.rs`, `goal_a.rs`, `dashes.rs`, `drafting_eval/phases.rs`, `w3.rs`.
- Only now-false ask limits and export claims changed in `prompt_en.txt` and `prompt_ar.txt`. Workflow prompt files, `blind_reader.rs`, and the second-reading trigger were not edited.
- This note. Other working-tree changes belong to concurrent runs.

## Test changes, not executed

Added inline tests for country provenance, raw large brief reports reaching the watcher, failed reads lacking edit fingerprints, selective/uncertain requirement invalidation and restore, compact versus full brief evidence, MCP repeated-call isolation, and separate Arabic MCP finish status.

Added W1 regressions for blocked answers and latest-message language, narrow continuation, failed reads and successful no-change calls, and multiple active phases. Expanded ask coverage to optional choices and the 20/600/6 boundaries. Added PlanDock tests for distinct status rendering, Arabic direction, English/Arabic save labels, replay and legacy transcripts.

Changed existing tests:

- `plan_watch`: `the_finish_gate_names_open_phases_and_lets_go_after_two_refusals_or_a_stop` now checks one note per turn; `an_identical_plan_does_not_hide_unfinished_work` excludes skipped phases from statuses.
- `judgement`: `continue_and_stop_are_read_from_short_messages_in_both_languages` no longer treats permission to assume as permission to resume everything. Deleted `an_answer_in_the_wrong_language_is_found_but_names_do_not_count` with its removed classifier.
- `takeoff_phase`: `families_and_limits_cover_nested_string_and_typed_commands` checks advice rather than refusal.
- `agent` unit tests: `a_local_agent_is_not_retried_or_sent_the_cancelled_request_again` now permits legacy approval settings; integration `identical_malformed_input_stops_on_sixth_attempt` now expects only call refusal and normal completion.
- `surface`: `plan_is_discoverable_validated_and_never_changes_the_drawing_or_undo` permits multiple active phases; `approval_modes_pause_at_the_job_and_phase_boundaries` now checks no pauses; `identical_ineffective_edits_advise_then_stop_only_the_sixth_attempt` uses real failed edits and normal completion; `goal_d_budget_and_workflow_follow_the_job_and_chat_language` checks factual cost notes; `quantity_guard_refuses_mixed_batches_and_planless_edits_once` now checks execution with advice; `quantity_three_phases_generate_boq_and_continue_resumes_active_phase` checks scope-preserving wording. `takeoff_export_reports_manual_generated_and_dimension_rows` and `evidence_export_is_available_through_the_agent_surface_beside_the_boq` follow the actual returned draft path.
- `w1`: `a_bad_question_is_an_error_the_model_can_fix_in_the_next_request` uses an oversized question, since one option is valid; `ask_is_an_operation_with_a_contract_and_never_a_drawing_edit` covers the new boundaries; `ending_with_plan_phases_open_is_sent_back_naming_them`, `the_finish_gate_lets_go_when_the_user_said_stop_and_after_two_refusals`, and `the_finish_operation_is_refused_while_phases_are_open` now assert advisory finish behavior; `the_cost_note_is_given_once_and_never_says_to_stop` and `continue_resumes_the_open_phases_instead_of_starting_over` assert neutral wording. Replaced `an_answer_in_the_wrong_language_is_asked_for_again_once` with no-rewrite coverage; extended `a_finished_phase_that_changed_a_titled_drawing_is_saved` to exclude save text from model history; replaced `quantities_are_not_exported_after_a_failed_check_unless_named_a_draft` with automatic-prefix and collision coverage.
- `goal_a`: `stale_plan_unmet_brief_and_blind_state_reach_the_final_answer_and_resume` now expects separate statuses and uncertain rechecks after resume.
- `dashes`: `plans_and_approval_titles_use_plain_dashes` now checks plan/status titles, without an approval event.
- `drafting_eval/phases`: `villa_in_phases_joins_walls_and_recovers_using_the_wall_read` completes autonomously rather than requiring initial approval.
- `w3`: the existing villa export assertion accepts Qiyas's DRAFT prefix; its second-reading tests and setup were not changed.

All Rust tests were added to existing source modules or already-registered integration files; no new `tests/all.rs` entry was needed. The removed language-classifier test is the only deleted test; other obsolete behavior assertions were replaced.

## Limits and decisions

Call sites and affected assertions were reviewed by reading only. Compilation, formatting and runtime behavior remain unverified under the founder's no-checks rule. Blocker recognition in free-form final prose is a conservative English/Arabic heuristic; structured ask always ends directly. Exact requirement invalidation depends on explicit evidence handles and complete mutation handles; unknown or aggregate evidence is labelled uncertain. External MCP clients that supply no user-turn text expose no latest user message to Qiyas, so finish status language falls back to the answer's language. No engineering values or scope decisions were invented.
