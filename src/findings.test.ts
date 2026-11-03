import { expect, test } from "vitest";
import type { Conflict } from "./api";
import { conflictFinding, drawingFinding, findingFix, findingQuantity, findingText, packFindings } from "./findings";

test("old conversation findings gain stable identities without changing their evidence", () => {
  const old = { rule: "overlap", message: "Podium objects overlap", handles: [901, 900] };
  const finding = drawingFinding(old);
  expect(finding).toMatchObject({ source: "check", message: old.message, handles: old.handles });
  expect(drawingFinding({ ...old, handles: [900, 901], message: "New wording" }).id).toBe(finding.id);
  expect(drawingFinding({ ...old, sheet: "Tower 12" }).id).not.toBe(finding.id);
  expect(drawingFinding({ ...finding, id: "saved-id", severity: "error" })).toEqual({ ...finding, id: "saved-id", severity: "error" });
});

test("pack pass/fix records become bilingual findings and keep identity after calibration", () => {
  const line = { id: "calibration", passes: false, found_en: "Calibrate the scan", found_ar: "ضبط مقياس الصورة", fix_en: "Pick a known dimension", fix_ar: "اختيار بعد معلوم" };
  const finding = packFindings({ ready: false, lines: [line] }).findings[0];
  expect(finding.passed).toBe(false);
  expect(findingText(finding, "ar")).toBe(line.found_ar);
  expect(findingFix(finding, "ar")).toBe(line.fix_ar);
  const calibrated = packFindings({ ready: true, lines: [{ ...line, passes: true, found_en: "Calibrated", fix_en: "", fix_ar: "" }] }).findings[0];
  expect(calibrated.id).toBe(finding.id);
  expect(calibrated.severity).toBe("info");
  expect(findingFix(calibrated, "en")).toBe("");
});

test("discrepancies keep saved IDs, status and multi-sheet query evidence", () => {
  const conflict: Conflict = {
    id: "a123456789012345", kind: "quantity_mismatch", boq_code: "P01", measurements: ["m1"],
    schedule: "Podium", sheets: ["P-01", "P-02"], expected: 900, found: 850, unit: "m2",
    reason: "Areas differ", reason_ar: "المساحات مختلفة", status: "resolved", note: "Engineer confirmed",
    created: "2026-11-03", hash_key: "old key", query: {
      rule: "podium_area", severity: "information", question_en: "Confirm area", question_ar: "تأكيد المساحة",
      references: [{ handle: 900, sheet: "P-01", grid: "A1" }],
      citations: [{ source: "Specification", clause: "4" }], clouds: [950],
    },
  };
  const finding = conflictFinding(conflict);
  expect(finding).toMatchObject({ id: conflict.id, rule: "podium_area", severity: "info", handles: [900] });
  expect(finding.details).toBe(conflict);
  expect(findingQuantity(finding, "expected")).toEqual({ name: "expected", value: 900, unit: "m2" });
  expect(findingText(finding, "ar")).toBe(conflict.reason_ar);
  expect(finding.citations).toEqual(conflict.query?.citations);
  expect(findingText({ ...finding, message_ar: "" }, "ar")).toBe(conflict.reason);
});

test("pack adapter IDs match the core finding contract", () => {
  expect(packFindings({ ready: true, lines: [{ id: "units", passes: true, found_en: "Known", found_ar: "معروفة", fix_en: "", fix_ar: "" }] }).findings[0].id).toBe("0a1965748e88242a");
});
