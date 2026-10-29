# Agent approval and decision corrections, 29 October 2026

This correction supersedes the approval-removal and answer-keyword behavior described in the earlier agent-loop note.

1. Restored HEAD approval behavior for PerJob and PerPhase, the AwaitingApproval event, native-provider claude_approval refusal, approval-message recognition, and the selected approval mode in state. Autonomous remains the default. Calls after an approval boundary receive the waiting result in English and Arabic.
2. Removed asks_or_blocked and all its callers. A structured ask suppresses the native finish reminder; answer wording does not. User stop requests still suppress it. MCP uses only the user's stop request. The one-note limit remains.
3. Continuation guidance in English and Arabic preserves open scope and unanswered questions, asks only about the idea, and assigns technical design decisions to Mizan with an engineering basis and a recorded reason. Updated the conflicting MCP startup guidance to the same definition.
4. Removed the stray blank lines after plan_watch status and judgement budget.

Files changed for this correction: crates/qiyas-ai/src/{agent,judgement,mcp,plan_watch}.rs; existing tests/{surface,w1,dashes}.rs and tests/drafting_eval/phases.rs. This required documentation note is the only file outside that source/test scope. No frontend or prompt files changed in this correction.

Tests added:
- continuation_assigns_technical_decisions_and_preserves_idea_questions: both languages and decision records.
- a_structured_ask_suppresses_the_gate_without_using_its_note: structured state controls the gate.
- approval_settings_answer_remaining_calls_without_running_them: no drawing mutation while awaiting approval, with bilingual results.
- structured_ask_with_open_phases_ends_without_a_reminder: ask ends the turn without a retry.

Tests restored or changed:
- a_local_agent_is_not_retried_or_sent_the_cancelled_request_again: restored HEAD claude_approval assertions and removed the legacy-settings loop.
- approval_modes_pause_at_the_job_and_phase_boundaries: replaces legacy_approval_modes_do_not_pause_autonomous_work; restored approval behavior while retaining the single finish reminder.
- plans_and_approval_titles_use_plain_dashes: restored approval-event coverage in place of status-title coverage.
- villa_in_phases_joins_walls_and_recovers_using_the_wall_read: restored initial approval and subsequent continuation.
- skipped_phases_finish_but_plain_questions_do_not_suppress_the_reminder: replaces the old closing-question bypass assertion.
- blocked_answer_text_gets_one_reminder_and_status_uses_the_latest_message_language: replaces the keyword-based bypass assertion.
- continue_resumes_the_open_phases_instead_of_starting_over and scoped_continue_does_not_inject_unrelated_open_phases: use structured asks between turns.
- the_finish_operation_returns_advice_without_refusing: a question in the finish answer still receives the single note.
- finish_returns_separate_status_and_a_note_regardless_of_answer_wording: replaces the MCP blocked-answer bypass test and covers a user stop.

No test cases were dropped without replacement. Existing integration files are already registered in tests/all.rs. Callers and affected assertions were reviewed by reading. No build, test, formatter or other check was run; compilation and execution remain unverified. No engineering values were guessed and no decision is pending. Work remains uncommitted.
