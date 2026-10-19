/** The measure, draw and modify tools: which clicks become which command. The rules live in a pure
 *  reducer (the draw tools' in `drawTools`, the modify tools' in `modifyTools`); the hook does the
 *  async picking around it and hands each finished command to the app. It also keeps the selection
 *  set: the entities a Shift click or a selection box picked with the select tool, which a modify
 *  tool then works on. */
import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { AiRegion, Boundary, Command, Drafting, EntityInfo, Picked, Snap, SnapKind, Ucs } from "../api";
import type { Target } from "../viewport/batch";
import type { Bounds, Point } from "../viewport/camera";
import { markerSegments } from "../viewport/snapMarker";
import type { Tool } from "../viewport/Viewport";
import { coalesce } from "./coalesce";
import {
  drawOptions,
  drawPreview,
  drawReducer,
  initialDrawState,
  isDrawTool,
  takesNumber,
  type DrawInput,
  type DrawState,
} from "./drawTools";
import {
  initialModifyState,
  isModifyTool,
  modifyOptions,
  modifyPreview,
  modifyReducer,
  takesModifyNumber,
  type ModifyInput,
  type ModifyState,
} from "./modifyTools";
import {
  editBoxes,
  editHighlight,
  editOptions,
  editPicks,
  editReducer,
  initialEditState,
  isEditTool,
  takesEditNumber,
  type EditInput,
  type EditState,
} from "./editTools";
import {
  initialTextState,
  isTextTool,
  takesTextNumber,
  textOptions,
  textPreview,
  textReducer,
  type TextInput,
  type TextRequest,
  type TextState,
} from "./textTools";
import {
  dimBoxes,
  dimHighlight,
  dimOptions,
  dimPicks,
  dimPreview,
  dimReducer,
  initialDimState,
  isDimTool,
  lastDimPoint,
  takesDimNumber,
  type DimInput,
  type DimRequest,
  type DimState,
} from "./dimTools";
import {
  constraintBoxes,
  constraintHighlight,
  constraintPicks,
  constraintReducer,
  initialConstraintState,
  isConstraintTool,
  type ConstraintInput,
  type ConstraintState,
} from "./constraintTools";
import { constrain, gridRound, NO_TRACK, type Track } from "./tracking";

/** Two clicks this close in time and on screen are a double click. */
const DOUBLE_CLICK_MS = 300;
const DOUBLE_CLICK_PX = 4;
/** Picking reaches this many CSS pixels around the cursor, unless the options say otherwise. */
const PICK_PX = 6;
/** Object snaps reach this many CSS pixels around the cursor, unless the options say otherwise. */
const SNAP_PX = 10;

export type ToolState = {
  tool: Tool;
  /** Points placed so far by `distance` or `area`, in drawing coordinates. */
  points: Point[];
  /** Set by the event that finished a measure; the hook runs it. */
  pendingCommand: Command | null;
  /** An i18n key for the status bar, or "". */
  hint: string;
  /** Values for the `{name}` slots of `hint`. */
  hintArgs: Record<string, string> | null;
  /** What `select` picked, waiting to be measured: the entity itself, or with `whole` the
   *  top-level block reference it sits in. `path` names the blocks along its chain. `toggled` is
   *  true when the last click swapped `whole`, so a double click can take that swap back. */
  selected: { picked: Picked; path: string[]; whole: boolean; toggled: boolean } | null;
  lastClick: { screen: Point; time: number } | null;
  /** `divide` and `measureAlong`: the entity clicked, waiting for the number of parts or the
   *  step to be typed. */
  asking: { kind: "divide" | "measureAlong"; target: Target } | null;
  /** `zoomWindow`: the first corner typed, waiting for the other. */
  corner: Point | null;
  /** Set by the event that gave `zoomWindow` its second corner; the hook zooms to it. */
  pendingZoom: Bounds | null;
  /** Set by the box or the closed polygon that marked an area for Mizan; the hook hands it on. */
  pendingRegion: AiRegion | null;
  /** A draw tool's own state; null for the other tools. */
  draw: DrawState | null;
  /** A modify tool's own state, with the entities it works on; null for the other tools. */
  modify: ModifyState | null;
  /** An edit tool's own state (trim, extend, fillet…); null for the other tools. */
  edit: EditState | null;
  /** A text tool's own state (TEXT, MTEXT); null for the other tools. */
  text: TextState | null;
  /** An annotate tool's own state (dimensions, leaders, tolerances); null for the others. */
  dim: DimState | null;
  /** A parametric tool's own state (constraints); null for the others. */
  param: ConstraintState | null;
  /** Set by the click that asks to edit a text or a table (the text and table edit tools', or the
   *  select tool's double click on one): the top-level handle; the hook hands it to the app. */
  pendingEdit: number | null;
  /** Where that click was: a table opens at the cell under it. */
  editAt: Point | null;
  /** The selection set: top-level handles the select tool picked (a click replaces it, Shift and
   *  boxes add). A modify tool takes it when it starts; Esc and resets clear it. */
  selection: number[];
  /** The entity kind of each handle in the selection set, when known ("Line", "Dimension"…). */
  kinds: Record<number, string>;
};

export type ToolEvent =
  | {
      type: "click";
      world: Point;
      screen: Point;
      time: number;
      /** What is under the click (`select` and `count` only), and its info: for `count` the
       *  top-level entity's, for `select` the picked entity's inside its blocks. */
      picked: Picked | null;
      info: EntityInfo | null;
      /** `world` is an object snap's point: placed as it is, without ortho or polar. */
      snapped?: boolean;
      /** The kind of that snap, for `idPoint`. */
      snapKind?: SnapKind | null;
      /** Ortho and polar tracking for `distance` and `area`'s second and later points. */
      track?: Track;
      /** Shift was held: the select tool adds the entity to the selection set. */
      shift?: boolean;
      /** The group the entity picked belongs to (its top-level handles and their kinds): the
       *  select tool selects all of them. Absent with Ctrl held or group selection off. */
      group?: { handles: number[]; kinds: string[] } | null;
    }
  /** What a selection box picked (top-level handles, and each one's kind when known); `add`
   *  (Shift) keeps what was selected. */
  | { type: "selected"; handles: number[]; add: boolean; kinds?: (string | null | undefined)[] }
  /** The drawing changed under the selection (an undo, a redo): keep the handles `present`. */
  | { type: "prune"; present: (handle: number) => boolean }
  /** `mizanArea`: a box dragged marks the area. */
  | { type: "region"; region: AiRegion }
  /** Mizan selected these entities (top-level handles): the select tool with them as its
   *  selection set. */
  | { type: "aiSelect"; handles: number[] }
  /** For a modify tool: what its stretch window touches, or a box as that window. */
  | { type: "modify"; input: ModifyInput }
  | { type: "finish" }
  /** The Esc key: drops what is waiting, but keeps a polyline drawn so far. */
  | { type: "escape" }
  /** A reset (another drawing or sheet, another mode): drops everything, draws nothing. */
  | { type: "cancel" }
  | { type: "tool"; tool: Tool }
  /** The number of parts (`divide`) or the step in drawing units (`measureAlong`) was typed. */
  | { type: "answer"; value: number }
  /** For a draw tool: an option or a distance typed, or the ring a hatch asked for. */
  | { type: "draw"; input: DrawInput };

const PROMPT: Partial<Record<Tool, string>> = {
  select: "measure.clickEntity",
  distance: "measure.needTwo",
  area: "measure.needThree",
  count: "measure.clickBlock",
  zoomWindow: "zoom.firstCorner",
  geometry: "inquiry.clickGeometry",
  divide: "inquiry.clickDivide",
  measureAlong: "inquiry.clickAlong",
  idPoint: "inquiry.clickPoint",
  textEdit: "text.prompt.edit",
  tableEdit: "table.prompt.edit",
  refedit: "block.prompt.refedit",
  pasteclip: "clip.prompt.paste",
  pasteblock: "clip.prompt.paste",
  mview: "vport.prompt.first",
  ucs: "ucs.prompt.origin",
  ucsObject: "ucs.prompt.object",
  mizanArea: "ai.area.prompt",
};

/** The tools that act on the entity clicked. */
const picksEntity = (tool: Tool) =>
  tool === "select" ||
  tool === "count" ||
  tool === "geometry" ||
  tool === "divide" ||
  tool === "measureAlong" ||
  tool === "textEdit" ||
  tool === "tableEdit" ||
  tool === "refedit" ||
  tool === "ucsObject";

/** The first prompt of `tool`. */
const promptOf = (tool: Tool): string =>
  isDrawTool(tool)
    ? initialDrawState(tool).hint
    : isModifyTool(tool)
      ? initialModifyState(tool, []).hint
      : isEditTool(tool)
        ? initialEditState(tool).hint
        : isTextTool(tool)
          ? initialTextState(tool).hint
          : isDimTool(tool)
            ? initialDimState(tool).hint
            : isConstraintTool(tool)
              ? initialConstraintState(tool).hint
              : (PROMPT[tool] ?? "");

/** The draw tools that take points until Enter, which a double click also ends. */
const MANY_POINTS = new Set(["polyline", "spline", "revcloud", "wipeout"]);

/** `select` starts quiet, so the status bar shows the file name. A modify tool starts on
 *  `selection` (and may finish at once: erase and explode); the other tools keep it. */
export const initialToolState = (
  tool: Tool,
  selection: number[] = [],
  kinds: Record<number, string> = {},
): ToolState => {
  const modify = isModifyTool(tool) ? initialModifyState(tool, selection, kinds) : null;
  const edit = isEditTool(tool) ? initialEditState(tool, selection) : null;
  const dim = isDimTool(tool) ? initialDimState(tool, selection) : null;
  const param = isConstraintTool(tool) ? initialConstraintState(tool, selection) : null;
  const own = modify ?? edit ?? dim ?? param;
  const text = isTextTool(tool) ? initialTextState(tool) : null;
  return {
    tool,
    points: [],
    pendingCommand: modify?.command ?? edit?.command ?? param?.command ?? null,
    hint: own ? own.hint : tool === "select" ? "" : promptOf(tool),
    hintArgs: own ? own.hintArgs : (text?.hintArgs ?? null),
    selected: null,
    lastClick: null,
    asking: null,
    corner: null,
    pendingZoom: null,
    pendingRegion: null,
    draw: isDrawTool(tool) ? initialDrawState(tool) : null,
    modify,
    edit,
    text,
    dim,
    param,
    pendingEdit: null,
    editAt: null,
    selection: own ? [] : selection,
    kinds: own ? {} : kinds,
  };
};

/** `kinds` with each of `handles` given the kind at the same place in `found`, when known. */
function withKinds(
  kinds: Record<number, string>,
  handles: number[],
  found: (string | null | undefined)[] | undefined,
): Record<number, string> {
  const out = { ...kinds };
  handles.forEach((h, i) => {
    const kind = found?.[i];
    if (kind) out[h] = kind;
  });
  return out;
}

/** Hands `input` to the modify tool; a change it finishes becomes the pending command. */
function modifyStep(s: ToolState, input: ModifyInput): ToolState {
  if (!s.modify) return s;
  const modify = modifyReducer(s.modify, input);
  return { ...s, modify, hint: modify.hint, hintArgs: modify.hintArgs, pendingCommand: modify.command };
}

/** Hands `input` to the edit tool; a change it finishes becomes the pending command. */
function editStep(s: ToolState, input: EditInput): ToolState {
  if (!s.edit) return s;
  const edit = editReducer(s.edit, input);
  return { ...s, edit, hint: edit.hint, hintArgs: edit.hintArgs, pendingCommand: edit.command };
}

/** Hands `input` to the annotate tool; a change it finishes becomes the pending command. */
function dimStep(s: ToolState, input: DimInput): ToolState {
  if (!s.dim) return s;
  const dim = dimReducer(s.dim, input);
  return { ...s, dim, hint: dim.hint, hintArgs: dim.hintArgs, pendingCommand: dim.command };
}

/** Hands `input` to the parametric tool; a constraint it finishes becomes the pending command. */
function paramStep(s: ToolState, input: ConstraintInput): ToolState {
  if (!s.param) return s;
  const param = constraintReducer(s.param, input);
  return { ...s, param, hint: param.hint, hintArgs: param.hintArgs, pendingCommand: param.command };
}

/** Hands `input` to the text tool. */
function textStep(s: ToolState, input: TextInput): ToolState {
  if (!s.text) return s;
  const text = textReducer(s.text, input);
  return { ...s, text, hint: text.hint, hintArgs: text.hintArgs };
}

/** Where an edit tool's last click was (the first break point, the entity offset). */
const lastEditPoint = (s: ToolState): Point | null => s.edit?.first?.at ?? null;

/** The last point a modify tool placed. */
const lastModifyPoint = (s: ToolState): Point | null =>
  s.modify && s.modify.points.length > 0 ? s.modify.points[s.modify.points.length - 1] : null;

/** `a` and then the handles of `b` not in it. */
const union = (a: number[], b: number[]) => [...new Set([...a, ...b])];

/** Hands `input` to the draw tool; a draw it finishes becomes the pending command. */
function drawStep(s: ToolState, input: DrawInput): ToolState {
  if (!s.draw) return s;
  const draw = drawReducer(s.draw, input);
  const pendingCommand: Command | null = draw.spec ? { type: "draw", ...draw.spec } : null;
  return { ...s, draw, hint: draw.hint, hintArgs: draw.hintArgs, pendingCommand };
}

/** The last point a draw tool placed. */
const lastDrawPoint = (s: ToolState): Point | null =>
  s.draw && s.draw.points.length > 0 ? s.draw.points[s.draw.points.length - 1] : null;

const samePick = (a: Picked, b: Picked) =>
  a.top === b.top && a.entity === b.entity && a.chain.join(",") === b.chain.join(",");

/** What a `select` choice draws and measures. */
export function selectedTarget(selected: ToolState["selected"]): Target | null {
  if (!selected) return null;
  const { picked, whole } = selected;
  return whole || picked.chain.length === 0
    ? { handle: picked.top, chain: [] }
    : { handle: picked.entity, chain: picked.chain };
}

/** The status bar line for a `select` choice: which block it is in, or which block it is. */
function selectHint(s: ToolState, selected: NonNullable<ToolState["selected"]>): ToolState {
  const { path, whole } = selected;
  if (path.length === 0) return { ...s, selected, hint: "", hintArgs: null };
  return whole
    ? { ...s, selected, hint: "measure.wholeBlock", hintArgs: { name: path[0] } }
    : { ...s, selected, hint: "measure.insideBlock", hintArgs: { name: path[path.length - 1] } };
}

function measureSelected(s: ToolState): ToolState {
  const target = selectedTarget(s.selected);
  if (!target) return s;
  const pendingCommand: Command = { type: "measure_entity", handle: target.handle, chain: target.chain };
  return { ...s, selected: null, hint: "", hintArgs: null, pendingCommand };
}

function finish(s: ToolState): ToolState {
  if (s.modify) return modifyStep(s, { type: "finish" });
  if (s.edit) return editStep(s, { type: "finish" });
  if (s.dim) return dimStep(s, { type: "finish" });
  if (s.param) return paramStep(s, { type: "finish" });
  if (s.text) return textStep(s, { type: "finish" });
  if (s.tool === "select") return measureSelected(s);
  if (s.draw) return { ...drawStep(s, { type: "finish" }), lastClick: null };
  if (s.tool === "mizanArea") {
    if (s.points.length < 3) return { ...s, hint: promptOf(s.tool) };
    return { ...s, points: [], lastClick: null, pendingRegion: { kind: "polygon", points: s.points } };
  }
  const need = s.tool === "area" ? 3 : 2;
  if (s.tool !== "distance" && s.tool !== "area") return s;
  if (s.points.length < need) return { ...s, hint: promptOf(s.tool) };
  const type = s.tool === "area" ? "measure_area" : "measure_distance";
  return { ...s, points: [], lastClick: null, pendingCommand: { type, points: s.points } };
}

export function toolReducer(state: ToolState, event: ToolEvent): ToolState {
  // A command or a zoom is pending for one state only: the hook sees it once.
  const s = { ...state, pendingCommand: null, pendingZoom: null, pendingRegion: null, pendingEdit: null, editAt: null };
  switch (event.type) {
    case "tool": {
      // The selection set goes along; with none, the entity the select tool picked (whole).
      const carried = s.selection.length > 0 ? s.selection : s.selected ? [s.selected.picked.top] : [];
      const next = initialToolState(event.tool, carried, s.kinds);
      return next.modify || next.edit || next.dim || next.param ? next : { ...next, hint: promptOf(event.tool) };
    }
    case "draw":
      if (s.modify && event.input.type !== "ring") return modifyStep(s, event.input);
      if (s.edit && event.input.type !== "ring") return editStep(s, event.input);
      if (s.dim && event.input.type !== "ring") return dimStep(s, event.input);
      if (s.text && event.input.type !== "ring") return textStep(s, event.input);
      return drawStep(s, event.input);
    case "modify":
      return modifyStep(s, event.input);
    case "selected": {
      if (s.modify) return modifyStep(s, { type: "picked", handles: event.handles, kinds: event.kinds });
      if (s.edit) return editStep(s, { type: "picked", handles: event.handles });
      if (s.dim) return dimStep(s, { type: "picked", handles: event.handles });
      if (s.param) return paramStep(s, { type: "picked", handles: event.handles });
      if (s.tool !== "select") return s;
      const kinds = withKinds(event.add ? s.kinds : {}, event.handles, event.kinds);
      return { ...s, selection: event.add ? union(s.selection, event.handles) : event.handles, kinds };
    }
    case "region":
      return s.tool === "mizanArea" ? { ...s, points: [], pendingRegion: event.region } : s;
    case "aiSelect":
      return { ...initialToolState("select", event.handles), hint: "" };
    case "prune": {
      const selection = s.selection.filter(event.present);
      const modify = s.modify && { ...s.modify, handles: s.modify.handles.filter(event.present) };
      const edit = s.edit && {
        ...s.edit,
        handles: s.edit.handles.filter(event.present),
        first: s.edit.first && event.present(s.edit.first.handle) ? s.edit.first : null,
      };
      const dim = s.dim && {
        ...s.dim,
        handles: s.dim.handles.filter(event.present),
        first: s.dim.first !== null && event.present(s.dim.first) ? s.dim.first : null,
      };
      const param = s.param && {
        ...s.param,
        handles: s.param.handles.filter(event.present),
        picks: s.param.picks.filter((p) => event.present(p.handle)),
      };
      return { ...s, selection, modify, edit, dim, param };
    }
    case "cancel":
      return initialToolState(s.tool);
    case "escape":
      // Esc keeps a polyline drawn so far, as AutoCAD does.
      if (s.draw?.kind === "polyline" && s.draw.points.length >= 2) {
        return { ...finish(s), hint: promptOf(s.tool), hintArgs: null };
      }
      return initialToolState(s.tool);
    case "finish":
      return finish(s);
    case "answer": {
      const asking = s.asking;
      if (!asking || !(event.value > 0) || !Number.isFinite(event.value)) return s;
      const { handle, chain } = asking.target;
      const pendingCommand: Command =
        asking.kind === "divide"
          ? { type: "divide_along", handle, chain, count: Math.round(event.value) }
          : { type: "measure_along", handle, chain, step: event.value };
      return { ...s, asking: null, hint: promptOf(s.tool), pendingCommand };
    }
    case "click": {
      const last = s.lastClick;
      const double =
        last !== null &&
        event.time - last.time < DOUBLE_CLICK_MS &&
        Math.hypot(event.screen[0] - last.screen[0], event.screen[1] - last.screen[1]) <=
          DOUBLE_CLICK_PX;
      const clicked = { ...s, lastClick: { screen: event.screen, time: event.time } };
      if (s.edit) {
        if (editPicks(s.edit)) {
          const picked = event.picked;
          if (!picked) return clicked;
          return editStep(clicked, {
            type: "pick",
            handle: picked.top,
            kind: picked.kind,
            at: event.world,
            shift: event.shift,
          });
        }
        const point = event.snapped ? event.world : constrain(lastEditPoint(s), event.world, event.track ?? NO_TRACK);
        return editStep(clicked, { type: "point", p: point });
      }
      if (s.dim) {
        if (dimPicks(s.dim)) {
          const picked = event.picked;
          if (!picked) return clicked;
          return dimStep(clicked, { type: "pick", handle: picked.top, kind: picked.kind, at: event.world });
        }
        const point = event.snapped ? event.world : constrain(lastDimPoint(s.dim), event.world, event.track ?? NO_TRACK);
        return dimStep(clicked, { type: "point", p: point });
      }
      if (s.param) {
        if (constraintPicks(s.param)) {
          const picked = event.picked;
          if (!picked) return clicked;
          return paramStep(clicked, { type: "pick", handle: picked.top, at: event.world });
        }
        return paramStep(clicked, { type: "point", p: event.world });
      }
      if (s.modify) {
        if (s.modify.phase === "select") {
          const picked = event.picked;
          return picked ? modifyStep(clicked, { type: "picked", handles: [picked.top], kinds: [picked.kind] }) : clicked;
        }
        // Ortho and polar square a point to the one before; never a window's corner.
        const tracked = s.modify.phase === "points" && !event.snapped;
        const point = tracked ? constrain(lastModifyPoint(s), event.world, event.track ?? NO_TRACK) : event.world;
        return modifyStep(clicked, { type: "point", p: point });
      }
      if (s.text) {
        const point = event.snapped ? event.world : constrain(s.text.corner, event.world, event.track ?? NO_TRACK);
        return textStep(clicked, { type: "point", p: point });
      }
      if (s.draw) {
        if (double && MANY_POINTS.has(s.draw.kind)) return finish(s);
        // The second click of a double click places no second point or hatch.
        if (double && (s.draw.kind === "point" || s.draw.kind === "hatch")) return clicked;
        const prev = lastDrawPoint(s);
        const point = event.snapped ? event.world : constrain(prev, event.world, event.track ?? NO_TRACK);
        return drawStep(clicked, { type: "point", p: point });
      }
      switch (s.tool) {
        case "distance":
        case "area":
        case "mizanArea": {
          if (double) return { ...finish(s), lastClick: null };
          const prev = s.points.length > 0 ? s.points[s.points.length - 1] : null;
          const point = event.snapped ? event.world : constrain(prev, event.world, event.track ?? NO_TRACK);
          return { ...clicked, points: [...s.points, point] };
        }
        case "select": {
          const picked = event.picked;
          // A grouped entity brings its whole group (AutoCAD's PICKSTYLE).
          const group = event.group && event.group.handles.length > 0 ? event.group : null;
          const whole = picked ? (group ? group.handles : [picked.top]) : [];
          const known = (kinds: Record<number, string>) =>
            withKinds(group ? withKinds(kinds, group.handles, group.kinds) : kinds, [picked!.top], [picked!.kind]);
          // Shift adds the whole top-level entity to the selection set, and measures nothing.
          if (event.shift) {
            if (!picked) return clicked;
            return { ...clicked, selection: union(s.selection, whole), kinds: known(s.kinds) };
          }
          // Otherwise the click replaces the selection set (AutoCAD's PICKADD), and measures as before.
          if (picked === null) return { ...clicked, selected: null, hint: "", hintArgs: null, selection: [], kinds: {} };
          clicked.selection = whole;
          clicked.kinds = known({});
          const current = s.selected;
          if (current && samePick(current.picked, picked)) {
            // Inside a block, a slow second click swaps the entity for the whole block and back;
            // a double click (or Enter) measures. Elsewhere the second click measures.
            if (picked.chain.length > 0 && !double) {
              return selectHint(clicked, { ...current, whole: !current.whole, toggled: true });
            }
            // A double click on a text, a table or a block's attribute (picked as its reference)
            // edits it.
            const editable =
              picked.kind === "Text" || picked.kind === "MText" || picked.kind === "Table" || picked.kind === "Insert";
            if (double && picked.chain.length === 0 && editable) {
              return { ...clicked, pendingEdit: picked.top, editAt: event.world };
            }
            // The first click of a double click swapped: measure what was shown before the pair.
            const whole = double && current.toggled ? !current.whole : current.whole;
            return measureSelected({ ...clicked, selected: { ...current, whole } });
          }
          const path = event.info?.block_path ?? [];
          return selectHint(clicked, { picked, path, whole: false, toggled: false });
        }
        case "count":
          // The second click of a double click would count the same block twice.
          if (double) return clicked;
          if (event.picked === null) return { ...clicked, hint: promptOf("count") };
          if (!event.info?.block_name) return { ...clicked, hint: "measure.notABlock" };
          return {
            ...clicked,
            hint: promptOf("count"),
            pendingCommand: { type: "count_block", block_name: event.info.block_name, layer: null },
          };
        case "geometry": {
          if (double) return clicked;
          const picked = event.picked;
          if (picked === null) return { ...clicked, hint: promptOf("geometry") };
          const pendingCommand: Command = {
            type: "measure_geometry",
            handle: picked.entity,
            chain: picked.chain,
            at: event.world,
          };
          return { ...clicked, hint: promptOf("geometry"), pendingCommand };
        }
        case "divide":
        case "measureAlong": {
          const picked = event.picked;
          if (picked === null) return { ...clicked, asking: null, hint: promptOf(s.tool) };
          const target = { handle: picked.entity, chain: picked.chain };
          const hint = s.tool === "divide" ? "inquiry.askCount" : "inquiry.askStep";
          return { ...clicked, asking: { kind: s.tool, target }, hint };
        }
        case "zoomWindow": {
          // Typed corners (a drag zooms in the viewport itself), in any order.
          const a = s.corner;
          if (!a) return { ...clicked, corner: event.world, hint: "zoom.otherCorner" };
          const b = event.world;
          const pendingZoom: Bounds = [
            [Math.min(a[0], b[0]), Math.min(a[1], b[1])],
            [Math.max(a[0], b[0]), Math.max(a[1], b[1])],
          ];
          return { ...clicked, corner: null, hint: promptOf("zoomWindow"), pendingZoom };
        }
        case "textEdit":
          if (double || event.picked === null) return clicked;
          return { ...clicked, pendingEdit: event.picked.top, editAt: event.world };
        case "tableEdit":
          if (double || event.picked === null) return clicked;
          if (event.picked.kind !== "Table") return { ...clicked, hint: "table.prompt.notTable" };
          return { ...clicked, hint: promptOf("tableEdit"), pendingEdit: event.picked.top, editAt: event.world };
        case "refedit": {
          const picked = event.picked;
          if (double || picked === null) return clicked;
          if (picked.kind !== "Insert" || picked.xref) return { ...clicked, hint: "block.prompt.notBlock" };
          const target = { handle: picked.top, chain: [] };
          return { ...clicked, hint: promptOf("refedit"), pendingCommand: { type: "edit_block_in_place", target } };
        }
        case "idPoint":
          return {
            ...clicked,
            pendingCommand: { type: "id_point", p: event.world, kind: event.snapKind ?? null },
          };
        case "pasteclip":
        case "pasteblock": {
          if (double) return clicked;
          const pendingCommand: Command = {
            type: "paste_from_clipboard",
            at: event.world,
            as_block: s.tool === "pasteblock",
          };
          return { ...clicked, pendingCommand };
        }
        case "mview":
        case "ucs": {
          const prev = s.points.length > 0 ? s.points[0] : null;
          const point = event.snapped ? event.world : constrain(prev, event.world, event.track ?? NO_TRACK);
          if (!prev) {
            const hint = s.tool === "mview" ? "vport.prompt.second" : "ucs.prompt.xAxis";
            return { ...clicked, points: [point], hint };
          }
          if (point[0] === prev[0] && point[1] === prev[1]) return clicked;
          const pendingCommand: Command =
            s.tool === "mview"
              ? { type: "mview", corner1: prev, corner2: point }
              : { type: "ucs", origin: prev, x_axis: [point[0] - prev[0], point[1] - prev[1]] };
          return { ...clicked, points: [], hint: promptOf(s.tool), pendingCommand };
        }
        case "ucsObject": {
          const picked = event.picked;
          if (double || picked === null) return clicked;
          const pendingCommand: Command = {
            type: "ucs_object",
            target: { handle: picked.entity, chain: picked.chain },
          };
          return { ...clicked, pendingCommand };
        }
        default:
          return clicked;
      }
    }
  }
}

export type ToolApi = {
  /** `mizanArea` marked an area: hand it to Mizan. */
  markRegion?(region: AiRegion): void;
  pick(x: number, y: number, tol: number): Promise<Picked | null>;
  /** What a selection box picks: wholly inside, or with `crossing` touching it. */
  pickWindow(bounds: Bounds, crossing: boolean): Promise<Picked[]>;
  /** The object snap within `tol` of (x, y); `from` is the previous point. */
  snap(x: number, y: number, tol: number, from: Point | null): Promise<Snap | null>;
  entityInfo(handle: number, chain?: number[]): Promise<EntityInfo>;
  /** Applies a finished command. */
  run(cmd: Command): void;
  /** Shows the window `zoomWindow` was given by two typed corners. */
  zoomTo(bounds: Bounds): void;
  /** The closed area around `p` the drawn lines make, for a hatch. */
  boundary(p: Point): Promise<Boundary>;
  /** A hatch pattern's scale when none is typed: 0.025 metres per drawing unit's worth. */
  hatchScale(): number;
  /** Takes back the last change (the line tool's U). */
  undo(): void;
  /** A text tool placed a text: open its editor. */
  placeText?(request: TextRequest): void;
  /** Edit the text of the top-level entity `handle` (a text, an MText, a block's attributes, a
   *  table's cells), clicked at `at`. */
  editText?(handle: number, at: Point | null): void;
  /** An annotate tool needs typed text: a leader's, a tolerance's frame, a dimension's. */
  dimText?(request: DimRequest): void;
  /** The group the top-level entity `handle` is in, its handles and their kinds; null when none
   *  (or when group selection is off). */
  groupOf?(handle: number): { handles: number[]; kinds: string[] } | null;
  onError(e: unknown): void;
};

/** The tools that place points, and so snap and track. A hatch's click is inside an area, never
 *  snapped onto its edge. */
const placesPoints = (tool: Tool) =>
  tool === "distance" ||
  tool === "area" ||
  tool === "mizanArea" ||
  (isDrawTool(tool) && tool !== "hatch") ||
  isTextTool(tool) ||
  tool === "pasteclip" ||
  tool === "pasteblock" ||
  tool === "mview" ||
  tool === "ucs";
/** The tools that snap: those that place points, and `idPoint`. */
const snaps = (tool: Tool) => placesPoints(tool) || tool === "idPoint";

/** The same empty list each render, so memoised readers see no change. */
const NO_PICKS: number[] = [];

type SnapAsk = { p: Point; tol: number; from: Point | null; wpp: number };

/** Tool state for `tool`, following it as it changes. `ucs` is the user coordinate system ortho,
 *  polar and the grid run along. `drafting` gives the snaps, ortho, polar and
 *  grid snap the point tools place their points with (none while it is null). `px`: how many CSS
 *  pixels around the cursor snaps and picks reach (the Options dialog's snap aperture and pick box). */
export function useTool(
  tool: Tool,
  api: ToolApi,
  drafting: Drafting | null = null,
  ucs?: Ucs,
  px: { snap: number; pick: number } = { snap: SNAP_PX, pick: PICK_PX },
) {
  const [state, dispatch] = useReducer(toolReducer, tool, initialToolState);
  /** The cursor in drawing coordinates as it is, and drawing units per CSS pixel there. */
  const [raw, setRaw] = useState<{ p: Point; wpp: number } | null>(null);
  /** The object snap under the cursor, and the zoom it was found at. */
  const [hover, setHover] = useState<{ snap: Snap; wpp: number } | null>(null);
  const apiRef = useRef(api);
  apiRef.current = api;
  const snapper = useRef(
    coalesce(
      (ask: SnapAsk) => apiRef.current.snap(ask.p[0], ask.p[1], ask.tol, ask.from),
      (snap, ask) => setHover(snap ? { snap, wpp: ask.wpp } : null),
      // A failed snap is no snap; the click asks again and reports its own error.
      () => setHover(null),
    ),
  );
  const modifyPhase = state.modify?.phase ?? null;
  const editPicking = state.edit
    ? editPicks(state.edit)
    : state.dim
      ? dimPicks(state.dim)
      : state.param
        ? constraintPicks(state.param)
        : null;
  /** Clicks place snapped points, pick an entity, or neither (corners). */
  const snapsNow = snaps(tool) || modifyPhase === "points" || editPicking === false;
  const picksNow = picksEntity(tool) || modifyPhase === "select" || editPicking === true;
  const placesNow = placesPoints(tool) || modifyPhase === "points" || editPicking === false;
  const snapping = snapsNow && !!drafting?.osnap;
  const track: Track = drafting
    ? { ortho: drafting.ortho, polar: drafting.polar, polarStepDeg: drafting.polar_step_deg, ucs }
    : NO_TRACK;
  const prev = state.text
    ? state.text.corner
    : state.draw
    ? lastDrawPoint(state)
    : state.modify
      ? lastModifyPoint(state)
      : state.edit
        ? lastEditPoint(state)
        : state.dim
          ? lastDimPoint(state.dim)
          : state.points.length > 0
        ? state.points[state.points.length - 1]
        : null;

  useEffect(() => {
    if (state.tool !== tool) dispatch({ type: "tool", tool });
  }, [tool]);

  useEffect(() => {
    if (snapping) return;
    snapper.current.cancel();
    setHover(null);
  }, [snapping]);

  useEffect(() => {
    if (state.pendingCommand) api.run(state.pendingCommand);
  }, [state.pendingCommand]);

  useEffect(() => {
    if (state.pendingZoom) api.zoomTo(state.pendingZoom);
  }, [state.pendingZoom]);

  useEffect(() => {
    if (state.pendingRegion) api.markRegion?.(state.pendingRegion);
  }, [state.pendingRegion]);

  useEffect(() => {
    if (state.draw?.undo) api.undo();
  }, [state.draw]);

  useEffect(() => {
    const request = state.text?.request;
    if (request) api.placeText?.(request);
  }, [state.text]);

  useEffect(() => {
    const request = state.dim?.request;
    if (request) api.dimText?.(request);
  }, [state.dim]);

  useEffect(() => {
    if (state.pendingEdit !== null) api.editText?.(state.pendingEdit, state.editAt);
  }, [state.pendingEdit, state.lastClick]);

  // A hatch's click: find the ring around it, then hand it back.
  const seek = state.draw?.seek ?? null;
  useEffect(() => {
    if (!seek) return;
    let gone = false;
    const ring = (found: Boundary) =>
      dispatch({ type: "draw", input: { type: "ring", found, scale: api.hatchScale() } });
    api.boundary(seek).then(
      (found) => !gone && ring(found),
      (e) => {
        if (gone) return;
        api.onError(e);
        ring({ found: "none" });
      },
    );
    return () => {
      gone = true;
    };
  }, [seek]);

  // A stretch window: find what it touches, then hand it back.
  const windowSeek = state.modify?.seek ?? null;
  useEffect(() => {
    if (!windowSeek) return;
    let gone = false;
    const found = (picked: Picked[]) =>
      dispatch({
        type: "modify",
        input: { type: "picked", handles: picked.map((p) => p.top), kinds: picked.map((p) => p.kind) },
      });
    api.pickWindow(windowSeek, true).then(
      (picked) => !gone && found(picked),
      (e) => {
        if (gone) return;
        api.onError(e);
        found([]);
      },
    );
    return () => {
      gone = true;
    };
  }, [windowSeek]);

  // Said once: dimensions and tables the modify tool left out.
  const skipped = state.modify?.skipped ?? 0;

  /** Where an unsnapped point goes: to the grid when grid snap is on. */
  const onGrid = (p: Point): Point =>
    drafting?.grid_snap ? gridRound(p, drafting.grid_spacing, ucs) : p;

  function onMove(world: Point, worldPerPixel: number) {
    setRaw({ p: world, wpp: worldPerPixel });
    if (snapping) snapper.current.request({ p: world, tol: px.snap * worldPerPixel, from: prev, wpp: worldPerPixel });
  }

  async function onClick(world: Point, screen: Point, worldPerPixel: number, shift = false, ctrl = false) {
    const time = performance.now();
    if (snapsNow) {
      let found: Snap | null = null;
      try {
        if (snapping) found = await api.snap(world[0], world[1], px.snap * worldPerPixel, prev);
      } catch (e) {
        api.onError(e);
        return;
      }
      const at = found ? found.point : onGrid(world);
      dispatch({
        type: "click",
        world: at,
        screen,
        time,
        picked: null,
        info: null,
        snapped: !!found,
        snapKind: found?.kind ?? null,
        track,
      });
      return;
    }
    let picked: Picked | null = null;
    let info: EntityInfo | null = null;
    try {
      if (picksNow) {
        picked = await api.pick(world[0], world[1], px.pick * worldPerPixel);
      }
      if (tool === "count" && picked !== null) info = await api.entityInfo(picked.top);
      // Inside a block, the block names go to the status bar.
      if (tool === "select" && picked !== null && picked.chain.length > 0) {
        info = await api.entityInfo(picked.entity, picked.chain);
      }
    } catch (e) {
      api.onError(e);
      return;
    }
    // Ctrl picks one entity out of its group.
    const group = tool === "select" && picked && !ctrl ? (api.groupOf?.(picked.top) ?? null) : null;
    dispatch({ type: "click", world, screen, time, picked, info, shift, group });
  }

  /** A selection box dragged in the viewport: picks for the select tool or a modify tool, or is
   *  stretch's window. */
  async function onBox(bounds: Bounds, crossing: boolean, shift: boolean) {
    if (tool === "mizanArea") {
      dispatch({ type: "region", region: { kind: "box", points: [bounds[0], bounds[1]] } });
      return;
    }
    if (modifyPhase === "window") {
      dispatch({ type: "modify", input: { type: "box", bounds } });
      return;
    }
    const editBox = state.edit
      ? editBoxes(state.edit)
      : state.dim
        ? dimBoxes(state.dim)
        : state.param
          ? constraintBoxes(state.param)
          : false;
    if (tool !== "select" && modifyPhase !== "select" && !editBox) return;
    try {
      const picked = await api.pickWindow(bounds, crossing);
      dispatch({ type: "selected", handles: picked.map((p) => p.top), add: shift, kinds: picked.map((p) => p.kind) });
    } catch (e) {
      api.onError(e);
    }
  }

  const snap = snapping ? (hover?.snap ?? null) : null;
  /** Where a click would place the next point now. */
  const cursor = useMemo((): Point | null => {
    if (!raw) return null;
    if (snap) return snap.point;
    return placesNow ? constrain(prev, onGrid(raw.p), track) : raw.p;
  }, [raw, snap, tool, prev, drafting, placesNow, ucs]);
  const marker = useMemo(
    () => (snap && hover ? markerSegments(snap.point, snap.kind, hover.wpp) : null),
    [snap, hover],
  );

  const draw = state.draw;
  const modify = state.modify;
  const edit = state.edit;
  const text = state.text;
  const dim = state.dim;
  const param = state.param;
  return {
    points: state.points,
    /** A draw tool's state (null for the others), the option words and whether a typed distance
     *  is taken now (a modify tool's too), the last point it placed, and what it would draw at
     *  the cursor. */
    draw,
    /** A modify tool's state; null for the other tools. */
    modify,
    /** An edit tool's state (trim, extend, fillet…); null for the other tools. */
    edit,
    /** A text tool's state (TEXT, MTEXT); null for the other tools. */
    text,
    /** An annotate tool's state; null for the other tools. */
    dim,
    drawOptions: draw
      ? drawOptions(draw)
      : modify
        ? modifyOptions(modify)
        : edit
          ? editOptions(edit)
          : text
            ? textOptions(text)
            : dim
              ? dimOptions(dim)
              : [],
    takesNumber: draw
      ? takesNumber(draw)
      : modify
        ? takesModifyNumber(modify)
        : edit
          ? takesEditNumber(edit)
          : text
            ? takesTextNumber(text)
            : dim
              ? takesDimNumber(dim)
              : false,
    lastDrawPoint:
      lastDrawPoint(state) ??
      lastModifyPoint(state) ??
      lastEditPoint(state) ??
      text?.corner ??
      (dim ? lastDimPoint(dim) : null),
    preview: useMemo(
      () =>
        draw
          ? drawPreview(draw, cursor)
          : modify
            ? modifyPreview(modify, cursor)
            : text
              ? textPreview(text, cursor)
              : dim
                ? dimPreview(dim, cursor)
                : null,
      [draw, modify, text, dim, cursor],
    ),
    /** The entities selected, to draw highlighted: the modify tool's, or the selection set (less
     *  the select tool's own pick, which is highlighted as picked, maybe inside a block). */
    selection: modify
      ? modify.handles
      : edit
        ? editHighlight(edit)
        : dim
          ? dimHighlight(dim)
          : param
            ? constraintHighlight(param)
            : state.selection.filter((h) => !state.selected || h !== state.selected.picked.top),
    /** The select tool's whole selection set (top-level handles): the properties palette sums
     *  it when two or more are picked. */
    picks: tool === "select" ? state.selection : NO_PICKS,
    /** The kind of each handle in the selection set, when known ("Line", "Insert"…). */
    kinds: state.kinds,
    /** Mizan selected `handles` (top-level): the select tool takes them as its selection set. The
     *  app chooses the select tool too. */
    select: (handles: number[]) => dispatch({ type: "aiSelect", handles }),
    /** Dimensions and tables the modify tool just left out: say how many. */
    skipped,
    /** The drawing changed: keep only the selected handles `present`. */
    prune: (present: (handle: number) => boolean) => dispatch({ type: "prune", present }),
    /** A left drag in the viewport draws a selection box now. */
    boxes:
      tool === "select" ||
      tool === "mizanArea" ||
      modifyPhase === "select" ||
      modifyPhase === "window" ||
      (edit !== null && editBoxes(edit)) ||
      (dim !== null && dimBoxes(dim)) ||
      (param !== null && constraintBoxes(param)),
    onBox,
    /** Hands a typed option or distance to the draw or modify tool. */
    drawInput: (input: DrawInput) => dispatch({ type: "draw", input }),
    /** See `cursor` above: snapped, on the grid or tracked. */
    cursor,
    /** The object snap under the cursor, for the status bar. */
    snap,
    /** Its marker, segments in drawing coordinates for the overlay. */
    marker,
    onMove,
    hint: state.hint,
    hintArgs: state.hintArgs,
    /** What `select` would measure now, or what `divide` and `measureAlong` will place points
     *  along, to draw highlighted. */
    highlighted: selectedTarget(state.selected) ?? state.asking?.target ?? null,
    /** `divide` or `measureAlong` waits for its number: which one. */
    asking: state.asking?.kind ?? null,
    /** `zoomWindow` has its first corner and waits for the other. */
    corner: state.corner,
    /** Hands the typed number of parts or step (drawing units) to `divide` or `measureAlong`. */
    answer: (value: number) => dispatch({ type: "answer", value }),
    onClick,
    /** A point typed on the command line, for `distance`, `area`, `idPoint` and the corners of
     *  `zoomWindow`: placed as typed, without snaps, grid, ortho or polar, and never half of a
     *  double click. */
    typePoint: (p: Point) =>
      dispatch({
        type: "click",
        world: p,
        screen: [Number.NaN, Number.NaN],
        time: performance.now(),
        picked: null,
        info: null,
        snapped: true,
        snapKind: null,
      }),
    finish: () => dispatch({ type: "finish" }),
    /** The Esc key: keeps a polyline drawn so far. */
    escape: () => dispatch({ type: "escape" }),
    /** Drops the tool's state without drawing anything: for resets, not the Esc key. */
    cancel: () => dispatch({ type: "cancel" }),
  };
}
