# Mizan conversation history: implementation and checks

## Changes by plan section

1. Agent memory: serialized model messages, exact signed thinking/raw data, provider state,
   picture-free snapshots, transcript replay data, resume notice and persistent phase error
   attribution. Existing compaction stays in place. Service endpoint changes also discard
   signatures; only an endpoint fingerprint is saved, never its address or credentials.
2. Desktop store: app-wide private atomic files, index-only listing, 100-chat retention,
   filtered list/resume/delete IPC, turn/reset/crash checkpoints, orphan recovery at startup,
   and Save As association. Reset paths converge on `AiRun::new_conversation` because tab
   operations already hold locks and did not all use the surveyed commands wrapper.
3. Window: Base UI History, current/all drawing filters, delete controls, Continue, drawing
   switching and autosave-aware opening, restored tool cards, English/Arabic and brand CSS.
4. Settings: approval modes and Approve/Change; local method envelope listing and reporting;
   development data isolation. No method content ships. The model-list test waits for the
   asynchronously loaded option before selecting it.
5. Documentation: CLAUDE.md and the short conversation-history note explain persistence,
   deletion, privacy, data homes, measurement rules and exact error attribution.

## Files touched

- `CLAUDE.md`
- `Cargo.lock`
- `crates/qiyas-ai/Cargo.toml`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/context.rs`
- `crates/qiyas-ai/src/conversation.rs`
- `crates/qiyas-ai/src/lib.rs`
- `crates/qiyas-ai/src/openai.rs`
- `crates/qiyas-ai/src/prompt.rs`
- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/prompt_en.txt`
- `crates/qiyas-ai/src/provider.rs`
- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/tests/agent.rs`
- `crates/qiyas-ai/tests/drafting_eval/phases.rs`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-core/src/check.rs`
- `crates/qiyas-desktop/src/ai.rs`
- `crates/qiyas-desktop/src/commands.rs`
- `crates/qiyas-desktop/src/conversations.rs`
- `crates/qiyas-desktop/src/files.rs`
- `crates/qiyas-desktop/src/lib.rs`
- `crates/qiyas-desktop/src/methods.rs`
- `crates/qiyas-desktop/src/settings.rs`
- `crates/qiyas-desktop/src/state.rs`
- `crates/qiyas-desktop/tests/settings.rs`
- `docs/2026-10-21-mizan-conversation-history-check.md`
- `docs/2026-10-21-mizan-conversation-history.md`
- `src/ai/AssistantPanel.test.tsx`
- `src/ai/AssistantPanel.tsx`
- `src/ai/ConversationHistory.test.tsx`
- `src/ai/ConversationHistory.tsx`
- `src/ai/PlanChecklist.tsx`
- `src/ai/history.css`
- `src/ai/messages.test.ts`
- `src/ai/messages.ts`
- `src/ai/useAssistant.ts`
- `src/api.ts`
- `src/app/App.test.tsx`
- `src/app/App.tsx`
- `src/app/Dialogs.tsx`
- `src/app/Panels.tsx`
- `src/app/useDrawings.test.tsx`
- `src/app/useDrawings.ts`
- `src/i18n/ar.json`
- `src/i18n/en.json`
- `src/shell/SettingsDialog.test.tsx`
- `src/shell/SettingsDialog.tsx`

## Verification

Every Rust build/test uses `CARGO_BUILD_JOBS=2`; every Vitest command uses `--maxWorkers=2`.
The requested full commands were each invoked once. Follow-ups target failures or tests that
Cargo did not reach after an earlier failure.

| Requested check | Real result |
| --- | --- |
| `cargo fmt --all` | Passed. Later Rust edits were formatted with rustfmt on their individual files. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Initially failed on ambiguous `.into()` in the new MText test. Removed it; follow-up Clippy for qiyas-ai and qiyas-desktop, all targets and their dependencies, passed without warnings. |
| `cargo test --offline -p qiyas-ai --lib --test surface --test drafting_eval --test authoring` | Library: 70 passed; 1 existing port-opening test failed with sandbox `PermissionDenied`, stopping Cargo before the integration executables. Follow-ups: authoring 16 passed, drafting_eval 4 passed, surface 42 passed. |
| `cargo test --offline -p qiyas-desktop` | Library: 90 passed; 1 existing MCP port-opening test failed with sandbox `PermissionDenied`. Follow-up integration tests: options 10 passed, settings 15 passed. |
| `node_modules/.bin/tsc --noEmit` | Passed. Not repeated after the later small UI control/localization fixes and regression additions, respecting the once-only request; those changes have targeted Vitest coverage. |
| `node_modules/.bin/vitest run --maxWorkers=2` | 829 passed, 1 failed because App's mock lacked the new measurement_methods response. Fixed the mock; App's 32 tests then passed. |

Follow-up commands and results:

- `cargo clippy --offline -p qiyas-ai -p qiyas-desktop --all-targets -- -D warnings`: passed.
- `cargo test --offline -p qiyas-ai --test surface --test drafting_eval --test authoring`:
  authoring passed 16; drafting initially passed 3 and failed 1 because the old fixture never
  approved its multi-phase plan. The fixture now asserts that drawing cannot begin before
  approval, sends approval, and retains all its original assertions.
- `cargo test --offline -p qiyas-ai --test surface --test drafting_eval`: passed 42 + 4.
- `cargo test --offline -p qiyas-desktop --test settings --test options`: passed 15 + 10.
- `vitest run src/app/App.test.tsx --maxWorkers=2`: passed 32 after the mock fix.
- Targeted `AssistantPanel`, `ConversationHistory`, `messages` and `useDrawings`: passed 58.
- Final Arabic and untitled-drawing regressions: Arabic passed; the new untitled test initially
  attempted a native file open for an unsaved drawing. It now uses the actual existing-tab
  `drawing:shown` flow, retains its Continue assertion, and passed on the targeted rerun.
- `vitest run src/shell/SettingsDialog.test.tsx --repeat 20 --maxWorkers=2`: unsupported CLI
  option in installed Vitest 5. The supported `--repeats 20` command passed all 9 tests.
- `git diff --check`: passed. `scripts/check.sh` was not run.

The sandbox failures, left unchanged for the reviewer, are
`models::tests::an_unreachable_service_falls_back_to_the_builtin_list` and
`commands::tests::the_mcp_server_follows_the_settings`; both failed opening local ports
with `Operation not permitted`. No tests were dropped or weakened.

## Limits and explicit decisions

- No windows or browser were opened. No hand-run Tauri test or live Gemini call was made;
  those are left to the reviewer as requested. No actual API key or real app settings/config
  was accessed. Tests use temporary directories and mock credentials.
- The supplied method envelope omits a version member. It is accepted as version 1;
  `version: 1` is also accepted and unsupported explicit versions are rejected. Rule contents
  stay opaque. A missing or empty rule array reports that rules are not loaded.
- Crash recovery resumes the latest saved checkpoint. Unfinished tool calls explicitly say
  the tool may already have run and must be checked before retrying. Streaming text after
  the last durable checkpoint can be lost on an abrupt process kill.
- Approval error attribution covers the existing drawing check rules and their analysis
  bounds. It does not claim to detect every possible defect. Phase baselines survive resume.
- No dependencies new to the workspace were introduced. qiyas-ai uses the existing locked
  workspace uuid dependency. Work is uncommitted; no add, commit or push was run.
