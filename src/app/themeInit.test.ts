// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import script from "../../public/theme-init.js?raw";
import { initialTheme, THEME_KEY } from "./shared";

// `public/theme-init.js` runs before the first paint and must choose what `initialTheme` chooses,
// or the window flashes one theme and then switches to the other.

function run(stored: string | null, systemDark: boolean) {
  localStorage.clear();
  if (stored !== null) localStorage.setItem(THEME_KEY, stored);
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: systemDark && query.includes("dark"), media: query }));
  delete document.documentElement.dataset.theme;
  new Function(script)();
  return document.documentElement.dataset.theme;
}

describe("theme-init.js", () => {
  afterEach(() => vi.unstubAllGlobals());

  it.each([
    [null, false],
    [null, true],
    ["light", true],
    ["dark", false],
    ["sepia", true],
    ["sepia", false],
  ])("stored %s, system dark %s: the theme initialTheme gives", (stored, systemDark) => {
    const early = run(stored, systemDark);
    expect(early).toBe(initialTheme());
  });

  it("falls back to the system theme when storage throws", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("dark"), media: query }));
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    delete document.documentElement.dataset.theme;
    new Function(script)();
    expect(document.documentElement.dataset.theme).toBe("dark");
    getItem.mockRestore();
  });
});
