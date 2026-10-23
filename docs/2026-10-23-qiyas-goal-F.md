# Goal F — plain language, Arabic and the window

Worktree: `.local/wt/goal-f`, branch `goal/f`. No core commands, quantities or drawing-check rules changed. No new dependencies, paid calls, commits or pushes.

The implementation covers the approved window changes. Desktop visual acceptance remains open: this sandbox refuses both local web-server sockets and Chromium startup. Component HTML, interaction tests and real headless drawing evidence are kept separately below so none is mistaken for a desktop screenshot.

## What changed

| Approved item | Before | After |
| --- | --- | --- |
| U1, U2, M7, X-D8: activity cards | Command names, English summaries, raw input in a hover tooltip, red recoverable failure | Short translated sentences from registry category/effect and structured result counts. A failed step followed by recovery in the same turn says “Fixing a problem…”. Raw input/result text is in a closed **Technical details** section and the Log. No JSON tooltip. |
| U7: answers | Prompts allowed implementation names in prose | Both prompts require surveying language, file names without paths, and no tools, commands, handles or internal field/block names. Live provider compliance still needs founder testing. |
| U17, U16 | Raw Thinking and a Log control in the chat header | Thinking does not render. Settings contains an optional support Log, loaded only when opened. |
| U4, U19, X-D1 | Arabic font depended on window language; several controls/headings missed it | Thmanyah Arabic Unicode ranges are first in all font stacks, including controls and monospace. Headings, legends and tabs enable swash in either window language. Latin keeps its existing face. |
| X-D2 | Font availability could silently change official installer typography | The existing sealed-font workflow now requires all three Thmanyah weights for official tagged releases. Forks and unsigned dry runs have an explicit fallback. See the release decision below. |
| U3, U5, X-D3 | First Latin identifier could set Arabic content direction | Messages use their prose language. Each list item and table cell has its own direction. Latin words, quantities and substituted names are isolated inside Arabic. |
| U20 | Several Arabic words for one concept | Glossary below applied to visible catalog entries. User text, layer names and wire identifiers remain intact. |
| U9, X-D7 | Caught service exceptions appeared directly in panels | One translation boundary handles known failure classes and a plain fallback for unknown ones. Raw text is available in support details. Already translated validation errors survive nested catches. |
| U10 | PDF transport marker and absolute path appeared in chat | File-name/page chips replace the marker, including old saved chats. Display text reduces local paths to file names. Stored transport data is preserved. |
| U11 | Approval followed window language | Approval and its posted sentence follow the conversation language. |
| U12, U13 | Wire enum names and hexadecimal handles | Shared plain kind names; counts and “Show on drawing” replace handles in ordinary panels. Diagnostic records remain in details. |
| U14, U15 | Evidence fields, raw formulas/notes and empty source subtitles | Explanation, generated quantities, recognition, project outputs and take-off panels use short labels. Internal expressions and raw notes are folded. Unknown source/basis filler is omitted. Script results show plain status, with commands/output folded. |
| U6, X-D4 | Twelfth layer silently created prefix folders | Flat list by default at every size. Remembered **Show folders** choice, folder icons, explicit folder tooltip and layer counts. Drawing ownership is unchanged. |
| U18, U22 | Hover controls covered labels; narrow foot clipped; tree focus could disagree with browser focus | Controls reserve space, the foot wraps, properties can shrink, and focus is synchronized. Arrows follow Arabic direction and leave property controls alone. Single row selection is explicit. |
| U21 | Delete all acted immediately | Confirmation gives the conversation count. Cancel or Escape preserves saved chats. |
| X-D6, X-D9 | Failed value was treated as already sent; merge required a mouse range | Failed cell text remains editable and can be submitted again. In-flight duplicates are blocked. Structural edits wait for cell saves and stop if the save fails. Shift+arrows select a rectangular range for Merge; grid columns preserve drawing order in either language. |

Card titles do not contain a command-name lookup table. New registered commands use their category and declared effect; argument-dependent effects, such as a purge preview, use the registry's existing effect function. Older stored cards without metadata get a generic translated sentence. Result counts come from data, never from parsing an English summary.

The error audit originally reported 96 conversion sites. This branch's baseline contains 88 exact `String(e|err|error)` sites across 44 files. The [inventory](../.local/goal-run/error-boundaries.json) records every original matching file and its ten retained internal conversions: assistant control codes, error classification and the agent/window reply. Those uses are translated at their display boundary. This is a presentation boundary, not a redesign of IPC errors. Unknown errors get a safe, plain message rather than a guessed diagnosis. The support buffer retains the latest 50 errors for this window session.

## Arabic glossary

| Concept | Arabic |
| --- | --- |
| Drawing | الرسم |
| Layer | طبقة |
| Block | بلوك |
| Item | عنصر |
| Sheet | لوحة |
| Take-off | الحصر |
| Technical details | التفاصيل التقنية |

Western digits remain in the interface; units and drawing names keep their spelling. Direction detection is a prose heuristic, so an unusually balanced bilingual sentence may still need manual review.

## Release font decision

The existing project stores an encrypted font bundle and has a release step to unlock it. This change uses that existing route and checks Regular, Medium and Bold before an official tagged release build continues. Unsigned dry runs can use the documented fallback, so an external pull request does not need a release secret. A local production frontend build is also checked for those font assets. No font files were added, and no keys or secrets were read.

For the founder: retain mandatory Thmanyah in official installers and ensure its existing distribution permission and release secret are in place. The sealed bundle proves storage, not distribution rights. The workflow has not been run on GitHub, and no installer was published. Forks without the font intentionally use the system fallback. Verifying release rights and an installed package is still a founder/release check.

## Checks

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo fmt --all` | Passed; subsequently edited Rust files were formatted with `rustfmt`, and the final Rust format check passed | [Log](../.local/goal-run/fmt.log) |
| Offline CLI build | Passed, debug information disabled | [Log](../.local/goal-run/build.log) |
| Workspace Clippy, all targets, warnings denied | Passed. First attempt ended with signal 143; the restarted check completed successfully | [Current log](../.local/goal-run/clippy.log), [interrupted log](../.local/goal-run/clippy-interrupted.log) |
| All `qiyas-ai` tests | Ran with `--no-fail-fast`: 183 passed; 35 local-port tests blocked; one prompt-text regression fixed with all 15 prompt rechecks passing; one pre-existing ignored test | [Log](../.local/goal-run/cargo-test.log) |
| Explicit `qiyas-ai` library, surface and agent tests | Surface: 54/54 passed. Agent: 2 passed, 25 blocked by ports. Library: 74 passed, one port restriction and the prompt-text regression corrected by the 15/15 recheck | [Log](../.local/goal-run/cargo-required.log) |
| Prompt checks after restoring the identity sentence | Passed: 15/15, including both languages and the 900-word cap | [Log](../.local/goal-run/prompt-recheck.log) |
| TypeScript `--noEmit` | Passed | [Log](../.local/goal-run/tsc.log) |
| Full frontend suite, two workers | Passed: 859 tests | [JSON results](../.local/goal-run/vitest-complete.json) |
| Production frontend build | Passed; all three Thmanyah OTF assets present. Large-chunk/dynamic-import warnings remain | [Log](../.local/goal-run/vite-build.log) |
| Baseline/current component capture job | Passed: both window languages | [Log](../.local/goal-run/dom-evidence.log) |
| Four-unit MCP mutations and saved-file reopen | Passed | [Job log](../.local/goal-run/jobs.log), [reopen log](../.local/goal-run/verify-saved.log) |
| Diff whitespace check | Passed | `git diff --check` |

The [blocked-test list](../.local/goal-run/sandbox-blocked-tests.json) names all 35 local-port failures. The socket error is `EPERM` / “Operation not permitted”. The complete crate command therefore exited 101; it is not reported as a clean pass. No test was disabled or made less strict. The prompt regression was an exact identity sentence removed while keeping the prompt below 900 words; that sentence was restored in the implementation, keeping the existing assertion. The English prompt is now 898 words.

Every Cargo invocation used the required shared target, two jobs, no debug information and no incremental build. No build cache was cleaned and no other build was stopped.

Tests that previously expected raw exceptions, enum names, hexadecimal labels, automatic folders, an in-chat Log or immediate deletion were updated because those were the audited wrong behaviors. Assertions for command payloads and quantities were kept. A socket-free scripted-provider regression also runs the real agent/session loop, checks the emitted category/effect/count and reads back the added entity. It checks presentation plumbing, not live model wording. Additional regressions cover same-value retry, duplicate pending edits, keyboard merge, Arabic folder arrows, mixed directions, hidden transport paths and recovery limited to the current turn.

## Before/after and end-to-end evidence

The [component capture job](../.local/goal-run/window.evidence.tsx) renders the baseline from `HEAD` and the changed real components with identical input, in English and Arabic. It saves standalone HTML, with closed details and local font links:

- [English before](../.local/goal-run/ui-before-en.html) and [English after](../.local/goal-run/ui-after-en.html).
- [Arabic before](../.local/goal-run/ui-before-ar.html) and [Arabic after](../.local/goal-run/ui-after-ar.html).

These are real component DOM snapshots, not screenshots or a running desktop session. They show the earlier automatic folders, raw card text/tooltip, and PDF path beside the changed flat list and readable cards. The captures cannot prove painted hover behavior, clipping, swash shaping or font loading.

The [headless job](../.local/goal-run/jobs.py) reuses the audit's [stdlib MCP client](../.local/goal-run/mcp_client.py). It launches the newly built `qiyas-cli mcp --drawing … --workdir …` separately in mm, cm, m and inches. Each job reads the registry, creates twelve layers, draws and measures a 4 × 3 m boundary, adds Arabic/English text and a table, checks that a failed cell write changed nothing, corrects that cell, merges a range and captures before/after PNGs. Every mutation is followed by entity/object/layer readback and a direct check of the intended result. No model or live account is involved.

The table portion follows the audit's `core_more_probes.py` edit-cell/readback pattern on an ordinary table. It deliberately does not fix generated schedule behavior owned by Goal B. The original UI findings were source-reviewed observations, not an executable browser audit suite; their regressions are now the component tests and capture job above.

All four jobs passed (76 attempted mutations, including four deliberate invalid-cell errors). Every attempt has the following three record snapshots. The saved files were reopened and the Arabic text, table data, layer names and merge width checked again. Save/export adds the normal measurement display layers, so reopening checks the required layer names rather than assuming the live-session layer count stays exactly 13.

| Drawing units | Measured area | Edited cell | Merge | Transcript and PNGs |
| --- | --- | --- | --- | --- |
| mm | 12 m² | غرفة مراجعة | 2 columns × 1 row | [Transcript](../.local/goal-run/mm/transcript.jsonl), [before](../.local/goal-run/mm/0004-before-0.png), [after](../.local/goal-run/mm/0103-after-0.png), [reopened](../.local/goal-run/mm/reopened/0007-after-auto-fit-0.png) |
| cm | 12 m² | غرفة مراجعة | 2 columns × 1 row | [Transcript](../.local/goal-run/cm/transcript.jsonl), [before](../.local/goal-run/cm/0004-before-0.png), [after](../.local/goal-run/cm/0103-after-0.png), [reopened](../.local/goal-run/cm/reopened/0007-after-auto-fit-0.png) |
| m | 12 m² | غرفة مراجعة | 2 columns × 1 row | [Transcript](../.local/goal-run/m/transcript.jsonl), [before](../.local/goal-run/m/0004-before-0.png), [after](../.local/goal-run/m/0103-after-0.png), [reopened](../.local/goal-run/m/reopened/0007-after-auto-fit-0.png) |
| in | 12 m² (floating-point difference below 0.00000000000001) | غرفة مراجعة | 2 columns × 1 row | [Transcript](../.local/goal-run/in/transcript.jsonl), [before](../.local/goal-run/in/0004-before-0.png), [after](../.local/goal-run/in/0103-after-0.png), [reopened](../.local/goal-run/in/reopened/0007-after-auto-fit-0.png) |

I inspected the before/after and reopened drawing PNGs. The final drawings show the rectangular boundary, readable Arabic/English label, corrected Arabic table cell and merged last row. The mm/cm after images are byte-identical. The drawings use the existing drawing renderer's font and do not prove the window uses Thmanyah.

Machine-readable evidence: [readbacks](../.local/goal-run/readback-summary.json), [mutation/capture inventory](../.local/goal-run/evidence-summary.json), [reopen job](../.local/goal-run/verify_saved.py), [reopen log](../.local/goal-run/verify-saved.log), and [CLI binary checksum](../.local/goal-run/qiyas-cli.sha256). A local copy of the freshly built binary keeps these jobs tied to this worktree while other agents use the shared target. `selection` correctly returns `needs_window` in all four headless sessions; the run does not claim to have inspected the live desktop selection.

## Remaining acceptance work

- Actual desktop PNGs and a visual check of narrow-panel hover, clipping, Arabic swashes and form controls. Vite failed to listen with `EPERM`; Chromium failed before rendering with sandbox socket/shutdown errors. Logs: [Vite](../.local/goal-run/vite.log), [Chromium](../.local/goal-run/chrome.log), [offline file attempt](../.local/goal-run/chrome-offline.log).
- A live Mizan conversation to assess generated prose. Prompt rules and offline event/render tests cannot prove every provider follows them. No paid calls were made.
- Founder confirmation of the existing font distribution permission and a release package check as described above.

## Files touched

The complete [file inventory](../.local/goal-run/files-touched.txt) lists 119 source, test, workflow and documentation files. Main implementation areas: `crates/qiyas-ai` presentation metadata and prompts; `src/ai`; `src/i18n`; `src/fonts/fonts.css`; `src/shell` panels and shared CSS; `src/boq`; caught-error boundaries in `src/app`; their frontend tests; `.github/workflows/release.yml`; and `CLAUDE.md`. Evidence scripts and logs stay under this worktree's ignored `.local/goal-run/`. The pre-existing `node_modules` and `src/fonts/thmanyah` symlinks are not changes to commit.
