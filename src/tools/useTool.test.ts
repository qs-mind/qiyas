import { expect, test } from "vitest";
import type { EntityInfo, Picked } from "../api";
import type { Point } from "../viewport/camera";
import { mergeAliases } from "./aliases";
import { parse, type ParseState } from "./commandParser";
import {
  initialToolState,
  selectedTarget,
  toolReducer,
  type ToolEvent,
  type ToolState,
} from "./useTool";

/** An entity on the sheet. */
const onSheet = (handle: number): Picked => ({ top: handle, entity: handle, chain: [], xref: null });

/** A click at `world`, drawn at the same screen point, `time` ms apart from the others. */
const click = (
  world: Point,
  time: number,
  picked: Picked | number | null = null,
  info: EntityInfo | null = null,
): ToolEvent => ({
  type: "click",
  world,
  screen: [world[0] * 10, world[1] * 10],
  time,
  picked: typeof picked === "number" ? onSheet(picked) : picked,
  info,
});

const run = (tool: ToolState["tool"], events: ToolEvent[]) =>
  events.reduce(toolReducer, initialToolState(tool));

const door: EntityInfo = {
  handle: 42,
  kind: "Insert",
  layer: "A-DOOR",
  layer_short: "A-DOOR",
  length: null,
  area: null,
  block_name: "DOOR",
  text: null,
  inside_block: null,
  chain: [],
  block_path: [],
  color: "ByLayer",
  color_rgb: [0, 255, 0],
  linetype: "ByLayer",
  lineweight: -1,
  radius: null,
  attributes: [],
  height: null,
  style: null,
};

test("distance tool needs two points then a finish", () => {
  const one = run("distance", [click([0, 0], 0), { type: "finish" }]);
  expect(one.pendingCommand).toBeNull();
  expect(one.hint).toBe("measure.needTwo");

  const two = run("distance", [click([0, 0], 0), click([10, 0], 1000), { type: "finish" }]);
  expect(two.pendingCommand).toEqual({
    type: "measure_distance",
    points: [
      [0, 0],
      [10, 0],
    ],
  });
  expect(two.points).toEqual([]);
});

test("typed points are placed as typed and never make a double click", () => {
  // As `typePoint` sends them: snapped, with no screen position.
  const typed = (world: Point, time: number): ToolEvent => ({
    type: "click",
    world,
    time,
    picked: null,
    info: null,
    screen: [Number.NaN, Number.NaN],
    snapped: true,
    track: { ortho: true, polar: false, polarStepDeg: 45 },
  });
  const s = run("distance", [typed([0, 0], 0), typed([3, 4], 10)]);
  expect(s.points).toEqual([
    [0, 0],
    [3, 4],
  ]);
  expect(s.pendingCommand).toBeNull();
});

test("area tool rejects finish with two points", () => {
  const s = run("area", [click([0, 0], 0), click([4, 0], 1000), { type: "finish" }]);
  expect(s.pendingCommand).toBeNull();
  expect(s.hint).toBe("measure.needThree");
  expect(s.points).toHaveLength(2);
});

test("escape clears points", () => {
  const s = run("distance", [click([0, 0], 0), click([10, 0], 1000), { type: "escape" }]);
  expect(s.points).toEqual([]);
  expect(s.pendingCommand).toBeNull();
});

test("a double click finishes the area without a duplicate point", () => {
  const s = run("area", [
    click([0, 0], 0),
    click([4, 0], 1000),
    click([4, 3], 2000),
    click([4, 3], 2200),
  ]);
  expect(s.pendingCommand).toEqual({
    type: "measure_area",
    points: [
      [0, 0],
      [4, 0],
      [4, 3],
    ],
  });
  expect(s.points).toEqual([]);
  // A slow second click is just another point.
  expect(run("area", [click([0, 0], 0), click([0, 0], 400)]).points).toHaveLength(2);
});

test("count tool counts blocks and refuses other entities", () => {
  const counted = run("count", [click([1, 1], 0, 42, door)]);
  expect(counted.pendingCommand).toEqual({ type: "count_block", block_name: "DOOR", layer: null });

  const line = { ...door, kind: "Line", block_name: null };
  const refused = run("count", [click([1, 1], 0, 7, line)]);
  expect(refused.pendingCommand).toBeNull();
  expect(refused.hint).toBe("measure.notABlock");
});

test("select highlights, then a second click on the same entity measures it", () => {
  const first = run("select", [click([0, 0], 0, 7)]);
  expect(selectedTarget(first.selected)).toEqual({ handle: 7, chain: [] });
  expect(first.pendingCommand).toBeNull();
  expect(first.hint).toBe("");

  const second = toolReducer(first, click([5, 0], 1000, 7));
  expect(second.pendingCommand).toEqual({ type: "measure_entity", handle: 7, chain: [] });

  const other = toolReducer(first, click([5, 0], 1000, 8));
  expect(selectedTarget(other.selected)).toEqual({ handle: 8, chain: [] });
  expect(other.pendingCommand).toBeNull();
  expect(toolReducer(first, { type: "escape" }).selected).toBeNull();
});

test("inside a block, a second click toggles between the entity and the block", () => {
  // The leaf line 9 inside DOOR reference 8, which sits inside PLAN reference 5.
  const leaf: Picked = { top: 5, entity: 9, chain: [5, 8], xref: null };
  const info: EntityInfo = {
    ...door,
    handle: 9,
    kind: "Line",
    block_name: null,
    inside_block: "DOOR",
    chain: [5, 8],
    block_path: ["PLAN", "DOOR"],
  };
  const first = run("select", [click([0, 0], 0, leaf, info)]);
  expect(selectedTarget(first.selected)).toEqual({ handle: 9, chain: [5, 8] });
  expect([first.hint, first.hintArgs]).toEqual(["measure.insideBlock", { name: "DOOR" }]);

  const whole = toolReducer(first, click([0, 0], 1000, leaf, info));
  expect(whole.pendingCommand).toBeNull();
  expect(selectedTarget(whole.selected)).toEqual({ handle: 5, chain: [] });
  expect([whole.hint, whole.hintArgs]).toEqual(["measure.wholeBlock", { name: "PLAN" }]);

  const back = toolReducer(whole, click([0, 0], 2000, leaf, info));
  expect(selectedTarget(back.selected)).toEqual({ handle: 9, chain: [5, 8] });

  // Enter measures what is selected: the entity in world units, through its chain.
  expect(toolReducer(back, { type: "finish" }).pendingCommand).toEqual({
    type: "measure_entity",
    handle: 9,
    chain: [5, 8],
  });
  expect(toolReducer(whole, { type: "finish" }).pendingCommand).toEqual({
    type: "measure_entity",
    handle: 5,
    chain: [],
  });
  // So does a double click.
  const double = toolReducer(first, click([0, 0], 100, leaf, info));
  expect(double.pendingCommand).toEqual({ type: "measure_entity", handle: 9, chain: [5, 8] });
});

test("a double click measures what the status bar showed before it", () => {
  const leaf: Picked = { top: 5, entity: 9, chain: [5, 8], xref: null };
  const info: EntityInfo = { ...door, handle: 9, chain: [5, 8], block_path: ["PLAN", "DOOR"] };
  const block = { type: "measure_entity", handle: 5, chain: [] };
  const entity = { type: "measure_entity", handle: 9, chain: [5, 8] };
  const inner = run("select", [click([0, 0], 0, leaf, info)]);
  const whole = toolReducer(inner, click([0, 0], 1000, leaf, info));
  expect(whole.hint).toBe("measure.wholeBlock");

  // Showing the whole block: the pair's first click swaps to the entity, the second takes it back.
  const fromWhole = [click([0, 0], 3000, leaf, info), click([0, 0], 3100, leaf, info)].reduce(
    toolReducer,
    whole,
  );
  expect(fromWhole.pendingCommand).toEqual(block);
  // Showing the entity, after a pause.
  const fromInner = [click([0, 0], 3000, leaf, info), click([0, 0], 3100, leaf, info)].reduce(
    toolReducer,
    inner,
  );
  expect(fromInner.pendingCommand).toEqual(entity);
  // A swap back and then a double click still measures what is shown.
  const back = toolReducer(whole, click([0, 0], 2000, leaf, info));
  expect(back.hint).toBe("measure.insideBlock");
  const fromBack = [click([0, 0], 4000, leaf, info), click([0, 0], 4100, leaf, info)].reduce(
    toolReducer,
    back,
  );
  expect(fromBack.pendingCommand).toEqual(entity);

  // Enter measures what the status shows, in both states.
  expect(toolReducer(whole, { type: "finish" }).pendingCommand).toEqual(block);
  expect(toolReducer(inner, { type: "finish" }).pendingCommand).toEqual(entity);
  expect(toolReducer(back, { type: "finish" }).pendingCommand).toEqual(entity);
});

const ORTHO = { ortho: true, polar: false, polarStepDeg: 45 };
const POLAR = { ortho: false, polar: true, polarStepDeg: 45 };

/** A click placing a point with tracking, `snapped` when it is an object snap's point. */
const tracked = (world: Point, time: number, track: typeof ORTHO, snapped = false): ToolEvent => ({
  type: "click",
  world,
  screen: [world[0] * 10, world[1] * 10],
  time,
  picked: null,
  info: null,
  track,
  snapped,
});

test("ortho keeps the second point level with the first", () => {
  const s = run("distance", [
    tracked([1, 1], 0, ORTHO),
    tracked([6, 1.8], 1000, ORTHO),
  ]);
  expect(s.points).toEqual([
    [1, 1],
    [6, 1],
  ]);
});

test("ortho goes up when the cursor is more up than across", () => {
  const s = run("area", [
    tracked([0, 0], 0, ORTHO),
    tracked([0.5, 4], 1000, ORTHO),
  ]);
  expect(s.points[1]).toEqual([0, 4]);
});

test("polar snaps the direction to 45 degrees when within 5 degrees", () => {
  const s = run("distance", [
    tracked([0, 0], 0, POLAR),
    // About 47 degrees.
    tracked([3, 3.2], 1000, POLAR),
  ]);
  const [x, y] = s.points[1];
  expect(x).toBeCloseTo(y, 9);
  expect(Math.hypot(x, y)).toBeCloseTo((3 + 3.2) / Math.SQRT2, 9);

  // 30 degrees is 15 away from both 0 and 45: left alone.
  const free = run("distance", [
    tracked([0, 0], 0, POLAR),
    tracked([Math.cos(Math.PI / 6), Math.sin(Math.PI / 6)], 1000, POLAR),
  ]);
  expect(free.points[1][0]).toBeCloseTo(Math.cos(Math.PI / 6), 12);
});

test("a snapped point is placed as it is, ortho or not", () => {
  const s = run("distance", [
    tracked([0, 0], 0, ORTHO),
    tracked([4, 3], 1000, ORTHO, true),
  ]);
  expect(s.points[1]).toEqual([4, 3]);
});

test("geometry asks about the entity clicked, inside its blocks, with the click point", () => {
  const inBlock: Picked = { top: 7, entity: 42, chain: [7], xref: null };
  const s = run("geometry", [click([3, 4], 0, inBlock)]);
  expect(s.pendingCommand).toEqual({ type: "measure_geometry", handle: 42, chain: [7], at: [3, 4] });
  const nothing = run("geometry", [click([3, 4], 0, null)]);
  expect(nothing.pendingCommand).toBeNull();
  expect(nothing.hint).toBe("inquiry.clickGeometry");
});

test("divide waits for the number of parts, then divides", () => {
  const asked = run("divide", [click([5, 0], 0, 9)]);
  expect(asked.pendingCommand).toBeNull();
  expect(asked.asking).toEqual({ kind: "divide", target: { handle: 9, chain: [] } });
  expect(asked.hint).toBe("inquiry.askCount");
  const done = toolReducer(asked, { type: "answer", value: 5 });
  expect(done.pendingCommand).toEqual({ type: "divide_along", handle: 9, chain: [], count: 5 });
  expect(done.asking).toBeNull();
  // Nothing typed yet, or a bad number: still waiting.
  expect(toolReducer(asked, { type: "answer", value: 0 }).asking).not.toBeNull();
  expect(toolReducer(asked, { type: "escape" }).asking).toBeNull();
  // A click on nothing lets go of the entity.
  expect(toolReducer(asked, click([50, 50], 1000, null)).asking).toBeNull();
});

test("measure along waits for the step, given in drawing units", () => {
  const asked = run("measureAlong", [click([5, 0], 0, 9)]);
  expect(asked.hint).toBe("inquiry.askStep");
  const done = toolReducer(asked, { type: "answer", value: 2.5 });
  expect(done.pendingCommand).toEqual({ type: "measure_along", handle: 9, chain: [], step: 2.5 });
});

test("ID point names the point clicked and the snap that found it", () => {
  const onEnd: ToolEvent = {
    type: "click",
    world: [1, 2],
    screen: [10, 20],
    time: 0,
    picked: null,
    info: null,
    snapped: true,
    snapKind: "end",
  };
  const snapped = run("idPoint", [onEnd]);
  expect(snapped.pendingCommand).toEqual({ type: "id_point", p: [1, 2], kind: "end" });
  const loose = run("idPoint", [click([1.25, 2], 0)]);
  expect(loose.pendingCommand).toEqual({ type: "id_point", p: [1.25, 2], kind: null });
  expect(initialToolState("idPoint").hint).toBe("inquiry.clickPoint");
});

test("Z W takes two typed corners and zooms to the window between them", () => {
  const line = (text: string, expect: ParseState["expect"]) =>
    parse(text, { expect, last: null, aliases: mergeAliases() });
  expect(line("Z W", "command")).toEqual({ type: "tool", tool: "zoomWindow" });
  const corner = (text: string, time: number): ToolEvent => {
    const a = line(text, "point");
    if (a.type !== "point") throw new Error(`not a point: ${text}`);
    const screen: Point = [Number.NaN, Number.NaN];
    return { type: "click", world: a.p, screen, time, picked: null, info: null, snapped: true };
  };

  const first = run("zoomWindow", [corner("12,9", 0)]);
  expect(first.corner).toEqual([12, 9]);
  expect(first.hint).toBe("zoom.otherCorner");
  expect(first.pendingZoom).toBeNull();

  const both = toolReducer(first, corner("0,0", 10));
  expect(both.pendingZoom).toEqual([
    [0, 0],
    [12, 9],
  ]);
  expect(both.corner).toBeNull();
  expect(both.hint).toBe("zoom.firstCorner");
  // The zoom is handed over once.
  expect(toolReducer(both, { type: "escape" }).pendingZoom).toBeNull();
});

test("a drag zoom after one typed corner drops that corner", () => {
  const typed = (world: Point, time: number): ToolEvent => ({
    type: "click",
    world,
    screen: [Number.NaN, Number.NaN],
    time,
    picked: null,
    info: null,
    snapped: true,
  });
  // Z W, then "0,0"; the viewport's drag zoom sends escape (`onWindowZoomed`).
  const dragged = run("zoomWindow", [typed([0, 0], 0), { type: "escape" }]);
  expect(dragged.corner).toBeNull();
  expect(dragged.hint).toBe("zoom.firstCorner");
  // The next point is a new first corner, not the second of a window from 0,0.
  const next = toolReducer(dragged, typed([12, 9], 10));
  expect(next.pendingZoom).toBeNull();
  expect(next.corner).toEqual([12, 9]);
});

test("draw tools run each finished draw and end a polyline on a double click", () => {
  const line = run("line", [click([0, 0], 0), click([3, 4], 1000)]);
  expect(line.pendingCommand).toEqual({ type: "draw", kind: "line", a: [0, 0], b: [3, 4] });
  expect(line.hint).toBe("draw.prompt.lineNext");

  // The second click of a double click ends the polyline without a doubled point.
  const pline = run("polyline", [click([0, 0], 0), click([2, 0], 1000), click([2, 2], 2000), click([2, 2], 2100)]);
  expect(pline.pendingCommand).toEqual({
    type: "draw",
    kind: "polyline",
    points: [
      [0, 0],
      [2, 0],
      [2, 2],
    ],
    bulges: [0, 0, 0],
    closed: false,
  });

  // A typed option and a typed radius reach the tool.
  const circle = run("circle", [click([1, 1], 0), { type: "draw", input: { type: "number", value: 2 } }]);
  expect(circle.pendingCommand).toEqual({ type: "draw", kind: "circle", centre: [1, 1], radius: 2 });
  const closed = run("polyline", [
    click([0, 0], 0),
    click([2, 0], 1000),
    click([2, 2], 2000),
    { type: "draw", input: { type: "option", option: "C" } },
  ]);
  expect(closed.pendingCommand).toMatchObject({ type: "draw", kind: "polyline", closed: true });

  // Ortho squares the next point to the last one, as for a distance.
  const ortho = run("line", [
    click([0, 0], 0),
    { ...click([5, 1], 1000), track: { ortho: true, polar: false, polarStepDeg: 45 } } as ToolEvent,
  ]);
  expect(ortho.pendingCommand).toEqual({ type: "draw", kind: "line", a: [0, 0], b: [5, 0] });

  // Esc keeps the polyline drawn so far, as AutoCAD does; a lone point is dropped.
  const kept = toolReducer(run("polyline", [click([0, 0], 0), click([3, 0], 1000)]), {
    type: "escape",
  });
  expect(kept.pendingCommand).toMatchObject({ type: "draw", kind: "polyline", closed: false });
  expect(kept.draw?.points).toEqual([]);
  // A reset (another drawing or sheet) draws nothing, even with a polyline under way.
  const reset = toolReducer(run("polyline", [click([0, 0], 0), click([3, 0], 1000)]), {
    type: "cancel",
  });
  expect(reset.pendingCommand).toBeNull();
  expect(reset.draw?.points).toEqual([]);
  const dropped = toolReducer(run("polyline", [click([0, 0], 0)]), { type: "escape" });
  expect(dropped.draw?.points).toEqual([]);
  expect(dropped.pendingCommand).toBeNull();

  // The second click of a double click places no second point or hatch.
  const points = run("point", [click([1, 1], 0), click([1, 1], 100)]);
  expect(points.pendingCommand).toBeNull();
  expect(run("point", [click([1, 1], 0), click([1, 1], 1000)]).pendingCommand).toMatchObject({
    kind: "point",
  });
  const hatch = run("hatch", [click([1, 1], 0)]);
  expect(hatch.draw?.seek).toEqual([1, 1]);
  expect(toolReducer(hatch, click([1, 1], 100)).draw?.seek).toEqual([1, 1]);
});

const targets = (handles: number[]) => handles.map((handle) => ({ handle, chain: [] }));

test("the select tool's selection set: Shift adds a click, a box replaces or adds, Esc clears", () => {
  const shift = (handle: number, time: number): ToolEvent => ({ ...click([handle, 0], time, handle), shift: true } as ToolEvent);
  let s = run("select", [shift(5, 0), shift(6, 1000), shift(5, 2000)]);
  expect(s.selection).toEqual([5, 6]);
  // A Shift click measures nothing.
  expect(s.selected).toBeNull();
  expect(s.pendingCommand).toBeNull();
  s = toolReducer(s, { type: "selected", handles: [7, 8], add: false });
  expect(s.selection).toEqual([7, 8]);
  s = toolReducer(s, { type: "selected", handles: [8, 9], add: true });
  expect(s.selection).toEqual([7, 8, 9]);
  // Another tool keeps it; Esc and resets clear it.
  expect(toolReducer(s, { type: "tool", tool: "distance" }).selection).toEqual([7, 8, 9]);
  expect(toolReducer(s, { type: "escape" }).selection).toEqual([]);
  expect(toolReducer(s, { type: "cancel" }).selection).toEqual([]);
  // A box with another tool picks nothing.
  expect(toolReducer(run("distance", []), { type: "selected", handles: [1], add: false }).selection).toEqual([]);
});

test("clicking a grouped entity selects its group; Shift adds the group", () => {
  const grouped = (handle: number, time: number, shift = false): ToolEvent =>
    ({
      ...click([handle, 0], time, handle),
      group: { handles: [4, 5, 6], kinds: ["Line", "Dimension", "Line"] },
      shift,
    }) as ToolEvent;
  let s = run("select", [grouped(5, 0)]);
  expect(s.selection).toEqual([4, 5, 6]);
  // Every member's kind is known, so a modify tool leaves the dimension out.
  expect(s.kinds).toEqual({ 4: "Line", 5: "Dimension", 6: "Line" });
  expect(toolReducer(s, { type: "tool", tool: "rotate" }).modify?.handles).toEqual([4, 6]);
  // The palette still shows the entity clicked.
  expect(s.selected?.picked.top).toBe(5);
  s = run("select", [click([9, 0], 0, 9), grouped(4, 1000, true)]);
  expect(s.selection).toEqual([9, 4, 5, 6]);
  // Without a group (Ctrl held, or group selection off) a click takes the entity alone.
  expect(run("select", [click([5, 0], 0, 5)]).selection).toEqual([5]);
});

test("a modify tool starts on the selection set, or on what the select tool picked (whole)", () => {
  const set = toolReducer(run("select", []), { type: "selected", handles: [3], add: false });
  const move = toolReducer(set, { type: "tool", tool: "move" });
  expect(move.modify?.phase).toBe("points");
  expect(move.modify?.handles).toEqual([3]);
  expect(move.hint).toBe("modify.prompt.base");
  // A line inside a block reference: the reference is moved.
  const inside: Picked = { top: 40, entity: 41, chain: [40], xref: null };
  const picked = run("select", [click([0, 0], 0, inside)]);
  expect(toolReducer(picked, { type: "tool", tool: "rotate" }).modify?.handles).toEqual([40]);
  // Erase with a selection erases at once.
  expect(toolReducer(set, { type: "tool", tool: "erase" }).pendingCommand).toEqual({
    type: "delete_entities",
    targets: targets([3]),
  });
});

test("a modify tool picks by clicks and boxes, then takes tracked points and options", () => {
  let s = run("move", [click([0, 0], 0, 5), click([1, 0], 1000, 6), click([2, 0], 2000)]);
  expect(s.modify?.handles).toEqual([5, 6]);
  expect(s.hintArgs).toEqual({ n: "2" });
  s = toolReducer(s, { type: "selected", handles: [7], add: false });
  expect(s.modify?.handles).toEqual([5, 6, 7]);
  s = toolReducer(s, { type: "finish" });
  s = toolReducer(s, click([1, 1], 3000));
  // Ortho squares the second point to the base.
  s = toolReducer(s, { ...click([4, 2], 4000), track: { ortho: true, polar: false, polarStepDeg: 45 } } as ToolEvent);
  expect(s.pendingCommand).toEqual({ type: "modify", kind: "move", targets: targets([5, 6, 7]), delta: [3, 0] });
  expect(s.modify?.phase).toBe("select");
  expect(s.modify?.handles).toEqual([]);

  // Typed options and numbers reach the tool as they reach a draw tool.
  const rotate = run("rotate", [click([0, 0], 0, 5), { type: "finish" }, click([1, 1], 1000)]);
  const copied = toolReducer(toolReducer(rotate, { type: "draw", input: { type: "option", option: "C" } }), {
    type: "draw",
    input: { type: "number", value: 45 },
  });
  expect(copied.pendingCommand).toEqual({
    type: "modify",
    kind: "rotate",
    targets: targets([5]),
    base: [1, 1],
    angle: 45,
    copy: true,
  });
});

test("stretch's window: two clicks, then what it touches comes back", () => {
  const s = run("stretch", [click([4, 4], 0), click([0, 0], 1000)]);
  expect(s.modify?.seek).toEqual([
    [0, 0],
    [4, 4],
  ]);
  const found = toolReducer(s, { type: "modify", input: { type: "picked", handles: [2] } });
  expect(found.modify?.phase).toBe("points");
  expect(found.modify?.handles).toEqual([2]);
});

test("a plain click replaces the selection set, Shift adds (PICKADD); nothing clears it", () => {
  const shift = (handle: number, time: number): ToolEvent => ({ ...click([handle, 0], time, handle), shift: true } as ToolEvent);
  const s = run("select", [shift(5, 0), shift(6, 1000), click([7, 0], 2000, 7)]);
  expect(s.selection).toEqual([7]);
  expect(toolReducer(s, click([0, 9], 3000)).selection).toEqual([]);
  // What is highlighted as the selection leaves out the select tool's own pick.
  const inside: Picked = { top: 40, entity: 41, chain: [40], xref: null };
  expect(run("select", [click([0, 0], 0, inside)]).selection).toEqual([40]);
});

test("the selection set keeps only what is still drawn (after an undo or a redo)", () => {
  const set = toolReducer(run("select", []), { type: "selected", handles: [7, 8, 9], add: false });
  const kept = toolReducer(set, { type: "prune", present: (h) => h !== 8 });
  expect(kept.selection).toEqual([7, 9]);
  const move = toolReducer(set, { type: "tool", tool: "move" });
  expect(toolReducer(move, { type: "prune", present: (h) => h === 9 }).modify?.handles).toEqual([9]);
});

test("the kinds picked go with the selection set, so a rotate leaves dimensions out", () => {
  const set = toolReducer(run("select", []), {
    type: "selected",
    handles: [1, 2],
    add: false,
    kinds: ["Line", "Dimension"],
  });
  const rotate = toolReducer(set, { type: "tool", tool: "rotate" });
  expect(rotate.modify?.handles).toEqual([1]);
  expect(rotate.modify?.skipped).toBe(1);
  // A click in a modify tool's selection gives its kind too.
  const picked: Picked = { top: 3, entity: 3, chain: [], xref: null, kind: "Table" };
  const scale = run("scale", [click([0, 0], 0, picked)]);
  expect(scale.modify?.handles).toEqual([]);
  expect(scale.modify?.skipped).toBe(1);
});

test("an edit tool picks with the click point, swaps trim for extend on Shift, and snaps its points", () => {
  // Trim: Enter (everything cuts), then a click on a piece and a Shift+click.
  let s = run("trim", [{ type: "finish" }, click([3, 4], 0, 5)]);
  expect(s.pendingCommand).toEqual({ type: "edit", kind: "trim", cutters: [], target: targets([5])[0], at_point: [3, 4] });
  s = toolReducer(s, { ...click([6, 0], 1000, 6), shift: true } as ToolEvent);
  expect(s.pendingCommand).toEqual({ type: "edit", kind: "extend", boundaries: [], target: targets([6])[0], at_end: [6, 0] });
  // Esc ends the clicking: back to the edges.
  expect(toolReducer(s, { type: "escape" }).edit?.phase).toBe("edges");

  // Offset: the side is a point, placed as snapped (no entity picked).
  let o = toolReducer(initialToolState("offset"), { type: "draw", input: { type: "number", value: 0.5 } });
  o = toolReducer(o, click([0, 0], 0, 3));
  expect(o.edit?.phase).toBe("point");
  o = toolReducer(o, { ...click([1, -2], 1000), snapped: true } as ToolEvent);
  expect(o.pendingCommand).toEqual({
    type: "edit",
    kind: "offset",
    target: targets([3])[0],
    distance: 0.5,
    side_point: [1, -2],
  });

  // A selection made with the select tool goes along: overkill runs on it at once.
  const picked = toolReducer(initialToolState("select"), { type: "selected", handles: [8, 9], add: false });
  const overkill = toolReducer(picked, { type: "tool", tool: "overkill" });
  expect(overkill.pendingCommand).toEqual({ type: "edit", kind: "overkill", targets: targets([8, 9]), tol: 1e-6 });
});

test("the text tools place a text per click and take H and R from the command line", () => {
  let s = run("text", [click([1, 2], 0)]);
  expect(s.text?.request).toEqual({ kind: "text", at: [1, 2], rotation: 0, height: null });
  expect(s.hint).toBe("text.prompt.start");
  const events: ToolEvent[] = [
    { type: "draw", input: { type: "option", option: "R" } },
    { type: "draw", input: { type: "number", value: 45 } },
    click([5, 5], 2000),
  ];
  s = events.reduce(toolReducer, s);
  expect(s.text?.request).toEqual({ kind: "text", at: [5, 5], rotation: 45, height: null });
  // The next event drops the request: it is handed on once.
  expect(toolReducer(s, { type: "finish" }).text?.request).toBeNull();

  const box = run("mtext", [click([0, 4], 0), click([6, 0], 2000)]);
  expect(box.text?.request).toEqual({ kind: "mtext", at: [0, 4], width: 6, height: null });
});

test("a double click on a text edits it; the edit tool asks on each click", () => {
  const label = { ...onSheet(7), kind: "Text" };
  const twice = run("select", [click([0, 0], 0, label), click([0, 0], 100, label)]);
  expect(twice.pendingEdit).toBe(7);
  expect(twice.pendingCommand).toBeNull();
  // A line is measured as before.
  const line = { ...onSheet(8), kind: "Line" };
  const measured = run("select", [click([0, 0], 0, line), click([0, 0], 100, line)]);
  expect(measured.pendingEdit).toBeNull();
  expect(measured.pendingCommand).toEqual({ type: "measure_entity", handle: 8, chain: [] });

  // Inside a block, the block reference is what is edited (its attributes).
  const inside: Picked = { top: 9, entity: 3, chain: [9], xref: null, kind: "Insert" };
  const edit = run("textEdit", [click([0, 0], 0, inside)]);
  expect(edit.hint).toBe("text.prompt.edit");
  expect(edit.pendingEdit).toBe(9);
  expect(toolReducer(edit, click([5, 5], 2000, null)).pendingEdit).toBeNull();
});

test("an annotate tool places snapped points, picks dimensions and asks for text", () => {
  const linear = run("dimLinear", [click([0, 0], 0), click([10, 0], 1000), click([5, 2], 2000)]);
  expect(linear.pendingCommand).toEqual({
    type: "dimension",
    kind: "linear",
    p1: [0, 0],
    p2: [10, 0],
    line_point: [5, 2],
    rotation: null,
  });
  expect(linear.hint).toBe("dim.prompt.dimLinear0");

  const dimension: Picked = { ...onSheet(7), kind: "Dimension" };
  const edit = run("dimEdit", [click([1, 1], 0, dimension), { type: "draw", input: { type: "option", option: "T" } }]);
  expect(edit.dim?.request).toEqual({ kind: "override", handle: 7 });

  const qdim = toolReducer(initialToolState("qdim", [3]), { type: "selected", handles: [4], add: true });
  expect(qdim.dim?.handles).toEqual([3, 4]);
  expect(qdim.selection).toEqual([]);
});

test("a double click on a table, or TABLEDIT's click, opens its editor at the cell clicked", () => {
  const table = { ...onSheet(7), kind: "Table" };
  const twice = run("select", [click([1, 2], 0, table), click([1, 2], 100, table)]);
  expect(twice.pendingEdit).toBe(7);
  expect(twice.editAt).toEqual([1, 2]);
  expect(twice.pendingCommand).toBeNull();

  const edit = run("tableEdit", [click([3, 4], 0, table)]);
  expect(edit.hint).toBe("table.prompt.edit");
  expect(edit.pendingEdit).toBe(7);
  expect(edit.editAt).toEqual([3, 4]);
  // Anything else is no table.
  const line = run("tableEdit", [click([3, 4], 0, { ...onSheet(8), kind: "Line" })]);
  expect(line.pendingEdit).toBeNull();
  expect(line.hint).toBe("table.prompt.notTable");
});

test("TABLE and FIELD hand their click to the app", () => {
  const table = run("table", [click([1, 2], 0)]);
  expect(table.text?.request).toEqual({ kind: "table", at: [1, 2] });
  const field = run("field", [click([3, 4], 0)]);
  expect(field.text?.request).toEqual({ kind: "field", at: [3, 4], height: null });
});

test("a double click on a block's attribute edits its attributes", () => {
  // An attribute is picked as its block reference, at the top of the sheet.
  const tag = { ...onSheet(7), kind: "Insert" };
  const twice = run("select", [click([1, 2], 0, tag), click([1, 2], 100, tag)]);
  expect(twice.pendingEdit).toBe(7);
  expect(twice.pendingCommand).toBeNull();
});

test("REFEDIT edits the block of the reference clicked; anything else is no block", () => {
  const leaf: Picked = { top: 9, entity: 3, chain: [9], xref: null, kind: "Insert" };
  const edit = run("refedit", [click([0, 0], 0, leaf)]);
  expect(edit.pendingCommand).toEqual({ type: "edit_block_in_place", target: { handle: 9, chain: [] } });
  const line = run("refedit", [click([0, 0], 0, { ...onSheet(8), kind: "Line" })]);
  expect(line.pendingCommand).toBeNull();
  expect(line.hint).toBe("block.prompt.notBlock");
  // An xref is no block to edit.
  const xref: Picked = { top: 5, entity: 4, chain: [5], xref: "plan", kind: "Insert" };
  expect(run("refedit", [click([0, 0], 0, xref)]).pendingCommand).toBeNull();
});

test("INSERT and ATTDEF hand their click to the app", () => {
  const insert = run("insertBlock", [click([1, 2], 0)]);
  expect(insert.text?.request).toEqual({ kind: "insertBlock", at: [1, 2], rotation: 0, scale: 1 });
  const attdef = run("attdef", [click([3, 4], 0)]);
  expect(attdef.text?.request).toEqual({ kind: "attdef", at: [3, 4], height: null });
});

test("paste tools paste at each click, as a block for PASTEBLOCK", () => {
  const s = run("pasteclip", [click([3, 4], 0)]);
  expect(s.pendingCommand).toEqual({ type: "paste_from_clipboard", at: [3, 4], as_block: false });
  expect(s.hint).toBe("clip.prompt.paste");
  const b = run("pasteblock", [click([1, 2], 0)]);
  expect(b.pendingCommand).toEqual({ type: "paste_from_clipboard", at: [1, 2], as_block: true });
});

test("MVIEW takes two corners", () => {
  const first = run("mview", [click([0, 0], 0)]);
  expect(first.pendingCommand).toBeNull();
  expect(first.hint).toBe("vport.prompt.second");
  const done = toolReducer(first, click([100, 80], 1000));
  expect(done.pendingCommand).toEqual({ type: "mview", corner1: [0, 0], corner2: [100, 80] });
  expect(done.hint).toBe("vport.prompt.first");
});

test("UCS takes an origin and a point on its x axis; UCS OB an entity", () => {
  const first = run("ucs", [click([10, 5], 0)]);
  expect(first.hint).toBe("ucs.prompt.xAxis");
  // The same point again gives no axis.
  expect(toolReducer(first, click([10, 5], 1000)).pendingCommand).toBeNull();
  const done = toolReducer(first, click([10, 7], 2000));
  expect(done.pendingCommand).toEqual({ type: "ucs", origin: [10, 5], x_axis: [0, 2] });
  const ob = run("ucsObject", [click([1, 1], 0, 9)]);
  expect(ob.pendingCommand).toEqual({ type: "ucs_object", target: { handle: 9, chain: [] } });
  expect(run("ucsObject", [click([1, 1], 0)]).pendingCommand).toBeNull();
});

test("a parametric tool picks parts where they are clicked, places a dimension, and takes the selection", () => {
  // The rectangle's bottom edge picked near (2, 0.1): the core takes the nearest part.
  const horizontal = run("gcHorizontal", [click([2, 0.1], 0, 7)]);
  expect(horizontal.pendingCommand).toEqual({
    type: "add_constraint",
    kind: "horizontal",
    targets: [{ handle: 7, chain: [] }],
    points: [[2, 0.1]],
  });
  // A click on nothing picks nothing.
  expect(run("gcParallel", [click([2, 2], 0, null)]).param?.picks).toEqual([]);

  const linear = run("dcLinear", [click([2, 0], 0, 7), click([2, -1], 1000, null)]);
  expect(linear.pendingCommand).toMatchObject({ type: "add_dim_constraint", kind: "linear", line_point: [2, -1] });

  // AUTOCONSTRAIN runs at once on what was selected; DELCONSTRAINT boxes until Enter.
  expect(initialToolState("autoConstrain", [3]).pendingCommand).toEqual({
    type: "auto_constrain",
    targets: [{ handle: 3, chain: [] }],
  });
  const del = toolReducer(initialToolState("delConstraint"), { type: "selected", handles: [4, 5], add: false });
  expect(del.param?.handles).toEqual([4, 5]);
  expect(toolReducer(del, { type: "finish" }).pendingCommand).toEqual({
    type: "delete_constraints_on",
    targets: [
      { handle: 4, chain: [] },
      { handle: 5, chain: [] },
    ],
  });
  // An undo that took an entity away drops it from the picks.
  const pruned = toolReducer(run("gcEqual", [click([0, 0], 0, 6)]), { type: "prune", present: (h) => h !== 6 });
  expect(pruned.param?.picks).toEqual([]);
});

test("the area tool marks a polygon closed with Enter or a double click, never with two corners", () => {
  const two = run("mizanArea", [click([0, 0], 0), click([4, 0], 1000), { type: "finish" }]);
  expect(two.pendingRegion).toBeNull();
  expect(two.hint).toBe("ai.area.prompt");
  const polygon = run("mizanArea", [click([0, 0], 0), click([4, 0], 1000), click([4, 3], 2000), { type: "finish" }]);
  expect(polygon.pendingRegion).toEqual({
    kind: "polygon",
    points: [
      [0, 0],
      [4, 0],
      [4, 3],
    ],
  });
  expect(polygon.points).toEqual([]);
  const double = run("mizanArea", [click([0, 0], 0), click([4, 0], 1000), click([4, 3], 2000), click([4, 3], 2200)]);
  expect(double.pendingRegion?.points).toHaveLength(3);
  // The region is handed on once.
  expect(toolReducer(polygon, { type: "escape" }).pendingRegion).toBeNull();
});

test("a box dragged with the area tool marks it; other tools ignore it", () => {
  const box = run("mizanArea", [{ type: "region", region: { kind: "box", points: [[0, 0], [5, 2]] } }]);
  expect(box.pendingRegion).toEqual({ kind: "box", points: [[0, 0], [5, 2]] });
  expect(run("select", [{ type: "region", region: { kind: "box", points: [[0, 0], [5, 2]] } }]).pendingRegion).toBeNull();
});

test("Mizan's selection becomes the select tool's selection set, from any tool", () => {
  const fromLine = run("line", [{ type: "aiSelect", handles: [7, 9] }]);
  expect(fromLine.tool).toBe("select");
  expect(fromLine.selection).toEqual([7, 9]);
  expect(fromLine.hint).toBe("");
  const replaced = run("select", [click([0, 0], 0, 3), { type: "aiSelect", handles: [5] }]);
  expect(replaced.selection).toEqual([5]);
  expect(replaced.selected).toBeNull();
});
