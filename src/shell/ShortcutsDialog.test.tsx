// @vitest-environment jsdom
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { LanguageProvider } from "../i18n";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";
import { BUILT_IN_SHORTCUTS } from "../tools/shortcuts";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { SHORTCUT_SECTIONS } from "./shortcutList";

afterEach(() => {
  cleanup();
  localStorage.removeItem("qiyas.lang");
});

test("the cheat sheet lists every built-in key, so the list and the keyboard handler cannot drift apart", () => {
  const listed = new Set(SHORTCUT_SECTIONS.flatMap((s) => s.rows.flatMap((r) => r.keys)));
  const missing = BUILT_IN_SHORTCUTS.map(([, key]) => key).filter((key) => !listed.has(key));
  expect(missing).toEqual([]);
});

test("every row and section has words in English and in Arabic", () => {
  const keys = [
    ...SHORTCUT_SECTIONS.map((s) => `shortcuts.s.${s.id}`),
    ...SHORTCUT_SECTIONS.flatMap((s) => s.rows.map((r) => `shortcuts.r.${r.id}`)),
    ...SHORTCUT_SECTIONS.flatMap((s) => s.rows.flatMap((r) => r.keys.filter((k) => k.startsWith("@")).map((k) => k.slice(1)))),
  ];
  for (const key of keys) {
    expect((en as Record<string, string>)[key], key).toBeTruthy();
    expect((ar as Record<string, string>)[key], key).toBeTruthy();
  }
});

test("it shows keys as people write them: Slash is /, and mouse gestures are words", () => {
  render(<LanguageProvider><ShortcutsDialog onClose={() => {}} /></LanguageProvider>);
  const mizan = within(screen.getByRole("region", { name: en["shortcuts.s.mizan"] }));
  expect(mizan.getByText("/")).toBeTruthy();
  expect(mizan.getByText("Ctrl+K")).toBeTruthy();
  expect(mizan.getByText("Ctrl+/")).toBeTruthy();
  expect(screen.getByText(en["shortcuts.k.doubleMiddle"])).toBeTruthy();
  expect(screen.getByRole("heading", { name: en["shortcuts.title"] })).toBeTruthy();
});

test("in the Arabic window it speaks Arabic and keeps the keys left to right", () => {
  localStorage.setItem("qiyas.lang", "ar");
  render(<LanguageProvider><ShortcutsDialog onClose={() => {}} /></LanguageProvider>);
  expect(screen.getByRole("heading", { name: ar["shortcuts.title"] })).toBeTruthy();
  expect(document.querySelector("kbd")?.getAttribute("dir")).toBe("ltr");
});
