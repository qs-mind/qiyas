// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import type { TabInfo } from "../api";
import { cycleTab, FileTabs } from "./FileTabs";
import { FileName } from "./FileName";

afterEach(cleanup);

const tabs: TabInfo[] = [
  { id: 1, path: "/plans/villa.dwg", title: "villa.dwg", dirty: false },
  { id: 4, path: "/plans/tower.dxf", title: "tower.dxf", dirty: true },
];

test("a tab per drawing: the active one selected, a dot while unsaved", () => {
  const onSelect = vi.fn();
  render(<FileTabs tabs={tabs} active={4} onSelect={onSelect} onClose={() => {}} />);
  const villa = screen.getByRole("tab", { name: "villa.dwg" });
  const tower = screen.getByRole("tab", { name: /tower\.dxf/ });
  expect(villa.getAttribute("aria-selected")).toBe("false");
  expect(tower.getAttribute("aria-selected")).toBe("true");
  expect(tower.querySelector(".dirty")).not.toBeNull();
  expect(villa.querySelector(".dirty")).toBeNull();
  fireEvent.click(villa);
  expect(onSelect).toHaveBeenCalledWith(1);
});

test("× and a middle click close a tab", () => {
  const onClose = vi.fn();
  const onSelect = vi.fn();
  render(<FileTabs tabs={tabs} active={1} onSelect={onSelect} onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Close villa.dwg" }));
  expect(onClose).toHaveBeenLastCalledWith(1);
  fireEvent(
    screen.getByRole("tab", { name: /tower\.dxf/ }),
    new MouseEvent("auxclick", { bubbles: true, button: 1 }),
  );
  expect(onClose).toHaveBeenLastCalledWith(4);
  expect(onSelect).not.toHaveBeenCalled();
});

test("no tabs with no drawing open", () => {
  const { container } = render(
    <FileTabs tabs={[]} active={null} onSelect={() => {}} onClose={() => {}} />,
  );
  expect(container.textContent).toBe("");
});

test("Ctrl+Tab goes round the tabs both ways", () => {
  const three = [...tabs, { id: 7, path: "/c.dwg", title: "c.dwg", dirty: false }];
  expect(cycleTab(three, 1, 1)).toBe(4);
  expect(cycleTab(three, 7, 1)).toBe(1);
  expect(cycleTab(three, 1, -1)).toBe(7);
  expect(cycleTab([], null, 1)).toBeNull();
});

test("one tab is in the Tab order and the arrows move between tabs", () => {
  render(<FileTabs tabs={tabs} active={4} onSelect={() => {}} onClose={() => {}} />);
  const villa = screen.getByRole("tab", { name: "villa.dwg" });
  const tower = screen.getByRole("tab", { name: /tower\.dxf/ });
  expect([villa.tabIndex, tower.tabIndex]).toEqual([-1, 0]);
  tower.focus();
  fireEvent.keyDown(tower, { key: "ArrowLeft" });
  expect(document.activeElement).toBe(villa);
});

test("each drawing exposes its own Mizan status", () => {
  render(<LanguageProvider><FileTabs tabs={[
    { id: 1, path: "one.dxf", title: "One", dirty: false, status: "working" },
    { id: 2, path: "two.dxf", title: "Two", dirty: false, status: "queued" },
    { id: 3, path: "three.dxf", title: "Three", dirty: false, status: "done" },
  ]} active={1} onSelect={() => {}} onClose={() => {}} /></LanguageProvider>);
  expect(screen.getByLabelText("Mizan is working…")).toBeTruthy();
  expect(screen.getByLabelText("Waiting for a free Mizan slot…")).toBeTruthy();
  expect(screen.getByLabelText("Mizan finished")).toBeTruthy();
});

test("a drawing Mizan works on has a dot before its name and no words (note 9)", () => {
  const working: TabInfo[] = [{ ...tabs[0], busy: true, status: "working" }, { ...tabs[1], status: "done" }];
  render(<FileTabs tabs={working} active={1} onSelect={() => {}} onClose={() => {}} />);
  const villa = screen.getAllByRole("tab")[0];
  const children = [...villa.children].map((el) => el.className || el.tagName);
  expect(children[0]).toBe("tab-dot working");
  expect(children[1]).toBe("BDI");
  expect(villa.textContent).toBe("villa.dwg");
  expect(screen.queryByText(/working/i)).toBeNull();
  // Done is a still dot; an idle drawing has none.
  expect(screen.getAllByRole("tab")[1].querySelector(".tab-dot.done")).not.toBeNull();
});

test("an Arabic file name keeps its extension at its end", () => {
  render(<FileName name="فيلا صغيرة.dwg" />);
  const outer = document.querySelector("bdi[dir='ltr']")!;
  expect(outer.firstElementChild?.textContent).toBe("فيلا صغيرة");
  expect(outer.lastChild?.textContent).toBe(".dwg");
});
