// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { ScriptResult } from "../api";
import { LanguageProvider, useLanguage } from "../i18n";
import ar from "../i18n/ar.json";
import { ScriptResultDialog } from "./ScriptResultDialog";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

const failed: ScriptResult = {
  ran: true,
  lines: [
    { line_no: 2, text: "COUNT DOOR", ok: true, message: "DOOR count = 3 pcs" },
    { line_no: 3, text: "LAYOFF A-WALL A-DOOR", ok: true, message: "layer A-WALL off" },
    { line_no: 3, text: "LAYOFF A-WALL A-DOOR", ok: true, message: "layer A-DOOR off" },
    { line_no: 5, text: "MEASURE FFFF", ok: false, message: "No entity FFFF" },
  ],
  changed: true,
  took_ms: 1340,
};

test("each line run shows with its mark and what it did; the failing line says why", () => {
  const onClose = vi.fn();
  render(
    <LanguageProvider>
      <ScriptResultDialog path="/jobs/takeoff/count.scr" result={failed} onClose={onClose} />
    </LanguageProvider>,
  );
  expect(screen.getByRole("heading").textContent).toBe("Script count.scr");
  expect(screen.getByText("Stopped at line 5 · 1.3 s")).toBeTruthy();
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows).toHaveLength(4);
  expect(rows.map((r) => within(r).getAllByRole("cell")[0].textContent)).toEqual(["2", "3", "3", "5"]);
  expect(within(rows[0]).getByRole("img", { name: "Done" })).toBeTruthy();
  expect(rows[0].textContent).toContain("COUNT DOOR");
  expect(rows[0].textContent).toContain("DOOR count = 3 pcs");
  expect(within(rows[3]).getByRole("img", { name: "Failed" })).toBeTruthy();
  expect(rows[3].textContent).toContain("No entity FFFF");
  // Diagnostic words are available only by opening the details, in their own direction.
  const details = rows[3].querySelector("details")!;
  expect(details.open).toBe(false);
  expect(details.querySelector("pre")?.getAttribute("dir")).toBe("auto");
  expect(rows[3].querySelector("td > span[dir]")?.textContent).not.toContain("FFFF");
  // The file name is isolated inside the title.
  expect(screen.getByRole("heading").querySelector("bdi")?.textContent).toBe("count.scr");

  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledOnce();
});

test("a run that went through says how many lines ran, in Arabic too", () => {
  const done: ScriptResult = { ran: true, lines: failed.lines.slice(0, 2), changed: true, took_ms: 40 };
  function Arabic() {
    const { setLang } = useLanguage();
    return (
      <>
        <button type="button" onClick={() => setLang("ar")}>
          ar
        </button>
        <ScriptResultDialog path="C:\\jobs\\count.scr" result={done} onClose={() => {}} />
      </>
    );
  }
  render(
    <LanguageProvider>
      <Arabic />
    </LanguageProvider>,
  );
  expect(screen.getByText("2 lines run · 40 ms")).toBeTruthy();
  act(() => fireEvent.click(screen.getByRole("button", { name: "ar" })));
  expect(screen.getByRole("heading").textContent).toBe(ar["script.title"].replace("{name}", "count.scr"));
  expect(screen.getAllByRole("img", { name: ar["script.ok"] })).toHaveLength(2);
});

test("a script that doesn't read says nothing ran, not where it stopped", () => {
  const unread: ScriptResult = {
    ran: false,
    lines: [{ line_no: 3, text: "FOO bar", ok: false, message: "unknown command FOO" }],
    changed: false,
    took_ms: 2,
  };
  render(
    <LanguageProvider>
      <ScriptResultDialog path="/jobs/bad.scr" result={unread} onClose={() => {}} />
    </LanguageProvider>,
  );
  expect(screen.getByText("Line 3 could not be read; nothing ran · 2 ms")).toBeTruthy();
  expect(screen.queryByText(/Stopped at line/)).toBeNull();
});
