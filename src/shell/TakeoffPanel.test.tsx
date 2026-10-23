// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Boq, Measurement, Total } from "../api";
import { boqItem } from "../boq/testItems";
import { LanguageProvider } from "../i18n";
import { TakeoffPanel } from "./TakeoffPanel";

afterEach(cleanup);

/** The grid's rows under its header. */
const bodyRows = () => screen.getAllByRole("row").slice(1);

const item = (id: string, description: string, shown: number, more: Partial<Measurement> = {}): Measurement => ({
  id,
  kind: "length",
  value: shown,
  unit: "m",
  shown,
  description,
  layer: "A-WALL",
  sheet: "*Model_Space",
  handles: [],
  points: [],
  block_name: null,
  self_intersecting: false,
  source: "user",
  note: "",
  created: "2026-10-08T00:00:00Z",
  height: null,
  boq_code: null,
  chain: [],
  block_path: [],
  ...more,
});

test("takeoff panel lists items and totals", () => {
  const onSelect = vi.fn();
  const onRemove = vi.fn();
  const onRename = vi.fn();
  const totals: Total[] = [{ kind: "length", unit: "m", count: 2, sum: 15 }];
  render(
    <TakeoffPanel
      items={[item("a", "Wall run", 10), item("b", "Kerb", 5, { self_intersecting: true })]}
      totals={totals}
      selectedId="a"
      onSelect={onSelect}
      onRemove={onRemove}
      onRename={onRename}
      boq={null}
      onLink={() => {}}
    />,
  );

  const rows = bodyRows();
  expect(rows).toHaveLength(2);
  expect(rows[0].textContent).toContain("10.000");
  expect(rows[0].getAttribute("aria-current")).toBe("true");
  expect(screen.getByTitle("Self-crossing shape")).toBeTruthy();

  const totalsList = screen.getByRole("list", { name: "Totals" });
  expect(totalsList.textContent).toContain("15.000");
  expect(totalsList.textContent).toContain("2 ×");

  fireEvent.click(within(rows[1]).getByText("Kerb"));
  expect(onSelect).toHaveBeenCalledWith("b");
  fireEvent.click(screen.getByRole("button", { name: "Remove: Kerb" }));
  expect(onRemove).toHaveBeenCalledWith("b");

  fireEvent.click(screen.getByRole("button", { name: "Rename: Wall run" }));
  const input = screen.getByRole("textbox", { name: "Description" });
  fireEvent.change(input, { target: { value: "Ground floor walls" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onRename).toHaveBeenCalledWith("a", "Ground floor walls");
});

test("counts show as whole numbers", () => {
  const totals: Total[] = [
    { kind: "area", unit: "m2", count: 1, sum: 15 },
    { kind: "count", unit: "pcs", count: 1, sum: 3 },
  ];
  render(
    <TakeoffPanel
      items={[item("c", "Doors", 3, { kind: "count", unit: "pcs" })]}
      totals={totals}
      selectedId={null}
      onSelect={() => {}}
      onRemove={() => {}}
      onRename={() => {}}
      boq={null}
      onLink={() => {}}
    />,
  );
  const [row] = bodyRows();
  expect(row.textContent).toContain("3pcs");
  expect(row.textContent).not.toContain("3.000");
  const [area, count] = within(screen.getByRole("list", { name: "Totals" })).getAllByRole("listitem");
  expect(area.textContent).toContain("15.000");
  expect(count.textContent).toContain("3 pcs");
  expect(count.textContent).not.toContain("3.000");
});

test("the BOQ column and the link picker", () => {
  const onLink = vi.fn();
  const boq: Boq = {
    source: "boq.xlsx",
    imported: "2026-10-08T00:00:00Z",
    skipped_rows: 0,
    items: [boqItem("A.1", "Block walls", "m", 10), boqItem("A.2", "Plaster", "m2", 5)],
  };
  const items = [item("a", "Wall run", 10, { boq_code: "A.1" }), item("b", "Kerb", 5)];
  const props = { items, totals: [], selectedId: null, onSelect() {}, onRemove() {}, onRename() {} };
  const { rerender } = render(<TakeoffPanel {...props} boq={null} onLink={onLink} />);
  const rows = bodyRows();
  expect(within(rows[0]).getByText("A.1")).toBeTruthy();
  expect(within(rows[1]).getByText("—")).toBeTruthy();
  const button = within(rows[1]).getByRole("button", { name: "Import a BOQ first" }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);

  rerender(<TakeoffPanel {...props} boq={boq} onLink={onLink} />);
  fireEvent.click(screen.getByRole("button", { name: "Link: Kerb" }));
  const search = screen.getByRole("searchbox", { name: "Search…" });
  fireEvent.change(search, { target: { value: "plaster" } });
  fireEvent.keyDown(search, { key: "Enter" });
  expect(onLink).toHaveBeenCalledWith("b", "A.2");
  expect(screen.queryByRole("dialog")).toBeNull();
  // The keyboard goes on from the button that opened the picker.
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Link: Kerb" }));

  fireEvent.click(screen.getByRole("button", { name: "Link: Kerb" }));
  fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Link: Kerb" }));

  fireEvent.click(screen.getByRole("button", { name: "Link: Wall run" }));
  fireEvent.click(screen.getByRole("button", { name: "Unlink" }));
  expect(onLink).toHaveBeenLastCalledWith("a", null);
});

test("a row measured inside a block names the block path in its tooltip", () => {
  render(
    <TakeoffPanel
      items={[
        item("a", "Line on A-DOOR", 1.8, { chain: [5, 8], block_path: ["PLAN", "DOOR"] }),
        item("b", "Wall run", 10),
      ]}
      totals={[]}
      selectedId={null}
      onSelect={() => {}}
      onRemove={() => {}}
      onRename={() => {}}
      boq={null}
      onLink={() => {}}
    />,
  );
  const row = (text: string) => screen.getByText(text).closest("button")!;
  expect(row("Line on A-DOOR").getAttribute("title")).toBe("Line on A-DOOR\nIn block PLAN / DOOR");
  expect(row("Wall run").getAttribute("title")).toBe("Wall run");
});

test("the layer column shows a bound xref's layer by its own name, the full name as its tooltip", () => {
  render(
    <TakeoffPanel
      items={[item("a", "Wall run", 10, { layer: "01-IVORY$0$A-WALL" })]}
      totals={[]}
      selectedId={null}
      onSelect={() => {}}
      onRemove={() => {}}
      onRename={() => {}}
      boq={null}
      onLink={() => {}}
    />,
  );
  const shown = screen.getByText("A-WALL");
  expect(shown.tagName).toBe("BDI");
  expect(shown.closest('[role="cell"]')!.getAttribute("title")).toBe("01-IVORY$0$A-WALL");
});

test("area basis and bounded count remain visible, including legacy rows", () => {
  render(<TakeoffPanel
    items={[
      item("a", "Boundary", 12, { kind: "area", area_origin: "existing_boundary", measurement_basis: "gross" }),
      item("b", "Wall", 12, { kind: "area", area_origin: "wall_face", measurement_basis: "wall_face" }),
      item("c", "Old", 12, { kind: "area" }),
      item("d", "Doors", 3, { kind: "count", count_scope: "top_level_insert_records_with_loaded_xref_tops" }),
    ]}
    totals={[]} selectedId={null} onSelect={() => {}} onRemove={() => {}}
    onRename={() => {}} boq={null} onLink={() => {}}
  />);
  expect(screen.getByText("Gross")).toBeTruthy();
  expect(screen.getByText("Wall face basis")).toBeTruthy();
  expect(screen.queryByText("Area source unspecified · Basis unspecified")).toBeNull();
  expect(screen.getByText("Count may miss nested blocks")).toBeTruthy();
});


test.each(["en", "ar"])("full names and quantity limits remain separate in %s", (lang) => {
  localStorage.setItem("qiyas.lang", lang);
  const description = "Doors / أبواب الدور الأرضي — long measurement description";
  render(<LanguageProvider><TakeoffPanel
    items={[
      item("a", description, 4, { kind: "count", count_scope: "top_level_insert_records_with_loaded_xref_tops" }),
      item("b", "Legacy area / مساحة قديمة", 12, { kind: "area" }),
    ]}
    totals={[]} selectedId={null} onSelect={() => {}} onRemove={() => {}}
    onRename={() => {}} boq={null} onLink={() => {}}
  /></LanguageProvider>);
  const name = screen.getByRole("button", { name: description });
  const limit = name.parentElement!.querySelector("small")!;
  expect(limit.textContent).toBe(lang === "en"
    ? "Count may miss nested blocks"
    : "قد لا يشمل العدّ البلوكات المتداخلة");
  expect(screen.queryByText(lang === "en"
    ? "Area source unspecified · Basis unspecified"
    : "مصدر المساحة غير محدد · أساس غير محدد")).toBeNull();
  expect(name.closest(".grid-row")!.getAttribute("data-index")).toBe("0");
  localStorage.removeItem("qiyas.lang");
});
