// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { DataGrid, type GridColumn } from "./DataGrid";

beforeEach(() => localStorage.clear());
afterEach(() => {
  cleanup();
  localStorage.clear();
});

type Item = { id: string; code: string; text: string; qty: number | null; section: string };

const items: Item[] = [
  { id: "1", code: "A.10", text: "Walls", qty: 30, section: "Masonry" },
  { id: "2", code: "A.2", text: "Plaster", qty: 5, section: "Masonry" },
  { id: "3", code: "B.1", text: "Doors", qty: null, section: "Joinery" },
  { id: "4", code: "B.2", text: "Windows", qty: 12.5, section: "Joinery" },
];

const columns: GridColumn<Item>[] = [
  { id: "code", header: "Code", size: 60, sortValue: (i) => i.code, cell: (i) => i.code },
  { id: "text", header: "Description", size: 140, grow: true, sortValue: (i) => i.text, cell: (i) => i.text, title: (i) => i.text },
  { id: "qty", header: "Qty", size: 70, align: "end", sortValue: (i) => i.qty, cell: (i) => (i.qty === null ? "—" : i.qty.toFixed(3)) },
  { id: "plain", header: "Plain", size: 50, cell: () => "x" },
];

const key = (i: Item) => i.id;

function grid(more: Partial<Parameters<typeof DataGrid<Item>>[0]> = {}) {
  return render(<DataGrid label="Things" columns={columns} rows={items} rowKey={key} {...more} />);
}

/** The first cell of each row under the header, in order (sections give their name). */
const order = () =>
  screen
    .getAllByRole("row")
    .slice(1)
    .map((r) => r.querySelector('[role="cell"], [role="rowheader"]')!.textContent);

const header = (name: string) => screen.getByRole("columnheader", { name: new RegExp(name) });
const sortButton = (name: string) => within(header(name)).getByRole("button");

test("a table with a header row and one row per item; numbers sit at the end in tabular figures", () => {
  grid();
  expect(screen.getByRole("table", { name: "Things" }).getAttribute("aria-rowcount")).toBe("5");
  expect(screen.getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Code", "Description", "Qty", "Plain"]);
  expect(order()).toEqual(["A.10", "A.2", "B.1", "B.2"]);
  const walls = screen.getAllByRole("row")[1];
  expect(within(walls).getAllByRole("cell")[2].className).toContain("end");
  // The full text of a cell is its tooltip.
  expect(within(walls).getByText("Walls").getAttribute("title")).toBe("Walls");
  expect(header("Qty").className).toContain("end");
});

test("a click on a title sorts: ascending, descending, then back to the data's order; the text in natural order", () => {
  grid();
  fireEvent.click(sortButton("Code"));
  // Natural: A.2 before A.10.
  expect(order()).toEqual(["A.2", "A.10", "B.1", "B.2"]);
  expect(header("Code").getAttribute("aria-sort")).toBe("ascending");
  fireEvent.click(sortButton("Code"));
  expect(order()).toEqual(["B.2", "B.1", "A.10", "A.2"]);
  expect(header("Code").getAttribute("aria-sort")).toBe("descending");
  fireEvent.click(sortButton("Code"));
  expect(order()).toEqual(["A.10", "A.2", "B.1", "B.2"]);
  expect(header("Code").getAttribute("aria-sort")).toBe("none");
});

test("numbers sort as numbers, the largest first; a row with no value stays last either way", () => {
  grid();
  fireEvent.click(sortButton("Qty"));
  expect(order()).toEqual(["A.10", "B.2", "A.2", "B.1"]);
  expect(header("Qty").getAttribute("aria-sort")).toBe("descending");
  fireEvent.click(sortButton("Qty"));
  expect(order()).toEqual(["A.2", "B.2", "A.10", "B.1"]);
  expect(header("Qty").getAttribute("aria-sort")).toBe("ascending");
});

test("a column with no sort value is not a button", () => {
  grid();
  expect(within(header("Plain")).queryByRole("button")).toBeNull();
  expect(header("Plain").getAttribute("aria-sort")).toBeNull();
});

test("sections list their rows under a head, in the data's order; sorting reorders inside them", () => {
  grid({ section: (i) => i.section, sectionCount: true });
  expect(order()).toEqual(["Masonry 2", "A.10", "A.2", "Joinery 2", "B.1", "B.2"]);
  fireEvent.click(sortButton("Description"));
  fireEvent.click(sortButton("Description"));
  // Descending by description: Plaster before Walls, Windows before Doors; the sections stay put.
  expect(order()).toEqual(["Masonry 2", "A.10", "A.2", "Joinery 2", "B.2", "B.1"]);
  fireEvent.click(sortButton("Description"));
  fireEvent.click(sortButton("Code"));
  expect(order()).toEqual(["Masonry 2", "A.2", "A.10", "Joinery 2", "B.1", "B.2"]);
});

test("rows without a section name have no head", () => {
  grid({ section: (i) => (i.id === "3" ? "" : i.section) });
  expect(screen.getAllByRole("rowheader").map((h) => h.textContent)).toEqual(["Masonry", "Joinery"]);
  // The row with no name sits where the first of its kind appears, under no head.
  expect(order()).toEqual(["Masonry", "A.10", "A.2", "B.1", "Joinery", "B.2"]);
});

test("a click, or Enter on the focused row, is the row's action; a disabled row has none", () => {
  const onRowClick = vi.fn();
  grid({ onRowClick, isDisabled: (i) => i.id === "2", isCurrent: (i) => i.id === "1" });
  const [walls, plaster] = screen.getAllByRole("row").slice(1);
  expect(walls.getAttribute("aria-current")).toBe("true");
  fireEvent.click(walls);
  expect(onRowClick).toHaveBeenLastCalledWith(items[0]);
  fireEvent.click(plaster);
  expect(onRowClick).toHaveBeenCalledTimes(1);
  expect(plaster.getAttribute("aria-disabled")).toBe("true");
  fireEvent.keyDown(walls, { key: "Enter" });
  expect(onRowClick).toHaveBeenCalledTimes(2);
  // A key typed in something inside the row is not the row's.
  const inner = document.createElement("input");
  walls.appendChild(inner);
  fireEvent.keyDown(inner, { key: "Enter" });
  expect(onRowClick).toHaveBeenCalledTimes(2);
});

test("the arrows move between rows, Home and End go to the ends; one row is the tab stop", async () => {
  grid();
  const rows = () => screen.getAllByRole("row").slice(1);
  expect(rows().map((r) => r.tabIndex)).toEqual([0, -1, -1, -1]);
  rows()[0].focus();
  fireEvent.keyDown(rows()[0], { key: "ArrowDown" });
  expect(document.activeElement).toBe(rows()[1]);
  expect(rows().map((r) => r.tabIndex)).toEqual([-1, 0, -1, -1]);
  fireEvent.keyDown(rows()[1], { key: "End" });
  expect(document.activeElement).toBe(rows()[3]);
  fireEvent.keyDown(rows()[3], { key: "ArrowDown" });
  expect(document.activeElement).toBe(rows()[3]);
  fireEvent.keyDown(rows()[3], { key: "Home" });
  expect(document.activeElement).toBe(rows()[0]);
});

test("only the rows in view are in the page: a long list is virtualised", () => {
  const many: Item[] = Array.from({ length: 2000 }, (_, i) => ({ id: String(i), code: `C.${i}`, text: `Item ${i}`, qty: i, section: "S" }));
  render(<DataGrid label="Many" columns={columns} rows={many} rowKey={key} />);
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows.length).toBeGreaterThan(10);
  expect(rows.length).toBeLessThan(80);
  expect(screen.getByRole("table").getAttribute("aria-rowcount")).toBe("2001");
});

test("the actions are in the row, for the keyboard and the pointer", () => {
  const onAct = vi.fn();
  grid({
    actions: (i) => (
      <button type="button" onClick={() => onAct(i.id)}>
        Act {i.id}
      </button>
    ),
  });
  fireEvent.click(screen.getByRole("button", { name: "Act 2" }));
  expect(onAct).toHaveBeenCalledWith("2");
  expect(screen.getByRole("button", { name: "Act 2" }).closest(".grid-actions")).toBeTruthy();
});

test("a column is resized from the keyboard on its edge, never below its narrowest, and the width is remembered", () => {
  const { unmount } = grid({ storageKey: "test" });
  const widths = () => (document.querySelector(".grid-inner") as HTMLElement).style.getPropertyValue("--cols");
  expect(widths()).toBe("60px minmax(140px, 1fr) 70px 50px");
  const edge = screen.getByRole("separator", { name: "Resize column Qty" });
  expect(edge.getAttribute("aria-valuenow")).toBe("70");
  fireEvent.keyDown(edge, { key: "ArrowRight" });
  expect(widths()).toBe("60px minmax(140px, 1fr) 86px 50px");
  for (let i = 0; i < 10; i++) fireEvent.keyDown(edge, { key: "ArrowLeft" });
  expect(widths()).toBe("60px minmax(140px, 1fr) 40px 50px");
  fireEvent.keyDown(edge, { key: "ArrowRight" });
  // Another grid of the same name starts from it.
  unmount();
  grid({ storageKey: "test" });
  expect(widths()).toBe("60px minmax(140px, 1fr) 56px 50px");
  // A double click on the edge puts it back.
  fireEvent.doubleClick(screen.getByRole("separator", { name: "Resize column Qty" }));
  expect(widths()).toBe("60px minmax(140px, 1fr) 70px 50px");
});

test("a drag on an edge resizes the column", () => {
  grid();
  const edge = screen.getByRole("separator", { name: "Resize column Code" });
  fireEvent.mouseDown(edge, { clientX: 100 });
  act(() => void document.dispatchEvent(new MouseEvent("mousemove", { clientX: 130, bubbles: true })));
  act(() => void document.dispatchEvent(new MouseEvent("mouseup", { clientX: 130, bubbles: true })));
  const widths = (document.querySelector(".grid-inner") as HTMLElement).style.getPropertyValue("--cols");
  expect(widths).toBe("90px minmax(140px, 1fr) 70px 50px");
});

test("in Arabic the grid mirrors: the edge's right arrow narrows a column, the left one widens it", () => {
  localStorage.setItem("qiyas.lang", "ar");
  try {
    render(
      <LanguageProvider>
        <DataGrid label="Things" columns={columns} rows={items} rowKey={key} />
      </LanguageProvider>,
    );
    const widths = () => (document.querySelector(".grid-inner") as HTMLElement).style.getPropertyValue("--cols");
    const edge = screen.getByRole("separator", { name: "تغيير عرض العمود \u2068Qty\u2069" });
    fireEvent.keyDown(edge, { key: "ArrowRight" });
    expect(widths()).toBe("60px minmax(140px, 1fr) 54px 50px");
    fireEvent.keyDown(edge, { key: "ArrowLeft" });
    fireEvent.keyDown(edge, { key: "ArrowLeft" });
    expect(widths()).toBe("60px minmax(140px, 1fr) 86px 50px");
  } finally {
    localStorage.clear();
    document.documentElement.dir = "ltr";
    document.documentElement.lang = "en";
  }
});

test("without rows it shows what it is given", () => {
  render(<DataGrid label="None" columns={columns} rows={[]} rowKey={key} empty={<p>Nothing here</p>} />);
  expect(screen.getByText("Nothing here")).toBeTruthy();
  expect(screen.getAllByRole("row")).toHaveLength(1);
});
