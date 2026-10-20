// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { AiUi } from "../api";
import { useAssistantWiring } from "./useAssistantWiring";

const callbacks = vi.hoisted(() => ({ onUi: null as null | ((ui: AiUi) => void) }));
vi.mock("../ai/useAssistant", () => ({
  useAssistant: (options: { onUi(ui: AiUi): void }) => {
    callbacks.onUi = options.onUi;
    return { markUndone() {}, markUndoStale() {}, send: async () => true };
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

afterEach(cleanup);

function setup() {
  const order: string[] = [];
  const fitTo = vi.fn(() => { order.push("zoom"); });
  const redraw = vi.fn(async () => { order.push("redraw"); });
  const setSheetBlock = vi.fn(() => { order.push("sheet"); });
  const setHidden = vi.fn();
  const cancel = vi.fn();
  const setAiHighlight = vi.fn();
  const deps = {
    t: (key: string) => key,
    lang: "en",
    switchGen: { current: 0 },
    info: { path: "/d/plan.dwg", units: "millimetres", layers: [{ name: "A-WALL" }] },
    run: (step: () => Promise<void>) => step(),
    viewport: { current: { fitTo } },
    batch: null,
    setSheetBlock,
    setHidden,
    redraw,
    setAiHighlight,
    measure: { cancel, picks: [], highlighted: null, kinds: new Map() },
    tool: "select",
  } as unknown as Parameters<typeof useAssistantWiring>[0];
  renderHook(() => useAssistantWiring(deps));
  return { order, fitTo, redraw, setSheetBlock, setHidden, cancel, setAiHighlight };
}

test("a view result changes the sheet before zooming to its bounds", async () => {
  const view = setup();
  const bounds: [[number, number], [number, number]] = [[1, 2], [3, 4]];
  await act(async () => callbacks.onUi!({
    action: "zoom", sheet: "*Paper_Space", bounds, handles: [7],
  }));
  await waitFor(() => expect(view.fitTo).toHaveBeenCalledWith(bounds));
  expect(view.order).toEqual(["sheet", "redraw", "zoom"]);
  expect(view.setSheetBlock).toHaveBeenCalledWith("*Paper_Space");
  expect(view.cancel).toHaveBeenCalledTimes(1);
  expect(view.redraw).toHaveBeenCalledWith(false);
  expect(view.setAiHighlight).toHaveBeenCalledWith([7]);
});

test("sheet and redraw results keep hidden layer names; a sheet without bounds fits", async () => {
  const view = setup();
  await act(async () => callbacks.onUi!({
    action: "show_sheet", sheet: "*Paper_Space", hidden: ["a-wall"],
  }));
  expect(view.redraw).toHaveBeenCalledWith(true);
  expect(view.fitTo).not.toHaveBeenCalled();
  expect(view.setHidden).toHaveBeenCalledWith(new Set(["A-WALL"]));
  await act(async () => callbacks.onUi!({ action: "redraw", hidden: [] }));
  expect(view.setHidden).toHaveBeenLastCalledWith(new Set());
  expect(view.redraw).toHaveBeenCalledTimes(2);
});
