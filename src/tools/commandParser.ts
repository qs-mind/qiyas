/** The command line's grammar: one typed line to one action. Pure; the app maps each action to
 *  what the ribbon and the keyboard already do.
 *
 *  Command names and aliases match in any case. Numbers take Arabic-Indic digits and the Arabic
 *  decimal separator (٫); points take `,` or `،` between x and y. Points are in drawing units:
 *  `x,y` absolute, `@dx,dy` from the last point, `@distance<angle` (degrees) from the last point. */
import type { DrawSpec, Ucs } from "../api";
import type { Point } from "../viewport/camera";
import type { Tool } from "../viewport/Viewport";
import { COMMANDS, type CommandName } from "./aliases";
import type { DraftingToggle } from "./shortcuts";
import { dirToWorld, toWorld, ucsDegrees, WORLD } from "./ucs";

/** What the command line is waiting for: a command, a point (which a command may replace), or the
 *  number of parts or the step (metres) of a divide or measure along. */
export type Expect = "command" | "point" | "count" | "step";

export type ParseState = {
  expect: Expect;
  /** The last point placed or typed, for `@`; null when there is none yet. */
  last: Point | null;
  /** Alias to command, upper-case aliases (`mergeAliases`). */
  aliases: Record<string, string>;
  /** Where a point is expected, the option words a draw tool takes now (upper case). */
  options?: string[];
  /** Where a point is expected, a lone number is a distance (a radius, an axis, an arc length,
   *  above 0) or, with "angle", an angle in degrees (any sign). */
  numberToo?: boolean | "distance" | "angle";
  /** The user coordinate system typed points are in (the world's when left out). */
  ucs?: Ucs;
};

/** One of the app's own actions, run from the command line. */
export type Run =
  | "new"
  | "open"
  | "save"
  | "saveAs"
  | "export"
  | "importBoq"
  | "compare"
  | "undo"
  | "redo"
  | "attachPdf"
  | "attachImage"
  | "xrefs"
  | "calibrate"
  | "extract"
  | "find"
  | "dimStyle"
  | "updateFields"
  | "insertBlock"
  | "writeBlock"
  | "refClose"
  | "attributeManager"
  | "layers"
  | "layerStates"
  | "layerTranslate"
  | "vpLayer"
  | "properties"
  /** PLOT (and PRINT) opens the Plot dialog; QUICKPRINT prints the view; PUBLISH prints every
   *  layout; STYLESMANAGER edits a plot style. */
  | "plot"
  | "quickPrint"
  | "printAll"
  | "plotStyles"
  /** VPORTS: two views side by side and back (`vports2` two, `vports1` one). */
  | "vports"
  | "vports1"
  | "vports2"
  /** UCS W: back to the world coordinate system. */
  | "ucsWorld"
  | "sheetSet"
  /** SHORTCUTS: the keyboard shortcuts cheat sheet (Ctrl+/). */
  | "shortcuts"
  | "recover"
  /** SCRIPT: runs a `.scr` command script. */
  | "script"
  /** The Manage tab's dialogs: OPTIONS, CUI (customize the ribbon), ALIASEDIT, PURGE, AUDIT. */
  | "options"
  | "customize"
  | "aliasEdit"
  | "purge"
  | "audit"
  /** PARAMETERS: the parameters manager. */
  | "parameters";

export type Action =
  /** An empty line where a command is expected: run the last command again. */
  | { type: "repeat" }
  /** An empty line where a point is expected: finish the measure. */
  | { type: "finish" }
  | { type: "cancel" }
  | { type: "point"; p: Point }
  /** The number of parts, or the step in metres. */
  | { type: "number"; value: number }
  /** A draw tool's option word (upper case), or a distance typed for it (drawing units). */
  | { type: "option"; option: string }
  | { type: "distance"; value: number }
  /** A draw typed whole (polygon, donut, ray, xline, solid, trace, mline, sketch). */
  | { type: "draw"; spec: DrawSpec }
  /** Starts a tool; `preset` is the divide's number of parts or the measure along's step (m),
   *  used once the entity is clicked. */
  | { type: "tool"; tool: Tool; preset?: number }
  | { type: "count"; block: string }
  | { type: "layers"; hide: boolean; names: string[] }
  | { type: "isolate"; name: string }
  | { type: "unisolate" }
  | { type: "showAll" }
  | { type: "zoom"; how: "extents" | "in" | "out" }
  | { type: "toggle"; toggle: DraftingToggle }
  /** UCSICON ON or OFF; null flips it. */
  | { type: "ucsIcon"; on: boolean | null }
  | { type: "sheet"; name: string }
  | { type: "ai"; text: string }
  | { type: "run"; what: Run };

/** A line that can't be run: an i18n key and its values. */
export type ParseError = { type: "error"; key: string; args?: Record<string, string> };

const RUNS: Partial<Record<CommandName, Run>> = {
  NEW: "new",
  OPEN: "open",
  SAVE: "save",
  SAVEAS: "saveAs",
  EXPORT: "export",
  BOQ: "importBoq",
  COMPARE: "compare",
  UNDO: "undo",
  REDO: "redo",
  PDFATTACH: "attachPdf",
  IMAGEATTACH: "attachImage",
  XREF: "xrefs",
  CALIBRATE: "calibrate",
  EXTRACT: "extract",
  PLOT: "plot",
  PRINT: "plot",
  QUICKPRINT: "quickPrint",
  PUBLISH: "printAll",
  STYLESMANAGER: "plotStyles",
  FIND: "find",
  DIMSTYLE: "dimStyle",
  UPDATEFIELD: "updateFields",
  INSERT: "insertBlock",
  WBLOCK: "writeBlock",
  REFCLOSE: "refClose",
  BATTMAN: "attributeManager",
  LAYER: "layers",
  LAYSTATE: "layerStates",
  LAYTRANS: "layerTranslate",
  VPLAYER: "vpLayer",
  PROPERTIES: "properties",
  SHEETSET: "sheetSet",
  SHORTCUTS: "shortcuts",
  RECOVER: "recover",
  SCRIPT: "script",
  OPTIONS: "options",
  CUI: "customize",
  ALIASEDIT: "aliasEdit",
  PURGE: "purge",
  AUDIT: "audit",
  PARAMETERS: "parameters",
};

const TOOLS: Partial<Record<CommandName, Tool>> = {
  DIST: "distance",
  AREA: "area",
  GEOMETRY: "geometry",
  ID: "idPoint",
  LINE: "line",
  PLINE: "polyline",
  RECTANG: "rectangle",
  CIRCLE: "circle",
  ARC: "arc",
  ELLIPSE: "ellipse",
  SPLINE: "spline",
  POINT: "point",
  HATCH: "hatch",
  REVCLOUD: "revcloud",
  WIPEOUT: "wipeout",
  MOVE: "move",
  COPY: "copy",
  ROTATE: "rotate",
  SCALE: "scale",
  MIRROR: "mirror",
  STRETCH: "stretch",
  ALIGN: "align",
  ARRAY: "array",
  EXPLODE: "explode",
  ERASE: "erase",
  GROUP: "group",
  UNGROUP: "ungroup",
  MATCHPROP: "matchprop",
  TRIM: "trim",
  EXTEND: "extend",
  BREAK: "break",
  JOIN: "join",
  LENGTHEN: "lengthen",
  REVERSE: "reverse",
  OFFSET: "offset",
  FILLET: "fillet",
  CHAMFER: "chamfer",
  BLEND: "blend",
  PEDIT: "pedit",
  OVERKILL: "overkill",
  FLATTEN: "flatten",
  TEXT: "text",
  MTEXT: "mtext",
  TEXTEDIT: "textEdit",
  DIMLINEAR: "dimLinear",
  DIMALIGNED: "dimAligned",
  DIMANGULAR: "dimAngular",
  DIMRADIUS: "dimRadius",
  DIMDIAMETER: "dimDiameter",
  DIMARC: "dimArc",
  DIMORDINATE: "dimOrdinate",
  DIMBASELINE: "dimBaseline",
  DIMCONTINUE: "dimContinue",
  QDIM: "qdim",
  DIMEDIT: "dimEdit",
  DIMBREAK: "dimBreak",
  DIMSPACE: "dimSpace",
  DIMJOGLINE: "dimJogLine",
  LEADER: "leader",
  MLEADER: "mleader",
  TOLERANCE: "tolerance",
  COMMENT: "commentCloud",
  TABLE: "table",
  TABLEDIT: "tableEdit",
  FIELD: "field",
  BLOCK: "block",
  REFEDIT: "refedit",
  BEDIT: "refedit",
  ATTDEF: "attdef",
  ATTEDIT: "textEdit",
  COPYCLIP: "copyclip",
  COPYBASE: "copybase",
  PASTECLIP: "pasteclip",
  PASTEBLOCK: "pasteblock",
  MVIEW: "mview",
  AUTOCONSTRAIN: "autoConstrain",
  DELCONSTRAINT: "delConstraint",
};

/** GEOMCONSTRAINT's and DIMCONSTRAINT's option words (AutoCAD's), each to its tool. */
const GEOM_OPTIONS: Record<string, Tool> = {
  C: "gcCoincident",
  COINCIDENT: "gcCoincident",
  COL: "gcCollinear",
  COLLINEAR: "gcCollinear",
  CON: "gcConcentric",
  CONCENTRIC: "gcConcentric",
  F: "gcFixed",
  FIX: "gcFixed",
  FIXED: "gcFixed",
  PAR: "gcParallel",
  PARALLEL: "gcParallel",
  P: "gcPerpendicular",
  PERPENDICULAR: "gcPerpendicular",
  H: "gcHorizontal",
  HORIZONTAL: "gcHorizontal",
  V: "gcVertical",
  VERTICAL: "gcVertical",
  T: "gcTangent",
  TANGENT: "gcTangent",
  SM: "gcSmooth",
  SMOOTH: "gcSmooth",
  S: "gcSymmetric",
  SYMMETRIC: "gcSymmetric",
  E: "gcEqual",
  EQUAL: "gcEqual",
};

const DIM_OPTIONS: Record<string, Tool> = {
  L: "dcLinear",
  LINEAR: "dcLinear",
  AL: "dcAligned",
  ALIGNED: "dcAligned",
  AN: "dcAngular",
  ANGULAR: "dcAngular",
  R: "dcRadius",
  RADIUS: "dcRadius",
  D: "dcDiameter",
  DIAMETER: "dcDiameter",
};

/** The draws typed whole on one line. */
const TYPED_DRAWS = ["POLYGON", "DONUT", "RAY", "XLINE", "SOLID", "TRACE", "MLINE", "SKETCH"];

const TOGGLES: Partial<Record<CommandName, DraftingToggle>> = {
  OSNAP: "osnap",
  ORTHO: "ortho",
  POLAR: "polar",
  GRID: "grid",
};

const ZOOMS: Record<string, Action> = {
  E: { type: "zoom", how: "extents" },
  EXTENTS: { type: "zoom", how: "extents" },
  W: { type: "tool", tool: "zoomWindow" },
  WINDOW: { type: "tool", tool: "zoomWindow" },
  IN: { type: "zoom", how: "in" },
  OUT: { type: "zoom", how: "out" },
};

/** Western digits, `.` and `,` for Arabic-Indic and Eastern Arabic-Indic digits, `٫` and `،`. */
export function normalizeDigits(text: string): string {
  return text
    .replace(/[٠-٩]/g, (d) => String(d.charCodeAt(0) - 0x0660))
    .replace(/[۰-۹]/g, (d) => String(d.charCodeAt(0) - 0x06f0))
    .replace(/٫/g, ".")
    .replace(/،/g, ",");
}

const NUMBER = /^[-+]?(\d+\.?\d*|\.\d+)$/;

/** A number typed in either script, or null. */
function readNumber(text: string): number | null {
  const t = normalizeDigits(text.trim());
  return NUMBER.test(t) ? Number(t) : null;
}

/** Looks like a try at a point or a number rather than a command. */
const numeric = (text: string) => /^[-+.@\d]/.test(normalizeDigits(text.trim()));

/** A typed point, an error for a malformed one, or null when `text` is no point at all. Read in
 *  `ucs`: `x,y` from its origin along its axes, `@dx,dy` along its axes, `@d<a` with `a` from
 *  its x axis. */
function readPoint(text: string, last: Point | null, ucs: Ucs = WORLD): Point | ParseError | null {
  const t = normalizeDigits(text.trim());
  if (!numeric(t)) return null;
  const bad: ParseError = { type: "error", key: "cmd.badPoint" };
  const relative = t.startsWith("@");
  const body = relative ? t.slice(1).trim() : t;
  if (relative && !last) return { type: "error", key: "cmd.noLastPoint" };
  if (relative && body.includes("<")) {
    const [d, a] = body.split("<").map((s) => readNumber(s));
    if (d === null || a === null || body.split("<").length !== 2) return bad;
    const rad = ((a + ucsDegrees(ucs)) * Math.PI) / 180;
    return [last![0] + d * Math.cos(rad), last![1] + d * Math.sin(rad)];
  }
  const parts = body.split(",");
  if (parts.length !== 2) return bad;
  const [x, y] = parts.map((s) => readNumber(s));
  if (x === null || y === null) return bad;
  if (!relative) return toWorld(ucs, [x, y]);
  const [dx, dy] = dirToWorld(ucs, [x, y]);
  return [last![0] + dx, last![1] + dy];
}

/** Splits on spaces; double quotes keep a name with spaces in one word. */
export function tokenize(text: string): string[] {
  const words: string[] = [];
  const re = /"([^"]*)"?|(\S+)/g;
  for (let m = re.exec(text); m; m = re.exec(text)) words.push(m[1] ?? m[2]);
  return words;
}

/** The rest of a line as one name: unquoted when the whole of it is quoted. */
function oneName(rest: string): string {
  const t = rest.trim();
  return t.length >= 2 && t.startsWith('"') && t.endsWith('"') ? t.slice(1, -1) : t;
}

/** A command line, with its first word's alias expanded. */
export function expand(line: string, aliases: Record<string, string>): { name: string; rest: string } {
  const t = line.trim();
  const first = t.split(/\s/, 1)[0];
  let rest = t.slice(first.length).trim();
  let name = first.toUpperCase();
  const alias = aliases[name];
  if (alias !== undefined) {
    const target = alias.trim();
    const head = target.split(/\s/, 1)[0];
    name = head.toUpperCase();
    rest = [target.slice(head.length).trim(), rest].filter((s) => s !== "").join(" ");
  }
  return { name, rest };
}

/** Reads the words of a typed draw: numbers and points, `@` from the point before (the first from
 *  `last`). Null when a word is not what `pattern` asks for ("n" a number, "p" a point). */
function readWords(
  words: string[],
  pattern: string,
  last: Point | null,
  ucs?: Ucs,
): (number | Point)[] | null {
  if (words.length !== pattern.length) return null;
  const out: (number | Point)[] = [];
  for (let i = 0; i < words.length; i++) {
    if (pattern[i] === "n") {
      const n = readNumber(words[i]);
      if (n === null) return null;
      out.push(n);
    } else {
      const p = readPoint(words[i], last, ucs);
      if (!Array.isArray(p)) return null;
      out.push(p);
      last = p;
    }
  }
  return out;
}

/** The draw a whole line describes (see `TYPED_DRAWS`), or null when it is malformed:
 *  POLYGON sides centre radius [I|C], DONUT inner outer centre, RAY and XLINE point through,
 *  SOLID 3 or 4 points, TRACE width points, MLINE and SKETCH points. */
function typedDraw(cmd: string, rest: string, last: Point | null, ucs?: Ucs): Action | null {
  const words = tokenize(rest);
  const draw = (spec: DrawSpec): Action => ({ type: "draw", spec });
  const points = (from: number, min: number, max = Infinity) => {
    const n = words.length - from;
    if (n < min || n > max) return null;
    const read = readWords(words.slice(from), "p".repeat(n), last, ucs);
    return read as Point[] | null;
  };
  switch (cmd) {
    case "POLYGON": {
      const how = words[3]?.toUpperCase();
      if (how !== undefined && how !== "I" && how !== "C") return null;
      const read = readWords(words.slice(0, 3), "npn", last, ucs);
      if (!read) return null;
      const [sides, centre, radius] = read as [number, Point, number];
      return draw({ kind: "polygon", centre, sides, radius, inscribed: how !== "C" });
    }
    case "DONUT": {
      const read = readWords(words, "nnp", last, ucs);
      if (!read) return null;
      const [inner, outer, centre] = read as [number, number, Point];
      return draw({ kind: "donut", centre, inner, outer });
    }
    case "RAY":
    case "XLINE": {
      const read = points(0, 2, 2);
      if (!read) return null;
      const [origin, through] = read;
      const dir: Point = [through[0] - origin[0], through[1] - origin[1]];
      return draw(cmd === "RAY" ? { kind: "ray", origin, dir } : { kind: "xline", origin, dir });
    }
    case "SOLID": {
      const read = points(0, 3, 4);
      return read ? draw({ kind: "solid2d", points: read }) : null;
    }
    case "TRACE": {
      const width = words.length > 0 ? readNumber(words[0]) : null;
      const read = points(1, 2);
      return width !== null && read ? draw({ kind: "trace", width, points: read }) : null;
    }
    case "MLINE": {
      const read = points(0, 2);
      return read ? draw({ kind: "mline", points: read, style: null, scale: null }) : null;
    }
    case "SKETCH": {
      const read = points(0, 2);
      return read ? draw({ kind: "sketch", points: read }) : null;
    }
    default:
      return null;
  }
}

function command(
  line: string,
  aliases: Record<string, string>,
  last: Point | null,
  ucs?: Ucs,
): Action | ParseError {
  const { name, rest } = expand(line, aliases);
  if (!(COMMANDS as readonly string[]).includes(name)) {
    return { type: "error", key: "cmd.unknown", args: { name: line.trim().split(/\s/, 1)[0] } };
  }
  const cmd = name as CommandName;
  const run = RUNS[cmd];
  if (run) return { type: "run", what: run };
  const tool = TOOLS[cmd];
  if (tool) return { type: "tool", tool };
  const toggle = TOGGLES[cmd];
  if (toggle) return { type: "toggle", toggle };
  const needs = (key: string): ParseError => ({ type: "error", key, args: { name: cmd } });
  if (TYPED_DRAWS.includes(cmd)) return typedDraw(cmd, rest, last, ucs) ?? needs("cmd.drawUsage");
  switch (cmd) {
    case "ESC":
      return { type: "cancel" };
    case "COUNT":
      return rest ? { type: "count", block: oneName(rest) } : { type: "tool", tool: "count" };
    case "DIVIDE":
    case "MEASURE": {
      const tool = cmd === "DIVIDE" ? "divide" : "measureAlong";
      if (!rest) return { type: "tool", tool };
      const value = readNumber(rest);
      const bad = cmd === "DIVIDE" ? badCount(value) : badStep(value);
      return bad ?? { type: "tool", tool, preset: value! };
    }
    case "LAYOFF":
    case "LAYON": {
      const names = tokenize(rest);
      return names.length > 0
        ? { type: "layers", hide: cmd === "LAYOFF", names }
        : needs("cmd.needLayer");
    }
    case "LAYISO":
      return rest ? { type: "isolate", name: oneName(rest) } : needs("cmd.needLayer");
    case "LAYUNISO":
      return { type: "unisolate" };
    case "LAYALL":
      return { type: "showAll" };
    case "ZOOM":
      return ZOOMS[rest.toUpperCase()] ?? { type: "error", key: "cmd.zoomOption" };
    case "SHEET":
      return rest ? { type: "sheet", name: oneName(rest) } : needs("cmd.needSheet");
    case "AI":
      return rest ? { type: "ai", text: rest } : needs("cmd.needText");
    case "VPORTS": {
      const how = rest.trim();
      if (how === "") return { type: "run", what: "vports" };
      if (how === "1" || how === "2") return { type: "run", what: how === "1" ? "vports1" : "vports2" };
      return { type: "error", key: "cmd.vportsOption" };
    }
    case "GEOMCONSTRAINT":
    case "DIMCONSTRAINT": {
      const options = cmd === "GEOMCONSTRAINT" ? GEOM_OPTIONS : DIM_OPTIONS;
      const tool = options[rest.trim().toUpperCase()];
      if (tool) return { type: "tool", tool };
      return { type: "error", key: cmd === "GEOMCONSTRAINT" ? "cmd.gcOption" : "cmd.dcOption" };
    }
    case "UCS": {
      const how = rest.trim().toUpperCase();
      if (how === "") return { type: "tool", tool: "ucs" };
      if (how === "W" || how === "WORLD") return { type: "run", what: "ucsWorld" };
      if (how === "OB" || how === "OBJECT") return { type: "tool", tool: "ucsObject" };
      return { type: "error", key: "cmd.ucsOption" };
    }
    case "UCSICON": {
      const how = rest.trim().toUpperCase();
      if (how === "") return { type: "ucsIcon", on: null };
      if (how === "ON" || how === "OFF") return { type: "ucsIcon", on: how === "ON" };
      return { type: "error", key: "cmd.ucsIconOption" };
    }
    default:
      return { type: "error", key: "cmd.unknown", args: { name: cmd } };
  }
}

/** Divide takes 2 to 1000 parts, as its prompt does. */
function badCount(value: number | null): ParseError | null {
  return value !== null && Number.isInteger(value) && value >= 2 && value <= 1000
    ? null
    : { type: "error", key: "cmd.badCount" };
}

function badStep(value: number | null): ParseError | null {
  return value !== null && value > 0 ? null : { type: "error", key: "cmd.badStep" };
}

/** The action for one typed line, or why it can't run. */
export function parse(line: string, state: ParseState): Action | ParseError {
  const text = line.trim();
  if (text === "") {
    if (state.expect === "point") return { type: "finish" };
    if (state.expect === "command") return { type: "repeat" };
    return { type: "error", key: state.expect === "count" ? "cmd.badCount" : "cmd.badStep" };
  }
  if (state.expect === "point") {
    const word = text.toUpperCase();
    if (state.options?.includes(word)) return { type: "option", option: word };
    const value = state.numberToo ? readNumber(text) : null;
    if (value !== null) {
      return value > 0 || state.numberToo === "angle"
        ? { type: "distance", value }
        : { type: "error", key: "cmd.badDistance" };
    }
    const p = readPoint(text, state.last, state.ucs);
    if (p !== null) return Array.isArray(p) ? { type: "point", p } : p;
  }
  if ((state.expect === "count" || state.expect === "step") && numeric(text)) {
    const value = readNumber(text);
    const bad = state.expect === "count" ? badCount(value) : badStep(value);
    return bad ?? { type: "number", value: value! };
  }
  return command(text, state.aliases, state.last, state.ucs);
}

/** One autocomplete entry: a command, or an alias and the command it runs. */
export type Suggestion = { name: string; target?: string };

/** The commands and aliases starting with `prefix` (any case): exact match first, then commands,
 *  then aliases, each alphabetical. None once the line has a space. */
export function suggest(prefix: string, aliases: Record<string, string>): Suggestion[] {
  const p = prefix.trim().toUpperCase();
  if (p === "" || /\s/.test(prefix.trimStart())) return [];
  const found: Suggestion[] = [
    ...COMMANDS.filter((c) => c.startsWith(p)).map((name) => ({ name })),
    ...Object.keys(aliases)
      .filter((a) => a.startsWith(p) && !(COMMANDS as readonly string[]).includes(a))
      .sort()
      .map((name) => ({ name, target: aliases[name] })),
  ];
  const exact = found.findIndex((s) => s.name === p);
  if (exact > 0) found.unshift(...found.splice(exact, 1));
  return found;
}
