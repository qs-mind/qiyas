import { expect, test } from "vitest";
import { difference, matches, measuredFor, searchKey } from "./boqMath";
import { boqItem, row } from "./testItems";

test("measured sums the linked rows in the item's unit", () => {
  const item = boqItem("A.1", "Walls", "m", 20);
  const takeoff = [
    row("a", 12, "m", "A.1"),
    row("b", 9, "m", "A.1"),
    row("c", 50, "m", "A.2"),
    row("d", 7, "m", null),
  ];
  const m = measuredFor(item, takeoff);
  expect(m.sum).toBe(21);
  expect(m.links.map((l) => l.id)).toEqual(["a", "b"]);
  expect(m.other).toEqual([]);
});

test("rows in another unit are a mismatch, summed per unit", () => {
  const item = boqItem("B.1", "Plaster", "m2", 40);
  const takeoff = [row("a", 30, "m2", "B.1"), row("b", 4, "m", "B.1"), row("c", 6, "m", "B.1")];
  const m = measuredFor(item, takeoff);
  expect(m.sum).toBe(30);
  expect(m.other).toEqual([{ unit: "m", sum: 10 }]);
  expect(measuredFor(boqItem("C", "Doors", "pcs", 3), [row("x", 2, "m", "C")]).sum).toBeNull();
});

test("an item without a code has no links", () => {
  expect(measuredFor(boqItem("", "Walls", "m", 1), [row("a", 1, "m", "")]).links).toEqual([]);
});

test("difference against the tolerance", () => {
  const item = boqItem("A", "Walls", "m", 100);
  expect(difference(item, null)).toEqual({ percent: null, status: "none" });
  expect(difference(item, 104)).toEqual({ percent: 4, status: "within" });
  expect(difference(item, 105).status).toBe("within");
  expect(difference(item, 110)).toEqual({ percent: 10, status: "over" });
  expect(difference(item, 80)).toEqual({ percent: -20, status: "under" });
  expect(difference({ ...item, tolerance: 15 }, 110).status).toBe("within");
  expect(difference({ ...item, tolerance: 0 }, 100.5).status).toBe("over");
  expect(difference({ ...item, quantity: 0 }, 3)).toEqual({ percent: null, status: "over" });
});

test("search ignores case and Arabic letter forms", () => {
  expect(searchKey("أعمالُ الخرسانة")).toBe("اعمال الخرسانه");
  const item = boqItem("C-02", "أعمال الخرسانة المسلحة", "m3", 1);
  expect(matches(item, "c-0")).toBe(true);
  expect(matches(item, "اعمال")).toBe(true);
  expect(matches(item, "الخرسانه")).toBe(true);
  expect(matches(item, "steel")).toBe(false);
  expect(matches(item, "  ")).toBe(true);
});

test("search keeps Arabic-Indic digits", () => {
  const item = boqItem("C-03", "خرسانة الدور ٢", "m3", 1);
  expect(searchKey("٢")).toBe("٢");
  expect(matches(item, "٢")).toBe(true);
  expect(matches(item, "٣")).toBe(false);
});

test("view-only geometry keeps its link but contributes no quantity", () => {
  const item = boqItem("A", "Walls", "m", 20);
  const takeoff = [row("measured", 5, "m", "A"), { ...row("detail", 20, "m", "A"), view_only: true }];
  const result = measuredFor(item, takeoff);
  expect(result.sum).toBe(5);
  expect(result.links.map((r) => r.id)).toEqual(["measured", "detail"]);
});
