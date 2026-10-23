import { expect, test } from "vitest";
import { Fills, ImageQuads, NO_CHAIN, Quads, Ranges, type Batch } from "./batch";
import {
  ENTITY_MAX_MS,
  ENTITY_MIN_MS,
  MAX_SEGMENTS,
  STEP_MAX_MS,
  STEP_MS,
  planReveal,
  revealFrame,
} from "./reveal";

/** Ranges in drawing order; a different entity belongs to the top-level block reference. */
function batch(
  parts: { top: number; count: number; entity?: number }[],
  segments?: number[],
  origin: [number, number] = [0, 0],
): Batch {
  const bytes = new ArrayBuffer(parts.length * 32);
  const wide = new BigUint64Array(bytes);
  const narrow = new Uint32Array(bytes);
  let first = 0;
  parts.forEach(({ top, count, entity = top }, i) => {
    wide[4 * i] = BigInt(top);
    wide[4 * i + 1] = BigInt(entity);
    narrow[8 * i + 4] = entity === top ? NO_CHAIN : 0;
    narrow[8 * i + 5] = first;
    narrow[8 * i + 6] = count;
    first += count;
  });
  const lines = segments ?? Array.from({ length: first }, (_, i) => [i, 0, i + 1, 0]).flat();
  return {
    segments: new Float32Array(lines),
    origin,
    ranges: new Ranges(wide, narrow),
    chains: [],
    colors: new Uint32Array(first),
    bounds: [[0, 0], [first, 0]],
    quads: new Quads(new Float32Array(), new Uint16Array(), new Uint32Array()),
    textRanges: new Ranges(new BigUint64Array(), new Uint32Array()),
    atlasVersion: 0,
    atlasPages: 0,
    pxPerUnit: 0,
    images: new ImageQuads(new Float32Array(), new Uint32Array()),
    imageTable: [],
    fills: new Fills(new Float32Array(), new Uint32Array()),
    fillRanges: new Ranges(new BigUint64Array(), new Uint32Array()),
  };
}

test("entity and step timings stay within their bounds, with overlap for large steps", () => {
  for (const count of [1, 2, 5, 20, 100, 1000]) {
    const handles = Array.from({ length: count }, (_, i) => i + 1);
    const plan = planReveal(batch(handles.map((top) => ({ top, count: 1 }))), handles)!;
    expect(plan.entities).toHaveLength(count);
    expect(plan.entities[0].start).toBe(0);
    expect(plan.total).toBeGreaterThanOrEqual(ENTITY_MIN_MS);
    expect(plan.total).toBeLessThanOrEqual(STEP_MAX_MS);
    if (count > 1) expect(plan.total).toBeGreaterThanOrEqual(STEP_MS);
    for (const [i, entity] of plan.entities.entries()) {
      expect(entity.duration).toBeGreaterThanOrEqual(ENTITY_MIN_MS);
      expect(entity.duration).toBeLessThanOrEqual(ENTITY_MAX_MS);
      expect(entity.start + entity.duration).toBeLessThanOrEqual(plan.total);
      if (i > 0) expect(entity.start).toBeGreaterThan(plan.entities[i - 1].start);
    }
    if (count >= 20) {
      expect(plan.entities[1].start).toBeLessThan(plan.entities[0].duration);
    }
  }
});

test("handles set entity order, leaving missing and repeated handles out", () => {
  const plan = planReveal(
    batch([
      { top: 10, count: 1 },
      { top: 20, count: 2 },
      { top: 30, count: 1 },
    ]),
    [30, 99, 10, 30, 20],
  )!;
  expect(plan.entities.map(({ first, count }) => [first, count])).toEqual([
    [0, 1],
    [1, 1],
    [2, 2],
  ]);
  expect(Array.from(plan.segments)).toEqual([
    3, 0, 4, 0,
    0, 0, 1, 0,
    1, 0, 2, 0,
    2, 0, 3, 0,
  ]);
  expect(Array.from(revealFrame(plan, plan.entities[0].duration / 2))).toEqual([3, 0, 3.5, 0]);
});

test("a block grows all its ranges together in absolute drawing coordinates", () => {
  const plan = planReveal(
    batch(
      [
        { top: 7, entity: 70, count: 1 },
        { top: 8, count: 1 },
        { top: 7, entity: 71, count: 1 },
      ],
      [-2, -1, 2, 1, 5, 5, 6, 6, 0, 1, 0, 3],
      [1_000_000, -2_000_000],
    ),
    [7],
  )!;
  expect(plan.entities).toHaveLength(1);
  expect(plan.entities[0].count).toBe(2);
  expect(Array.from(plan.segments)).toEqual([
    999_998, -2_000_001, 1_000_002, -1_999_999,
    1_000_000, -1_999_999, 1_000_000, -1_999_997,
  ]);
});

test("a frame grows a partial segment, then keeps it while the next segment grows", () => {
  const plan = planReveal(
    batch([{ top: 1, count: 2 }], [0, 0, 10, 0, 10, 0, 10, 20]),
    [1],
  )!;
  expect(revealFrame(plan, -1)).toHaveLength(0);
  expect(revealFrame(plan, 0)).toHaveLength(0);
  expect(Array.from(revealFrame(plan, plan.total / 4))).toEqual([0, 0, 5, 0]);
  expect(Array.from(revealFrame(plan, plan.total / 2))).toEqual([0, 0, 10, 0]);
  expect(Array.from(revealFrame(plan, plan.total * 0.75))).toEqual([0, 0, 10, 0, 10, 0, 10, 10]);
  expect(revealFrame(plan, plan.total)).toEqual(plan.segments);
  expect(revealFrame(plan, plan.total + 100)).toEqual(plan.segments);
});

test("nothing to draw gives no plan", () => {
  const drawing = batch([{ top: 1, count: 1 }]);
  expect(planReveal(drawing, [])).toBeNull();
  expect(planReveal(drawing, [2])).toBeNull();
  expect(planReveal(batch([]), [1])).toBeNull();
});

test("the segment cap counts only requested entities and rejects larger steps", () => {
  const drawing = batch([
    { top: 1, count: MAX_SEGMENTS },
    { top: 2, count: 1 },
  ]);
  expect(planReveal(drawing, [1])!.segments.length).toBe(MAX_SEGMENTS * 4);
  expect(planReveal(drawing, [2])!.segments.length).toBe(4);
  expect(planReveal(drawing, [1, 2])).toBeNull();
  expect(planReveal(batch([{ top: 1, count: MAX_SEGMENTS + 1 }]), [1])).toBeNull();
});
