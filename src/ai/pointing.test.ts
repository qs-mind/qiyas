import { describe, expect, it } from "vitest";
import { kindsLabel, pointedKey, pointedTargets, regionBounds, regionRing } from "./pointing";

describe("pointing", () => {
  it("takes the selection set, or the one entity picked inside a block", () => {
    expect(pointedTargets([5, 9], null)).toEqual([
      { handle: 5, chain: [] },
      { handle: 9, chain: [] },
    ]);
    // A line inside the door block: the line, not the whole door.
    expect(pointedTargets([40], { handle: 41, chain: [40] })).toEqual([{ handle: 41, chain: [40] }]);
    // Shift added another: whole entities then.
    expect(pointedTargets([40, 50], { handle: 41, chain: [40] })).toEqual([
      { handle: 40, chain: [] },
      { handle: 50, chain: [] },
    ]);
    expect(pointedTargets([], null)).toEqual([]);
  });

  it("names the kinds, most first", () => {
    const sel = [5, 6, 7].map((handle) => ({ handle, chain: [] }));
    expect(kindsLabel(sel, { 5: "Insert", 6: "LwPolyline", 7: "LwPolyline" })).toBe("2 Polyline, 1 Block");
    expect(kindsLabel(sel, {})).toBe("");
  });

  it("rings a box round and bounds a polygon", () => {
    expect(regionRing({ kind: "box", points: [[4, 3], [0, 0]] })).toEqual([
      [0, 0],
      [4, 0],
      [4, 3],
      [0, 3],
    ]);
    expect(regionBounds({ kind: "polygon", points: [[0, 0], [10, 0], [4, 6]] })).toEqual([
      [0, 0],
      [10, 6],
    ]);
  });

  it("keys change with the selection or the region only", () => {
    const a = pointedKey([{ handle: 1, chain: [] }], null);
    expect(pointedKey([{ handle: 1, chain: [] }], null)).toBe(a);
    expect(pointedKey([{ handle: 2, chain: [] }], null)).not.toBe(a);
    expect(pointedKey([{ handle: 1, chain: [] }], { kind: "box", points: [[0, 0], [1, 1]] })).not.toBe(a);
  });
});
