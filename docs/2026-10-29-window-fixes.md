# Window fixes, 29 October 2026

## Changes by task item

1. The non-passive wheel listener is on the drawing interaction root, including
   TraceCard, ConstraintBar, AlongPrompt, InPlaceText and ImageDisplayPanel. It
   consumes drawing zoom events and lets scrollable overlay controls scroll,
   containing their wheel at the boundary. Split views retain their own root.
   Both chat containers contain overscroll. Upward wheel or navigation-key intent
   immediately releases chat following, including a one-pixel movement near the end.
2. Tool cards use localized operation subjects, including measurement setup and
   each exported report. Batch exports describe their contents. Existing geometry
   metadata retains its counts and scope. Unknown command, object and collection
   identifiers use translated generic words; raw inputs remain in support details.
   English and Arabic strings were added together.
3. Partial: the frontend accepts `tool_progress {id, completed, total}` and an
   optional `tool_call.progress` with the same counts. Running cards show, for
   example, `3 of 10` or `3 من 10`. Invalid counts and updates after completion are
   ignored. Reattachment replays progress. No count is inferred from elapsed time,
   plan phases or the number of input commands.

   There is no producer for this event in the current backend. The agent has only
   ToolCall and ToolResult, and its batch loop in qiyas-ai/src/surface.rs has no
   progress callback exposed to desktop. Truthful live intermediate counts require
   a change in qiyas-ai, which this task explicitly excludes. The stated desktop
   exception is also limited to item 4. No Rust files were edited. The pending scope
   decision must cover the agent's batch callback, not just a desktop event name.
4. The ribbon measures all translated tab labels, the More label, quick access,
   app name, collapse control and available row width. Only overflowing tabs go
   into More. ResizeObserver and font readiness update the measurement. The
   measurement copies have no focus targets and are hidden from accessibility.
   Reading order, selection, keyboard navigation and Arabic mirroring are retained.
5. Opening a drawing restores its latest saved conversation when its workspace is
   empty and idle. Existing or running conversations are retained. A deliberate
   New chat stays empty when switching away and back. Explicit history selection
   is serialized after an automatic resume so the chosen conversation wins.
   Opening a conversation first focuses an open drawing, including untitled tabs,
   or opens its drawing. When its original file is absent, its main autosave or
   latest available older copy is recovered first. If neither drawing nor autosave
   exists, the conversation is not attached to an unrelated drawing; a bilingual
   notice explains why. Legacy chats with no drawing identity cannot be paired.
6. Layer names occupy one line with an end ellipsis. The existing title retains
   the whole name on hover, including bidirectional names.

## Files touched by this task

Production:

- src/app/Panels.tsx
- src/app/useDrawings.ts
- src/viewport/Viewport.tsx
- src/ai/AssistantPanel.tsx
- src/ai/useStickToBottom.ts
- src/ai/ToolCard.tsx
- src/ai/operationActivity.ts (new)
- src/ai/messages.ts
- src/ai/useAssistant.ts
- src/api.ts
- src/i18n/en.json
- src/i18n/ar.json
- src/shell/Ribbon.tsx
- src/shell/ribbonLayout.ts
- src/shell/theme.css

Tests:

- src/viewport/Viewport.test.tsx
- src/ai/AssistantPanel.test.tsx
- src/ai/ToolCard.test.tsx (new)
- src/ai/messages.test.ts
- src/ai/workspaces.test.tsx
- src/app/useDrawings.test.tsx
- src/app/App.test.tsx
- src/shell/ribbonLayout.test.ts
- src/shell/Ribbon.test.tsx
- src/shell/LayersPanel.test.tsx

Documentation: docs/2026-10-29-window-fixes.md, explicitly requested outside src.
Other concurrent edits in shared files were preserved. PlanDock changes and agent
status/approval changes in shared files belong to other runs.

## Test changes

Added regressions for overlay zoom and scroll boundary ownership, wheel-listener
cleanup, immediate wheel and keyboard release of chat following, English and
Arabic operation labels and export subjects, untranslated identifier suppression,
reported progress display and reducer replay, measured tab fitting and resize in
both languages, automatic named/untitled chat restoration, deliberate new-chat
preservation, and recovery from main or older autosaves.

Existing tests changed:

- AssistantPanel's transcript card now expects the specific measured-area label
  instead of generic updated-measurements wording.
- Ribbon's roving-tab test expects all eight tabs in the unmeasured test layout;
  Output replaces More as its last keyboard stop.
- Ribbon's Arabic-order test expects all eight translated tab names.
- Ribbon's CSS assertion follows the shared real/measurement tab selector; tab
  helpers select the real More control, excluding its hidden measurement copy.
- LayersPanel's wrapping test now requires nowrap, hidden overflow and ellipsis,
  while retaining its complete hover title.
- useDrawings' missing-drawing test rejects attaching the chat to the current
  unrelated drawing and expects the new notice.
- App's untitled Continue-offer test now expects the conversation to open
  automatically; its IPC mock retains the resumed transcript.

No tests were deleted. No builds, tests, type checks, browser sessions or other
checks were run, as instructed. The code and call sites were reviewed by reading.

## Remaining limitations

Item 3 is not complete until the out-of-scope backend emits actual step progress.
Unknown future operations have plain generic labels until specific words are
added. A conversation with no retained drawing path cannot be reconstructed from
its prose. Recovery uses existing desktop APIs and their identity migration.
All changes remain uncommitted; no dependencies were added.
