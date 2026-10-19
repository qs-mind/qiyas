/** Violet line work drawn over the drawing: the rubber band of a measure in progress, and the
 *  points of the selected take-off line. */
import type { Measurement } from "../api";
import { matchingRanges, type Batch, type Target } from "../viewport/batch";
import type { Bounds, Point } from "../viewport/camera";
import type { Tool } from "../viewport/Viewport";

/**
 * Segments `x0 y0 x1 y1` in absolute drawing coordinates: the polyline through `points`, on to the
 * cursor, and for `area` and `mizanArea` back to the first point once there are two points or more. Float64, so far
 * coordinates stay exact until the renderer makes them relative to the batch origin.
 */
export function overlaySegments(points: Point[], cursor: Point | null, tool: Tool): Float64Array {
  const path = cursor && points.length > 0 ? [...points, cursor] : [...points];
  if ((tool === "area" || tool === "mizanArea") && points.length >= 2) path.push(points[0]);
  const out = new Float64Array(4 * Math.max(0, path.length - 1));
  for (let i = 1; i < path.length; i++) out.set([...path[i - 1], ...path[i]], 4 * (i - 1));
  return out;
}

/** Where a take-off line sits: around its points and its entities as drawn in `batch` (a count
 *  of an xref's blocks has both). `null` when neither is on the current batch. */
export function measurementBounds(m: Measurement, batch: Batch | null): Bounds | null {
  return around(m.points, measurementTargets(m), batch);
}

/** Around the entities `handles` (on the sheet) as drawn in `batch`; `null` when none is on it. */
export function handlesBounds(handles: number[], batch: Batch | null): Bounds | null {
  return around(
    [],
    handles.map((handle) => ({ handle, chain: [] })),
    batch,
  );
}

/** Around the entity `handle` (on the sheet) as drawn in `batch`, with twice its size of room on
 *  every side, to show a find's match in its surroundings; `null` when it is not on it. */
export function matchBounds(handle: number, batch: Batch | null): Bounds | null {
  const box = handlesBounds([handle], batch);
  if (!box) return null;
  const [[x0, y0], [x1, y1]] = box;
  const pad = Math.max(x1 - x0, y1 - y0) * 2;
  return [
    [x0 - pad, y0 - pad],
    [x1 + pad, y1 + pad],
  ];
}

/** What a take-off line measured: its entities, inside the blocks of its chain if it has one. */
export function measurementTargets(m: Measurement): Target[] {
  return m.handles.map((handle) => ({ handle, chain: m.chain }));
}

function around(points: Point[], entities: Target[], batch: Batch | null): Bounds | null {
  const min: Point = [Infinity, Infinity];
  const max: Point = [-Infinity, -Infinity];
  const add = (x: number, y: number) => {
    min[0] = Math.min(min[0], x);
    min[1] = Math.min(min[1], y);
    max[0] = Math.max(max[0], x);
    max[1] = Math.max(max[1], y);
  };
  for (const [x, y] of points) add(x, y);
  if (batch) {
    const [ox, oy] = batch.origin;
    for (const [first, count] of matchingRanges(batch, entities)) {
      for (let i = 4 * first; i < 4 * (first + count); i += 2) {
        add(batch.segments[i] + ox, batch.segments[i + 1] + oy);
      }
    }
  }
  return min[0] <= max[0] ? [min, max] : null;
}

/** A small cross `size` drawing units wide at each point, as segments `x0 y0 x1 y1`: where a
 *  count's places are (points are not drawn otherwise). */
export function pointMarks(points: Point[], size: number): Float64Array {
  const h = size / 2;
  const out = new Float64Array(8 * points.length);
  points.forEach(([x, y], i) =>
    out.set([x - h, y - h, x + h, y + h, x - h, y + h, x + h, y - h], 8 * i),
  );
  return out;
}

/** How big `pointMarks` crosses are on a sheet of `bounds`: a hundredth of its larger side. */
export function markSize(bounds: Bounds | null): number {
  if (!bounds) return 1;
  const side = Math.max(bounds[1][0] - bounds[0][0], bounds[1][1] - bounds[0][1]);
  return side > 0 && Number.isFinite(side) ? side / 100 : 1;
}
