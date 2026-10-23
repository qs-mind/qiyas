import { entityKind } from "../i18n/entityKind";
/** The properties palette's Geometry and Selection rows, as data: what each kind of entity shows
 *  (like AutoCAD's Properties palette), in the drawing's units converted to metres with the
 *  drafting precision. Pure: the palette draws the rows and lets the editable ones become fields. */
import type { EntityInfo, Units } from "../api";
import { metresPerUnit } from "./AttachPdfDialog";

type Translate = (key: string, vars?: Record<string, string | number>) => string;

/** One read-only line. `key` names the value, so the palette can swap in the field that edits it;
 *  `label` is the translation key of its name; `text` is shown, `copy` is what a click copies (the
 *  number without its unit). `prose` reads in its own direction (a name or a text). */
export type GeoRow = { key: string; label: string; text: string; copy: string; prose?: boolean };

/** The picked entities' kinds that measure something the QS adds up: not a dimension (it reports
 *  what it says, not a length drawn) and not a block reference (counted, never summed). */
const SUMMED_KINDS = new Set(["Dimension", "Insert"]);

/** Formats numbers in the drawing's units as metres, square metres and degrees. */
export function formatter(units: Units, precision: number, t: Translate) {
  const m = metresPerUnit(units);
  const row = (key: string, label: string, v: number, unit: string, scale: number): GeoRow => {
    const n = (v * scale).toFixed(precision);
    return { key, label, text: `${n} ${unit}`, copy: n };
  };
  return {
    length: (key: string, label: string, v: number) => row(key, label, v, t("qty.m"), m),
    area: (key: string, label: string, v: number) => row(key, label, v, t("qty.m2"), m * m),
    angle: (key: string, label: string, degrees: number): GeoRow => {
      const n = degrees.toFixed(precision);
      return { key, label, text: `${n}°`, copy: n };
    },
  };
}

/** An angle in degrees, 0 up to 360. */
const turn = (degrees: number) => ((degrees % 360) + 360) % 360;

/** The Geometry section's rows for `e`, in the order AutoCAD lists them. A value the core did not
 *  give (null or absent) has no row. */
export function geometryRows(e: EntityInfo, units: Units, precision: number, t: Translate): GeoRow[] {
  const f = formatter(units, precision, t);
  const rows: GeoRow[] = [];
  const add = (r: GeoRow | null) => r && rows.push(r);
  const point = (key: string, labelX: string, labelY: string, p: [number, number] | null | undefined) => {
    if (!p) return;
    rows.push(f.length(`${key}X`, labelX, p[0]), f.length(`${key}Y`, labelY, p[1]));
  };
  const length = (label = "props.length") => e.length !== null && add(f.length("length", label, e.length));
  const area = () => e.area !== null && add(f.area("area", "props.area", e.area));
  const text = (label: string, value: string, key = "text") =>
    rows.push({ key, label, text: value, copy: value, prose: true });

  switch (e.kind) {
    case "Line": {
      point("start", "props.startX", "props.startY", e.start);
      point("end", "props.endX", "props.endY", e.end);
      if (e.start && e.end) {
        const [dx, dy] = [e.end[0] - e.start[0], e.end[1] - e.start[1]];
        rows.push(f.length("deltaX", "props.deltaX", dx), f.length("deltaY", "props.deltaY", dy));
        length();
        add(f.angle("angle", "props.angle", turn((Math.atan2(dy, dx) * 180) / Math.PI)));
      } else {
        length();
      }
      break;
    }
    case "Circle": {
      point("centre", "props.centreX", "props.centreY", e.centre);
      if (e.radius !== null) {
        add(f.length("radius", "props.radius", e.radius));
        add(f.length("diameter", "props.diameter", 2 * e.radius));
      }
      // The circumference is the core's own length of the circle.
      length("props.circumference");
      area();
      break;
    }
    case "Arc": {
      point("centre", "props.centreX", "props.centreY", e.centre);
      if (e.radius !== null) add(f.length("radius", "props.radius", e.radius));
      if (e.start_angle != null && e.end_angle != null) {
        add(f.angle("startAngle", "props.startAngle", e.start_angle));
        add(f.angle("endAngle", "props.endAngle", e.end_angle));
        // Ends that meet are a full turn (the core draws it so).
        const total = turn(e.end_angle - e.start_angle) || 360;
        add(f.angle("totalAngle", "props.totalAngle", total));
        if (e.radius !== null) {
          add(f.length("chord", "props.chord", 2 * e.radius * Math.sin((total * Math.PI) / 360)));
        }
      }
      length();
      break;
    }
    case "Ellipse": {
      point("centre", "props.centreX", "props.centreY", e.centre);
      length();
      area();
      break;
    }
    case "Hatch": {
      if (e.pattern) text("props.pattern", e.pattern, "pattern");
      area();
      break;
    }
    case "Text":
    case "MText": {
      point("position", "props.insertionX", "props.insertionY", e.position);
      if (e.height !== null) add(f.length("height", "props.height", e.height));
      if (e.rotation != null) add(f.angle("rotation", "props.rotation", e.rotation));
      const contents = e.plain_text ?? e.text;
      if (contents !== null) text("props.text", contents);
      if (e.style) text("props.style", e.style, "style");
      break;
    }
    case "Insert": {
      if (e.block_name) text("props.block", e.block_name, "block");
      point("position", "props.insertionX", "props.insertionY", e.position);
      if (e.scale != null) {
        const n = String(Number(e.scale.toFixed(6)));
        rows.push({ key: "scale", label: "props.scale", text: n, copy: n });
      }
      if (e.rotation != null) add(f.angle("rotation", "props.rotation", e.rotation));
      break;
    }
    case "Dimension": {
      length("props.measured");
      if (e.text !== null) text("props.text", e.text);
      if (e.height !== null) add(f.length("height", "props.height", e.height));
      if (e.style) text("props.style", e.style, "style");
      break;
    }
    default: {
      if (e.vertex_count != null) {
        rows.push({ key: "vertices", label: "props.vertices", text: String(e.vertex_count), copy: String(e.vertex_count) });
      }
      if (e.closed != null) {
        const yes = t(e.closed ? "props.yes" : "props.no");
        rows.push({ key: "closed", label: "props.closed", text: yes, copy: yes });
      }
      point("centre", "props.centreX", "props.centreY", e.centre);
      const perimeter = e.closed || (e.vertex_count != null && e.area !== null);
      length(perimeter ? "props.perimeter" : "props.length");
      area();
      if (e.global_width != null) add(f.length("width", "props.globalWidth", e.global_width));
      if (e.radius !== null) add(f.length("radius", "props.radius", e.radius));
      if (e.block_name) text("props.block", e.block_name, "block");
      if (e.text !== null) text("props.text", e.plain_text ?? e.text);
      if (e.height !== null) add(f.length("height", "props.height", e.height));
      if (e.style) text("props.style", e.style, "style");
      if (e.viewport_scale != null) {
        const n = String(Number(e.viewport_scale.toFixed(6)));
        rows.push({ key: "viewportScale", label: "props.viewportScale", text: n, copy: n });
      }
      if (e.viewport_centre) {
        const [x, y] = e.viewport_centre.map((v) => String(Number(v.toFixed(6))));
        rows.push({ key: "viewportCentre", label: "props.viewportCentre", text: `${x}, ${y}`, copy: `${x}, ${y}` });
      }
    }
  }
  return rows;
}

/** What several picked entities have in common, for the Selection and General sections. */
export type SelectionSummary = {
  count: number;
  /** Each kind and how many of it, most first. */
  kinds: [string, number][];
  /** The sum of the lengths and of the areas, in drawing units; null when no entity has one. */
  length: number | null;
  area: number | null;
  /** The value they all share, or null where they differ. */
  layer: string | null;
  layerShort: string | null;
  color: string | null;
  colorRgb: [number, number, number] | null;
  linetype: string | null;
  lineweight: number | null;
};

const same = <T,>(values: T[]): T | null => (values.every((v) => v === values[0]) ? values[0] : null);

export function summarise(infos: EntityInfo[]): SelectionSummary {
  const counts = new Map<string, number>();
  for (const e of infos) counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1);
  const counted = infos.filter((e) => !SUMMED_KINDS.has(e.kind));
  const lengths = counted.filter((e) => e.length !== null);
  const areas = counted.filter((e) => e.area !== null);
  return {
    count: infos.length,
    kinds: [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])),
    length: lengths.length > 0 ? lengths.reduce((n, e) => n + (e.length as number), 0) : null,
    area: areas.length > 0 ? areas.reduce((n, e) => n + (e.area as number), 0) : null,
    layer: same(infos.map((e) => e.layer)),
    layerShort: same(infos.map((e) => e.layer_short)),
    color: same(infos.map((e) => e.color)),
    colorRgb: same(infos.map((e) => e.color_rgb.join(",")))?.split(",").map(Number) as [number, number, number] | null,
    linetype: same(infos.map((e) => e.linetype)),
    lineweight: same(infos.map((e) => e.lineweight)),
  };
}

/** The Selection section's rows: how many, of which kinds, and the sums of length and area. */
export function selectionRows(s: SelectionSummary, units: Units, precision: number, t: Translate): GeoRow[] {
  const f = formatter(units, precision, t);
  const rows: GeoRow[] = [{ key: "count", label: "props.count", text: String(s.count), copy: String(s.count) }];
  const kinds = s.kinds.map(([k, n]) => `${entityKind(k)} ×${n}`).join(", ");
  rows.push({ key: "kinds", label: "props.kinds", text: kinds, copy: kinds, prose: true });
  if (s.length !== null) rows.push(f.length("totalLength", "props.totalLength", s.length));
  if (s.area !== null) rows.push(f.area("totalArea", "props.totalArea", s.area));
  return rows;
}
