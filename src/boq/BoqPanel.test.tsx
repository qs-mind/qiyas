// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Boq } from "../api";
import { BoqPanel } from "./BoqPanel";
import { boqItem, row } from "./testItems";

afterEach(cleanup);

const boq: Boq = {
  source: "/jobs/villa/boq.xlsx",
  imported: "2026-10-08T00:00:00Z",
  skipped_rows: 2,
  items: [
    boqItem("A.1", "Block walls 200 mm", "m", 100, { section: "Masonry" }),
    boqItem("A.2", "Plaster to walls", "m2", 40, { section: "Masonry", tolerance: 10 }),
    boqItem("B.1", "Doors", "pcs", 3, { section: "Joinery" }),
  ],
};

const takeoff = [
  row("w1", 70, "m", "A.1"),
  row("w2", 50, "m", "A.1"),
  row("p1", 42, "m2", "A.2"),
  row("p2", 6, "m", "A.2"),
];

function renderPanel(more: Partial<Parameters<typeof BoqPanel>[0]> = {}) {
  const props = {
    boq,
    takeoff,
    selectedCode: null,
    onSelect: vi.fn(),
    onImport: vi.fn(),
    onTolerance: vi.fn(),
    ...more,
  };
  render(<BoqPanel {...props} />);
  return props;
}

/** The grid's rows under its header: the section heads and the items. */
const bodyRows = () => screen.getAllByRole("row").slice(1);

test("lists items under their section headers, with the skipped rows notice", () => {
  renderPanel();
  const rows = bodyRows();
  expect(rows.map((r) => r.textContent?.slice(0, 12))).toEqual([
    "Masonry",
    expect.stringContaining("A.1"),
    expect.stringContaining("A.2"),
    "Joinery",
    expect.stringContaining("B.1"),
  ]);
  // A section head is one cell across the row.
  expect(within(rows[0]).getByRole("rowheader").textContent).toBe("Masonry");
  expect(within(rows[0]).queryAllByRole("cell")).toHaveLength(0);
  expect(screen.getByRole("status").textContent).toBe("Skipped rows: 2");
  expect(screen.getByText("boq.xlsx")).toBeTruthy();
});

test("measured sum and an over-tolerance difference", () => {
  const props = renderPanel({ selectedCode: "A.1" });
  const cells = within(bodyRows()[1]).getAllByRole("cell");
  expect(cells.map((c) => c.textContent)).toEqual([
    "A.1",
    "Block walls 200 mm",
    "m",
    "100.000",
    "120.000",
    "+20.0 %",
    "2",
    "5 %",
    "?",
  ]);
  expect(within(cells[8]).getByRole("button", { name: "How did you get this number? boq-row:0" })).toBeTruthy();
  expect(cells[5].className).toContain("diff-over");
  expect(bodyRows()[1].getAttribute("aria-current")).toBe("true");

  // No links: a muted dash.
  const doors = within(bodyRows()[4]).getAllByRole("cell");
  expect(doors[3].textContent).toBe("3");
  expect(doors[5].className).toContain("diff-none");
  expect(doors[5].textContent).toBe("—");

  fireEvent.click(cells[1]);
  expect(props.onSelect).toHaveBeenCalledWith("A.1");
});

test("generated quantities own their codes in the imported BOQ comparison and follow edits", () => {
  const item = { code: "A.1", description_en: "Generated", description_ar: "مشتق", section: "Test", trade: "Test", unit: "m", quantity: 100, lines: [] };
  const props = { boq, takeoff, selectedCode: null, onSelect: vi.fn(), onTolerance: vi.fn() };
  const view = render(<BoqPanel {...props} generated={{ items: [item], warnings: [] }} />);
  const cells = () => within(bodyRows()[1]).getAllByRole("cell");
  expect(cells()[4].textContent).toBe("100.000");
  expect(cells()[5].textContent).toBe("0.0 %");
  expect(cells()[6].textContent).toBe("1");
  view.rerender(<BoqPanel {...props} generated={{ items: [{ ...item, quantity: 110 }], warnings: [] }} />);
  expect(cells()[4].textContent).toBe("110.000");
  expect(cells()[5].textContent).toBe("+10.0 %");
});

test("a link in another unit shows a unit mismatch", () => {
  renderPanel();
  const cells = within(bodyRows()[2]).getAllByRole("cell");
  expect(cells[4].textContent).toBe("42.000 · 6.000 m");
  expect(within(cells[4]).getByTitle("Unit mismatch")).toBeTruthy();
  // 42 against 40 is +5 %, within the item's own 10 %.
  expect(cells[5].className).toContain("diff-within");
  expect(cells[6].textContent).toBe("2");
  expect(cells[7].textContent).toBe("10 %");
});

test("search filters on code and description", () => {
  renderPanel();
  const search = screen.getByRole("searchbox", { name: "Search…" });
  fireEvent.change(search, { target: { value: "DOOR" } });
  const rows = bodyRows().map((r) => r.textContent?.slice(0, 7));
  expect(rows).toEqual(["Joinery", expect.stringContaining("B.1")]);
  fireEvent.change(search, { target: { value: "a.2" } });
  expect(bodyRows()).toHaveLength(2);
  fireEvent.change(search, { target: { value: "nothing like it" } });
  expect(screen.getByText("No items match")).toBeTruthy();
});

test("double click the tolerance to change it; empty goes back to the default", () => {
  const props = renderPanel();
  const tolerance = within(bodyRows()[2]).getAllByRole("cell")[7];
  fireEvent.click(tolerance);
  fireEvent.click(tolerance);
  fireEvent.doubleClick(tolerance);
  // Editing the tolerance doesn't select the row and zoom to it.
  expect(props.onSelect).not.toHaveBeenCalled();
  const input = screen.getByRole("spinbutton", { name: "Tolerance" });
  fireEvent.change(input, { target: { value: "12.5" } });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.blur(input);
  expect(props.onTolerance).toHaveBeenCalledWith("A.2", 12.5);

  fireEvent.doubleClick(tolerance);
  const again = screen.getByRole("spinbutton", { name: "Tolerance" });
  fireEvent.change(again, { target: { value: "" } });
  fireEvent.blur(again);
  expect(props.onTolerance).toHaveBeenLastCalledWith("A.2", null);
});

test("no BOQ: the import button, disabled without a drawing", () => {
  const props = renderPanel({ boq: null });
  expect(screen.getByText("No BOQ")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Import BOQ" }));
  expect(props.onImport).toHaveBeenCalled();
  cleanup();
  renderPanel({ boq: null, onImport: undefined });
  expect((screen.getByRole("button", { name: "Import BOQ" }) as HTMLButtonElement).disabled).toBe(true);
});

test("an item without a code can't be selected", () => {
  const props = renderPanel({
    boq: { ...boq, items: [boqItem("", "Preliminaries", "item", 1), ...boq.items] },
  });
  const [first] = bodyRows();
  expect(first.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(within(first).getAllByRole("cell")[1]);
  expect(props.onSelect).not.toHaveBeenCalled();
  expect(bodyRows()[2].getAttribute("aria-disabled")).toBeNull();
});

test("a click on a title sorts the items inside their sections; the sections keep their order", () => {
  renderPanel();
  const title = () => within(screen.getByRole("columnheader", { name: /Difference/ })).getByRole("button");
  // The largest difference first (numbers start with the largest): A.1 is +20 %, A.2 within 5 %.
  fireEvent.click(title());
  expect(bodyRows().map((r) => r.textContent?.slice(0, 7))).toEqual([
    "Masonry",
    expect.stringContaining("A.1"),
    expect.stringContaining("A.2"),
    "Joinery",
    expect.stringContaining("B.1"),
  ]);
  fireEvent.click(title());
  expect(bodyRows().map((r) => r.textContent?.slice(0, 7))).toEqual([
    "Masonry",
    expect.stringContaining("A.2"),
    expect.stringContaining("A.1"),
    "Joinery",
    expect.stringContaining("B.1"),
  ]);
  expect(screen.getByRole("columnheader", { name: /Difference/ }).getAttribute("aria-sort")).toBe("ascending");
});

test("the description's full text is its tooltip, and the numbers are in tabular figures at the end", () => {
  renderPanel();
  const cells = within(bodyRows()[1]).getAllByRole("cell");
  expect(cells[1].getAttribute("title")).toBe("Block walls 200 mm");
  for (const i of [3, 4, 5, 6, 7]) expect(cells[i].className).toContain("end");
});
