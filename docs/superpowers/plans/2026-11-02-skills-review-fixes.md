# Skills review fixes Implementation Plan

> **For agentic workers:** Each task goes to Codex on the founder's computer as one task; Claude reviews its diff. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the confirmed crash, data-loss, security, Arabic-display and speed problems the 2 November skills review found, and carry out the founder's three rulings of the same day (CLAUDE.md, Founder's rulings).

**Architecture:** Small, independent fixes, each in the module that owns the code, each proven by a test written first. No new crates, no new window libraries.

**Tech Stack:** Rust (qiyas-core, qiyas-plot, qiyas-ai, qiyas-desktop), React 19 + TypeScript + Vitest, Tauri 2.

**Spec:** `docs/2026-11-02-skills-review.md` (finding ids S, R, W, G, A, T are its rows).

## Global Constraints

- pnpm, not npm. Run `scripts/check.sh` before every commit; commit inside the repo; never push `main` or tags.
- Only MPL-2.0, Apache-2.0, MIT, BSD, ISC, OFL, Zlib or BSL-1.0 dependencies. Tasks here add none, except `proptest = "1"` (MIT OR Apache-2.0) as a dev-dependency in Task 14.
- One build or test run at a time on the laptop, niced.
- Knowledge and metadata JSON written to xdata is ASCII-only (`xdata::ascii_json`) and decoded on read (`decode_legacy_unicode`), in every DWG version and DXF.
- The window shows Western digits in Arabic (Goal F); Arabic buttons are verbal nouns (حفظ، إلغاء).
- Tests and examples use mega projects and extreme cases, never a villa.
- Every mutating command stays one undo step.

## Review Focus

- A plot style, drawing or PDF from another firm: no input file may abort the process; it is refused with a message.
- An Arabic drawing saved as DWG 2000 or 2004 and reopened: every measurement, note and constraint comes back unchanged.
- Arabic text mixing numbers, units, signs, codes and Latin names reads in the right order in the chat, panels and PDFs.
- A drawing 3 km from the origin in millimetres, or with one stray entity at 0,0: the view neither jitters nor snaps.
- A long Mizan conversation (300+ messages) while drawing with OSNAP on: pointer moves stay smooth.

---

### Task 1: Plot style nesting limit (R1)

**Files:**
- Modify: `crates/qiyas-plot/src/ctb.rs:374-413` (`parse_block`), `:478`
- Test: `crates/qiyas-plot/tests/ctb.rs`

**Interfaces:**
- Produces: `const MAX_BLOCK_DEPTH: usize = 8;` and `parse_block(lines, nested, depth: usize)`; past the limit `Err(bad("The plot style is nested too deeply"))`. `PlotStyle::parse` keeps its signature.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn deep_nesting_is_refused_not_a_crash() {
    let text = format!("description=\"x\"\n{}", "a{\n".repeat(200_000));
    assert!(qiyas_plot::PlotStyle::parse(text.as_bytes()).is_err());
}
```

Add the same body through the compressed path (`CTB_HEADER` + `flate2::write::ZlibEncoder`), as the existing compressed tests build it.

- [ ] **Step 2: Run them: the test binary aborts with a stack overflow**

Run: `nice cargo test -p qiyas-plot --test ctb deep_nesting`

- [ ] **Step 3: Pass `depth` down and refuse past `MAX_BLOCK_DEPTH` (real files nest two levels)**
- [ ] **Step 4: Run again: PASS, and the rest of `--test ctb` passes**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 2: Measurement and constraint xdata survive older DWG versions (R2, R3)

**Files:**
- Modify: `crates/qiyas-core/src/embed.rs:235` (write), `:314`, `:347`, `:375` (read); `crates/qiyas-core/src/param.rs:262` (read), `:301` (write)
- Test: `crates/qiyas-core/src/embed.rs` tests module (`:434`), modelled on `knowledge.rs:474` `arabic_knowledge_survives_save_and_reopen_in_every_dwg_version`

**Interfaces:**
- Consumes: `xdata::ascii_json(&str) -> String`, `xdata::decode_legacy_unicode(&str) -> String`, the chunking `xdata::write_json` uses.
- Produces: the 10-value measurement layout stays readable; the JSON moves to values 9..n in chunks under 250 bytes and is joined on read.

- [ ] **Step 1: Write the failing test** — a take-off row with `note = "ملاحظة على الجدار الخارجي"` and an Arabic description, `embed_measurements`, save and reopen in AC1015, AC1018, AC1021, AC1024, AC1027, AC1032 × DWG and DXF; `read_embedded` returns the row with the note and description byte-equal. A second test does the same for a constraint whose expression is `if(a > 1, 'نعم', 'لا')`.
- [ ] **Step 2: Run: the AC1015 and AC1018 cases FAIL (row missing or `\U+` text)**

Run: `nice cargo test -p qiyas-core --lib embed`

- [ ] **Step 3: Write through `ascii_json` and chunk; decode every read (JSON and the plain string values at `:375`)**
- [ ] **Step 4: Run: PASS; also `nice cargo test -p qiyas-core --test all knowledge`**
- [ ] **Step 5: Add to `scripts/check.sh` the guard the review suggests: `rg -l XDataValue crates/qiyas-core/src | xargs rg -n 'serde_json::to_string|json!\(' | rg -v 'ascii_json|#\[cfg\(test'` must print nothing (fix or allow-list `objects.rs:807`, `walls.rs:846`)**
- [ ] **Step 6: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 3: Mizan and MCP ask before reading a PDF outside the work (S1)

**Files:**
- Modify: `crates/qiyas-desktop/src/drawing_host.rs` (add `before_tool` to its `AppHost` impl, next to the open-drawing confirmation at `:259-277`)
- Test: `crates/qiyas-desktop/src/drawing_host.rs` tests (`:395`)

**Interfaces:**
- Produces: for `qiyas_read` op `document` (and capture source `document`), the path is canonicalised and allowed when inside: an open drawing's folder or its export folder, an open project's folders, `qiyas_dir()/attachments`, or a path the user typed (Task 4's exact match). Otherwise the user is asked with the `confirm_open` wording; refusal returns a tool error.

- [ ] **Step 1: Write the failing tests** — an MCP `qiyas_read {op:"document", path:<temp dir>/outside.pdf}` with a drawing open elsewhere returns an error and never calls `page_count`; a PDF beside the open drawing is read; a symlink beside the drawing pointing outside is refused.
- [ ] **Step 2: Run: the first and third FAIL**
- [ ] **Step 3: Implement the guard**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 4: Exact match for "the user named this path" (S8)

**Files:**
- Modify: `crates/qiyas-desktop/src/host.rs:189`, and the same `user_text.contains(named)` test in `crates/qiyas-desktop/src/drawing_host.rs` `before_tool_with_user` (added by Task 3 for PDF reads)
- Test: `host.rs` tests

- [ ] **Step 1: Failing test** — message `please open /home/k/plans/tower-a.dwg`; the model asks for `/tower-a.dwg`: not named (today: named, since `contains`).
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Split the message on whitespace and quotes, canonicalise both sides, compare equal; relative paths are never "named"**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 5: MCP token off the command line (S2)

**Files:**
- Modify: `crates/qiyas-ai/src/claude_code.rs:420`, `:669-672`
- Test: the fake-Claude test near `claude_code.rs:1670`

**Interfaces:**
- Produces: the MCP config is written to a mode-0600 file in the turn's `work_directory()`, `--mcp-config <path>` is passed, and the file is deleted when the turn ends (also on cancel and error).

- [ ] **Step 1: Failing test** — the fake Claude records its argv: no argument contains `Bearer`; the `--mcp-config` value is a path to a 0600 file holding the token; after the turn the file is gone.
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement (Unix: `OpenOptions` with `mode(0o600)`; Windows: the file in the user's own folder)**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 6: CSP directives and a test of the shipped capabilities (S5, S6)

**Files:**
- Modify: `crates/qiyas-desktop/tauri.conf.json:22-23` (`csp`, `devCsp`)
- Test: new test in `crates/qiyas-desktop/src/launch.rs` tests (it already reads the config with `include_str!` at `:260`)

- [ ] **Step 1: Failing test** — parse `tauri.conf.json`; both CSPs contain `base-uri 'none'`, `form-action 'none'`, `object-src 'none'`, no `unsafe-eval`, no remote script source; snapshot the permission identifiers in `capabilities/default.json`.
- [ ] **Step 2: Run: FAIL on the three directives**
- [ ] **Step 3: Add them to both CSPs; check the dev app still loads (`pnpm tauri dev --config crates/qiyas-desktop/tauri.conf.json`)**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 7: Heavy opens off the async workers (R5)

**Files:**
- Modify: `crates/qiyas-desktop/src/files.rs:430` (`recover_drawing`), `:488` (reached from `sheet_set_entry`, `:505`), `crates/qiyas-desktop/src/plot.rs:134` (`load_plot_style`)

- [ ] **Step 1: Wrap each parse in `tauri::async_runtime::spawn_blocking`, as `open_drawing` does (`commands.rs:82`); no behaviour change, so the existing tests are the check**
- [ ] **Step 2: Run: `nice cargo test -p qiyas-desktop`: PASS**
- [ ] **Step 3: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 8: Numbers and signs in Mizan's chat keep their order (A1)

**Files:**
- Modify: `src/ai/content.tsx:33` (`isolateRuns`), `src/ai/Markdown.tsx` (`p` and headings get `readingDirection`)
- Test: `src/ai/content.test.ts`

- [ ] **Step 1: Failing test**

```ts
it("keeps signs, thousands and products in one isolate", () => {
  const { container } = render(<>{isolateRuns("الفرق -12.5 م² من 1,234.50 و 3 × 4 و +12%")}</>);
  const runs = [...container.querySelectorAll("bdi")].map(b => b.textContent);
  expect(runs).toEqual(expect.arrayContaining(["-12.5", "1,234.50", "3 × 4", "+12%"]));
});
```

- [ ] **Step 2: Run: `pnpm vitest run src/ai/content.test.ts`: FAIL**
- [ ] **Step 3: Allow an optional leading `[+\-−±(]`, and `,` and ` [×x*/=+−-] ` between digit groups, in the run pattern**
- [ ] **Step 4: Run: PASS, and `src/ai` tests still pass**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 9: Western digits and whole ranges in the Arabic window (A4, A6)

**Files:**
- Create: `src/i18n/locale.ts` (`export const locale = (lang: Lang) => lang === "ar" ? "ar-EG-u-nu-latn" : "en-GB"` — keep the English locale the window already uses)
- Modify: `src/ai/PlanDock.tsx:55`, `src/ai/providerFailure.ts:36`, `src/shell/recoveryQueue.ts:54`, `src/boq/GeneratedBoq.tsx:17` (use `unitText(item.unit)`, `dir` from the language), `src/i18n/ar.json` (`attachPdf.pages` → `الصفحات من {from} إلى {to} من أصل {n}`; `ai.planDock.count` and `ai.planDock.done` take one `{progress}`)
- Test: `src/ai/PlanDock.test.tsx:120,213`, `src/i18n/arabicStyle.test.ts`

- [ ] **Step 1: Change the PlanDock tests to expect `3/8` inside one isolate; add to arabicStyle: no Arabic value contains `}–{` or `}/{`**
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement; build `progress` as `"⁦" + done + "/" + total + "⁩"`**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 10: Arabic copy (A5, A7, A8, A9)

**Files:**
- Modify: `src/i18n/ar.json` (and `en.json` for A9); `crates/qiyas-core/src/measurement/evaluate.rs:536` (generated Arabic BOQ descriptions write sizes in Arabic-Indic digits, `٩٠٠ × ٢١٠٠ مم`, beside Western quantities in the window: write Western digits); `crates/qiyas-core/src/discrepancy/compare.rs:72`, `crates/qiyas-ai/src/agent.rs:1316`, `crates/qiyas-core/src/space_planning.rs:521,1504`
- Test: `src/i18n/arabicStyle.test.ts`; the Rust Arabic glossary test (extend its pattern)

- [ ] **Step 1: Failing tests** — arabicStyle: an Arabic value with `@`, `/`, `<` or a leading `.` next to Latin keeps that run inside U+2066…U+2069; the five button keys are not imperatives (`ai.send` = `إرسال`). Rust: no Arabic string matches `\{\}\s+(صفوف|ملاحظة|خطأ|حمامات|غرف|بند)`.
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Fix the strings**
  - `cmd.badPoint`, `settings.claudeLogin`, `ai.error.claude_auth`, `save.confirmRecovered`: syntax inside LRI…PDI, Latin placeholders inside it.
  - `ai.send` إرسال · `ai.newConversation` محادثة جديدة · `text.place` وضع النص · `inquiry.place` وضع النقاط · `ai.markArea` تحديد منطقة لميزان · `attachPdf.noFile` لم يحدد ملف.
  - `ai.finding.sheet_without_title_block`, `ai.finding.part_overlap`: plain words, no tool names, both languages.
  - Rust counts as `…؛ عدد الصفوف: {n}` (the glossary's count construct).
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 11: Mizan's thread stops re-rendering with the window (W1)

**Files:**
- Modify: `src/app/Panels.tsx:477,483` (`useMemo` the `drawing` and `artifact` props, `useCallback` `back`), `src/ai/useStickToBottom.ts:79` (one observer per box, made in the ref callback; observe new children when `changed` changes), `src/ai/ThreadItem.tsx` (`memo`)
- Test: create `src/ai/AssistantPanel.renders.test.tsx`

- [ ] **Step 1: Failing test** — spy on the `ResizeObserver` constructor (the stub in `src/test/setup.ts`), render `SidePanel` and re-render it 10 times with equal values: 1 construction (10 today).
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run: PASS, and `src/app/App.test.tsx` passes**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 12: Selection highlight and DataGrid measuring only when something changed (W2, W3)

**Files:**
- Modify: `src/tools/useTool.ts:1044` (`useMemo` the selection), `src/viewport/Viewport.tsx:455` (skip `setHighlight` for equal targets), `src/viewport/gl.ts:522-538` (a sized `Uint32Array`, no `push`); `src/ui/DataGrid.tsx:167` (`storedSizes` in a `useState` initialiser), `:170-177` (write sizes when resizing ends), `:246-276` (measure newly mounted rows only, one `getComputedStyle` per column); `src/app/Panels.tsx:416` (`useCallback` `showGenerated`)
- Test: `src/viewport/Viewport.test.tsx`, `src/ui/DataGrid.test.tsx`

- [ ] **Step 1: Failing tests** — Viewport (renderer mocked): re-render with an equal selection, `setHighlight` called once. DataGrid: spy on `getComputedStyle`, re-render with the same rows: 0 calls; drag a column edge over 20 moves: 1 `localStorage.setItem`.
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 13: Recover the canvas after a lost WebGL context (G1)

**Files:**
- Modify: `src/viewport/Viewport.tsx:329` (listeners), `src/viewport/gl.ts:333`, and the comment at `gl.ts:989`
- Test: `src/viewport/Viewport.test.tsx`

**Interfaces:**
- Produces: on `webglcontextlost`: `preventDefault()`, cancel the frame, drop the renderer. On `webglcontextrestored`: `createRenderer(canvas)` again and re-send atlas, batch, pictures, inset, highlight, overlay and grid from refs (add refs where a prop has none).

- [ ] **Step 1: Failing test** — mock `createRenderer`; dispatch both events on the canvas; it is created twice and the second renderer's `setBatch` gets the current batch.
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 14: Property tests for the parsers (stable fallback for fuzzing)

**Files:**
- Modify: `crates/qiyas-plot/Cargo.toml`, `crates/qiyas-param/Cargo.toml` (dev-dependency `proptest = "1"`; add it to `LICENSES/THIRD-PARTY.md` only if that file lists dev tools)
- Test: `crates/qiyas-plot/tests/ctb.rs`, `crates/qiyas-param/tests/expr.rs`

- [ ] **Step 1: Write the tests**

```rust
proptest! {
  #[test] fn ctb_fixed_point(body in "[a-z_0-9={}\"\n. -]{0,400}") {
    if let Ok(s) = PlotStyle::parse(format!("description=\"x\n{body}").as_bytes()) {
      let b = s.to_bytes(); prop_assert_eq!(PlotStyle::parse(&b).unwrap().to_bytes(), b); } }
  #[test] fn expr_round_trip(t in "[0-9a-z_ +*/^().,<>=!'-]{0,64}") {
    if let Ok(e) = Expr::parse(&t) { let s = e.to_string();
      prop_assert_eq!(Expr::parse(&s).unwrap().to_string(), s); } }
}
```

- [ ] **Step 2: Run: `nice cargo test -p qiyas-plot --test ctb` and `-p qiyas-param --test expr`; fix any case they shrink to, each with its own named regression test first**
- [ ] **Step 3: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 15: Mizan tells the engineer what it measured, deducted and applied (founder's ruling 1; T2, T6, T7)

**Files:**
- Modify: `crates/qiyas-core/src/digest.rs:508-532` (`assumed_values`), `crates/qiyas-core/src/measurement/evaluate.rs:370-381`, the end report (`takeoff_check.end_report` and the eval `end-report.json`), `crates/qiyas-ai/src/takeoff_workflow.txt:6,10` and `takeoff_workflow_ar.txt`, `crates/qiyas-core/src/discrepancy/mod.rs:133`
- Test: `crates/qiyas-core/tests/w3_output.rs` (or the take-off check tests beside it)

**Interfaces:**
- Produces: `takeoff_check.end_report.measurement_summary { measured: [{item, quantity, unit}], deductions: [{item, opening_or_void, quantity, unit, rule}], factors: [{item, name, value, source, decided_by: "drawing"|"document"|"engineer"|"mizan"|"qiyas_default"}] }`, and the same in Arabic words in Mizan's final answer (terms from `src/i18n/ar-glossary.json`).

- [ ] **Step 1: Failing tests** — a rule with an uncited 1.05 waste factor gives one `factors` row with `decided_by: "mizan"` and one `assumed_values` row; a wall with two openings gives two `deductions` rows with their m²; `discrepancy_check` with no tolerance set lists 5% with `decided_by: "qiyas_default"`.
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement; workflow lines 6 and 10 say: take deduction, waste, lap, allowance and rounding from the drawing, a document or the engineer; otherwise ask or decide with an engineering basis and record it; finish with the summary**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 16: Take-off checks that catch double counting and unit slips (T1, T3, T4, T5)

**Files:**
- Modify: `crates/qiyas-core/src/measurement.rs:208-212` (`PackCheck`: `x-takeoff-step` and kit entry), `:535-541` (refuse a kind/unit mismatch), `:725-742`; `crates/qiyas-core/src/digest/measurement_checks.rs:94-105`; `digest.rs` (pack-check count in the overview)
- Test: the take-off check tests

- [ ] **Step 1: Failing tests** — `kind:"area"` with `unit:"m"` is refused; a 10 m² wall with a 2 m² opening deducted again from `net_area` gives `measurement_error` `deduction_from_net`; two rules on one slab under code `S-1` give one `shared_source` row; a drawing with no register rule gives `overview.pack_check.failed >= 1`.
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement (questions with handles, not errors, for `shared_source`)**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 17: Western or Arabic-Indic digits in Arabic exports (founder's ruling 2; A3)

**Files:**
- Modify: `deliverable.export` (registry schema in `crates/qiyas-core/src/deliverables.rs`: new optional `digits: "western" | "arabic_indic"`, default `"western"`), `crates/qiyas-plot/src/report.rs:112` (`arabic_number` applies to every number in a cell, factors and expressions too, when `arabic_indic`), `crates/qiyas-boq/src/deliverables.rs:108` (Excel: numbers stay numeric; `arabic_indic` sets a native-digit number format), `qiyas-cli export --digits`, the window's export dialog (a choice shown for Arabic only)
- Test: `crates/qiyas-boq/tests/deliverables.rs`

- [ ] **Step 1: Failing tests** — an Arabic dimension-sheet PDF with `western` has no `[٠-٩]`; with `arabic_indic` no `[0-9]` in quantities and factors (handles, codes and sheet names stay Latin); the Arabic Excel's quantity cells are numbers in both, and `arabic_indic` cells carry the native-digit format. Decimals print fixed (`12.50`, not `12.5`).
- [ ] **Step 2: Run: FAIL**
- [ ] **Step 3: Implement; check the Excel file in LibreOffice and Excel**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

### Task 18: Every text follows the terms reference (founder's ruling 3)

**Files:**
- Modify: `src/i18n/arabicStyle.test.ts` and `crates/qiyas-ai/tests/arabic_style.rs` (also read `en` and `en_avoid`), `src/i18n/en.json`, `src/i18n/ar.json`, `crates/qiyas-ai/src/prompt_en.txt`, `prompt_ar.txt`, `takeoff_workflow*.txt`, the window's "Conflicts" labels
- Reference: `src/i18n/ar-glossary.json`, `docs/arabic-glossary.md`

- [ ] **Step 1: Failing tests** — no English value in en.json or English prompt contains a concept's `en_avoid` word ("Conflicts", "object" for an element or entity, "takeoff" in prose); no Arabic string uses مراجعة for a drawing revision (keys and prompts that name revisions use إصدار); parametric values are متغير, rule multipliers معامل.
- [ ] **Step 2: Run: FAIL, listing each string**
- [ ] **Step 3: Change the strings; keep identifiers and wire names as they are (`takeoff`, `conflicts` in JSON stay); remove the `check` alias of `discrepancy_check` (`commands.rs:72`) with a registry test that `"check"` no longer resolves to it**
- [ ] **Step 4: Run: PASS**
- [ ] **Step 5: Run scripts/check.sh, then commit inside the repo (never push main or tags)**

---

## Left for the founder's decision (not tasks yet)

- **Window paths and command permissions (S3, S4):** extension and folder limits on ~25 IPC commands, and a `build.rs` permission list for all ~135 handlers; worth doing before any second webview.
- **Arabic base direction in PDFs and panels (A2):** an RTL base level in `TextEngine::shape_line` for Arabic reports, and `dir="rtl"` for Arabic query text.
- **Float32 origin (G2):** tiles with their own origins, or a robust origin with far outliers as a second tile.
- **Design work (W4–W6, seams, words):** panel-rail drag, parallel opening fetches, Ribbon and Dialogs interfaces, removing `qiyas-takeoff` and the `execute.rs` dispatch, one typed check finding.
- **Fuzzing and mutation testing:** a nightly `fuzz/` outside the workspace (harnesses in the review report's sources), and a first `cargo-mutants` run on `crates/qiyas-param/src/expr.rs`, niced, `-j 1`, never `--in-place`.
