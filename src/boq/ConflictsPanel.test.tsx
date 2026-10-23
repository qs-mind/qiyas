// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ConflictsPanel } from "./ConflictsPanel";
import { conflict } from "./testItems";

afterEach(cleanup);

const conflicts = [
  conflict("c1", "unit_doubt", "Linked in m2, BOQ in m", { boq_code: "A.3", unit: "m" }),
  conflict("c2", "quantity_mismatch", "Walls: BOQ 16, measured 10", {
    boq_code: "A.1",
    expected: 16,
    found: 10,
    unit: "m",
    measurements: ["w1"],
    sheets: ["Model"],
    note: "Check the GF plan",
  }),
  conflict("c3", "quantity_mismatch", "Plaster: BOQ 12, measured 14", {
    boq_code: "A.2",
    expected: 12,
    found: 14,
    unit: "m2",
    status: "resolved",
  }),
  conflict("c4", "schedule_vs_plan", "Door D1: schedule 4, plan 3", {
    expected: 4,
    found: 3,
    unit: "pcs",
    sheets: ["GF Plan", "Schedules"],
  }),
];

function renderPanel(more: Partial<Parameters<typeof ConflictsPanel>[0]> = {}) {
  const props = {
    conflicts,
    compared: true,
    hasBoq: true,
    onCompare: vi.fn(),
    onExport: vi.fn(),
    onShow: vi.fn(),
    onResolve: vi.fn(),
    onOpenBoq: vi.fn(),
    ...more,
  };
  render(<ConflictsPanel {...props} />);
  return props;
}

/** The section heads of the grid: the kinds, each with its count. */
const groups = () => screen.getAllByRole("rowheader").map((h) => h.textContent);
const reasons = () => [...document.querySelectorAll(".reason")].map((p) => p.textContent);

test("rows are grouped by kind, in kind order, with their counts and details", () => {
  const props = renderPanel();
  expect(groups()).toEqual(["Quantity mismatch 2", "Schedule vs plan 1", "Unit doubt 1"]);
  expect(reasons()).toEqual([
    "Walls: BOQ 16, measured 10",
    "Plaster: BOQ 12, measured 14",
    "Door D1: schedule 4, plan 3",
    "Linked in m2, BOQ in m",
  ]);
  const walls = screen.getByText("Walls: BOQ 16, measured 10").closest<HTMLElement>('[role="row"]')!;
  expect(within(walls).getByTitle("Expected → found").textContent).toBe("16.000 → 10.000 m");
  expect(within(walls).getByText("A.1")).toBeTruthy();
  expect(within(walls).getByText("Model")).toBeTruthy();
  expect(within(walls).getByRole("button", { name: "Check the GF plan" })).toBeTruthy();
  const doors = screen.getByText("Door D1: schedule 4, plan 3").closest<HTMLElement>('[role="row"]')!;
  expect(within(doors).getByTitle("Expected → found").textContent).toBe("4 → 3 pcs");
  expect(within(doors).getByText("GF Plan, Schedules")).toBeTruthy();
  // Resolved and ignored rows are muted.
  const plaster = screen.getByText("Plaster: BOQ 12, measured 14").closest<HTMLElement>('[role="row"]')!;
  expect(plaster.className).toContain("done");
  expect(walls.className).not.toContain("done");

  fireEvent.click(within(walls).getByRole("button", { name: "Show" }));
  expect(props.onShow).toHaveBeenCalledWith(conflicts[1]);
  fireEvent.click(screen.getByRole("button", { name: "Compare" }));
  expect(props.onCompare).toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Export conflicts" }));
  expect(props.onExport).toHaveBeenCalled();
});

test("the status filter hides resolved ones; kind chips filter and count", () => {
  renderPanel();
  const status = screen.getByRole("radiogroup", { name: "Status" });
  fireEvent.click(within(status).getByRole("radio", { name: "Open" }));
  expect(reasons()).not.toContain("Plaster: BOQ 12, measured 14");
  expect(groups()).toEqual(["Quantity mismatch 1", "Schedule vs plan 1", "Unit doubt 1"]);

  const kinds = screen.getByRole("group", { name: "Kind" });
  expect(within(kinds).getAllByRole("button").map((b) => b.textContent)).toEqual([
    "All 3",
    "Quantity mismatch 1",
    "Schedule vs plan 1",
    "Unit doubt 1",
  ]);
  fireEvent.click(within(kinds).getByRole("button", { name: "Unit doubt 1" }));
  expect(reasons()).toEqual(["Linked in m2, BOQ in m"]);

  // Ignored: nothing matches; the chips stay.
  fireEvent.click(within(status).getByRole("radio", { name: "Ignored" }));
  expect(screen.getByText("No conflicts match the filter")).toBeTruthy();
  fireEvent.click(within(kinds).getByRole("button", { name: "All 0" }));
  fireEvent.click(within(status).getByRole("radio", { name: "Resolved" }));
  expect(reasons()).toEqual(["Plaster: BOQ 12, measured 14"]);
});

test("changing the status keeps the note; editing the note keeps the status", () => {
  const props = renderPanel();
  const walls = screen.getByText("Walls: BOQ 16, measured 10").closest<HTMLElement>('[role="row"]')!;
  fireEvent.change(within(walls).getByRole("combobox", { name: "Status" }), {
    target: { value: "ignored" },
  });
  expect(props.onResolve).toHaveBeenCalledWith({
    type: "resolve_conflict",
    id: "c2",
    status: "ignored",
    note: "Check the GF plan",
  });

  const plaster = screen.getByText("Plaster: BOQ 12, measured 14").closest<HTMLElement>('[role="row"]')!;
  fireEvent.click(within(plaster).getByRole("button", { name: "Note…" }));
  const input = within(plaster).getByRole("textbox", { name: "Note…" });
  fireEvent.change(input, { target: { value: "  Site measured  " } });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.blur(input);
  expect(props.onResolve).toHaveBeenLastCalledWith({
    type: "resolve_conflict",
    id: "c3",
    status: "resolved",
    note: "Site measured",
  });

  // Escape drops the edit.
  vi.mocked(props.onResolve).mockClear();
  fireEvent.click(within(walls).getByRole("button", { name: "Check the GF plan" }));
  const again = within(walls).getByRole("textbox", { name: "Note…" });
  fireEvent.change(again, { target: { value: "other" } });
  fireEvent.keyDown(again, { key: "Escape" });
  fireEvent.blur(again);
  expect(props.onResolve).not.toHaveBeenCalled();
});

test("empty states: no BOQ, not compared, none found", () => {
  const props = renderPanel({ conflicts: [], hasBoq: false, compared: false });
  expect(screen.getByText(/No BOQ loaded, import one first/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Go to the BOQ" }));
  expect(props.onOpenBoq).toHaveBeenCalled();
  expect((screen.getByRole("button", { name: "Export conflicts" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  cleanup();
  renderPanel({ conflicts: [], compared: false });
  expect(screen.getByText("Not compared yet, press Compare.")).toBeTruthy();
  cleanup();
  renderPanel({ conflicts: [] });
  expect(screen.getByText("No conflicts")).toBeTruthy();
  cleanup();
  renderPanel({ conflicts: [], onCompare: undefined });
  expect((screen.getByRole("button", { name: "Compare" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a hint under the toolbar says to link or name measurements", () => {
  renderPanel();
  expect(screen.getByText("Link or name your measurements to find gaps")).toBeTruthy();
});

test("a click on a conflict's row shows it, like its Show button; one with nothing to show is not clickable", () => {
  const nothing = conflict("c9", "unit_doubt", "Nothing to point at");
  const props = renderPanel({ conflicts: [...conflicts, nothing] });
  const walls = screen.getByText("Walls: BOQ 16, measured 10").closest<HTMLElement>('[role="row"]')!;
  fireEvent.click(walls);
  expect(props.onShow).toHaveBeenCalledWith(conflicts[1]);
  const empty = screen.getByText("Nothing to point at").closest<HTMLElement>('[role="row"]')!;
  expect(empty.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(empty);
  expect(props.onShow).toHaveBeenCalledTimes(1);
  expect((within(empty).getByRole("button", { name: "Show" }) as HTMLButtonElement).disabled).toBe(true);
  // Working in the status or the note is not a click on the row.
  fireEvent.click(within(walls).getByRole("combobox", { name: "Status" }));
  fireEvent.click(within(walls).getByRole("button", { name: "Check the GF plan" }));
  expect(props.onShow).toHaveBeenCalledTimes(1);
});

test("the columns sort inside each kind", () => {
  renderPanel();
  const reasonsNow = () => [...document.querySelectorAll(".reason")].map((p) => p.textContent);
  fireEvent.click(within(screen.getByRole("columnheader", { name: /What/ })).getByRole("button"));
  // Ascending by reason: Plaster before Walls, the kinds in their order.
  expect(reasonsNow()).toEqual([
    "Plaster: BOQ 12, measured 14",
    "Walls: BOQ 16, measured 10",
    "Door D1: schedule 4, plan 3",
    "Linked in m2, BOQ in m",
  ]);
  expect(groups()).toEqual(["Quantity mismatch 2", "Schedule vs plan 1", "Unit doubt 1"]);
});

test("queries show severity, question and cited drawing evidence without an imported BOQ", () => {
  const query = conflict("rfi", "document_mismatch", "Type absent from document", {
    sheets: ["Model"],
    query: {
      severity: "error", question_en: "Confirm the missing type?", question_ar: "يرجى تأكيد النوع؟",
      references: [{ handle: 42, sheet: "Model", grid: "A/1" }],
      citations: [{ source: "spec.pdf", clause: "Table 2" }], clouds: [123], rule: "spec",
    },
  });
  const props = renderPanel({ conflicts: [query], hasBoq: false });
  expect(screen.getByText("Confirm the missing type?")).toBeTruthy();
  expect(screen.getByText("Error")).toBeTruthy();
  expect(screen.getByText("Model / A/1")).toBeTruthy();
  expect(screen.getByTitle("spec.pdf: Table 2")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Show" }));
  expect(props.onShow).toHaveBeenCalledWith(query);
});
