import { expect, test } from "vitest";
import { uiError } from "./errors";

test("an edit on another sheet names it in both languages without exposing command syntax", () => {
  const error = 'Entity 987654 is on sheet Sheet A. Set "sheet":"Sheet A" on the edit command to target it without switching the view.';
  expect(uiError(error, "en")).toBe("This item is on Sheet A. Target that sheet for this edit.");
  expect(uiError(error, "ar")).toBe("هذا العنصر في اللوحة \u2068Sheet A\u2069. حدّدها لتنفيذ التعديل.");
});

test("view-only measurement refusals explain the setting in both languages", () => {
  const error = "This selection includes view-only geometry. Remove its measurement marker or measure the other geometry separately.";
  expect(uiError(error, "en")).toContain("view-only");
  expect(uiError(error, "ar")).toContain("للعرض فقط");
});
