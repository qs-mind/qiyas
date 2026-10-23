/** Mizan's changes drawn live: the entities a step added grow one after another, line by line,
 *  over the drawing as it was, then the new drawing takes their place. */
import type { Batch } from "./batch";

/** Longest and shortest time one entity takes to draw, in ms. */
export const ENTITY_MIN_MS = 120;
export const ENTITY_MAX_MS = 400;
/** About how long one step's reveal takes, in ms (longer for many entities, up to the cap). */
export const STEP_MS = 1000;
export const STEP_MAX_MS = 2500;
/** More segments than this appear at once. */
export const MAX_SEGMENTS = 20_000;

export type RevealPlan = {
  /** `x0 y0 x1 y1` per segment, absolute drawing coordinates, entity after entity. */
  segments: Float64Array;
  /** Per entity: its first segment, how many, when it starts and how long it takes (ms). */
  entities: { first: number; count: number; start: number; duration: number }[];
  /** When the last entity is drawn (ms). */
  total: number;
};

/** The reveal of `handles` in their given order as `batch` draws them; `null` when there is
 *  nothing to grow or too much. A block reference includes all its drawn parts. */
export function planReveal(batch: Batch, handles: number[]): RevealPlan | null {
  const parts = new Map<bigint, [number, number][]>();
  for (const handle of handles) parts.set(BigInt(handle), []);
  if (parts.size === 0) return null;

  let total = 0;
  const ranges = batch.ranges;
  // Scan the drawing once, even when the step changed many entities.
  for (let i = 0; i < ranges.length; i++) {
    const own = parts.get(ranges.top(i));
    const count = ranges.count(i);
    if (!own || count === 0) continue;
    total += count;
    if (total > MAX_SEGMENTS) return null;
    own.push([ranges.first(i), count]);
  }
  if (total === 0) return null;

  const drawn = [...parts.values()].filter((own) => own.length > 0);
  const n = drawn.length;
  const span = Math.min(STEP_MAX_MS, Math.max(STEP_MS, n * ENTITY_MIN_MS * 0.6));
  const duration = Math.min(ENTITY_MAX_MS, Math.max(ENTITY_MIN_MS, (2 * span) / n));
  const gap = n > 1 ? (span - duration) / (n - 1) : 0;
  const segments = new Float64Array(total * 4);
  const entities: RevealPlan["entities"] = [];
  const [ox, oy] = batch.origin;
  let at = 0;
  drawn.forEach((own, i) => {
    const first = at;
    for (const [start, count] of own) {
      for (let j = start; j < start + count; j++) {
        const s = batch.segments;
        segments[4 * at] = s[4 * j] + ox;
        segments[4 * at + 1] = s[4 * j + 1] + oy;
        segments[4 * at + 2] = s[4 * j + 2] + ox;
        segments[4 * at + 3] = s[4 * j + 3] + oy;
        at++;
      }
    }
    entities.push({ first, count: at - first, start: i * gap, duration });
  });
  const last = entities[entities.length - 1];
  return { segments, entities, total: last.start + last.duration };
}

/** The segments drawn `t` ms into `plan`: each entity's segments in order, the one being drawn
 *  cut where its pen is. */
export function revealFrame(plan: RevealPlan, t: number): Float64Array {
  const out = new Float64Array(plan.segments.length);
  let at = 0;
  for (const e of plan.entities) {
    const progress = Math.min(1, Math.max(0, (t - e.start) / e.duration));
    if (progress <= 0) continue;
    const drawn = progress * e.count;
    const whole = Math.floor(drawn);
    out.set(plan.segments.subarray(4 * e.first, 4 * (e.first + whole)), at);
    at += 4 * whole;
    const part = drawn - whole;
    if (whole < e.count && part > 0) {
      const i = e.first + whole;
      const [x0, y0, x1, y1] = plan.segments.subarray(4 * i, 4 * i + 4);
      out[at++] = x0;
      out[at++] = y0;
      out[at++] = x0 + (x1 - x0) * part;
      out[at++] = y0 + (y1 - y0) * part;
    }
  }
  return out.subarray(0, at);
}
