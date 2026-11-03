// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { SheetSet, SheetSetView } from "../api";
import { LanguageProvider } from "../i18n";
import { SheetSetManager, type SheetSetApi } from "./SheetSetManager";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

/** The view the core would give for `set` saved at /jobs/Terminal.qss; tower.dwg is missing. */
const viewOf = (set: SheetSet): SheetSetView => ({
  set,
  entries: set.drawings.map((d) => ({
    ...d,
    full: `/jobs/${d.path}`,
    missing: d.path === "tower.dwg",
  })),
});

function setup() {
  const api: SheetSetApi = {
    open: vi.fn(async () =>
      viewOf({ name: "Terminal", drawings: [{ path: "plans/ground.dwg", layouts: ["A1"] }] }),
    ),
    save: vi.fn(async (_path: string, set: SheetSet) => viewOf(set)),
    fromTabs: vi.fn(async (_qss: string, name: string) => ({
      set: {
        name,
        drawings: [
          { path: "plans/ground.dwg", layouts: ["A1", "A2"] },
          { path: "tower.dwg", layouts: [] },
        ],
      },
      skipped: ["/elsewhere/site.dwg"],
    })),
    entry: vi.fn(async () => ({ path: "roof.dwg", layouts: ["R1"] })),
    pickNew: vi.fn(async () => "/jobs/Terminal.qss"),
    pickOpen: vi.fn(async () => "/jobs/Terminal.qss"),
    pickDrawing: vi.fn(async () => "/jobs/roof.dwg"),
  };
  const onOpenAll = vi.fn(async () => {});
  render(
    <LanguageProvider>
      <SheetSetManager api={api} onOpenAll={onOpenAll} onClose={() => {}} />
    </LanguageProvider>,
  );
  return { api, onOpenAll };
}

const button = (name: string) => screen.getByRole("button", { name }) as HTMLButtonElement;

test("a sheet set made from the open drawings is written beside them", async () => {
  const { api } = setup();
  fireEvent.click(button("New from open drawings…"));
  await waitFor(() => expect(api.save).toHaveBeenCalled());
  expect(api.fromTabs).toHaveBeenCalledWith("/jobs/Terminal.qss", "Terminal");
  const rows = screen.getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(rows[0].textContent).toContain("plans/ground.dwg");
  expect(rows[0].textContent).toContain("A1, A2");
  expect(rows[1].textContent).toContain("Missing");
  // The open drawings outside its folder are named.
  expect(screen.getByRole("status").textContent).toContain("/elsewhere/site.dwg");
});

test("drawings are added and removed, each change saved; open all opens those found", async () => {
  const { api, onOpenAll } = setup();
  fireEvent.click(button("Open…"));
  await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));

  fireEvent.click(button("Add drawing…"));
  await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(2));
  expect(api.entry).toHaveBeenCalledWith("/jobs/Terminal.qss", "/jobs/roof.dwg");
  expect(api.save).toHaveBeenLastCalledWith("/jobs/Terminal.qss", {
    name: "Terminal",
    drawings: [
      { path: "plans/ground.dwg", layouts: ["A1"] },
      { path: "roof.dwg", layouts: ["R1"] },
    ],
  });

  fireEvent.click(screen.getAllByRole("button", { name: "Remove" })[0]);
  await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));
  expect(api.save).toHaveBeenLastCalledWith("/jobs/Terminal.qss", {
    name: "Terminal",
    drawings: [{ path: "roof.dwg", layouts: ["R1"] }],
  });

  fireEvent.click(button("Open all"));
  await waitFor(() => expect(onOpenAll).toHaveBeenCalledWith(["/jobs/roof.dwg"]));
  // Printing comes with plotting.
  expect(button("Print all").disabled).toBe(true);
});
