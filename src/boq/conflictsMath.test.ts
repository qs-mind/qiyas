import { expect, test } from "vitest";
import { countByKind, filterConflicts, groupByKind, reasonIn } from "./conflictsMath";
import { conflict } from "./testItems";

const list = [
  conflict("1", "unit_doubt", "Unit"),
  conflict("2", "quantity_mismatch", "Walls", { status: "resolved" }),
  conflict("3", "quantity_mismatch", "Plaster"),
  conflict("4", "not_in_drawings", "Skirting", { status: "ignored" }),
];

test("groups follow the kind order and keep each group's order", () => {
  const groups = groupByKind(list);
  expect(groups.map((g) => [g.kind, g.conflicts.map((c) => c.id)])).toEqual([
    ["quantity_mismatch", ["2", "3"]],
    ["not_in_drawings", ["4"]],
    ["unit_doubt", ["1"]],
  ]);
  expect(groupByKind([])).toEqual([]);
});

test("counts per kind list every kind; open counts only open ones", () => {
  expect(countByKind(list)).toEqual({
    quantity_mismatch: 2,
    missing_from_boq: 0,
    not_in_drawings: 1,
    schedule_vs_plan: 0,
    drawing_vs_drawing: 0,
    unit_doubt: 1,
    duplicate_item: 0,
    relation_mismatch: 0,
    document_mismatch: 0,
  });
});

test("filters by status and kind", () => {
  const ids = (cs: typeof list) => cs.map((c) => c.id);
  expect(ids(filterConflicts(list, "all", null))).toEqual(["1", "2", "3", "4"]);
  expect(ids(filterConflicts(list, "open", null))).toEqual(["1", "3"]);
  expect(ids(filterConflicts(list, "resolved", null))).toEqual(["2"]);
  expect(ids(filterConflicts(list, "all", "quantity_mismatch"))).toEqual(["2", "3"]);
  expect(ids(filterConflicts(list, "open", "quantity_mismatch"))).toEqual(["3"]);
  expect(ids(filterConflicts(list, "ignored", "unit_doubt"))).toEqual([]);
});

test("the reason follows the language, English when there is no Arabic", () => {
  expect(reasonIn(list[0], "en")).toBe("Unit");
  expect(reasonIn(list[0], "ar")).toBe("Unit (ar)");
  expect(reasonIn({ ...list[0], reason_ar: "" }, "ar")).toBe("Unit");
});
