// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { XrefInfo } from "../api";
import { XrefManager } from "./XrefManager";

const xref = (name: string, patch: Partial<XrefInfo> = {}): XrefInfo => ({
  name,
  found: true,
  loaded: true,
  chosen: false,
  path: `/jobs/${name}.dwg`,
  overlay: false,
  unloaded: false,
  layer_count: 3,
  problems: [],
  ...patch,
});

beforeAll(() => {
  // jsdom may not implement modal dialogs.
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

test("lists each xref's status and locates the ones not loaded", async () => {
  const onLocate = vi.fn(async () => {});
  const onReload = vi.fn(async () => {});
  const onClear = vi.fn(async () => {});
  render(
    <XrefManager
      xrefs={[
        xref("plan-arch", { chosen: true }),
        xref("grid", { found: false, loaded: false, path: "..\\grid.dwg" }),
        xref("old", { found: false, loaded: false, unloaded: true }),
        xref("site", { overlay: true }),
        xref("broken", { loaded: false, problems: ["Xref broken could not be read: bad"] }),
      ]}
      onReload={onReload}
      onLocate={onLocate}
      onClear={onClear}
      onClose={() => {}}
    />,
  );
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows.map((r) => r.textContent)).toEqual([
    "plan-archFound/jobs/plan-arch.dwgClear chosen file",
    "gridMissing..\\grid.dwgLocate…",
    "oldUnloaded/jobs/old.dwg",
    "siteOverlayFound/jobs/site.dwg",
    "brokenUnreadable/jobs/broken.dwgXref broken could not be read: badLocate…",
  ]);
  expect(screen.getAllByRole("button", { name: /^Locate…/ })).toHaveLength(2);

  fireEvent.click(screen.getByRole("button", { name: "Locate…: grid" }));
  await waitFor(() => expect(onLocate).toHaveBeenCalledWith("grid"));
  fireEvent.click(screen.getByRole("button", { name: "Clear chosen file: plan-arch" }));
  await waitFor(() => expect(onClear).toHaveBeenCalledWith("plan-arch"));
  fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  await waitFor(() => expect(onReload).toHaveBeenCalled());
});

test("a failed reload is shown and the dialog stays", async () => {
  render(
    <XrefManager
      xrefs={[]}
      onReload={async () => {
        throw "disk gone";
      }}
      onLocate={async () => {}}
      onClear={async () => {}}
      onClose={() => {}}
    />,
  );
  expect(screen.getByText("This drawing has no external references.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  expect((await screen.findByRole("alert")).textContent).toBe("The file could not be written. Check the folder and available space.");
});
