/** Model space inside a layout's viewports: the camera each viewport looks through and the part
 *  of the canvas it is clipped to, worked out from the layout's camera. Pure functions; `gl.ts`
 *  draws with them. */
import type { PaperView } from "../api";
import type { Batch } from "./batch";
import { worldToScreen, type Camera } from "./camera";

/** The camera model space is seen through inside `view` on a layout seen through `paper`: the
 *  model point at the view's centre sits where the frame's centre is on screen, zoomed by the
 *  view's scale. */
export function viewCamera(paper: Camera, view: PaperView): Camera {
  const [[x0, y0], [x1, y1]] = view.rect;
  const [cx, cy] = [(x0 + x1) / 2, (y0 + y1) / 2];
  return {
    cx: view.centre[0] + (paper.cx - cx) / view.scale,
    cy: view.centre[1] + (paper.cy - cy) / view.scale,
    scale: paper.scale / view.scale,
    width: paper.width,
    height: paper.height,
  };
}

/** The view's frame on the canvas as a GL scissor box `[x, y, width, height]` in device pixels
 *  (y from the bottom), cut to the canvas; null when none of it is on screen. */
export function scissorBox(
  paper: Camera,
  view: PaperView,
  canvasWidth: number,
  canvasHeight: number,
): [number, number, number, number] | null {
  const dpr = canvasWidth / paper.width;
  const [a, b] = [worldToScreen(paper, view.rect[0]), worldToScreen(paper, view.rect[1])];
  const left = Math.max(0, Math.floor(Math.min(a[0], b[0]) * dpr));
  const right = Math.min(canvasWidth, Math.ceil(Math.max(a[0], b[0]) * dpr));
  const top = Math.max(0, Math.floor(Math.min(a[1], b[1]) * dpr));
  const bottom = Math.min(canvasHeight, Math.ceil(Math.max(a[1], b[1]) * dpr));
  if (right <= left || bottom <= top) return null;
  return [left, canvasHeight - bottom, right - left, bottom - top];
}

/** The spans of `batch` the view draws: its own, or all when it freezes no layer. */
export function viewSpans(view: PaperView, batch: Batch) {
  const all = (n: number): [number, number][] => (n > 0 ? [[0, n]] : []);
  return {
    lines: view.lines ?? all(batch.colors.length),
    fills: view.fills ?? all(batch.fills.length),
    texts: view.texts ?? all(batch.quads.length),
  };
}

/** A sheet's batch held by the window, and the core's version of it. */
export type HeldSheet = { version: number; batch: Batch };

/** `next`, its glyphs drawn again for another zoom, with the lines, fills and ranges of `held`
 *  (the same arrays, so the GPU keeps them). */
export const keepLines = (held: Batch, next: Batch): Batch => {
  const { segments, colors, ranges, chains, fills, fillRanges, segmentTiles, fillTiles } = held;
  return { ...next, segments, colors, ranges, chains, fills, fillRanges, segmentTiles, fillTiles };
};

/** The batch of an off-screen sheet (model space for viewports, a split window's other sheet) as
 *  the core has it now: `held` itself when its version is the core's and no new glyphs are asked
 *  for; with `pxPerUnit`, the glyphs drawn again for that zoom and the held lines kept; otherwise
 *  the whole batch fetched (`fetch` resolves to null when there is nothing new). */
export async function refreshSheet(
  held: HeldSheet | null,
  pxPerUnit: number | undefined,
  api: { version(): Promise<number>; fetch(pxPerUnit?: number): Promise<Batch | null> },
): Promise<HeldSheet | null> {
  const version = await api.version();
  const same = held !== null && held.version === version;
  if (same && pxPerUnit === undefined) return held;
  const next = await api.fetch(pxPerUnit);
  if (!next) return held;
  return { version, batch: same ? keepLines(held.batch, next) : next };
}

/** Subtract in double precision before passing the camera to GL. Reuses `out`. */
export function tileCamera(camera: Camera, origin: [number, number], out: Float32Array): void {
  out[0] = camera.cx - origin[0];
  out[1] = camera.cy - origin[1];
}
