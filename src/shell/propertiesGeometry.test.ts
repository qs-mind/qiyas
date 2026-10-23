import { expect, test } from "vitest";
import type { EntityInfo } from "../api";
import { geometryRows, selectionRows, summarise } from "./propertiesGeometry";

const t = (key: string) => ({ "qty.m": "m", "qty.m2": "m²", "props.yes": "Yes", "props.no": "No" })[key] ?? key;

const base: EntityInfo = {
  handle: 1,
  kind: "Line",
  layer: "A-WALL",
  layer_short: "A-WALL",
  length: null,
  area: null,
  block_name: null,
  text: null,
  inside_block: null,
  chain: [],
  block_path: [],
  color: "ByLayer",
  color_rgb: [255, 255, 255],
  linetype: "ByLayer",
  lineweight: -1,
  radius: null,
  attributes: [],
  height: null,
  style: null,
};

/** [label, text] of each Geometry row, in millimetre drawings shown in metres. */
const shown = (e: EntityInfo, units: "millimetres" | "metres" = "millimetres") =>
  geometryRows(e, units, 3, t).map((r) => [r.label.replace("props.", ""), r.text]);

test("a line: ends, delta, length and angle in metres", () => {
  const line: EntityInfo = { ...base, start: [1000, 2000], end: [4000, 6000], length: 5000 };
  expect(shown(line)).toEqual([
    ["startX", "1.000 m"],
    ["startY", "2.000 m"],
    ["endX", "4.000 m"],
    ["endY", "6.000 m"],
    ["deltaX", "3.000 m"],
    ["deltaY", "4.000 m"],
    ["length", "5.000 m"],
    ["angle", "53.130°"],
  ]);
  // Drawn downwards to the left: the angle runs 0 up to 360.
  const back: EntityInfo = { ...line, start: [0, 0], end: [-1000, -1000], length: 1414.2136 };
  expect(shown(back).at(-1)).toEqual(["angle", "225.000°"]);
});

test("a polyline: vertices, closed, perimeter, area and global width", () => {
  const room: EntityInfo = {
    ...base,
    kind: "LwPolyline",
    vertex_count: 4,
    closed: true,
    length: 14000,
    area: 12_000_000,
    global_width: 0,
  };
  expect(shown(room)).toEqual([
    ["vertices", "4"],
    ["closed", "Yes"],
    ["perimeter", "14.000 m"],
    ["area", "12.000 m²"],
    ["globalWidth", "0.000 m"],
  ]);
  // Open: a length, no area.
  const open: EntityInfo = { ...room, closed: false, area: null };
  expect(shown(open).map((r) => r[0])).toEqual(["vertices", "closed", "length", "globalWidth"]);
  // A heavy polyline gives its vertices and, closed by its area, a perimeter.
  const heavy: EntityInfo = { ...base, kind: "Polyline2D", vertex_count: 5, length: 8000, area: 4_000_000 };
  expect(shown(heavy)).toEqual([
    ["vertices", "5"],
    ["perimeter", "8.000 m"],
    ["area", "4.000 m²"],
  ]);
});

test("an arc: centre, radius, angles, total angle, length and chord", () => {
  const arc: EntityInfo = {
    ...base,
    kind: "Arc",
    centre: [20000, 0],
    radius: 2000,
    start_angle: 0,
    end_angle: 180,
    length: 6283.185307,
  };
  expect(shown(arc)).toEqual([
    ["centreX", "20.000 m"],
    ["centreY", "0.000 m"],
    ["radius", "2.000 m"],
    ["startAngle", "0.000°"],
    ["endAngle", "180.000°"],
    ["totalAngle", "180.000°"],
    ["chord", "4.000 m"],
    ["length", "6.283 m"],
  ]);
  // Past 0°: 270 to 90 is half a turn; ends that meet are a full turn with a zero chord.
  const wrapped = shown({ ...arc, start_angle: 270, end_angle: 90 });
  expect(wrapped).toContainEqual(["totalAngle", "180.000°"]);
  const full = shown({ ...arc, start_angle: 30, end_angle: 30 });
  expect(full).toContainEqual(["totalAngle", "360.000°"]);
  expect(full).toContainEqual(["chord", "0.000 m"]);
});

test("a circle: centre, radius, diameter, circumference and area", () => {
  const circle: EntityInfo = {
    ...base,
    kind: "Circle",
    centre: [1, 2],
    radius: 1,
    length: 2 * Math.PI,
    area: Math.PI,
  };
  expect(shown(circle, "metres")).toEqual([
    ["centreX", "1.000 m"],
    ["centreY", "2.000 m"],
    ["radius", "1.000 m"],
    ["diameter", "2.000 m"],
    ["circumference", "6.283 m"],
    ["area", "3.142 m²"],
  ]);
});

test("an ellipse and a spline: length, and area when closed", () => {
  const ellipse: EntityInfo = { ...base, kind: "Ellipse", centre: [0, 0], length: 10, area: 12 };
  expect(shown(ellipse, "metres").map((r) => r[0])).toEqual(["centreX", "centreY", "length", "area"]);
  const spline: EntityInfo = { ...base, kind: "Spline", length: 7 };
  expect(shown(spline, "metres")).toEqual([["length", "7.000 m"]]);
});

test("a hatch: pattern and area", () => {
  const hatch: EntityInfo = { ...base, kind: "Hatch", pattern: "ANSI31", area: 96 };
  expect(shown(hatch, "metres")).toEqual([
    ["pattern", "ANSI31"],
    ["area", "96.000 m²"],
  ]);
});

test("a text: insertion, height, rotation, contents and style", () => {
  const text: EntityInfo = {
    ...base,
    kind: "MText",
    position: [100, -200],
    height: 250,
    rotation: 90,
    text: "{\\fArial;A}",
    plain_text: "A",
    style: "STANDARD",
  };
  expect(shown(text)).toEqual([
    ["insertionX", "0.100 m"],
    ["insertionY", "-0.200 m"],
    ["height", "0.250 m"],
    ["rotation", "90.000°"],
    ["text", "A"],
    ["style", "STANDARD"],
  ]);
});

test("a block reference: name, insertion, scale and rotation", () => {
  const door: EntityInfo = { ...base, kind: "Insert", block_name: "DOOR", position: [1, 1], scale: -2, rotation: 45 };
  expect(shown(door, "metres")).toEqual([
    ["block", "DOOR"],
    ["insertionX", "1.000 m"],
    ["insertionY", "1.000 m"],
    ["scale", "-2"],
    ["rotation", "45.000°"],
  ]);
});

test("a dimension: the measured value, its text, height and style", () => {
  const dim: EntityInfo = { ...base, kind: "Dimension", length: 3600, text: "3600", height: 180, style: "ISO-25" };
  expect(shown(dim)).toEqual([
    ["measured", "3.600 m"],
    ["text", "3600"],
    ["height", "0.180 m"],
    ["style", "ISO-25"],
  ]);
  // An angular one measures no length: only its text.
  expect(shown({ ...dim, length: null, text: "45°" }).slice(0, 1)).toEqual([["text", "45°"]]);
});

test("a value copies without its unit; a row the core did not give has none", () => {
  const rows = geometryRows({ ...base, kind: "Circle", radius: 1500, length: 9424.78 }, "millimetres", 2, t);
  expect(rows.find((r) => r.key === "radius")).toMatchObject({ text: "1.50 m", copy: "1.50" });
  expect(rows.map((r) => r.key)).not.toContain("centreX");
  expect(rows.map((r) => r.key)).not.toContain("area");
  expect(geometryRows({ ...base, kind: "Table" }, "metres", 3, t)).toEqual([]);
});

test("several entities: count, kinds, summed length and area, and what they share", () => {
  const wall = (length: number, layer = "A-WALL"): EntityInfo => ({ ...base, layer, layer_short: layer, length });
  const room: EntityInfo = { ...base, kind: "LwPolyline", length: 14, area: 12 };
  const hatch: EntityInfo = { ...base, kind: "Hatch", area: 8, color: "ACI 1" };
  const dim: EntityInfo = { ...base, kind: "Dimension", length: 99 };
  const door: EntityInfo = { ...base, kind: "Insert", block_name: "DOOR" };
  const sum = summarise([wall(10), wall(5), room, hatch, dim, door]);
  expect(sum.count).toBe(6);
  expect(sum.kinds).toEqual([
    ["Line", 2],
    ["Dimension", 1],
    ["Hatch", 1],
    ["Insert", 1],
    ["LwPolyline", 1],
  ]);
  // A dimension reports what it says and a block reference is only counted: neither is summed.
  expect(sum.length).toBe(29);
  expect(sum.area).toBe(20);
  expect(sum.layer).toBe("A-WALL");
  expect(sum.color).toBeNull();
  expect(sum.linetype).toBe("ByLayer");
  expect(sum.lineweight).toBe(-1);
  expect(summarise([wall(1), wall(1, "S-COL")]).layer).toBeNull();
  const rows = selectionRows(sum, "metres", 2, t);
  expect(rows.map((r) => [r.key, r.text])).toEqual([
    ["count", "6"],
    ["kinds", "Line ×2, Dimension ×1, Fill ×1, Block ×1, Polyline ×1"],
    ["totalLength", "29.00 m"],
    ["totalArea", "20.00 m²"],
  ]);
  // Nothing measures: no total rows.
  const none = selectionRows(summarise([door, door]), "metres", 2, t);
  expect(none.map((r) => r.key)).toEqual(["count", "kinds"]);
});
