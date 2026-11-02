import { expect, test, vi } from "vitest";
import { createRenderer, lineColors } from "./gl";
import { Fills, ImageQuads, NO_CHAIN, Quads, Ranges, TEXT_BOX, type Batch } from "./batch";
import { palette } from "./colours";

function surveyBatch(): Batch {
  const records = new ArrayBuffer(64);
  const wide = new BigUint64Array(records), narrow = new Uint32Array(records);
  for (let i = 0; i < 2; i++) {
    wide[4 * i] = wide[4 * i + 1] = BigInt(i + 1);
    narrow[8 * i + 4] = NO_CHAIN;
    narrow[8 * i + 5] = i;
    narrow[8 * i + 6] = 1;
  }
  const empty = new Ranges(new BigUint64Array(), new Uint32Array());
  return {
    segments: new Float32Array([0, 0, 0.125, 0.25, 0, 0, 1, 1]),
    colors: new Uint32Array([0x123456, 0xffffff]), ranges: new Ranges(wide, narrow), chains: [],
    bounds: [[0, 0], [500_001, 3_300_000_001]], origin: [250_000, 1_650_000_000],
    segmentTiles: [
      { first: 0, count: 1, origin: [500_000, 3_300_000_000] },
      { first: 1, count: 1, origin: [0, 0] },
    ],
    quads: new Quads(new Float32Array(), new Uint16Array(), new Uint32Array()), textRanges: empty,
    atlasVersion: 0, atlasPages: 0, pxPerUnit: 1,
    images: new ImageQuads(new Float32Array(), new Uint32Array()), imageTable: [],
    fills: new Fills(new Float32Array(), new Uint32Array()), fillRanges: empty,
  };
}

function context() {
  const draws: { mode: number; first: number; count: number; center: number[] }[] = [];
  let center: number[] = [], id = 0;
  const api: Record<string, unknown> = {
    getShaderParameter: () => true, getProgramParameter: () => true,
    getAttribLocation: () => 0, getUniformLocation: (_: unknown, name: string) => name,
    uniform2fv: (_: unknown, value: Float32Array) => { center = Array.from(value); },
    drawArrays: (mode: number, first: number, count: number) => draws.push({ mode, first, count, center }),
    drawElements: (mode: number, count: number, _: number, first: number) => draws.push({ mode, first, count, center }),
    bufferData: vi.fn(),
  };
  const gl = new Proxy(api, {
    get(target, key: string) {
      if (!(key in target)) target[key] = key.toUpperCase() === key ? ++id : key.startsWith("create") ? () => ({ id: ++id }) : () => {};
      return target[key];
    },
  }) as unknown as WebGL2RenderingContext;
  const canvas = { width: 800, height: 600, getContext: () => gl } as unknown as HTMLCanvasElement;
  return { renderer: createRenderer(canvas), draws, gl, uploads: api.bufferData as ReturnType<typeof vi.fn> };
}
const camera = { cx: 500_000.125, cy: 3_300_000_000.25, scale: 0.01, width: 800, height: 600 };

test("main lines and highlights use their tile's camera", () => {
  const { renderer, draws, gl } = context();
  renderer.setBatch(surveyBatch());
  renderer.setHighlight([{ handle: 1, chain: [] }]);
  renderer.draw(camera, palette("light"));
  const lines = draws.filter(d => d.mode === gl.LINES);
  expect(lines[0]).toMatchObject({ first: 0, count: 2, center: [0.125, 0.25] });
  expect(lines[1]).toMatchObject({ first: 2, count: 2, center: [Math.fround(camera.cx), Math.fround(camera.cy)] });
  expect(lines).toHaveLength(5);
  expect(lines.slice(2).every(d => d.center[0] === 0.125 && d.center[1] === 0.25)).toBe(true);
});

test("overlay buffers are converted and uploaded only on change, including after batch replacement", () => {
  const { renderer, draws, uploads } = context();
  renderer.setBatch(surveyBatch());
  renderer.setOverlay(new Float64Array([500_000, 3_300_000_000, 500_000.125, 3_300_000_000.25]));
  const upload = uploads.mock.calls.at(-1)!;
  expect(Array.from(upload[1])).toEqual([0, 0, 0.125, 0.25]);
  uploads.mockClear();
  const arrays = vi.spyOn(globalThis, "Float32Array");
  try {
    renderer.draw(camera, palette("light"));
    renderer.draw({ ...camera, cy: camera.cy + 0.125 }, palette("light"));
    expect(arrays).not.toHaveBeenCalled();
    expect(uploads).not.toHaveBeenCalled();
  } finally { arrays.mockRestore(); }
  expect(draws.at(-1)?.center).toEqual([0.125, 0.375]);
  renderer.setBatch({ ...surveyBatch(), origin: [0, 0] });
  renderer.draw(camera, palette("light"));
  expect(draws.at(-1)?.center).toEqual([0.125, 0.25]);
  renderer.setOverlay(null);
  draws.length = 0;
  renderer.draw(camera, palette("light"));
  expect(draws).toHaveLength(2);
});

test("an inset intersects frozen-layer spans with tiles before drawing", () => {
  const { renderer, draws } = context();
  renderer.setBatch(surveyBatch());
  renderer.setInset(surveyBatch(), [{
    handle: 7, rect: [[0, 0], [800, 600]], centre: [500_000.125, 3_300_000_000.25], scale: 1,
    lines: [[0, 1]], fills: [], texts: [],
  }]);
  renderer.draw({ cx: 400, cy: 300, width: 800, height: 600, scale: 1 }, palette("light"));
  expect(draws[0]).toMatchObject({ first: 0, count: 2, center: [0.125, 0.25] });
});

test("line colours preserve both endpoints and hide text boxes", () => {
  const b = surveyBatch();
  const records = new ArrayBuffer(32);
  const narrow = new Uint32Array(records);
  narrow[6] = 1; narrow[7] = TEXT_BOX;
  b.ranges = new Ranges(new BigUint64Array(records), narrow);
  expect(Array.from(lineColors(b))).toEqual([0x12, 0x34, 0x56, 0, 0x12, 0x34, 0x56, 0, 255, 255, 255, 255, 255, 255, 255, 255]);
});

test("fills, wipeouts, glyph pages and glyph highlights keep their origins", () => {
  const b = surveyBatch();
  const fill = new ArrayBuffer(56), f32 = new Float32Array(fill), u32 = new Uint32Array(fill);
  f32.set([0, 0, 1, 0, 0, 1]); f32.set([0, 0, 1, 0, 0, 1], 7);
  u32[6] = 0xffffff; u32[13] = 0x01000000;
  b.fills = new Fills(f32, u32); b.fillTiles = b.segmentTiles;
  const glyphs = new ArrayBuffer(96);
  const q32 = new Float32Array(glyphs), q16 = new Uint16Array(glyphs), qc = new Uint32Array(glyphs);
  for (let i = 0; i < 2; i++) {
    q32.set([0, 1, 1, 1, 1, 0, 0, 0], 12 * i);
    q16.set([0, 0, 1, 1], 24 * i + 16); qc[12 * i + 10] = 0xffffff;
  }
  b.quads = new Quads(q32, q16, qc); b.quadTiles = b.segmentTiles; b.textRanges = b.ranges;
  const { renderer, draws, gl, uploads } = context();
  renderer.setBatch(b);
  expect(uploads.mock.calls.some(c => c[1] instanceof Uint32Array &&
    Array.from(c[1]).join() === "0,1,2,0,2,3,4,5,6,4,6,7")).toBe(true);
  renderer.setAtlas([{ index: 0, width: 1, height: 1, data: new Uint8Array([255]) }]);
  renderer.setHighlight([{ handle: 1, chain: [] }]);
  renderer.draw(camera, palette("light"));
  const triangles = draws.filter(d => d.mode === gl.TRIANGLES);
  expect(triangles.map(d => d.center)).toEqual([
    [0.125, 0.25], [Math.fround(camera.cx), Math.fround(camera.cy)],
    [0.125, 0.25], [Math.fround(camera.cx), Math.fround(camera.cy)], [0.125, 0.25],
  ]);
  expect(triangles.slice(2).map(d => [d.first, d.count])).toEqual([[0, 6], [24, 6], [0, 6]]);
});

test("image placement, visibility and reveal restore each tile's world coordinates", async () => {
  const { newPictureBounds, picturesInView } = await import("./batch");
  const { planReveal } = await import("./reveal");
  const b = surveyBatch(), data = new ArrayBuffer(40), f32 = new Float32Array(data), u32 = new Uint32Array(data);
  f32.set([0, 1, 1, 1, 1, 0, 0, 0]); u32[8] = 7;
  b.images = new ImageQuads(f32, u32); b.imageTiles = [b.segmentTiles![0]];
  b.imageTable = [{ id: 7, version: 1 }];
  expect(newPictureBounds(null, b)).toEqual([[500_000, 3_300_000_000], [500_001, 3_300_000_001]]);
  expect(picturesInView(b, [[499_999, 3_299_999_999], [500_002, 3_300_000_002]])).toEqual(new Set([7]));
  expect(picturesInView(b, [[0, 0], [10, 10]]).size).toBe(0);
  expect(Array.from(planReveal(b, [1, 2])!.segments)).toEqual([
    500_000, 3_300_000_000, 500_000.125, 3_300_000_000.25, 0, 0, 1, 1,
  ]);
  const { renderer, draws, gl } = context();
  renderer.setBatch(b);
  renderer.setPictures(new Map([[7, { width: 1, height: 1, data: new Uint8Array([255, 255, 255, 255]) }]]));
  renderer.draw(camera, palette("light"));
  expect(draws.find(d => d.mode === gl.TRIANGLES)?.center).toEqual([0.125, 0.25]);
});
