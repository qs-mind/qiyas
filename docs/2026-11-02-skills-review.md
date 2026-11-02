# Skills review of Qiyas (2 November 2026)

The 38 skills added to `.claude/skills/` on 2 November 2026 were run over the code, read-only
(no builds while the dev app ran). CONFIRMED: the code path was read end to end. PLAUSIBLE: the
path is real, the cost or trigger is not measured. Items marked † were checked again by hand.
The fixes are tasks in `docs/superpowers/plans/2026-11-02-skills-review-fixes.md`.

## Security and the Tauri layer (tauri-security, tauri-rust, tauri-testing)

| # | Finding | Where | Status |
|---|---|---|---|
| S1 | Mizan and MCP clients read any PDF on the computer without asking: only the `.pdf` extension is checked; the desktop host has no `before_tool` guard for `document` (the headless host confines paths) | `qiyas-ai/src/document.rs:9-22`, `surface.rs:1096-1110`, `qiyas-desktop/src/host.rs:42` | CONFIRMED † |
| S2 | The private MCP bearer token is on Claude Code's command line (`--mcp-config <json>`), readable by local users in `/proc/<pid>/cmdline` | `qiyas-ai/src/claude_code.rs:420`, `:669-672` | CONFIRMED † |
| S3 | About 25 IPC commands trust window paths: `export_table` writes any path and extension; `file_view_bytes` reads any `.json/.txt/.md/.csv` up to 2 MB; `claude_code_status` runs any absolute executable | `commands.rs:723-730,997`, `output_files.rs:39-52,175`, `blocks.rs:72-83`, `files.rs:466` | PLAUSIBLE (needs a compromised window; the CSP is strict) |
| S4 | No permission list for the app's ~135 commands: every webview may call all of them | `qiyas-desktop/build.rs:1-3`, `capabilities/default.json` | CONFIRMED |
| S5 | IPC tests never load the shipped capabilities or CSP (`mock_context(noop_assets())`) | `commands.rs:2066` | CONFIRMED |
| S6 | CSP lacks `base-uri`, `form-action`, `object-src` (they don't fall back to `default-src`) | `tauri.conf.json:22-23` | CONFIRMED |
| S7 | Output cards hand `.xlsm` to the OS; `written` is collected at any depth of a tool result | `output_files.rs:32`, `src/shell/outputFiles.ts:15-20` | PLAUSIBLE |
| S8 | "The user named this path" is a substring test (`/home/k/villa.dwg` lets `/villa.dwg` open unasked) | `host.rs:189` | CONFIRMED |

Fine: the updater (signed feed, HTTPS, no downgrade), single instance (only existing `.dwg`/`.dxf`),
the MCP server (loopback, constant-time token, Host/Origin lists), provider endpoints, the Claude
Code subprocess, AI exports confined to the drawing folder, no fs/shell/http plugins.

## Rust (unsafe-rust-review, rust-review-idiomatic, variant-analysis)

| # | Finding | Where | Status |
|---|---|---|---|
| R1 | A deeply nested CTB/STB plot style aborts the whole app: `parse_block` recurses per `name{` with no depth limit (the class `expr.rs:217` already guards) | `qiyas-plot/src/ctb.rs:374-413` | CONFIRMED † |
| R2 | Measurements vanish on reopening a DWG 2000/2004: their metadata JSON skips `xdata::ascii_json`, is not chunked, and is read without `decode_legacy_unicode` (Arabic note → `\U+0645` → invalid JSON → row dropped silently) | `qiyas-core/src/embed.rs:235,314,347,375` | CONFIRMED † |
| R3 | The same for constraints whose expressions hold Arabic text literals | `qiyas-core/src/param.rs:262,301` | PLAUSIBLE |
| R4 | Plot style save writes a file it can't read back (names with `\n}`), and not through a temp file | `ctb.rs:182,276-348` | PLAUSIBLE |
| R5 | `recover_drawing`, `sheet_set_entry` and `load_plot_style` parse whole files on async workers (no `spawn_blocking`, unlike `open_drawing`) | `files.rs:430,488`, `plot.rs:134` | CONFIRMED |
| R6 | Plot style pens shift when indices have gaps | `ctb.rs:504-507` | PLAUSIBLE |
| R7 | `kill(pid as i32, …)` comments argue ownership, not the contract; use `pid_t::try_from` and `> 0`; the test-only inotify block has one comment for four calls | `qiyas-ai/src/claude_code.rs:550,564,1548-1577` | CONFIRMED (sound, comments weak) |
| R8 | `objects.rs:807` and `walls.rs:846` write raw JSON (read side decodes; breaks only on a backslash before an escape) | as named | PLAUSIBLE, low |

All 9 `unsafe` sites are sound (no caller pointers). Fuzz targets, in order: the DWG and DXF readers
with `Drawing::open`'s options, `PlotStyle::parse`, `Expr::parse`, `script::parse`.

## Window speed (react-best-practices, composition-patterns)

| # | Finding | Where | Status |
|---|---|---|---|
| W1 | Every root render re-renders Mizan's whole thread and rebuilds a ResizeObserver over every message (new `drawing`/`artifact` objects defeat `memo`; a layout effect with no deps) | `src/app/Panels.tsx:477,483`, `src/ai/useStickToBottom.ts:79` | CONFIRMED |
| W2 | The selection highlight is rebuilt and re-uploaded on every root render (new array from `filter` in render → BigInt scan of all ranges + `bufferData`) | `src/tools/useTool.ts:1044` → `useOverlay.ts:24-33` → `Viewport.tsx:455` → `gl.ts:522-538` | CONFIRMED path |
| W3 | DataGrid measures every visible cell with `getComputedStyle` after every render (scroll frames too), parses `localStorage` every render and writes it on every resize move | `src/ui/DataGrid.tsx:167,170-177,246-276` | CONFIRMED |
| W4 | Dragging a panel rail re-renders the window and writes storage at 60 Hz | `src/app/panelRails.tsx:84-89` | CONFIRMED |
| W5 | Opening a drawing waits for the take-off report before the canvas draws (serial invokes) | `src/app/useDrawings.ts:366-379`, `useSheets.ts:172,314-337` | CONFIRMED order |
| W6 | Ribbon: ~60 optional `onX?` props; dialogs: 22 separate open booleans in 8 hooks | `shell/Ribbon.tsx`, `app/Dialogs.tsx` | design, later |

## Viewport (webgl2-systems-architect-skill)

| # | Finding | Where | Status |
|---|---|---|---|
| G1 | No WebGL context-loss handling: after suspend or a GPU reset the canvas stays black until restart | `src/viewport/gl.ts:333`, `Viewport.tsx:329` | CONFIRMED |
| G2 | Float32 jitter: one batch origin at the bounding-box centre; a stray entity at 0,0 in a UTM drawing (northing ≈3.3e9 mm) snaps every vertex to 128 mm | `qiyas-render/src/batch.rs:618`, `gl.ts:80` | CONFIRMED in code |
| G3 | Allocations in upload and frame paths (`lineColors` arrays per segment, three index copies, the overlay converted every frame) | `gl.ts:259,278,966-976` | PLAUSIBLE cost |

## Arabic and English (arabic-ready, dualkit-*)

| # | Finding | Where | Status |
|---|---|---|---|
| A1 | Mizan's chat: `1,234.50 م²` shows as `234.50,1`, `-12.5` as `12.5-`, `+12%` as `12%+` (isolated runs must start with a letter or digit and can't hold a comma) | `src/ai/content.tsx:33` | CONFIRMED † |
| A2 | Arabic query text starting with a Latin name (`D1 عدده 3 في A-101 …`) lays out left to right in the window (`dir="auto"`) and in the Arabic PDF (each line's base level from its first strong letter) | `src/boq/ConflictsPanel.tsx:120`, `GeneratedBoq.tsx:17`, `qiyas-plot/src/report.rs:39`, `qiyas-render/src/text.rs:778` | CONFIRMED |
| A3 | Arabic deliverables mix digit systems (PDF converts whole-number cells only; Excel stays Western); `12.50` prints `12.5` | `report.rs:112`, `qiyas-boq/src/deliverables.rs:108`, `qiyas-core/src/deliverables.rs:409` | CONFIRMED |
| A4 | The window breaks its Western-digits rule (CLAUDE.md, Goal F): PlanDock shows `٣/٨` (`ar-EG` without `-u-nu-latn`, asserted by its tests); BOQ summaries mix `٩٠٠ × ٢١٠٠ مم` with Western quantities and raw `m2` | `src/ai/PlanDock.tsx:55`, `GeneratedBoq.tsx:17` | CONFIRMED |
| A5 | Syntax in ar.json is reordered: `@dx,dy` → `dx,dy@` (and `<` mirrored), `/login` → `login/`, `.bak` → `bak.` | `cmd.badPoint`, `settings.claudeLogin`, `ai.error.claude_auth`, `save.confirmRecovered` | CONFIRMED |
| A6 | `الصفحات {from}–{to}` shows pages 3–5 as 5–3; `{done}/{total}` likewise once digits are Western | `attachPdf.pages`, `ai.planDock.*` | CONFIRMED |
| A7 | Plural agreement in Rust's Arabic strings ("في 2 صفوف", "3 ملاحظة", "1 حمامات") | `discrepancy/compare.rs:72`, `qiyas-ai/src/agent.rs:1316`, `space_planning.rs:521,1504` | CONFIRMED |
| A8 | Imperative buttons against the verbal-noun rule: أرسل، ابدأ محادثة جديدة، ضع النص، ضع النقاط، حدد منطقة لميزان | `ai.send`, `ai.newConversation`, `text.place`, `inquiry.place`, `ai.markArea` | CONFIRMED |
| A9 | Tool names in findings shown to the user (`make_sheet`, `title_block`, `arrange`) | `ai.finding.sheet_without_title_block`, `ai.finding.part_overlap` | CONFIRMED |
| A10 | Letter-spacing, uppercase and italic reach Arabic text | `theme.css:1257,3452-3453,3606,3704` | PLAUSIBLE |

Fine: logical CSS (the physical rules are canvas overlays or paired with `:dir(rtl)`), `translate()`
isolating values, toasts, per-message direction in the chat, the Arabic Excel's right-to-left sheet,
HarfBuzz shaping with ActualText in PDFs.

## Take-off checks (takeoff-review-checklist, unit-rate-sanity-check)

Decided by the founder on 2 November 2026 (CLAUDE.md, Founder's rulings): Mizan asks or decides, and reports at the end what it measured, deducted and applied. The question was: CLAUDE.md line 15 says Mizan asks instead of assuming; line
3671 and `prompt_en.txt:11` say it decides technical matters and asks only about the idea. Opening
deduction thresholds, waste and lap allowances and rounding fall between them. The tasks below treat
them as facts Mizan asks for.

| # | Gap | Where it goes | Status |
|---|---|---|---|
| T1 | The drawing-pack (revision) check is outside the take-off path (`measurement.pack_check` has no `x-takeoff-step`, not in the workflow or `takeoff_check`) | `qiyas-core/src/measurement.rs:208-212`, `digest.rs`, `takeoff_workflow.txt:4` | CONFIRMED |
| T2 | Waste, lap and allowance factors typed into a rule are never listed as assumed | `measurement/evaluate.rs:370-381`, `digest.rs:508-532` | CONFIRMED |
| T3 | Openings deducted again from a net figure pass unless the total goes negative | `evaluate.rs:748-766`, `digest/measurement_checks.rs:94-105` | CONFIRMED |
| T4 | Two rules on one element under one code are added silently; overlapping elements under one rule are not checked | `measurement.rs:725-742` | CONFIRMED |
| T5 | A rule's kind is never checked against its unit (`kind:"area"`, `unit:"m"` passes) | `measurement.rs:535-541` | CONFIRMED |
| T6 | `discrepancy_check` falls back to a 5% tolerance shipped with Qiyas | `qiyas-core/src/discrepancy/mod.rs:133`, `qiyas-takeoff/src/lib.rs:9` | CONFIRMED |
| T7 | `takeoff_workflow.txt:6` allows "provisional geometric rules", against CLAUDE.md line 15 | prompt | CONFIRMED |

## Plans, seams and words (egm-drift-report, codebase-design, domain-modeling)

- "Where the plans live" in CLAUDE.md stopped at the 8 October plans (fixed today).
- `2026-10-20-qiyas-dynamic-core.md`: the `execute.rs` legacy dispatch it says to delete is still
  the path every surface op takes (`qiyas-ai/src/execute.rs:174`, `surface.rs:1411-1440`); generated
  TypeScript command types are not done (`src/api.ts:921`); its villa eval contradicts the
  29 October rule on mega projects (`qiyas-ai/tests/drafting_eval.rs:288`).
- `qiyas-takeoff` (165 lines) only re-exports core and repeats the 5% default; nothing would break
  if its callers called core.
- Five check modules with five result shapes (`takeoff_check`, `outside_check`, `pack_check`,
  `check::Finding`, `Conflict`); one typed finding would make T1–T5 small rules.
- `check` is both the read-only drawing check and an alias of the mutating `discrepancy_check`
  (`commands.rs:72`).
- Words: settled on 2 November 2026 in `src/i18n/ar-glossary.json`, now the one terms reference (plan Task 18).

## Flaky tests seen while landing the fixes

- `qiyas-desktop` `commands::tests::a_cancelled_turn_says_so_and_ends` (`commands.rs:2865-2871`):
  `ai_send` then `ai_cancel` at once; the last event was sometimes not `stopped/cancelled`
  (`None`). It failed in 2 of 10 runs of the desktop suite under memory pressure, passed 5 of 5
  without the change being landed and 6 of 6 after it; the race is between the cancel and the
  turn's end, not in the drawing code. Fixed the same day: a turn cancelled before it took
  its queue slot sent `stopped` only as `workspace:event`, never as `ai:event`; it now sends both
  (`a_queued_cancelled_turn_says_so_and_ends` holds the queue and fails without the fix).
- `src/shell/LayerDialogs.test.tsx` "layer states: save one, restore, rename and delete the
  others": `onRestore` not called once in a full `pnpm test` under load; 3 of 3 alone, 1182 of
  1182 on the rerun. Base UI's delayed focus moves likely need a longer wait.

## Not run

- Screenshots on a virtual display (`screenshotting-gui`): the second Qiyas on Xvfb `:99` with a
  private D-Bus opened its window, but every capture was black, with DMA-BUF off, software GL,
  compositing off and Skia CPU drawing. The dev app on the desktop was not touched.
- Fuzzing (`cargo-fuzz`): planned only, no nightly toolchain (founder's choice). Mutation testing:
  planned, `cargo-mutants` not installed.
