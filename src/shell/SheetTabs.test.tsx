// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Sheet } from "../api";
import { LanguageProvider } from "../i18n";
import { SheetTabs } from "./SheetTabs";
import css from "./theme.css?raw";

const sheets: Sheet[] = [
  { block: "*Model_Space", name: "Model", is_model: true },
  ...["A-101", "A-102", "A-201", "A-301", "A-501", "A-602"].map((name, i) => ({ block: `*Paper_Space${i}`, name, is_model: false })),
] as Sheet[];

/** jsdom has no layout: the strip's measures are given. */
let box = { client: 300, scroll: 300 };
beforeEach(() => {
  box = { client: 300, scroll: 300 };
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("sheet-tabs") ? box.client : 0;
  });
  vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("sheet-tabs") ? box.scroll : 0;
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const mount = () => render(<LanguageProvider><SheetTabs sheets={sheets} active="*Model_Space" onSelect={() => {}} /></LanguageProvider>);

test("tabs that fit have no arrows", () => {
  mount();
  expect(screen.queryByRole("button", { name: "More layouts" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Earlier layouts" })).toBeNull();
});

test("tabs that do not fit get an arrow at the edge with more, and never a scrollbar over the names (note 47)", () => {
  box = { client: 300, scroll: 700 };
  mount();
  const strip = document.querySelector(".sheet-strip")!;
  expect(strip.classList.contains("more-end")).toBe(true);
  expect(strip.classList.contains("more-start")).toBe(false);
  const list = document.querySelector<HTMLElement>(".sheet-tabs")!;
  const scrollBy = vi.fn();
  list.scrollBy = scrollBy;
  fireEvent.click(screen.getByRole("button", { name: "More layouts" }));
  expect(scrollBy).toHaveBeenCalledWith(expect.objectContaining({ left: 240 }));
  // Scrolled along: the start edge has more now, the end edge too until the last tab shows.
  list.scrollLeft = 200;
  fireEvent.scroll(list);
  expect(strip.classList.contains("more-start")).toBe(true);
  expect(screen.getByRole("button", { name: "Earlier layouts" })).toBeTruthy();
  list.scrollLeft = 400;
  fireEvent.scroll(list);
  expect(strip.classList.contains("more-end")).toBe(false);
  // The strip hides its scrollbar in both engines.
  expect(css).toMatch(/\.sheet-tabs \{[^}]*scrollbar-width: none/);
  expect(css).toMatch(/\.sheet-tabs::-webkit-scrollbar \{\s*display: none/);
});

test("in the Arabic window the arrow scrolls the other way", () => {
  box = { client: 300, scroll: 700 };
  mount();
  const list = document.querySelector<HTMLElement>(".sheet-tabs")!;
  list.style.direction = "rtl";
  const scrollBy = vi.fn();
  list.scrollBy = scrollBy;
  fireEvent.click(screen.getByRole("button", { name: "More layouts" }));
  expect(scrollBy).toHaveBeenCalledWith(expect.objectContaining({ left: -240 }));
});

test("a long layout name is in the tab's tooltip and the tab keeps its roles", () => {
  mount();
  const tab = screen.getByRole("tab", { name: "A-501" });
  expect(tab.getAttribute("title")).toBe("A-501");
  expect(screen.getAllByRole("tab")).toHaveLength(7);
});
