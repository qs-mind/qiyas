// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import * as api from "../api";
import { useSheets } from "./useSheets";
import { Ranges, type Batch } from "../viewport/batch";

vi.mock("../viewport/batch", async (original) => ({
  ...await original<typeof import("../viewport/batch")>(),
  parseBatch: (value: unknown) => value,
}));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function drawing(content: boolean): Batch {
  const bytes = new ArrayBuffer(content ? 32 : 0);
  const wide = new BigUint64Array(bytes);
  const narrow = new Uint32Array(bytes);
  if (content) { wide[0] = 7n; wide[1] = 7n; narrow[6] = 1; }
  return {
    ranges: new Ranges(wide, narrow), atlasVersion: 0, origin: [0, 0],
    bounds: content ? [[1000, 2000], [5000, 6000]] : [[0, 0], [0, 0]],
    segments: new Float32Array(content ? [1000, 2000, 5000, 6000] : []),
  } as unknown as Batch;
}

test("first content fits once; later edits preserve the engineer's zoom", async () => {
  let next = drawing(false);
  let version = 1;
  vi.spyOn(api, "batchVersion").mockImplementation(async () => version++);
  vi.spyOn(api, "renderBatch").mockImplementation(async () => next as unknown as ArrayBuffer);
  vi.spyOn(api, "paperViews").mockResolvedValue([]);
  const fitExtents = vi.fn();
  const fitTo = vi.fn();
  const hook = renderHook(() => useSheets({
    switchGen: { current: 0 }, viewport: { current: { fitExtents, fitTo, pxPerUnit: () => 1 } },
    run: (step: () => Promise<void>) => step(), autosave: async () => {},
    setInfo() {}, refreshTabs: async () => {}, items: [], measure: { current: null },
  } as unknown as Parameters<typeof useSheets>[0]));
  await act(async () => hook.result.current.redraw(true));
  fitExtents.mockClear();
  next = drawing(true);
  await act(async () => hook.result.current.redraw());
  expect(fitExtents).toHaveBeenCalledTimes(1);
  next = drawing(true);
  await act(async () => hook.result.current.redraw());
  expect(fitExtents).toHaveBeenCalledTimes(1);
});

test("a reveal frames new content on this sheet before showing it", async () => {
  const next = drawing(true);
  vi.spyOn(api, "batchVersion").mockResolvedValue(1);
  vi.spyOn(api, "renderBatch").mockResolvedValue(next as unknown as ArrayBuffer);
  vi.spyOn(api, "paperViews").mockResolvedValue([]);
  vi.spyOn(window, "matchMedia").mockReturnValue({ matches: true, addEventListener() {}, removeEventListener() {} } as unknown as MediaQueryList);
  const fitTo = vi.fn();
  const hook = renderHook(() => useSheets({
    switchGen: { current: 0 }, viewport: { current: { fitExtents() {}, fitTo, pxPerUnit: () => 1 } },
    run: (step: () => Promise<void>) => step(), autosave: async () => {},
    setInfo() {}, refreshTabs: async () => {}, items: [], measure: { current: null },
  } as unknown as Parameters<typeof useSheets>[0]));
  await act(async () => { hook.result.current.queueReveal([7]); });
  expect(fitTo).toHaveBeenCalledWith([[1000, 2000], [5000, 6000]]);
});
