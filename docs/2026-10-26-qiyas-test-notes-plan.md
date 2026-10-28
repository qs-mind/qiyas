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
| W6 | Take-off checks: a blind second measurement before a quantity reaches the BOQ, a drawing-pack check card before take-off, an evidence PDF of marked quantities | take-off method review, 27 Oct |

W6 runs straight after W3, before W5. Take-off numbers are what QS Mind sells, and the
evidence PDF reuses the sheet work W3 has just finished.

## W6: take-off checks (added 27 October)

Added after a review of a published AI take-off method (plan first, measure in small chunks,
mark the evidence on the drawing, have a fresh reader check the count). Qiyas already plans
and works in phases, explains each number and shows it on the drawing, runs `verify`,
`brief_check` and `takeoff_check`, and keeps unverified numbers out of exports through
`quantity_gate`. Three pieces are missing:

1. **Blind second measurement.** Before a generated quantity enters the BOQ, a fresh Mizan run
   with no conversation history and read-only tools measures the same item again. It sees
   the item's description, unit and rule but not the first quantity. Code, not the model,
   compares the two within a stated tolerance per unit. If they disagree, the item becomes a
   drawing query with both readings and is not written as a BOQ row. Large drawings check the
   largest items by value first. The BOQ line and Mizan panel say "Second reading agrees",
   "differs" or "not checked"; no confidence words. Tests use the scripted provider.
2. **Drawing-pack check card.** One short card shown before take-off, not a settings page:
   drawing units known (INSUNITS not 0), annotation scale set, each PDF page vector or
   scanned and, if scanned, calibrated, page orientation, and sheet number and revision read
   by the register rules. Each line passes or names the fix. It builds on the existing unit,
   scan-detection and calibration code.
3. **Evidence PDF.** One PDF per take-off: each BOQ item's elements highlighted on its sheet,
   with the item number beside them and the "For review" notice, so the engineer can check
   by eye and send it to the client. It goes in the same export folder as the BOQ.

Later, not in W6: IFC import, and a simple 3D extrusion for concrete and formwork volumes on
varying levels.

Done means: a seeded disagreement never reaches the BOQ and appears as a query; a drawing with
INSUNITS 0 or an uncalibrated scan is flagged on the card before take-off; the evidence PDF of
the sample villa marks every BOQ item; `scripts/check.sh` passes; one independent check.

## Notes from the founder's live test (28 October 2026)

1. **Activity lines in plain language.** The founder sees "Read the drawing" and "Updated the
   drawing" and cannot tell what Mizan read or changed. He considers this his right. Every
   activity line should say, in the chat's language, what was read or done. For example: "Drew
   the ground-floor outer walls (4 walls, 25 cm)" or "Checked the room sizes against the
   Egyptian code: 2 notes". A generic verb with no object is not allowed.
2. **Draw at once, part by part.** The first run spent about 3 minutes reading knowledge and
   checking a paper plan before drawing anything. The prompt now says to plan briefly, then
   draw one floor and one part at a time, and to check each floor once its walls exist. This
   change is uncommitted until the founder's test ends.
3. **Feels stuck.** About 60 steps of 2 to 10 seconds each, with one long first think of about
   2 minutes, look like nothing is happening. This ties to note 1: plain progress lines would
   show the work moving.
4. **Opaque stop.** At 06:05 the ChatGPT-plan run stopped with only `chatgpt_failed` in the log
   and window: no HTTP status and no service message. `failure()` in
   `chatgpt_plan/responses.rs` maps an unknown error to `chatgpt_failed` and drops the body. Log
   the status and the service's error, as the OpenAI-compatible provider does. Show the founder a
   plain sentence and offer "Continue" when the error is temporary.
5. **Export without a path.** In the earlier run (DeepSeek, 05:15), Mizan's first BOQ export
   omitted the required `path` and was refused. That run then stopped on the service's 402
   "Insufficient Balance". A deliverable export should default its path to the drawing's export
   folder, so no call fails for a missing file name.
