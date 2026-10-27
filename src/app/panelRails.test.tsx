// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";
import { LanguageProvider } from "../i18n";
import { PANEL_WIDTH, PanelHandle, usePanelRails } from "./panelRails";

const KEY = "qiyas.panels.hidden";

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.restoreAllMocks();
});

test("both panels start shown", () => {
  const { result } = renderHook(() => usePanelRails());
  expect(result.current.hidden).toEqual({ left: false, right: false });
});

test("toggle hides a panel, then shows it again, one side at a time", () => {
  const { result } = renderHook(() => usePanelRails());
  act(() => result.current.toggle("left"));
  expect(result.current.hidden).toEqual({ left: true, right: false });
  act(() => result.current.toggle("right"));
  expect(result.current.hidden).toEqual({ left: true, right: true });
  act(() => result.current.toggle("left"));
  expect(result.current.hidden).toEqual({ left: false, right: true });
});

test("the choice is kept on this computer and read back at the next start", () => {
  const first = renderHook(() => usePanelRails());
  act(() => first.result.current.toggle("right"));
  expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({ left: false, right: true });
  first.unmount();
  const second = renderHook(() => usePanelRails());
  expect(second.result.current.hidden).toEqual({ left: false, right: true });
});

test("a stored value that is not JSON, not an object or not true/false shows both panels", () => {
  for (const bad of ["{not json", "null", "42", '"left"', '{"left":"yes","right":1}']) {
    localStorage.setItem(KEY, bad);
    const { result, unmount } = renderHook(() => usePanelRails());
    expect(result.current.hidden, bad).toEqual({ left: false, right: false });
    unmount();
  }
});

test("storage that throws still folds the panel for this session", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  const { result } = renderHook(() => usePanelRails());
  expect(result.current.hidden).toEqual({ left: false, right: false });
  act(() => result.current.toggle("left"));
  expect(result.current.hidden).toEqual({ left: true, right: false });
});

const handle = (over: { side?: "left" | "right"; width?: number } = {}) => {
  const props = {
    side: "left" as "left" | "right",
    hidden: false,
    width: 220,
    onToggle: vi.fn(),
    onFold: vi.fn(),
    onResize: vi.fn(),
    ...over,
  };
  return { props, ...render(<PanelHandle {...props} />) };
};

test("the handle's round grip folds and unfolds; its label says hide while shown and show while hidden", () => {
  const { props, rerender } = handle();
  const grip = screen.getByRole("button", { name: en["panels.hide.left"] });
  expect(grip.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(grip);
  expect(props.onToggle).toHaveBeenCalledTimes(1);
  rerender(<PanelHandle {...props} hidden />);
  expect(screen.getByRole("button", { name: en["panels.show.left"] }).getAttribute("aria-expanded")).toBe("false");
});

test("the handle is a separator: double-click folds, Enter folds, arrows resize", () => {
  const { props } = handle();
  const sep = screen.getByRole("separator", { name: en["panels.resize.left"] });
  expect(sep.getAttribute("aria-valuenow")).toBe("220");
  fireEvent.doubleClick(sep);
  expect(props.onToggle).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(sep, { key: "Enter" });
  expect(props.onToggle).toHaveBeenCalledTimes(2);
  // The left panel grows towards the drawing (to the right in English).
  fireEvent.keyDown(sep, { key: "ArrowRight" });
  expect(props.onResize).toHaveBeenLastCalledWith(236);
  fireEvent.keyDown(sep, { key: "ArrowLeft" });
  expect(props.onResize).toHaveBeenLastCalledWith(204);
});

test("the right panel grows when its handle is dragged towards the start", () => {
  const { props } = handle({ side: "right", width: 340 });
  const sep = screen.getByRole("separator");
  fireEvent.pointerDown(sep, { button: 0, clientX: 800, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 700, pointerId: 1 });
  expect(props.onFold).toHaveBeenLastCalledWith(false);
  expect(props.onResize).toHaveBeenLastCalledWith(440);
  fireEvent.pointerUp(sep, { pointerId: 1 });
  // Moving after the drag ended changes nothing.
  props.onResize.mockClear();
  fireEvent.pointerMove(sep, { clientX: 600, pointerId: 1 });
  expect(props.onResize).not.toHaveBeenCalled();
});

test("dragging a panel nearly shut folds it; dragging a folded one open unfolds it", () => {
  const { props, rerender } = handle();
  const sep = screen.getByRole("separator");
  fireEvent.pointerDown(sep, { button: 0, clientX: 220, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 40, pointerId: 1 });
  expect(props.onFold).toHaveBeenLastCalledWith(true);
  fireEvent.pointerUp(sep, { pointerId: 1 });

  rerender(<PanelHandle {...props} hidden />);
  fireEvent.pointerDown(sep, { button: 0, clientX: 10, pointerId: 2 });
  fireEvent.pointerMove(sep, { clientX: 250, pointerId: 2 });
  expect(props.onFold).toHaveBeenLastCalledWith(false);
  expect(props.onResize).toHaveBeenLastCalledWith(240);
});

test("widths stay between each panel's narrowest and widest, and are kept on this computer", () => {
  const first = renderHook(() => usePanelRails());
  expect(first.result.current.width).toEqual({ left: 220, right: 440, rightWide: 672 });
  act(() => first.result.current.resize("left", 10));
  act(() => first.result.current.resize("right", 5000));
  expect(first.result.current.width).toMatchObject({ left: PANEL_WIDTH.left.min, right: PANEL_WIDTH.right.max });
  act(() => first.result.current.resize("left", 300.4));
  first.unmount();
  const second = renderHook(() => usePanelRails());
  expect(second.result.current.width).toEqual({ left: 300, right: PANEL_WIDTH.right.max, rightWide: 672 });
  localStorage.setItem("qiyas.panels.width", '{"left":"wide","right":null}');
  const third = renderHook(() => usePanelRails());
  expect(third.result.current.width).toEqual({ left: 220, right: 440, rightWide: 672 });
});

test("each side has its own labels, in English and in Arabic", () => {
  for (const [lang, strings] of [
    ["en", en],
    ["ar", ar],
  ] as const) {
    localStorage.setItem("qiyas.lang", lang);
    for (const side of ["left", "right"] as const) {
      for (const hidden of [false, true]) {
        const name = strings[`panels.${hidden ? "show" : "hide"}.${side}` as keyof typeof strings];
        const { unmount } = render(
          <LanguageProvider>
            <PanelHandle side={side} hidden={hidden} width={300} onToggle={() => {}} onFold={() => {}} onResize={() => {}} />
          </LanguageProvider>,
        );
        expect(screen.getByRole("button", { name })).toBeTruthy();
        expect(screen.getByRole("separator", { name: strings[`panels.resize.${side}` as keyof typeof strings] })).toBeTruthy();
        unmount();
      }
    }
  }
  const labels = (s: typeof en) =>
    ["hide.left", "show.left", "hide.right", "show.right", "resize.left", "resize.right"].map((k) => s[`panels.${k}` as keyof typeof s]);
  expect(new Set(labels(ar)).size).toBe(6);
  expect(labels(ar)).not.toEqual(labels(en));
});

test("folded, only the arrow that widens opens the panel, in English and in Arabic", () => {
  for (const [dir, widen, narrow] of [
    ["ltr", "ArrowRight", "ArrowLeft"],
    ["rtl", "ArrowLeft", "ArrowRight"],
  ] as const) {
    document.documentElement.dir = dir;
    const onFold = vi.fn();
    const onResize = vi.fn();
    const { unmount } = render(
      <PanelHandle side="left" hidden width={300} onToggle={() => {}} onFold={onFold} onResize={onResize} />,
    );
    const sep = screen.getByRole("separator");
    fireEvent.keyDown(sep, { key: narrow });
    expect(onFold, dir).not.toHaveBeenCalled();
    fireEvent.keyDown(sep, { key: widen });
    expect(onFold, dir).toHaveBeenCalledWith(false);
    expect(onResize, dir).toHaveBeenCalledWith(PANEL_WIDTH.left.min);
    unmount();
  }
  document.documentElement.dir = "ltr";
});

test("in Arabic the left panel is on the right: dragging its handle left widens it", () => {
  document.documentElement.dir = "rtl";
  const { props } = handle();
  const sep = screen.getByRole("separator");
  fireEvent.pointerDown(sep, { button: 0, clientX: 900, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 850, pointerId: 1 });
  expect(props.onResize).toHaveBeenLastCalledWith(270);
  document.documentElement.dir = "ltr";
});

test("a click on a folded panel's strip opens it; a drag does not count as a click", () => {
  const onToggle = vi.fn();
  render(<PanelHandle side="left" hidden width={220} onToggle={onToggle} onFold={() => {}} onResize={() => {}} />);
  const sep = screen.getByRole("separator");
  fireEvent.pointerDown(sep, { button: 0, clientX: 5, pointerId: 1 });
  fireEvent.pointerUp(sep, { clientX: 5, pointerId: 1 });
  expect(onToggle).toHaveBeenCalledTimes(1);
  fireEvent.pointerDown(sep, { button: 0, clientX: 5, pointerId: 2 });
  fireEvent.pointerMove(sep, { clientX: 300, pointerId: 2 });
  fireEvent.pointerUp(sep, { clientX: 300, pointerId: 2 });
  expect(onToggle).toHaveBeenCalledTimes(1);
});

test("the wide tabs keep their own width with the same handle", () => {
  const { result } = renderHook(() => usePanelRails());
  act(() => result.current.resize("rightWide", 700));
  expect(result.current.width).toEqual({ left: 220, right: 440, rightWide: 700 });
  const onResize = vi.fn();
  render(<PanelHandle side="right" slot="rightWide" hidden={false} width={700} onToggle={() => {}} onFold={() => {}} onResize={onResize} />);
  const sep = screen.getByRole("separator");
  expect(sep.getAttribute("aria-valuenow")).toBe("700");
  expect(sep.getAttribute("aria-valuemax")).toBe(String(PANEL_WIDTH.rightWide.max));
  fireEvent.pointerDown(sep, { button: 0, clientX: 500, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 450, pointerId: 1 });
  expect(onResize).toHaveBeenLastCalledWith(750);
});
