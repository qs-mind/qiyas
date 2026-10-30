// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider, translate } from "../i18n";
import { uiError } from "../i18n/errors";
import { setWorkspace } from "../workspaceIpc";
import * as api from "../api";
import { useShell } from "../app/useShell";
import { dismissNotice, NOTICE_MS, notify, notifyError, notifyUndo, notifyUndoGone, Notices, useUndoLabel } from "./notices";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  localStorage.clear();
  document.documentElement.dir = "ltr";
  document.documentElement.lang = "en";
  setWorkspace(null);
  vi.restoreAllMocks();
});

const advance = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
/** The toasts on screen: their text (the title), not those on their way out. */
const shown = () => [...document.querySelectorAll('[data-sonner-toast]:not([data-removed="true"]) .toast-title')].map((e) => e.textContent);

function mount() {
  return render(
    <LanguageProvider>
      <Notices theme="light" />
    </LanguageProvider>,
  );
}

test("a notice is a toast that goes by itself after a few seconds", async () => {
  mount();
  await advance(50);
  act(() => void notify("Saved to /d/plan.dwg"));
  await advance(500);
  expect(shown()).toEqual(["Saved to /d/plan.dwg"]);
  await advance(NOTICE_MS + 1000);
  expect(shown()).toEqual([]);
});

test("it is at the bottom end: bottom right in English, bottom left in Arabic, with the text direction of the language", async () => {
  const { unmount } = mount();
  await advance(50);
  act(() => void notify("Saved"));
  await advance(300);
  const toaster = () => document.querySelector("[data-sonner-toaster]") as HTMLElement;
  expect(toaster().getAttribute("data-x-position")).toBe("right");
  expect(toaster().getAttribute("data-y-position")).toBe("bottom");
  expect(toaster().getAttribute("dir")).toBe("ltr");
  unmount();
  localStorage.setItem("qiyas.lang", "ar");
  mount();
  await advance(50);
  act(() => void notify("تم الحفظ"));
  await advance(300);
  expect(toaster().getAttribute("data-x-position")).toBe("left");
  expect(toaster().getAttribute("dir")).toBe("rtl");
  // Its label is in the language too.
  expect(document.querySelector("section")!.getAttribute("aria-label")).toContain("الإشعارات");
});

test("an error stays until it is closed or taken away; it has a close button", async () => {
  mount();
  await advance(50);
  let id: string | number = 0;
  act(() => void (id = notifyError("Could not save: disk full")));
  await advance(NOTICE_MS * 4);
  expect(shown()).toEqual(["Could not save: disk full"]);
  fireEvent.click(screen.getByRole("button", { name: /close/i }));
  await advance(800);
  expect(shown()).toEqual([]);
  act(() => void (id = notifyError("Again")));
  await advance(300);
  expect(shown()).toEqual(["Again"]);
  act(() => dismissNotice(id));
  await advance(800);
  expect(shown()).toEqual([]);
});

test("the shell says one notice at a time, null takes it away, and an error goes with the next success", async () => {
  mount();
  const { result } = renderHook(() => useShell());
  await advance(50);
  act(() => result.current.setNotice("First"));
  await advance(400);
  act(() => result.current.setNotice("Second"));
  await advance(800);
  expect(shown()).toEqual(["Second"]);
  act(() => result.current.setNotice(null));
  await advance(800);
  expect(shown()).toEqual([]);

  act(() => result.current.setError("It failed"));
  await advance(NOTICE_MS * 2);
  expect(shown()).toEqual(["It failed"]);
  // A command that works clears it (`run` says null on success).
  await act(async () => result.current.run(async () => {}));
  await advance(800);
  expect(shown()).toEqual([]);
  // The same error again shows again.
  await act(async () => result.current.run(async () => Promise.reject("It failed")));
  await advance(400);
  expect(shown()).toEqual(["That didn’t work. Technical details are available in Settings."]);
});

test("a notice about something still going on stays, with the dots, until the next one takes its place", async () => {
  mount();
  await advance(50);
  act(() => void notify("Plotting…", { waiting: true }));
  await advance(NOTICE_MS * 3);
  expect(shown()).toEqual(["Plotting…"]);
  expect(document.querySelector("[data-sonner-toast] .dot-loader")).toBeTruthy();
});

function UndoButton({ onUndo }: { onUndo(): void }) {
  const label = useUndoLabel();
  return <button onClick={() => notifyUndo({ message: "Purged 14 entries (one undo step)", name: "Undo", label, seconds: 5, onUndo })}>go</button>;
}

test("an undo toast counts down the seconds; the button takes the change back and closes it", async () => {
  mount();
  const onUndo = vi.fn();
  render(
    <LanguageProvider>
      <UndoButton onUndo={onUndo} />
    </LanguageProvider>,
  );
  await advance(50);
  fireEvent.click(screen.getByRole("button", { name: "go" }));
  await advance(300);
  const button = () => screen.getByRole("button", { name: "Undo" });
  expect(button().textContent).toBe("Undo (5 s)");
  // The seconds change inside a live region: the name is fixed and the countdown is hidden from it.
  expect(button().getAttribute("aria-label")).toBe("Undo");
  expect(button().querySelector("span")!.getAttribute("aria-hidden")).toBe("true");
  await advance(1000);
  expect(button().textContent).toBe("Undo (4 s)");
  await advance(2000);
  expect(button().textContent).toBe("Undo (2 s)");
  fireEvent.click(button());
  expect(onUndo).toHaveBeenCalledTimes(1);
  await advance(3000);
  expect(shown()).toEqual([]);
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

test("an undo toast goes when the time is up, and when something else is done first", async () => {
  mount();
  const onUndo = vi.fn();
  const open = () => notifyUndo({ message: "Purged", name: "Undo", label: (n) => `Undo (${n} s)`, seconds: 3, onUndo });
  await advance(50);
  act(() => void open());
  await advance(300);
  expect(screen.getByRole("button", { name: "Undo" })).toBeTruthy();
  await advance(3500);
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  expect(onUndo).not.toHaveBeenCalled();

  // Another command: the toast's undo would be of that command, so it is gone.
  act(() => void open());
  await advance(300);
  act(() => notifyUndoGone());
  await advance(3000);
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

test("every kind uses our card and close button without Sonner icons or close glyphs", async () => {
  mount();
  await advance(50);
  act(() => {
    notify("Plain");
    notify("Waiting", { waiting: true });
    notifyError("Error");
    notifyUndo({ message: "Undoable", name: "Undo", label: String, seconds: 5, onUndo: vi.fn() });
  });
  await advance(300);
  expect([...document.querySelectorAll("[data-notice-kind]")].map(e => e.getAttribute("data-notice-kind")).sort()).toEqual(["error", "plain", "undo", "waiting"]);
  expect(document.querySelectorAll("[data-icon], [data-close-button]")).toHaveLength(0);
  expect(document.querySelectorAll(".toast .ib.toast-close")).toHaveLength(4);
});

test.each(["en", "ar"] as const)("busy edits show one %s notice and Stop keeps the original drawing", async lang => {
  localStorage.setItem("qiyas.lang", lang);
  mount();
  const stop = vi.spyOn(api, "aiCancel").mockResolvedValue();
  const { result } = renderHook(useShell);
  await advance(50);
  setWorkspace(7);
  for (const code of ["workspace_busy", "mizan_working", "workspace_busy"]) {
    act(() => result.current.setError(uiError(code, lang)));
    await advance(100);
  }
  expect(shown()).toEqual([translate(lang, "workspace.busy")]);
  expect(document.querySelectorAll("[data-icon], [data-close-button]")).toHaveLength(0);
  setWorkspace(8);
  fireEvent.click(screen.getByRole("button", { name: translate(lang, "ai.stop") }));
  await advance(800);
  expect(stop).toHaveBeenCalledExactlyOnceWith(7);
  expect(shown()).toEqual([]);
});
