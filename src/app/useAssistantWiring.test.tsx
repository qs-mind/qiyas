// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { AiUi } from "../api";
import * as api from "../api";
import { useAssistantWiring } from "./useAssistantWiring";
import { notifyError, notifyUndoGone } from "../ui/notices";

vi.mock("../ui/notices", () => ({ notifyError: vi.fn(), notifyUndoGone: vi.fn() }));

const callbacks = vi.hoisted(() => ({
  onUi: null as null | ((ui: AiUi) => void),
  onMutated: null as null | ((label: string) => Promise<void>),
}));
vi.mock("../ai/useAssistant", () => ({
  useAssistant: (options: { onUi(ui: AiUi): void; onMutated(label: string): Promise<void> }) => {
    callbacks.onUi = options.onUi;
    callbacks.onMutated = options.onMutated;
    return { markUndone() {}, markUndoStale() {}, send: async () => true };
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

afterEach(() => { cleanup(); vi.clearAllMocks(); });

function setup() {
  const order: string[] = [];
  const fitTo = vi.fn(() => { order.push("zoom"); });
  const redraw = vi.fn(async () => { order.push("redraw"); });
  const setSheetBlock = vi.fn(() => { order.push("sheet"); });
  const setHidden = vi.fn();
  const cancel = vi.fn();
  const setAiHighlight = vi.fn();
  const chooseSheet = vi.fn(async () => { order.push("choose"); });
  const cancelReveal = vi.fn();
  const queueReveal = vi.fn();
  const deps = {
    t: (key: string) => key,
    lang: "en",
    switchGen: { current: 0 },
    info: { path: "/d/plan.dwg", units: "millimetres", layers: [{ name: "A-WALL" }] },
    run: (step: () => Promise<void>) => step(),
    viewport: { current: { fitTo } },
    batch: null,
    sheet: "*Model_Space",
    queueReveal,
    chooseSheet,
    cancelReveal,
    setSheetBlock,
    setHidden,
    redraw,
    refreshTakeoffAndBoq: async () => {},
    syncInfo: async () => {},
    autosave: async () => {},
    setAiHighlight,
    measure: { cancel, picks: [], highlighted: null, kinds: new Map() },
    tool: "select",
  } as unknown as Parameters<typeof useAssistantWiring>[0];
  const hook = renderHook(() => useAssistantWiring(deps));
  return { order, fitTo, redraw, setSheetBlock, setHidden, cancel, setAiHighlight, cancelReveal, queueReveal, chooseSheet, hook };
}

test("Mizan view events preserve the user's sheet and zoom", async () => {
  const view = setup();
  await act(async () => callbacks.onUi!({ action: "zoom", sheet: "*Paper_Space", bounds: [[1, 2], [3, 4]], handles: [7] }));
  expect(view.fitTo).not.toHaveBeenCalled();
  expect(view.setSheetBlock).not.toHaveBeenCalled();
  expect(view.cancel).not.toHaveBeenCalled();
  expect(view.redraw).toHaveBeenCalledWith();
});

test("sheet and redraw events refresh without fitting the user's view", async () => {
  const view = setup();
  await act(async () => callbacks.onUi!({ action: "show_sheet", sheet: "*Paper_Space", hidden: ["a-wall"] }));
  expect(view.setHidden).toHaveBeenCalledWith(new Set(["A-WALL"]));
  await act(async () => callbacks.onUi!({ action: "redraw", hidden: [] }));
  expect(view.setHidden).toHaveBeenLastCalledWith(new Set());
  expect(view.redraw).toHaveBeenCalledTimes(2);
  expect(view.fitTo).not.toHaveBeenCalled();
  expect(view.setSheetBlock).not.toHaveBeenCalled();
});

test("reveal runs only on the working sheet, without issuing sheet IPC", async () => {
  const select = vi.spyOn(api, "setSheet").mockResolvedValue();
  const view = setup();
  await act(async () => callbacks.onUi!({ action: "reveal", sheet: "*Paper_Space", handles: [1] }));
  expect(view.queueReveal).not.toHaveBeenCalled();
  expect(view.cancelReveal).toHaveBeenCalledWith(false);
  await act(async () => callbacks.onUi!({ action: "reveal", sheet: "*Model_Space", handles: [2] }));
  expect(view.queueReveal).toHaveBeenCalledWith([2]);
  expect(select).not.toHaveBeenCalled();
  expect(view.setSheetBlock).not.toHaveBeenCalled();
  select.mockRestore();
});

test("an explicit Show action selects the evidence sheet before fitting it", async () => {
  const view = setup();
  const bounds: [[number, number], [number, number]] = [[1, 2], [3, 4]];
  await act(async () => view.hook.result.current.showCard({
    kind: "tool", id: "1", name: "draw", input: {}, status: "done", summary: "", handles: [7],
    ui: { action: "zoom", sheet: "*Paper_Space", bounds }, mutating: true, undone: false, stale: false,
  }));
  expect(view.chooseSheet).toHaveBeenCalledWith("*Paper_Space");
  expect(view.setAiHighlight).toHaveBeenCalledWith([7]);
  expect(view.order).toEqual(["choose", "zoom"]);
});

test("automatic drawing refresh skips a busy session and retries on the next event", async () => {
  const view = setup();
  view.redraw.mockRejectedValueOnce("workspace_busy");
  await act(async () => callbacks.onUi!({ action: "redraw" }));
  expect(notifyError).not.toHaveBeenCalled();
  await act(async () => callbacks.onUi!({ action: "redraw" }));
  expect(view.redraw).toHaveBeenCalledTimes(2);
  expect(view.order).toEqual(["redraw"]);
  expect(notifyUndoGone).not.toHaveBeenCalled();
});

test("automatic refresh still reports real failures", async () => {
  const view = setup();
  view.redraw.mockRejectedValueOnce("broken drawing data");
  await act(async () => callbacks.onUi!({ action: "redraw" }));
  expect(notifyError).toHaveBeenCalledOnce();
});

test("a real Mizan mutation still invalidates the previous Undo offer even if refresh is busy", async () => {
  const view = setup();
  view.redraw.mockRejectedValueOnce("workspace_busy");
  await act(async () => callbacks.onMutated!("draw"));
  expect(notifyUndoGone).toHaveBeenCalledOnce();
  expect(notifyError).not.toHaveBeenCalled();
});
