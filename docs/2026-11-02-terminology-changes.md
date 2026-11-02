# Engineering terminology and check alias — 2 November 2026

## Scope and decisions

Object stays per AutoCAD usage, as clarified by the orchestrator in the shared glossary: “Select objects”, “Object snap” / OSNAP, “UCS from object”, and object in offset, break and blend prompts. All 65 English translation values changed from object(s) to entity/entities or element(s) were restored, as was “region objects” in the English workflow. Matching object-related test expectations were restored. Both terminology guards now explicitly accept object wording and continue to read the narrowed en_avoid lists.

All other terminology and alias changes remain. Revision clouds mark drawing revisions, so سحابة مراجعة → سحابة إصدار and سحب مراجعة → سحب إصدار. Review remains مراجعة; named parameters use متغير/متغيرات; rule factors remain معامل. Keys, interpolation names and wire fields are unchanged. No founder decision is outstanding.

The English fixed prompt and Arabic workflow already meet the requested terms and were left unchanged. The Arabic fixed prompt’s term list now names إصدار; its review instructions retain مراجعة. Visible TSX panel/tab labels read translations, so their labels change through en.json. The remaining dependent TSX expectations use Discrepancies/discrepancies.

Removed both the serde command alias and the registry lookup fallback. Updated comparison callers to discrepancy_check. The AI surface test that deliberately rejects a read-side run of check remains a negative test, not an alias caller.

## Gates and limitations

- Original test-first run, before the initial string fixes: `nice pnpm vitest run src/i18n src/ai src/boq` exited 1, with 2 failed and 311 passed. The failing new tests were `English window text follows each concept's en_avoid terms` and `Arabic drawing revisions and named parameters retain their distinct meanings`. This used the earlier glossary before the orchestrator corrected object. Initial log: `/tmp/qiyas-terms-red.log`.
- Original post-fix run also caught two stale ConflictsPanel expectations, subsequently corrected; that original suite then passed 313 tests.
- First expanded correction run: 1 failed, 783 passed. `src/shell/RibbonUi.test.tsx` test “as the panel narrows, groups put more buttons behind their ⋯ until it fits, with nothing lost” still expected “Export conflicts”. Corrected that expectation to “Export discrepancies”. Log: `/tmp/qiyas-terms-autocad-tests.log`.
- Final `nice pnpm vitest run src/i18n src/ai src/boq src/shell`: 108 files passed, 784 tests passed, including arabicStyle.test.ts with narrowed en_avoid. Log: `/tmp/qiyas-terms-autocad-green.log`.
- Final `nice pnpm typecheck`: passed. Log: `/tmp/qiyas-terms-autocad-typecheck.log`.
- `git diff --check`: passed. No cargo command was run during this correction; the initial pass had run `cargo fmt --all` successfully.
- Existing Vite warning: extensionless `./scripts/live-plugin` import is unsupported by its future native config loader. No configuration changes made.
- Word counts (whitespace, as in the existing Rust gate): prompt_en.txt 600/600; prompt_ar.txt 599/600; takeoff_workflow.txt 399/400; takeoff_workflow_ar.txt 375/400.
- Rust compilation/tests were not run, as instructed; Rust gate execution remains with the orchestrator. The AI style tests are already included by tests/all.rs.
- No staging or commits.

## Files touched

This correction touched src/i18n/en.json, src/i18n/arabicStyle.test.ts, crates/qiyas-ai/src/takeoff_workflow.txt, crates/qiyas-ai/tests/arabic_style.rs, src/shell/FieldDialog.test.tsx, src/shell/RibbonUi.test.tsx and this report. FieldDialog.test.tsx now matches HEAD and has no remaining diff.

The remaining terminology/alias patch touches the following files. Pre-existing orchestrator edits to CLAUDE.md and src/i18n/ar-glossary.json are preserved and excluded from this patch inventory.

- `crates/qiyas-ai/src/prompt_ar.txt`
- `crates/qiyas-ai/src/takeoff_workflow.txt`
- `crates/qiyas-ai/tests/arabic_style.rs`
- `crates/qiyas-boq/tests/deliverables.rs`
- `crates/qiyas-boq/tests/queries.rs`
- `crates/qiyas-cli/tests/projects.rs`
- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/registry.rs`
- `crates/qiyas-core/tests/discrepancy.rs`
- `crates/qiyas-core/tests/goal_u.rs`
- `crates/qiyas-core/tests/registry.rs`
- `crates/qiyas-core/tests/w3_output.rs`
- `docs/2026-11-02-skills-review.md`
- `src/boq/ConflictsPanel.test.tsx`
- `src/i18n/ar.json`
- `src/i18n/arabicStyle.test.ts`
- `src/i18n/en.json`
- `src/shell/ArtifactFrame.test.tsx`
- `src/shell/RibbonUi.test.tsx`
- `docs/2026-11-02-terminology-changes.md`

## src/i18n/en.json: every remaining changed key

| Key | Old → new |
| --- | --- |
| `conflicts` | Conflicts → Discrepancies |
| `exportConflicts` | Export conflicts → Export discrepancies |
| `conflicts.none` | No conflicts → No discrepancies |
| `conflicts.noMatch` | No conflicts match the filter → No discrepancies match the filter |
| `conflicts.found` | {n} conflicts found → {n} discrepancies found |
| `tip.compare` | Check the take-off against the BOQ and list the conflicts. → Check the take-off against the BOQ and list the discrepancies. |
| `tip.exportConflicts` | Write the conflicts found to an Excel workbook. → Write the discrepancies found to an Excel workbook. |
| `outputs.conflicts` | Conflicts → Discrepancies |
| `outputs.ask.conflicts` | Ask Mizan about these conflicts → Ask Mizan about these discrepancies |
| `ai.slash.takeoff.prompt` | Take off the quantities from this drawing. → Take-off the quantities from this drawing. |
| `ai.slash.compare.desc` | Upload a BOQ and find conflicts with the drawings → Upload a BOQ and find discrepancies with the drawings |

## src/i18n/ar.json: every remaining changed key

| Key | Old → new |
| --- | --- |
| `draw.revcloud` | سحابة مراجعة → سحابة إصدار |
| `param.parameters` | مدير المعاملات… → مدير المتغيرات… |
| `param.manager` | مدير المعاملات → مدير المتغيرات |
| `activity.line.object.parameter.one` | معامل → متغير |
| `activity.line.object.parameter.many` | معاملات → متغيرات |
| `activity.line.object.revcloud.one` | سحابة مراجعة → سحابة إصدار |
| `activity.line.object.revcloud.many` | سحب مراجعة → سحب إصدار |
| `ai.packCheck.register` | رقم اللوحة والمراجعة → رقم اللوحة والإصدار |

## Other files: remaining exact changes

Each block records removed text with `-` and replacement/additional text with `+`, including new regression tests. Only changes still present in the working tree are listed.

### crates/qiyas-ai/src/prompt_ar.txt

```diff
diff --git i/crates/qiyas-ai/src/prompt_ar.txt w/crates/qiyas-ai/src/prompt_ar.txt
index 9ae9c5d..55f1448 100644
--- i/crates/qiyas-ai/src/prompt_ar.txt
+++ w/crates/qiyas-ai/src/prompt_ar.txt
@@ -24,2 +24,2 @@
 
-أسلوب العربية: استخدم مصطلحات ثابتة: رسم، لوحة، طبقة، حصر، مقايسة، بند، كمية، طريقة القياس، فتحة، باب، نافذة، جدار، غرفة، مساحة مغلقة، خطة، مرحلة، خطوة، فحص، استفسار، مراجعة، تصدير. اكتب حالات العمل مثل «قيد التنفيذ» و«اكتمل» و«تعذر التنفيذ» و«بانتظار الإجابة» و«موصى به». اكتب دون تشكيل بصياغة واضحة. افصل العربية عن الأرقام والوحدات واللاتينية بمسافة، مثل «19 بند» و«لوحتان A1» و«1.50 م». استخدم جملا قصيرة بعلامات ترقيم عربية. احتفظ بأسماء DWG و PDF و Excel.
+أسلوب العربية: استخدم مصطلحات ثابتة: رسم، لوحة، طبقة، حصر، مقايسة، بند، كمية، طريقة القياس، فتحة، باب، نافذة، جدار، غرفة، مساحة مغلقة، خطة، مرحلة، خطوة، فحص، استفسار، إصدار، تصدير. اكتب حالات العمل مثل «قيد التنفيذ» و«اكتمل» و«تعذر التنفيذ» و«بانتظار الإجابة» و«موصى به». اكتب دون تشكيل بصياغة واضحة. افصل العربية عن الأرقام والوحدات واللاتينية بمسافة، مثل «19 بند» و«لوحتان A1» و«1.50 م». استخدم جملا قصيرة بعلامات ترقيم عربية. احتفظ بأسماء DWG و PDF و Excel.
```

### crates/qiyas-ai/src/takeoff_workflow.txt

```diff
diff --git i/crates/qiyas-ai/src/takeoff_workflow.txt w/crates/qiyas-ai/src/takeoff_workflow.txt
index 2588177..d7b223d 100644
--- i/crates/qiyas-ai/src/takeoff_workflow.txt
+++ w/crates/qiyas-ai/src/takeoff_workflow.txt
@@ -5,3 +5,3 @@ Read raw geometry, properties, notes, legends, schedules and the engineer's docu
 
-Use the selected method's rules from the engineer's documents or the drawing, including the project's selected override. Take deductions, waste, laps, allowances and rounding from the drawing, a document or the engineer; otherwise ask, or decide with an engineering basis and record each with decision.set. Never invent clause numbers or claim that provisional rules implement the standard. Choose item structure from scope, evidence and method, without a fixed structure for any trade.
+Use the selected method's rules from the engineer's documents or the drawing, including the project's selected override. Take deductions, waste allowance, laps, allowances and rounding from the drawing, a document or the engineer; otherwise ask, or decide with an engineering basis and record each with decision.set. Never invent clause numbers or claim that provisional rules implement the standard. Choose item structure from scope, evidence and method, without a fixed structure for any trade.
```

### crates/qiyas-ai/tests/arabic_style.rs

```diff
diff --git i/crates/qiyas-ai/tests/arabic_style.rs w/crates/qiyas-ai/tests/arabic_style.rs
index f2e75e5..e2f8883 100644
--- i/crates/qiyas-ai/tests/arabic_style.rs
+++ w/crates/qiyas-ai/tests/arabic_style.rs
@@ -24,2 +24,5 @@ struct Concept {
     avoid: Vec<String>,
+    #[serde(default)]
+    en_avoid: Vec<String>,
+    preferred: String,
 }
@@ -376 +379,134 @@ let multiline = "نص
 }
+
+// Mask code spans and quoted command/wire names, but still inspect quoted prose.
+fn english_prose(source: &str) -> String {
+    let mut prose = String::new();
+    let mut rest = source;
+    while let Some(c) = rest.chars().next() {
+        if matches!(c, '`' | '"' | '\'') {
+            if let Some(end) = rest[c.len_utf8()..].find(c) {
+                let end = end + c.len_utf8();
+                let inner = &rest[c.len_utf8()..end];
+                let identifier = inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
+                let wire = identifier
+                    && (inner.contains('_')
+                        || matches!(inner, "conflicts" | "takeoff")
+                        || rest[end + c.len_utf8()..].trim_start().starts_with(':')
+                        || qiyas_core::registry::registry().get(inner).is_some());
+                if c == '`' || wire {
+                    prose.push(' ');
+                    rest = &rest[end + c.len_utf8()..];
+                    continue;
+                }
+            }
+        }
+        prose.push(c);
+        rest = &rest[c.len_utf8()..];
+    }
+    prose
+}
+fn english_errors(glossary: &Glossary, source: &str) -> Vec<String> {
+    let prose = english_prose(source).to_ascii_lowercase();
+    let word = |c: char| c.is_alphanumeric() || c == '_';
+    glossary
+        .concepts
+        .values()
+        .flat_map(|c| &c.en_avoid)
+        .filter(|term| {
+            let term = term.to_ascii_lowercase();
+            prose.match_indices(&term).any(|(i, _)| {
+                !prose[..i].chars().next_back().is_some_and(word)
+                    && !prose[i + term.len()..].chars().next().is_some_and(word)
+            })
+        })
+        .cloned()
+        .collect()
+}
+
+#[test]
+fn english_model_text_follows_shared_glossary() {
+    let glossary = glossary();
+    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
+    let mut files = Vec::new();
+    fn texts(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
+        for entry in fs::read_dir(dir).unwrap() {
+            let path = entry.unwrap().path();
+            if path.is_dir() {
+                texts(&path, files);
+            } else if path.extension().is_some_and(|e| e == "txt") {
+                files.push(path);
+            }
+        }
+    }
+    texts(&root, &mut files);
+    files.sort();
+    let mut errors = Vec::new();
+    for file in files {
+        if file.file_stem().unwrap().to_string_lossy().ends_with("_ar") {
+            continue;
+        }
+        let source = fs::read_to_string(&file).unwrap();
+        for term in english_errors(&glossary, &source) {
+            errors.push(format!("{}: {term}", file.display()));
+        }
+    }
+    assert!(errors.is_empty(), "{}", errors.join("\n"));
+}
+
+#[test]
+fn english_guard_distinguishes_prose_from_wire_names() {
+    let glossary = glossary();
+    for source in [
+        "Conflicts",
+        "CONFLICT",
+        "takeoff",
+        "take off",
+        "BOQ row",
+        "'Conflict'",
+    ] {
+        assert!(!english_errors(&glossary, source).is_empty(), "{source}");
+    }
+    for source in [
+        "Discrepancies",
+        "entity",
+        "an object",
+        "\"an object\"",
+        "Select objects",
+        "Object snap",
+        "UCS from object",
+        "take-off",
+        "conflicting",
+        "takeoff_check",
+        "discrepancy_check",
+        "takeoff_exclusion",
+        "`conflicts`",
+        "\"conflicts\"",
+        "'takeoff'",
+        r#"{"conflicts":[],"type":"discrepancy_check"}"#,
+        "`object` then an entity",
+    ] {
+        assert!(english_errors(&glossary, source).is_empty(), "{source}");
+    }
+    assert!(!english_errors(&glossary, "`conflicts` then conflicts").is_empty());
+}
+
+#[test]
+fn arabic_prompt_distinguishes_revision_from_review() {
+    let glossary = glossary();
+    let prompt = include_str!("../src/prompt_ar.txt");
+    assert!(prompt.contains(&format!(
+        "استفسار، {}، تصدير",
+        glossary.concepts["revision"].preferred
+    )));
+    assert!(prompt.contains(&format!("لل{}", glossary.concepts["review"].preferred)));
+    for text in [prompt, include_str!("../src/takeoff_workflow_ar.txt")] {
+        for old in [
+            "سحابة مراجعة",
+            "سحب مراجعة",
+            "رقم المراجعة",
+            "مدير المعاملات",
+        ] {
+            assert!(!text.contains(old), "{old}");
+        }
+    }
+}
```

### crates/qiyas-boq/tests/deliverables.rs

```diff
diff --git i/crates/qiyas-boq/tests/deliverables.rs w/crates/qiyas-boq/tests/deliverables.rs
index 502af15..59b3eac 100644
--- i/crates/qiyas-boq/tests/deliverables.rs
+++ w/crates/qiyas-boq/tests/deliverables.rs
@@ -23,3 +23,3 @@ fn test_session(path: &Path) -> Session {
     );
-    fixture::run(&mut s, json!({"type":"check"}));
+    fixture::run(&mut s, json!({"type":"discrepancy_check"}));
     assert!(!s.conflicts.is_empty());
```

### crates/qiyas-boq/tests/queries.rs

```diff
diff --git i/crates/qiyas-boq/tests/queries.rs w/crates/qiyas-boq/tests/queries.rs
index 94ecf06..a3693bd 100644
--- i/crates/qiyas-boq/tests/queries.rs
+++ w/crates/qiyas-boq/tests/queries.rs
@@ -7,3 +7,4 @@ fn queries_excel_preserves_bilingual_evidence_and_status() {
     s.run_input(&json!({"type":"set_boq","boq":{"source":"client.xlsx","imported":"","items":[{"code":"A","description":"Scope","quantity":2,"unit":"pcs","row":1}]}}),Source::User).unwrap();
-    s.run_input(&json!({"type":"check"}), Source::Ai).unwrap();
+    s.run_input(&json!({"type":"discrepancy_check"}), Source::Ai)
+        .unwrap();
     let path = d.path().join("takeoff.xlsx");
```

### crates/qiyas-cli/tests/projects.rs

```diff
diff --git i/crates/qiyas-cli/tests/projects.rs w/crates/qiyas-cli/tests/projects.rs
index 599e8ef..0440258 100644
--- i/crates/qiyas-cli/tests/projects.rs
+++ w/crates/qiyas-cli/tests/projects.rs
@@ -106,3 +106,3 @@ fn project_deliverable_flags_and_drawing_export_write_both_formats() {
     );
-    fixture::run(&mut session, json!({"type":"check"}));
+    fixture::run(&mut session, json!({"type":"discrepancy_check"}));
     session.save_as_headless(&drawing).unwrap();
```

### crates/qiyas-core/src/commands.rs

```diff
diff --git i/crates/qiyas-core/src/commands.rs w/crates/qiyas-core/src/commands.rs
index ca4880d..0ec4876 100644
--- i/crates/qiyas-core/src/commands.rs
+++ w/crates/qiyas-core/src/commands.rs
@@ -71,3 +71,3 @@ pub enum Command {
     #[schemars(extend("x-category" = "takeoff", "x-takeoff-step" = "6 check BOQ and documents; raise queries"))]
-    #[serde(rename = "discrepancy_check", alias = "check")]
+    #[serde(rename = "discrepancy_check")]
     #[schemars(example = json!({"type":"discrepancy_check"}))]
```

### crates/qiyas-core/src/registry.rs

```diff
diff --git i/crates/qiyas-core/src/registry.rs w/crates/qiyas-core/src/registry.rs
index a2bcf4b..b38646d 100644
--- i/crates/qiyas-core/src/registry.rs
+++ w/crates/qiyas-core/src/registry.rs
@@ -486,8 +486,3 @@ impl Registry {
         let key = name.trim().to_ascii_lowercase().replace('-', "_");
-        let key = if key == "check" {
-            "discrepancy_check"
-        } else {
-            &key
-        };
-        self.by_name.get(key).map(|&i| &self.commands[i])
+        self.by_name.get(&key).map(|&i| &self.commands[i])
     }
```

### crates/qiyas-core/tests/discrepancy.rs

```diff
diff --git i/crates/qiyas-core/tests/discrepancy.rs w/crates/qiyas-core/tests/discrepancy.rs
index 48abf6d..3e75e8b 100644
--- i/crates/qiyas-core/tests/discrepancy.rs
+++ w/crates/qiyas-core/tests/discrepancy.rs
@@ -33,3 +33,3 @@ fn client(s: &mut Session, items: Value) {
 fn check(s: &mut Session) {
-    run(s, json!({"type":"check"}));
+    run(s, json!({"type":"discrepancy_check"}));
 }
```

### crates/qiyas-core/tests/goal_u.rs

```diff
diff --git i/crates/qiyas-core/tests/goal_u.rs w/crates/qiyas-core/tests/goal_u.rs
index 6d3382f..f2cc9d6 100644
--- i/crates/qiyas-core/tests/goal_u.rs
+++ w/crates/qiyas-core/tests/goal_u.rs
@@ -270,3 +270,3 @@ fn u9_imported_block_enclosures_work_and_voids_are_an_explicit_type_property() {
 #[test]
-fn u11_discrepancy_check_has_a_clear_name_and_legacy_alias() {
+fn u11_discrepancy_check_has_a_clear_name_without_ambiguous_alias() {
     assert!(qiyas_core::registry::registry()
@@ -274,5 +274,7 @@ fn u11_discrepancy_check_has_a_clear_name_and_legacy_alias() {
         .is_some());
-    let old: qiyas_core::Command = serde_json::from_value(json!({"type":"check"})).unwrap();
+    assert!(serde_json::from_value::<qiyas_core::Command>(json!({"type":"check"})).is_err());
+    let command: qiyas_core::Command =
+        serde_json::from_value(json!({"type":"discrepancy_check"})).unwrap();
     assert_eq!(
-        serde_json::to_value(old).unwrap()["type"],
+        serde_json::to_value(command).unwrap()["type"],
         "discrepancy_check"
```

### crates/qiyas-core/tests/registry.rs

```diff
diff --git i/crates/qiyas-core/tests/registry.rs w/crates/qiyas-core/tests/registry.rs
index f553c78..cab70e9 100644
--- i/crates/qiyas-core/tests/registry.rs
+++ w/crates/qiyas-core/tests/registry.rs
@@ -441 +441,10 @@ fn domain_options_are_optional_and_registered_with_open_room_classification() {
 }
+
+#[test]
+fn check_is_not_a_discrepancy_command_alias() {
+    assert!(registry().get("check").is_none());
+    assert_eq!(
+        registry().get("discrepancy_check").unwrap().name,
+        "discrepancy_check"
+    );
+}
```

### crates/qiyas-core/tests/w3_output.rs

```diff
diff --git i/crates/qiyas-core/tests/w3_output.rs w/crates/qiyas-core/tests/w3_output.rs
index a5816f3..70d6fe9 100644
--- i/crates/qiyas-core/tests/w3_output.rs
+++ w/crates/qiyas-core/tests/w3_output.rs
@@ -921,3 +921,3 @@ fn numbered_door_marks_preserve_quantities_and_roll_up_for_a_client_boq() {
     run(&mut s, doors(3.0));
-    run(&mut s, json!({"type":"check"}));
+    run(&mut s, json!({"type":"discrepancy_check"}));
     assert!(
@@ -930,3 +930,3 @@ fn numbered_door_marks_preserve_quantities_and_roll_up_for_a_client_boq() {
     run(&mut s, doors(5.0));
-    run(&mut s, json!({"type":"check"}));
+    run(&mut s, json!({"type":"discrepancy_check"}));
     assert!(s.conflicts.iter().any(|c| {
```

### docs/2026-11-02-skills-review.md

```diff
diff --git i/docs/2026-11-02-skills-review.md w/docs/2026-11-02-skills-review.md
index eede9e1..dc60c3c 100644
--- i/docs/2026-11-02-skills-review.md
+++ w/docs/2026-11-02-skills-review.md
@@ -106,3 +106,3 @@ them as facts Mizan asks for.
   `check::Finding`, `Conflict`); one typed finding would make T1–T5 small rules.
-- `check` is both the read-only drawing check and an alias of the mutating `discrepancy_check`
+- `check` was both the read-only drawing check and an alias of the mutating `discrepancy_check` (alias removed under the 2 November ruling)
   (`commands.rs:72`).
```

### src/boq/ConflictsPanel.test.tsx

```diff
diff --git i/src/boq/ConflictsPanel.test.tsx w/src/boq/ConflictsPanel.test.tsx
index 3f66efd..968579f 100644
--- i/src/boq/ConflictsPanel.test.tsx
+++ w/src/boq/ConflictsPanel.test.tsx
@@ -79,3 +79,3 @@ test("rows are grouped by kind, in kind order, with their counts and details", (
   expect(screen.queryByRole("button", { name: "Export conflicts" })).toBeNull();
-  expect(screen.queryByRole("heading", { name: "Conflicts" })).toBeNull();
+  expect(screen.queryByRole("heading", { name: "Discrepancies" })).toBeNull();
 });
@@ -101,3 +101,3 @@ test("the status filter hides resolved ones; kind chips filter and count", () =>
   fireEvent.click(within(status).getByRole("radio", { name: "Ignored" }));
-  expect(screen.getByText("No conflicts match the filter")).toBeTruthy();
+  expect(screen.getByText("No discrepancies match the filter")).toBeTruthy();
   fireEvent.click(within(kinds).getByRole("button", { name: "All 0" }));
@@ -153,3 +153,3 @@ test("empty states: no BOQ, not compared, none found", () => {
   renderPanel({ conflicts: [] });
-  expect(screen.getByText("No conflicts")).toBeTruthy();
+  expect(screen.getByText("No discrepancies")).toBeTruthy();
 });
```

### src/i18n/arabicStyle.test.ts

```diff
diff --git i/src/i18n/arabicStyle.test.ts w/src/i18n/arabicStyle.test.ts
index 7886540..a7959d0 100644
--- i/src/i18n/arabicStyle.test.ts
+++ w/src/i18n/arabicStyle.test.ts
@@ -135 +135,41 @@ test("Arabic action buttons use verbal nouns", () => {
 });
+
+
+const englishBanned = [...new Set(Object.values(glossary.concepts)
+  .flatMap(c => "en_avoid" in c ? c.en_avoid : []))];
+function englishErrors(value: string): string[] {
+  // Substitution names are wire names, not displayed prose.
+  const prose = value.replace(/\{\w+\}/g, "");
+  return englishBanned.filter(term => new RegExp(
+    `\\b${term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "i",
+  ).test(prose));
+}
+
+test("English window text follows each concept's en_avoid terms", () => {
+  expect(Object.entries(en).flatMap(([key, value]) =>
+    englishErrors(value).map(term => `${key}: ${term} (${value})`))).toEqual([]);
+});
+
+test("English terms match whole words without changing identifiers or placeholders", () => {
+  for (const value of ["Conflicts", "CONFLICT", "takeoff", "Take off quantities", "BOQ row"]) {
+    expect(englishErrors(value), value).not.toEqual([]);
+  }
+  for (const value of ["Discrepancies", "entity", "Pick an object", "Select objects", "Object snap", "UCS from object", "take-off", "takeoff_check", "{object}", "conflicting"]) {
+    expect(englishErrors(value), value).toEqual([]);
+  }
+});
+
+test("Arabic drawing revisions and named parameters retain their distinct meanings", () => {
+  const revision = glossary.concepts.revision.preferred;
+  const parameter = glossary.concepts.parameter.preferred;
+  expect(Object.entries(en).filter(([key, value]) => /\brevision\b|\brev\b/i.test(value) || /revcloud/i.test(key))
+    .filter(([key]) => ar[key].includes(glossary.concepts.review.preferred))).toEqual([]);
+  for (const key of ["draw.revcloud", "activity.line.object.revcloud.one", "activity.line.object.revcloud.many", "ai.packCheck.register"]) {
+    expect(ar[key], key).toContain(revision);
+  }
+  for (const key of ["param.parameters", "param.manager", "activity.line.object.parameter.one", "activity.line.object.parameter.many"]) {
+    expect(ar[key], key).toContain(parameter);
+  }
+  expect(ar["boq.measurementReview"]).toContain(glossary.concepts.review.preferred);
+  expect(ar["modify.prompt.factor"]).toContain(glossary.concepts.factor.preferred);
+});
```

### src/shell/ArtifactFrame.test.tsx

```diff
diff --git i/src/shell/ArtifactFrame.test.tsx w/src/shell/ArtifactFrame.test.tsx
index a0dac38..c12efd4 100644
--- i/src/shell/ArtifactFrame.test.tsx
+++ w/src/shell/ArtifactFrame.test.tsx
@@ -63,3 +63,3 @@ test("an output has a way back, a menu to the others, its meta and one action",
   expect(onSelect).toHaveBeenCalledWith("takeoff");
-  expect(screen.queryByRole("menuitemradio", { name: /^Conflicts/ })).toBeNull();
+  expect(screen.queryByRole("menuitemradio", { name: /^Discrepancies/ })).toBeNull();
 });
```

### src/shell/RibbonUi.test.tsx

```diff
diff --git i/src/shell/RibbonUi.test.tsx w/src/shell/RibbonUi.test.tsx
index 898671c..3c9fd2f 100644
--- i/src/shell/RibbonUi.test.tsx
+++ w/src/shell/RibbonUi.test.tsx
@@ -345,3 +345,3 @@ test("as the panel narrows, groups put more buttons behind their ⋯ until it fi
     }
-    for (const name of ["Select", "Distance", "Area", "Count", "Geometry", "Divide", "Measure along", "ID point", "Extract data", "Export take-off", "Import BOQ", "Compare", "Export conflicts", "Linear", "Dimension style…", "Revision cloud", "Comment cloud", "Leader", "MText"]) {
+    for (const name of ["Select", "Distance", "Area", "Count", "Geometry", "Divide", "Measure along", "ID point", "Extract data", "Export take-off", "Import BOQ", "Compare", "Export discrepancies", "Linear", "Dimension style…", "Revision cloud", "Comment cloud", "Leader", "MText"]) {
       expect(reach.has(name), name).toBe(true);
```
