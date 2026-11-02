// @vitest-environment jsdom
import { render } from "@testing-library/react";
import { expect, test } from "vitest";
import { isolateRuns, plainDashes } from "./content";

test.each([
  ["الفرق -12.5 م² من 1,234.50 و 3 × 4 و +12%", ["-12.5", "1,234.50", "3 × 4", "+12%"]],
  ["الجدار A-WALL بطول 12.5 م", ["A-WALL", "12.5"]],
  ["قسم - ملاحظة", []],
  ["قسم / ملاحظة", []],
  ["القيمة −12 و ±12 و (12 و 1.234,50", ["−12", "±12", "(12", "1.234,50"]],
  ["المساحة 12m² والحجم 3m³", ["12m²", "3m³"]],
  ["الملف C:\\plans\\wall.dxf أو plans/wall.dxf", ["C:\\plans\\wall.dxf", "plans/wall.dxf"]],
])("isolates runs without changing characters in %s", (text, expected) => {
  const { container } = render(isolateRuns(text));
  expect(Array.from(container.querySelectorAll('bdi[dir="ltr"]'), (node) => node.textContent)).toEqual(expected);
  expect(container.textContent).toBe(text);
});

test.each(["×", "x", "*", "/", "=", "+", "−", "-"].flatMap((operator) =>
  ["", " "].flatMap((before) => ["", " "].map((after) => `3${before}${operator}${after}4`)),
))("keeps the numeric expression %s in one isolate", (expression) => {
  const text = `الأبعاد ${expression} م`;
  const { container } = render(isolateRuns(text));
  expect(Array.from(container.querySelectorAll("bdi"), (node) => node.textContent)).toEqual([expression]);
  expect(container.textContent).toBe(text);
});

test("saved Mizan words use plain dashes, keeping surrounding spaces", () => {
  expect(plainDashes("Take-off — ready – checked―done—yes–ok")).toBe("Take-off - ready - checked-done-yes-ok");
  expect(plainDashes("حصر — جاهز – راجع―تم")).toBe("حصر - جاهز - راجع-تم");
  expect(plainDashes("—start\nend–\n―")).toBe("-start\nend-\n-");
  expect(plainDashes("1-2: unchanged")).toBe("1-2: unchanged");
});
