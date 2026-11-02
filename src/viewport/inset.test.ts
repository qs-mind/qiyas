import { expect, test } from "vitest";
import type { PaperView } from "../api";
import { worldToScreen, type Camera } from "./camera";
import type { Batch } from "./batch";
import { refreshSheet, scissorBox, viewCamera } from "./inset";

const view: PaperView = {
  handle: 7,
  rect: [
    [10, 10],
    [110, 90],
  ],
  centre: [500, 300],
  scale: 0.1,
  lines: null,
  fills: null,
  texts: null,
};
/** A layout 200 units across on a 400 x 300 canvas. */
const paper: Camera = { cx: 100, cy: 75, scale: 0.5, width: 400, height: 300 };

test("model space sits in the frame: its centre on the frame's, scaled", () => {
  const model = viewCamera(paper, view);
  const at = worldToScreen(model, [500, 300]);
  const frame = worldToScreen(paper, [60, 50]);
  expect(at[0]).toBeCloseTo(frame[0], 9);
  expect(at[1]).toBeCloseTo(frame[1], 9);
  // 10 model units are 1 layout unit.
  const right = worldToScreen(model, [510, 300]);
  const oneOver = worldToScreen(paper, [61, 50]);
  expect(right[0]).toBeCloseTo(oneOver[0], 9);
});

test("the frame clips in device pixels from the bottom, and off screen it is null", () => {
  // Device pixels at a ratio of 2.
  expect(scissorBox(paper, view, 800, 600)).toEqual([40, 40, 400, 320]);
  expect(scissorBox({ ...paper, cx: 10_000 }, view, 800, 600)).toBeNull();
});

test("an off-screen sheet is fetched again only when the core's changed, or for its glyphs alone", async () => {
  const batch = (tag: number) =>
    ({ segments: new Float32Array([tag]), colors: new Uint32Array(), quads: { length: tag } }) as unknown as Batch;
  const first = batch(1);
  let version = 7;
  const fetched: (number | undefined)[] = [];
  const api = {
    version: async () => version,
    fetch: async (px?: number) => {
      fetched.push(px);
      return batch(2);
    },
  };
  const held = { version: 7, batch: first };
  // Unchanged: nothing fetched.
  expect(await refreshSheet(held, undefined, api)).toBe(held);
  expect(fetched).toEqual([]);
  // New glyphs: the lines stay the very arrays held.
  const glyphs = (await refreshSheet(held, 3, api))!;
  expect(fetched).toEqual([3]);
  expect(glyphs.batch.segments).toBe(first.segments);
  expect(glyphs.batch.quads.length).toBe(2);
  // A drawing change: the whole new batch.
  version = 8;
  const changed = (await refreshSheet(held, undefined, api))!;
  expect(changed.version).toBe(8);
  expect(changed.batch.segments).not.toBe(first.segments);
});

test("tile camera subtraction preserves fractional survey coordinates", async () => {
  const { tileCamera } = await import("./inset");
  const camera = { cx: 500_000.125, cy: 3_300_000_000.25, scale: 0.01, width: 800, height: 600 };
  const out = new Float32Array(2);
  tileCamera(camera, [500_000, 3_300_000_000], out);
  expect(Array.from(out)).toEqual([0.125, 0.25]);
  camera.cy += 0.125;
  tileCamera(camera, [500_000, 3_300_000_000], out);
  expect(out[1]).toBe(0.375);
});

test("glyph refresh retains origin spans with the held line and fill buffers", async () => {
  const { keepLines } = await import("./inset");
  const segmentTiles = [{ first: 0, count: 1, origin: [500_000, 3_300_000_000] }];
  const quadTiles = [{ first: 0, count: 2, origin: [500_001, 3_300_000_001] }];
  const held = { segments: new Float32Array(4), fills: {}, segmentTiles, fillTiles: segmentTiles } as unknown as Batch;
  const next = { segments: new Float32Array(4), fills: {}, quadTiles } as unknown as Batch;
  const refreshed = keepLines(held, next);
  expect(refreshed.segments).toBe(held.segments);
  expect(refreshed.segmentTiles).toBe(held.segmentTiles);
  expect(refreshed.fillTiles).toBe(held.fillTiles);
  expect(refreshed.quadTiles).toBe(next.quadTiles);
});
