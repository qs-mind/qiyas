# Qiyas test notes, 26 October 2026: plan

The founder tested Qiyas and Mizan for a day on his own computer (ChatGPT plan, gpt-6.1-sol).
All 69 notes, his interview answers, and reviews of each run are in
`.local/kareem-notes-2026-10-26.md` (git-ignored). The design mockups are in
`.local/design/2026-10-26-shell/`. His ruling: fix every problem, follow best practice,
improve the UI and UX, and make the app much lighter ("very, very heavy").

## Five problems

1. **Mizan cannot design or draft well.** It has no built-in knowledge of drawing standards,
   space planning or codes. The design is weak (35 m² lobby, kitchen bigger than living room,
   4 bedrooms sharing 2 baths). Elevations and sections are outlines only. It refuses whole
   disciplines because it treats them as "unverified".
2. **Mizan behaves badly.**
   - It asks several questions one at a time, in plain text.
   - It stops when it sees the cost advice, and stopped again after "continue and complete everything".
   - It re-reads results instead of acting, and never saves.
   - It delivers exports even when the checks failed.
   - It replies in the wrong language.
3. **Mizan is slow and expensive.** The last run took 37 minutes, 116 calls and 3.5 M input
   tokens, with only 7% of input tokens served from cache. The calls were 50 record reads,
   28 re-reads of shortened results and 28 command-schema lookups.
4. **Output quality is poor.**
   - The sheet layout is poor and the title block is mostly empty.
   - Text is unreadable when printed.
   - Areas show 3 decimals and no unit, and take-off IDs are printed on sheets.
   - Doors and windows have no tags.
   - The BOQ has 7 vague items, the workbook carries a raw JSON sheet, and exports land in different folders.
5. **The window needs a new shell, and the app is heavy.**
   - Build design direction B: one Mizan column whose artifact replaces the chat in place.
   - Bilingual slash commands.
   - A model icon inside the prompt box.
   - A dot-only file tab.
   - A ribbon that collapses on double-click.
   - Questions as an unboxed numbered list, where "Other" is typed in the composer.
   - No confidence percentages.
   - No Projects tab and no Comments tab.
   - Shortcuts that give visible feedback, with an audit against AutoCAD.
   - Save feedback.
   - Fix the scrollbar that overlaps the layout tabs.
   - Make MCP easy to turn on.
   - Show method names without "(RICS)".

## Rules

- **Responsibility notice.** Qiyas draws. It does not calculate loads, do design calculations
  or give approvals; the engineer and the company do. Every output says "For review".
  Assumptions are written on the sheet and listed in the Mizan panel. This is QS Mind's policy
  for every product.
- **No paid model runs.** Do not run Mizan against a paid model. Use the scripted provider and
  offline replays.
- **One build at a time.** The computer has 7.3 GB of RAM, so never run two builds, test suites
  or dev servers together.
- **Commits.** Commit to `main` after `scripts/check.sh` passes, with one commit per finished item.

## Order

Each workstream ends with evidence and one independent check.

| # | Workstream | Notes |
|---|---|---|
| W1 | Mizan behaviour and cost: question tool with options, never stop a requested job for cost, a finish gate tied to the plan, fewer reads, cache reuse, save at phase ends, reply language | 2–5, 7–12, 41–44, 48–53, 64–66 |
| W2 | Performance: measure startup, memory, frame time, IPC and render in a release build; fix the largest costs | founder's ruling |
| W3 | Output quality: sheet composition, title block, text sizes, decimals and units, no internal IDs, door and window tags, itemised BOQ, export folder, block exports with failed checks | 21–39, 54–62, 68 |
| W4 | Window: shell B and every UI note, shortcuts, save feedback | 1, 6, 9, 18–19, 47, 67, 69, interview |
| W5 | Built-in drafting and design knowledge as data: sheet sets, view standards, space-planning checks, Egypt/KSA/UAE code register | 13–17, 43, 45, 57, 60–63 |
