import { expect, test } from "vitest";
import type { DrawSpec } from "../api";
import type { Point } from "../viewport/camera";
import {
  drawOptions,
  drawPreview,
  drawReducer,
  initialDrawState,
  isDrawTool,
  takesNumber,
  type DrawInput,
  type DrawKind,
  type DrawState,
} from "./drawTools";

const pt = (p: Point): DrawInput => ({ type: "point", p });
const opt = (option: string): DrawInput => ({ type: "option", option });
const num = (value: number): DrawInput => ({ type: "number", value });
const finish: DrawInput = { type: "finish" };

/** The states after each input, from the tool's start. */
function steps(kind: DrawKind, inputs: DrawInput[]): DrawState[] {
  const out: DrawState[] = [];
  let s = initialDrawState(kind);
  for (const i of inputs) {
    s = drawReducer(s, i);
    out.push(s);
  }
  return out;
}

/** Every spec the inputs finished, in order. */
const specs = (kind: DrawKind, inputs: DrawInput[]): DrawSpec[] =>
  steps(kind, inputs).flatMap((s) => (s.spec ? [s.spec] : []));

const close = (a: number, b: number) => Math.abs(a - b) < 1e-9;

test("the draw tools are told apart from the others", () => {
  expect(isDrawTool("line")).toBe(true);
  expect(isDrawTool("wipeout")).toBe(true);
  expect(isDrawTool("distance")).toBe(false);
});

test("line repeats from the last point until Esc or Enter", () => {
  const all = steps("line", [pt([0, 0]), pt([1, 0]), pt([1, 1]), finish, pt([5, 5])]);
  expect(all.flatMap((s) => (s.spec ? [s.spec] : []))).toEqual([
    { kind: "line", a: [0, 0], b: [1, 0] },
    { kind: "line", a: [1, 0], b: [1, 1] },
  ]);
  expect(all[0].hint).toBe("draw.prompt.nextPoint");
  expect(all[1].hint).toBe("draw.prompt.lineNext");
  // Enter ends the chain: the next click starts a new one.
  expect(all[3].points).toEqual([]);
  expect(all[3].hint).toBe("draw.prompt.firstPoint");
  expect(all[4].points).toEqual([[5, 5]]);
  // The same point twice draws nothing.
  expect(specs("line", [pt([0, 0]), pt([0, 0])])).toEqual([]);
});

test("line closes back to its first point with C, and U takes back the last segment", () => {
  const closed = steps("line", [pt([0, 0]), pt([4, 0]), pt([4, 3]), opt("C")]);
  expect(closed[3].spec).toEqual({ kind: "line", a: [4, 3], b: [0, 0] });
  expect(closed[3].points).toEqual([]);
  // C is no circle here, and wants two segments first.
  expect(drawOptions(closed[1])).toEqual(["U"]);
  expect(drawOptions(closed[2])).toEqual(["C", "U"]);
  // U asks for the last line to be undone and goes on from its start.
  const undone = steps("line", [pt([0, 0]), pt([4, 0]), pt([4, 3]), opt("U"), pt([9, 9])]);
  expect(undone[3].undo).toBe(true);
  expect(undone[3].spec).toBeNull();
  expect(undone[3].points).toEqual([
    [0, 0],
    [4, 0],
  ]);
  expect(undone[4].undo).toBe(false);
  expect(undone[4].spec).toEqual({ kind: "line", a: [4, 0], b: [9, 9] });
});

test("polyline takes arcs, closes, and takes back its last point", () => {
  const plain = specs("polyline", [pt([0, 0]), pt([2, 0]), pt([2, 2]), finish]);
  expect(plain).toEqual([
    {
      kind: "polyline",
      points: [
        [0, 0],
        [2, 0],
        [2, 2],
      ],
      bulges: [0, 0, 0],
      closed: false,
    },
  ]);
  // U drops the last point; C closes.
  const closed = specs("polyline", [pt([0, 0]), pt([2, 0]), pt([9, 9]), opt("U"), pt([2, 2]), opt("C")]);
  expect(closed).toEqual([
    {
      kind: "polyline",
      points: [
        [0, 0],
        [2, 0],
        [2, 2],
      ],
      bulges: [0, 0, 0],
      closed: true,
    },
  ]);
  // An arc after a straight segment carries on its direction: a half circle from (2, 0) to (2, 2)
  // after heading +X turns left.
  const [arc] = specs("polyline", [pt([0, 0]), pt([2, 0]), opt("A"), pt([2, 2]), opt("L"), pt([0, 2]), finish]);
  expect(arc.kind).toBe("polyline");
  if (arc.kind !== "polyline" || !arc.bulges) throw new Error("a polyline with bulges");
  expect(close(arc.bulges[1], 1)).toBe(true);
  expect(arc.bulges[0]).toBe(0);
  expect(arc.bulges[2]).toBe(0);
  // A first segment drawn as an arc starts heading +X: a quarter turn to (1, 1).
  const [first] = specs("polyline", [pt([0, 0]), opt("A"), pt([1, 1]), finish]);
  if (first.kind !== "polyline" || !first.bulges) throw new Error("a polyline with bulges");
  expect(close(first.bulges[0], Math.tan(Math.PI / 8))).toBe(true);
  // One point is not a polyline.
  expect(specs("polyline", [pt([0, 0]), finish])).toEqual([]);
  expect(drawOptions(initialDrawState("polyline"))).toEqual([]);
  expect(drawOptions(steps("polyline", [pt([0, 0]), pt([1, 0])])[1])).toEqual(["A", "L", "C", "U"]);
});

test("rectangle by two corners", () => {
  expect(specs("rectangle", [pt([0, 0]), pt([3, 2])])).toEqual([
    { kind: "rectangle", a: [0, 0], b: [3, 2] },
  ]);
  const all = steps("rectangle", [pt([0, 0]), pt([3, 2])]);
  expect(all[0].hint).toBe("draw.prompt.otherCorner");
  expect(all[1].hint).toBe("draw.prompt.firstCorner");
});

test("circle by centre and radius, two points or three points", () => {
  expect(specs("circle", [pt([1, 1]), pt([4, 5])])).toEqual([{ kind: "circle", centre: [1, 1], radius: 5 }]);
  // A typed radius.
  const typed = steps("circle", [pt([1, 1])]);
  expect(takesNumber(typed[0])).toBe("distance");
  expect(specs("circle", [pt([1, 1]), num(2.5)])).toEqual([{ kind: "circle", centre: [1, 1], radius: 2.5 }]);
  expect(specs("circle", [opt("2P"), pt([0, 0]), pt([4, 0])])).toEqual([
    { kind: "circle", centre: [2, 0], radius: 2 },
  ]);
  const [three] = specs("circle", [opt("3P"), pt([2, 0]), pt([0, 2]), pt([-2, 0])]);
  expect(three.kind).toBe("circle");
  if (three.kind !== "circle") return;
  expect(close(three.centre[0], 0) && close(three.centre[1], 0) && close(three.radius, 2)).toBe(true);
  // Three points in a line: no circle, and it says so.
  const line = steps("circle", [opt("3P"), pt([0, 0]), pt([1, 1]), pt([2, 2])]);
  expect(line[3].spec).toBeNull();
  expect(line[3].hint).toBe("draw.prompt.inLine");
  expect(drawOptions(initialDrawState("circle"))).toEqual(["2P", "3P"]);
});

test("arc through three points, or by centre, start and end", () => {
  expect(specs("arc", [pt([2, 0]), pt([0, 2]), pt([-2, 0])])).toEqual([
    { kind: "three_point_arc", a: [2, 0], b: [0, 2], c: [-2, 0] },
  ]);
  const [arc] = specs("arc", [opt("C"), pt([0, 0]), pt([2, 0]), pt([0, 5])]);
  expect(arc.kind).toBe("arc");
  if (arc.kind !== "arc") return;
  expect([arc.centre, arc.radius, arc.start, arc.end]).toEqual([[0, 0], 2, 0, 90]);
  expect(drawOptions(initialDrawState("arc"))).toEqual(["C"]);
});

test("ellipse by centre, axis end and the other axis", () => {
  expect(specs("ellipse", [pt([0, 0]), pt([4, 0]), pt([0, 2])])).toEqual([
    { kind: "ellipse", centre: [0, 0], major: [4, 0], ratio: 0.5 },
  ]);
  // The other axis longer: it becomes the major one.
  const [e] = specs("ellipse", [pt([0, 0]), pt([1, 0]), num(4)]);
  if (e.kind !== "ellipse") throw new Error("an ellipse");
  expect(close(e.major[0], 0) && close(e.major[1], 4) && close(e.ratio, 0.25)).toBe(true);
});

test("spline, revision cloud and wipeout take points until Enter", () => {
  const ring = [pt([0, 0]), pt([4, 0]), pt([4, 4]), pt([0, 4]), finish];
  expect(specs("spline", [pt([0, 0]), pt([1, 1]), pt([2, 0]), finish])).toEqual([
    {
      kind: "spline",
      fit_points: [
        [0, 0],
        [1, 1],
        [2, 0],
      ],
    },
  ]);
  const [cloud] = specs("revcloud", ring);
  if (cloud.kind !== "revcloud") throw new Error("a cloud");
  expect(cloud.points.length).toBe(4);
  // About eight arcs a side when not typed: the outline's length over 32... any positive length.
  expect(cloud.arc_len).toBeGreaterThan(0);
  const [typedCloud] = specs("revcloud", [num(0.5), ...ring]);
  if (typedCloud.kind !== "revcloud") throw new Error("a cloud");
  expect(typedCloud.arc_len).toBe(0.5);
  expect(specs("wipeout", ring)).toEqual([
    {
      kind: "wipeout",
      points: [
        [0, 0],
        [4, 0],
        [4, 4],
        [0, 4],
      ],
    },
  ]);
  // Two points are not an area.
  const short = steps("wipeout", [pt([0, 0]), pt([1, 0]), finish]);
  expect(short[2].spec).toBeNull();
  expect(short[2].hint).toBe("draw.prompt.needThree");
});

test("point places one per click", () => {
  expect(specs("point", [pt([1, 2]), pt([3, 4])])).toEqual([
    { kind: "point", p: [1, 2] },
    { kind: "point", p: [3, 4] },
  ]);
});

test("hatch asks for the area around the click, then fills it with the chosen pattern", () => {
  const ring: Point[] = [
    [0, 0],
    [4, 0],
    [4, 3],
  ];
  const holes: Point[][] = [
    [
      [1, 1],
      [2, 1],
      [2, 2],
    ],
  ];
  const bulges = [[0, 0.4, 0], [0, 0, 0]];
  const found = (scale = 25): DrawInput => ({ type: "ring", found: { found: "ring", ring, holes, bulges }, scale });
  const all = steps("hatch", [opt("ANSI31"), pt([1, 1]), found()]);
  // The prompt names the pattern.
  expect(all[0].hint).toBe("draw.prompt.hatch");
  expect(all[0].hintArgs).toEqual({ pattern: "ANSI31" });
  expect(all[1].seek).toEqual([1, 1]);
  expect(all[1].spec).toBeNull();
  expect(all[2].seek).toBeNull();
  // The drawing's scale (25 in millimetres) unless one is typed; the islands stay empty.
  expect(all[2].spec).toEqual({
    kind: "hatch",
    boundary_handles: [],
    points: ring,
    holes,
    bulges,
    pattern: { custom: { name: "ANSI31", angle: 0, scale: 25 } },
  });
  expect(all[2].hintArgs).toEqual({ pattern: "ANSI31" });
  // Solid by default.
  expect(initialDrawState("hatch").hintArgs).toEqual({ pattern: "SOLID" });
  expect(specs("hatch", [pt([1, 1]), found()])[0]).toMatchObject({ pattern: "solid" });
  // S and A take the number typed next: a scale above 0, an angle of any sign.
  const typed = steps("hatch", [
    opt("NET"),
    opt("S"),
    num(0.5),
    opt("A"),
    num(-30),
    pt([1, 1]),
    found(0.025),
  ]);
  expect(takesNumber(typed[1])).toBe("distance");
  expect(takesNumber(typed[3])).toBe("angle");
  expect(typed[4].hintArgs).toEqual({ pattern: "NET, S 0.5, A -30°" });
  expect(typed[6].spec).toMatchObject({ pattern: { custom: { name: "NET", angle: -30, scale: 0.5 } } });
  // Kept for the next hatch.
  expect(typed[6].patternScale).toBe(0.5);
  // No area, or too much line work: it says so and waits for another click.
  const none = steps("hatch", [pt([1, 1]), { type: "ring", found: { found: "none" }, scale: 1 }]);
  expect(none[1].spec).toBeNull();
  expect(none[1].hint).toBe("draw.prompt.noBoundary");
  const dense = steps("hatch", [pt([1, 1]), { type: "ring", found: { found: "too_dense" }, scale: 1 }]);
  expect(dense[1].hint).toBe("draw.prompt.zoomIn");
  expect(drawOptions(initialDrawState("hatch"))).toEqual(
    expect.arrayContaining(["ANSI37", "S", "A"]),
  );
});

test("previews follow the cursor", () => {
  const segs = (s: DrawState, cursor: Point) => drawPreview(s, cursor).length / 4;
  const [one] = steps("line", [pt([0, 0])]);
  expect(Array.from(drawPreview(one, [1, 1]))).toEqual([0, 0, 1, 1]);
  const [corner] = steps("rectangle", [pt([0, 0])]);
  expect(segs(corner, [2, 2])).toBe(4);
  const [centre] = steps("circle", [pt([0, 0])]);
  const circle = drawPreview(centre, [2, 0]);
  expect(circle.length / 4).toBeGreaterThan(16);
  for (let i = 0; i < circle.length; i += 2) expect(close(Math.hypot(circle[i], circle[i + 1]), 2)).toBe(true);
  // Nothing placed yet: nothing to show.
  expect(drawPreview(initialDrawState("polyline"), [1, 1]).length).toBe(0);
  // A wipeout outline closes back to its start.
  const w = steps("wipeout", [pt([0, 0]), pt([4, 0])])[1];
  expect(segs(w, [4, 4])).toBe(3);
});
