# Goal I — panels and tables a QS can read

Worktree: `.local/wt/goal-i`, branch `goal/i`. No provider controls, prompts, agent execution, drawing geometry or quantity calculations changed. The small Rust change adds one saved setting. No new dependencies, credentials, paid calls, commits or pushes.

## What changed

| Item | Before | After |
| --- | --- | --- |
| I1: table columns | Fixed starting widths hid quantities and column titles. Conflicts clipped prose to two lines. Take-off rows reserved 72 px even for a short description. | Headers and rendered cells use cached canvas text measurement with their computed font. Widths have minimums and a cap; descriptions share the remaining space. All rows wrap and use the virtualiser's measured height. User widths override automatic widths, including descriptions; double click restores automatic sizing. Actions occupy their own column instead of covering the last value. |
| I1: notes and totals | A markup appeared as `0.000 qty.markup`, including in Totals. | The row says Note, with no quantity unit. Totals count notes separately. Measured totals retain their values, wrap in a narrow panel, and isolate numbers in Arabic. |
| I2: tabs | Folding the selected tab replaced the control's value with a sentinel that had no corresponding panel. Mizan and its busy mark disappeared into the menu. | The real selected value is retained. The active tab and Mizan stay visible. Choosing a menu item shows its tab and body; opening the menu leaves the current body visible. |
| I3: Arabic | The founder saw raised words with tanween. | Added a complete Thmanyah face for Arabic prose, explicit baseline alignment and inherited line height for inlines, and explicit swash-off for body text. Headings retain swash and Latin identifiers retain bidi isolation. **The painted defect is not verified as fixed; see the investigation below.** |
| I4: activity | Every tool had Technical details. Recovered failures each retained a card. | Show technical details is off by default, saved through the existing settings IPC to `settings.json`, and applied after Save and on restart. Tool, review-plan and refusal disclosures obey it. Recoverable failures share one calm line per turn; support mode exposes their grouped diagnostics. Unresolved failed steps and actionable alerts remain visible. |

Only a later successful retry with the same tool and operation label marks a failed card as recovered. A successful unrelated read cannot hide a failed edit. While a turn is running, failures share “Fixing a problem…”. A stopped turn without a matching success shows its failed cards again. The transcript and support Log retain the original records. There is no change to tool execution or the agent's recovery decisions.

Existing tests that expected folded Mizan, a selected More tab, or default diagnostic disclosures were updated because they encoded the reported wrong behaviour. Support-mode tests still require the original diagnostic content. No quantity expectations were relaxed.

## Arabic investigation and limits

The exact sentence from the founder's log, `أهلاً! ما في رسمة مفتوحة حالياً. عندك خيارين:`, renders as one text node in both the before and after Markdown DOM. Neither raised word is inside a `bdi`, span, superscript, subscript, or style attribute. Goal F's `bdi` isolation affects Latin names and numbers. The Markdown source does not remove or transform the tanween.

A local HarfBuzz probe of the actual Thmanyah Regular font places every base-letter glyph at a zero vertical offset, with only the combining marks raised. Removing tanween leaves the base glyph offsets unchanged. The [local Pango/HarfBuzz raster probe](../.local/goal-run/typography-probe.png), which I inspected, also shows these words on the baseline. This excludes a simple whole-word vertical offset in those shaping paths; it does not establish WebKit's font fallback or baseline behaviour. Evidence: [probe script](../.local/goal-run/font-probe.py), [glyph output](../.local/goal-run/before/font-shaping.txt).

The existing font faces use Unicode ranges. The new Arabic prose face uses the same local Thmanyah files without a range restriction, avoiding an unnecessary font-subset boundary for marked text. Body swash is explicitly disabled, headings enable it, and inlines inherit line height and baseline alignment. These are defensive typography changes, **not a confirmed root-cause diagnosis**. Browser font fallback or marked-run metrics remain suspects.

Chromium failed before rendering with `setsockopt: Operation not permitted` ([log](../.local/goal-run/browser.log)). A retry disabling Crashpad also failed before rendering with `shutdown: Operation not permitted`, exit 134 ([log](../.local/goal-run/browser-no-crashpad.log)). WebKitGTK 2.54 is installed, but `Gtk.init_check()` cannot open a display ([probe](../.local/goal-run/webkit-probe.json)). The built desktop app was also launched with fresh data directories and an isolated session bus; it exited 101 with `Failed to initialize GTK` ([launch script](../.local/goal-run/start-desktop.sh), [log](../.local/goal-run/desktop-start.log)). No real window screenshot is claimed. The founder must still verify `أهلاً` and `حالياً`, including bold text and list items, in the installed WebKitGTK app. The DOM and CSS tests cannot prove painted baselines, frame rate, or pixel clipping.

## Before and after evidence

I looked at the founder's `takeoff-columns.png`, `side-tabs-empty.png`, `arabic-raised.png` and `chatgpt-missing-replies.png` in `.local/audit/live-2026-10-24/` before editing.

The [component evidence job](../.local/goal-run/window.evidence.test.tsx) renders actual TakeoffPanel, SideTabs and AssistantPanel components, plus BOQ and conflicts after the shared grid change, in both languages at 260, 340 and 600 px. Standalone HTML includes the actual CSS and local font links. The matching JSON records visible panels, folded tabs and diagnostic/error-card counts. These are DOM snapshots, **not browser screenshots**; jsdom does not measure glyphs, so their recorded column widths are fallback widths. Separate canvas-stub tests verify automatic sizing and bounded measurement work. Additional BOQ/conflicts snapshots are `after/tables-{en,ar}-{260,340,600}.html`.

| Evidence | Before | After |
| --- | --- | --- |
| English, 260 px | [HTML](../.local/goal-run/before/ui-en-260.html), [facts](../.local/goal-run/before/ui-en-260.json) | [HTML](../.local/goal-run/after/ui-en-260.html), [facts](../.local/goal-run/after/ui-en-260.json) |
| Arabic, 260 px | [HTML](../.local/goal-run/before/ui-ar-260.html), [facts](../.local/goal-run/before/ui-ar-260.json) | [HTML](../.local/goal-run/after/ui-ar-260.html), [facts](../.local/goal-run/after/ui-ar-260.json) |
| Other widths | `before/ui-{en,ar}-{340,600}.{html,json}` | `after/ui-{en,ar}-{340,600}.{html,json}` |

At 260 px, before: zero visible panel bodies, Mizan folded, three diagnostic disclosures, and two failed cards. After: one visible body, Mizan visible, no diagnostic disclosures by default, and one calm recovery line. The note no longer shows a zero quantity or an untranslated unit. The original Arabic words and tanween are preserved in both snapshots.

## Checks and headless jobs

| Check | Real result | Evidence |
| --- | --- | --- |
| Format | Passed, exit 0 | [log](../.local/goal-run/fmt.log) |
| TypeScript | Passed, exit 0 on current source. An early unrestricted compiler attempt was killed with exit 137 during concurrent builds; the successful run limits the native compiler to two processors and a 512 MiB memory target. | [log](../.local/goal-run/tsc-final.log) |
| Full frontend suite, two workers | 897 passed, 3 failed, exit 1. Two unchanged ribbon tests exceeded their 5-second timeout under load. A new recovery assertion ran while its helper was being updated. | [log](../.local/goal-run/vitest.log) |
| Fresh frontend recheck | All 75 tests passed, including both ribbon failures, the recovery assertion, transcript tests and Goal F regressions; no timeouts were increased. | [log](../.local/goal-run/vitest-recheck.log) |
| Targeted table checks | All 43 passed: shared grid, take-off, BOQ and conflicts. The full suite also passed the subsequently added remembered-description-width test. | [log](../.local/goal-run/grid-final-targeted.log) |
| Component DOM evidence | Before: 6 passed. After: 18 passed, both languages and all three widths, plus actual take-off, BOQ and conflict rows read back from headless jobs. | [before](../.local/goal-run/before/dom.log), [after](../.local/goal-run/after-dom.log) |
| Production frontend build | Passed, exit 0. Existing large-chunk and mixed static/dynamic import warnings remain. | [log](../.local/goal-run/vite-build-final.log) |
| CLI build | Passed; 34m 41s for the cold build. | [log](../.local/goal-run/build-cli.log) |
| Table control and row-key rechecks | 17/17 passed after memoising row keys; 35/35 passed after measuring the current input/selected value rather than hidden options. TypeScript and the frontend build passed again after these changes. | [keys](../.local/goal-run/grid-key-recheck.log), [controls](../.local/goal-run/grid-controls-recheck.log) |
| Headless jobs | Four unit jobs passed, saved rows verified; sample BOQ job passed with the two expected conflicts and workbook quantities. | [units](../.local/goal-run/jobs.log), [sample](../.local/goal-run/sample-boq.log) |
| Desktop build | Passed, exit 0, with the built frontend embedded via `tauri/custom-protocol`. | [log](../.local/goal-run/build-desktop.log) |
| Built desktop launch | Exit 101: GTK could not initialise a display in the sandbox. No app window was captured. | [log](../.local/goal-run/desktop-start.log) |
| Workspace Clippy, all targets, warnings denied | Passed, exit 0. | [log](../.local/goal-run/clippy.log) |
| Desktop crate tests | Exit 101: 125 passed, 2 failed at local socket binding. Library: 97/98; options: 10/10; settings: 18/19. The new preference round-trip test passed. | [log](../.local/goal-run/cargo-test.log), [summary](../.local/goal-run/rust-test-summary.json) |
| Whitespace check | `git diff --check` passed, exit 0. | [log](../.local/goal-run/diff-check.log) |

The two Rust failures are `commands::tests::the_mcp_server_follows_the_settings` and `credentials_follow_the_saved_address_and_never_a_draft`. Both stop at `TcpListener::bind("127.0.0.1:0")` with `PermissionDenied: Operation not permitted`, before their assertions run. Their tests were not weakened or skipped.

Every Cargo command uses this worktree's target directory, two build jobs, no development/test debug information and no incremental build. No cache was cleaned and no other agent's process was stopped. No `qiyas-ai` or prompt source changed, so the extra AI crate tests are not applicable.

## End-to-end headless evidence

The newly built [CLI checksum](../.local/goal-run/qiyas-cli.sha256) identifies the executable used by the [four-unit job](../.local/goal-run/jobs.py) and the copied audit [stdlib MCP client](../.local/goal-run/mcp_client.py). Each process runs `target/debug/qiyas-cli mcp --drawing … --workdir .local/goal-run/<unit>` with no settings or model access.

Each job draws a 4 × 3 m outline, measures its perimeter and area from its actual handle, and adds a bilingual note. All 16 mutations have entity/object/layer readbacks plus direct checks of the changed geometry or take-off row. Results in all units: **14 m, 12 m², one note**. The before PNG shows the unmeasured outline; the after PNG shows the violet measured outline and the note's point. I inspected all eight images. These are drawing captures, not UI panel screenshots.

| Units | Transcript | Before | After |
| --- | --- | --- | --- |
| mm | [JSONL](../.local/goal-run/mm/transcript.jsonl) | [PNG](../.local/goal-run/mm/0011-before-quantities-0.png) | [PNG](../.local/goal-run/mm/0028-after-quantities-0.png) |
| cm | [JSONL](../.local/goal-run/cm/transcript.jsonl) | [PNG](../.local/goal-run/cm/0011-before-quantities-0.png) | [PNG](../.local/goal-run/cm/0028-after-quantities-0.png) |
| m | [JSONL](../.local/goal-run/m/transcript.jsonl) | [PNG](../.local/goal-run/m/0011-before-quantities-0.png) | [PNG](../.local/goal-run/m/0028-after-quantities-0.png) |
| in | [JSONL](../.local/goal-run/in/transcript.jsonl) | [PNG](../.local/goal-run/in/0011-before-quantities-0.png) | [PNG](../.local/goal-run/in/0028-after-quantities-0.png) |

The real readback rows also render through TakeoffPanel in [mm](../.local/goal-run/after/headless-takeoff-mm.html), [cm](../.local/goal-run/after/headless-takeoff-cm.html), [m](../.local/goal-run/after/headless-takeoff-m.html) and [in](../.local/goal-run/after/headless-takeoff-in.html) DOM snapshots: the numbers remain complete and the markup row says Note.

[Live readback summary](../.local/goal-run/readbacks.json), [saved-project summary](../.local/goal-run/reopen-readbacks.json), [job log](../.local/goal-run/jobs.log), and [persistence check](../.local/goal-run/reopen.py). The saved `input.qiyas` files retain all three rows in every unit. Fresh standalone MCP processes reopen the drawing geometry and save inspected PNGs under `<unit>/reopened/0005-reopened-0.png`; their transcripts are alongside them. Standalone MCP deliberately ignores adjacent projects, so these fresh processes are not claimed to reload take-off state.

Two harness corrections are retained for transparency. An initially empty DXF was refused as having no usable drawing data ([attempt](../.local/goal-run/empty-input-attempt/stderr.log)); the final jobs reuse the audit's minimal seed drawing. An extra reopen assertion initially expected automatic project loading ([attempt](../.local/goal-run/reopen-attempt.log)). That was the wrong expectation for `open_standalone`, so the final persistence check reads the saved project and separately reopens the drawing. No engine behaviour or quantity assertion was changed.

The copied [sample BOQ job](../.local/goal-run/sample_boq_job.py) also passed: six measured rows (42 m, 14 m, 58.29 m², 22.09 m², 3 doors and 5 windows), six verified links, and exactly the expected conflicts for A.4 and B.2. It reads back take-off, BOQ and conflicts after every operation and checks exported workbook quantities. Evidence: [transcript](../.local/goal-run/sample-boq/transcript.jsonl), [verified results](../.local/goal-run/sample-boq/verified-results.json), inspected [before PNG](../.local/goal-run/sample-boq/0010-sample-before-0.png) and [after PNG](../.local/goal-run/sample-boq/0126-sample-plan-0.png). The same saved data renders in the actual panels in [English](../.local/goal-run/after/headless-boq-en.html) and [Arabic](../.local/goal-run/after/headless-boq-ar.html), with quantity columns only. The first attempt reached the correct results but stopped at the copied XLSX reader's old audit-directory guard; its replacement remains confined to this worktree's evidence directory. The successful rerun and the prior attempt are both retained.

## Files touched

[Complete file inventory](../.local/goal-run/files-touched.txt).

- Shared table: `src/ui/DataGrid.tsx`, `textWidth.ts`, `DataGrid.test.tsx`; Take-off panel and tests; `src/shell/theme.css`.
- Tabs and settings wiring: `src/app/SideTabs.tsx` and tests, `Panels.tsx`, `App.tsx`, `Dialogs.tsx`, `useOptions.ts` and its tests.
- Chat: `src/ai/AssistantPanel.tsx`, `ToolCard.tsx`, `PlanChecklist.tsx`, the presentation-only `recoveringCard` helper in `messages.ts`, their tests and `ArabicTypography.test.tsx`.
- Preference and language: `src/shell/SettingsDialog.tsx` and tests, `src/api.ts`, `src/i18n/{en,ar}.json`, `src/fonts/fonts.css`.
- Persistence: `crates/qiyas-desktop/src/settings.rs` and `tests/settings.rs`. Rust is needed because the existing settings IPC serialises a typed Rust Settings struct; a frontend-only field would otherwise be discarded.
- Test CSS loading: `vite.config.ts`; behaviour notes: `CLAUDE.md`; this report. Evidence scripts and artefacts stay inside this worktree's ignored `.local/goal-run/`.

## Remaining acceptance

- The two local-port Rust tests need a run where loopback sockets are permitted.
- I3 needs a real WebKitGTK paint comparison; the root cause and visual fix remain unconfirmed.
- Actual painted narrow-panel clipping and 10,000-row scrolling smoothness need a browser/window check. Tests verify bounded rendered rows, cached text calls, measured-height positioning and both-language tab interactions; they do not measure frame rate.
- Recovery is inferred from a matching successful operation, because cards have no explicit recovery relationship. Different successful work remains visible as an unresolved failure instead of being silently assumed to repair it.
