/** Text, dimensions, tables and fields: what the annotate tools ask for (the in-place text
 *  editor, the MText and text edit dialogs, a leader's or a tolerance's text, the table and field
 *  dialogs), the dimension style panel, UPDATEFIELD, and find and replace. */
import { useState, type RefObject } from "react";
import {
  blocks as readBlocks,
  drawingInfo,
  entityInfo,
  runCommand,
  tableCellAt,
  type Command,
  type DrawingInfo,
  type FieldKind,
  type TableInfo,
  type TextMatch,
} from "../api";
import { metresPerUnit } from "../shell/AttachPdfDialog";
import { fieldCode } from "../shell/FieldDialog";
import type { FindQuery } from "../shell/FindReplaceDialog";
import { isStale } from "../shell/switcher";
import type { TextDraft } from "../shell/TextEditor";
import type { DimRequest } from "../tools/dimTools";
import { matchBounds } from "../tools/overlay";
import type { TextRequest } from "../tools/textTools";
import type { Point } from "../viewport/camera";
import type { T } from "./shared";
import type { Blocks } from "./useBlocks";
import type { Commands } from "./useCommands";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { TakeoffState } from "./useTakeoff";

type Deps = Pick<Shell, "run" | "setNotice" | "focusDrawing" | "viewport"> &
  Pick<Sheets, "redraw" | "sheet" | "fitMatch" | "chooseSheet" | "batchHeld"> &
  Pick<TakeoffState, "setAiHighlight"> &
  Pick<Blocks, "placeBlock" | "setAttDefAt" | "setAttributesAsk"> & {
    t: T;
    switchGen: RefObject<number>;
    info: DrawingInfo | null;
    setInfo(info: DrawingInfo): void;
    autosave(): Promise<void>;
    /** The root's `apply`, filled after this hook runs (TEXT's text goes through it). */
    apply: RefObject<Commands["apply"] | null>;
  };

export function useAnnotate({
  t,
  switchGen,
  info,
  setInfo,
  setNotice,
  run,
  autosave,
  focusDrawing,
  viewport,
  redraw,
  sheet,
  fitMatch,
  chooseSheet,
  batchHeld,
  setAiHighlight,
  placeBlock,
  setAttDefAt,
  setAttributesAsk,
  apply,
}: Deps) {
  const [findOpen, setFindOpen] = useState(false);
  const [dimStyleOpen, setDimStyleOpen] = useState(false);
  /** What an annotate tool asked the user to type: a leader's text, a tolerance, a dimension's. */
  const [dimText, setDimText] = useState<DimRequest | null>(null);
  /** TABLE's click: where the new table's top left corner goes. */
  const [newTableAt, setNewTableAt] = useState<Point | null>(null);
  /** The table being edited: its handle, its cells as they are now, and the cell chosen first. */
  const [tableEditing, setTableEditing] = useState<{
    handle: number;
    table: TableInfo;
    cell: [number, number];
  } | null>(null);
  /** The field dialog: into a new text at a click, or into a cell of the table being edited; and
   *  the entity an area or a length field reads, once picked. */
  const [fieldAsk, setFieldAsk] = useState<
    { mode: "text"; at: Point; height: number | null } | { mode: "cell"; cell: [number, number] } | null
  >(null);
  const [fieldTarget, setFieldTarget] = useState<{ handle: number; kind: string } | null>(null);
  /** The text height and style new text takes: the last ones used; null until then (2.5 mm in
   *  drawing units, and the drawing's current style). The height is a length in one drawing's
   *  units, so it is kept for that drawing only: 250 in a millimetre plan is never 250 m in a
   *  metre one opened beside it. Paper space keeps a separate height in millimetres. */
  const paperSpace = sheet.toLowerCase() !== "*model_space";
  const [keptHeight, setKeptHeight] = useState<{ path: string; paper: boolean; height: number } | null>(null);
  const textHeight =
    keptHeight && info && keptHeight.path === info.path && keptHeight.paper === paperSpace
      ? keptHeight.height
      : null;
  const setTextHeight = (height: number) =>
    setKeptHeight(info ? { path: info.path, paper: paperSpace, height } : null);
  const [textStyle, setTextStyle] = useState<string | null>(null);
  /** TEXT's in-place editor: where the text goes, and where that is on the canvas. */
  const [textEditor, setTextEditor] = useState<{
    request: Extract<TextRequest, { kind: "text" }>;
    at: Point;
    key: number;
  } | null>(null);
  /** The MText dialog (a new MText's box), or the text edit dialog (an entity's text). */
  const [textDialog, setTextDialog] = useState<
    | { kind: "mtext"; request: Extract<TextRequest, { kind: "mtext" }> }
    | { kind: "edit"; handle: number; value: string; multiline: boolean }
    | null
  >(null);

  /** The height new text takes: the last one used, else 2.5 mm on paper in drawing units at the
   *  annotation scale. */
  const currentTextHeight =
    textHeight ??
    (paperSpace ? 2.5 : (0.0025 / metresPerUnit(info?.units ?? "unknown")) * (info?.annotation_factor ?? 1));
  /** The style new text takes: the last one used when the drawing has it, else its current one. */
  const currentTextStyle =
    textStyle !== null && info?.text_styles.includes(textStyle) ? textStyle : (info?.text_style ?? "Standard");

  /** A text tool placed a text: TEXT opens the in-place editor where it starts, MTEXT the MText
   *  dialog. */
  function placeText(request: TextRequest) {
    if (request.kind === "mtext") {
      setTextDialog({ kind: "mtext", request });
      return;
    }
    if (request.kind === "table") {
      setNewTableAt(request.at);
      return;
    }
    if (request.kind === "field") {
      setFieldTarget(null);
      setFieldAsk({ mode: "text", at: request.at, height: request.height });
      return;
    }
    if (request.kind === "insertBlock") {
      placeBlock(request);
      return;
    }
    if (request.kind === "attdef") {
      setAttDefAt({ at: request.at, height: request.height });
      return;
    }
    const at = viewport.current?.toScreen(request.at) ?? [0, 0];
    setTextEditor({ request, at, key: performance.now() });
  }

  /** Opens the text edit dialog on the text of the top-level entity `handle`, or the table editor
   *  on a table (at the cell under `at`, the click). */
  async function openTextEdit(handle: number, at: Point | null = null) {
    const e = await entityInfo(handle);
    const value = e.plain_text ?? e.text;
    if (e.kind === "Table" && e.table) {
      const cell = (at && (await tableCellAt(handle, at))) ?? [0, 0];
      setTableEditing({ handle, table: e.table, cell });
    } else if ((e.kind === "Text" || e.kind === "MText") && value !== null) {
      setTextDialog({ kind: "edit", handle, value, multiline: e.kind === "MText" });
    } else if (e.kind === "Insert" && e.attributes.length > 0) {
      const defs = (await readBlocks()).find((b) => b.name === e.block_name)?.attdefs ?? [];
      const before = Object.fromEntries(e.attributes);
      setAttributesAsk({
        heading: "block.editAttributesTitle",
        fields: e.attributes.map(([tag, value]) => ({
          tag,
          prompt: defs.find((d) => d.tag === tag)?.prompt ?? "",
          value,
        })),
        // Only what changed; nothing changed sends nothing.
        command: (values) => {
          const changed = Object.entries(values).filter(([tag, v]) => v !== before[tag]);
          if (changed.length === 0) return null;
          return { type: "edit_attributes", target: { handle, chain: [] }, values: Object.fromEntries(changed) };
        },
      });
    } else {
      setNotice(t("text.noText"));
    }
  }

  /** Runs a dimension style or annotation scale change from the style panel, then shows the
   *  drawing's styles again and draws the dimensions the style rebuilt; an error goes back to
   *  the panel. */
  async function runDimStyle(cmd: Command) {
    const gen = switchGen.current;
    await runCommand(cmd);
    const next = await drawingInfo();
    if (isStale(gen, switchGen.current)) return;
    setInfo(next);
    await redraw();
    await autosave();
  }

  /** Opens the dimension style panel on the drawing's styles as they are now. */
  function openDimStyle() {
    const gen = switchGen.current;
    run(async () => {
      const next = await drawingInfo();
      if (isStale(gen, switchGen.current)) return;
      setInfo(next);
      setDimStyleOpen(true);
    });
  }

  /** Runs a text command from a dialog, then draws it; an error goes back to the dialog. */
  async function runText(cmd: Command) {
    await runCommand(cmd);
    await redraw();
    await autosave();
  }

  /** Runs a command on the table being edited, draws it, and hands the editor its cells as they
   *  are now; an error goes back to the editor. */
  async function runTable(cmd: Command) {
    const editing = tableEditing;
    if (!editing) return;
    await runText(cmd);
    const e = await entityInfo(editing.handle);
    if (e.table) setTableEditing({ ...editing, table: e.table });
  }

  /** Inserts the field the dialog chose: a new text where the user clicked, or the field's code
   *  after the text of the table cell being edited. */
  async function insertField(field: FieldKind) {
    const ask = fieldAsk;
    if (ask?.mode === "text") {
      await runText({ type: "field", at: ask.at, height: ask.height ?? currentTextHeight, field });
    } else if (ask?.mode === "cell" && tableEditing) {
      const [row, col] = ask.cell;
      const now = tableEditing.table.cells[row]?.[col] ?? "";
      const text = now ? `${now} ${fieldCode(field)}` : fieldCode(field);
      await runTable({ type: "edit_cell", target: { handle: tableEditing.handle, chain: [] }, row, col, text });
    }
  }

  /** UPDATEFIELD: works every field out again (one undo step when one changed), then draws it. */
  function updateFields() {
    run(async () => {
      const outcome = await runCommand({ type: "update_fields" });
      await redraw();
      await autosave();
      const locked = outcome.type === "updated" ? outcome.skipped : 0;
      const notes = [t("insertField.updated")];
      if (locked > 0) notes.push(t("locked.skipped", { n: locked }));
      setNotice(notes.join(" · "));
    });
  }

  /** Places TEXT's text, remembering its height and style for the next one. */
  function placeInPlace(request: Extract<TextRequest, { kind: "text" }>, draft: TextDraft) {
    setTextEditor(null);
    setTextHeight(draft.height);
    setTextStyle(draft.style);
    apply.current?.({
      type: "text",
      at: request.at,
      height: draft.height,
      rotation: request.rotation,
      value: draft.value,
      style: draft.style,
    });
    focusDrawing();
  }

  /** The find dialog's search: the matches, nothing changed. */
  async function findTexts(q: FindQuery) {
    const outcome = await runCommand({ type: "find_text", ...q });
    return outcome.type === "found" ? outcome : { count: 0, matches: [] };
  }

  /** Replaces every match (one undo step), draws the change and highlights what changed. */
  async function replaceTexts(q: FindQuery, replace: string) {
    const outcome = await runCommand({ type: "find_replace", ...q, replace });
    if (outcome.type !== "replaced") return { count: 0, skipped: 0 };
    if (outcome.count > 0) {
      await redraw();
      setAiHighlight(outcome.handles);
      await autosave();
    }
    return { count: outcome.count, skipped: outcome.skipped };
  }

  /** Shows one entity the find found, highlighted and zoomed to with room around; on another
   *  sheet, that sheet is shown and its first fit goes to the match. */
  async function showMatch(m: TextMatch) {
    setAiHighlight([m.handle]);
    if (m.sheet.toLowerCase() !== sheet.toLowerCase()) {
      fitMatch.current = m.handle;
      await chooseSheet(m.sheet);
      return;
    }
    const box = matchBounds(m.handle, batchHeld.current);
    if (box) viewport.current?.fitTo(box);
  }

  return {
    findOpen,
    setFindOpen,
    dimStyleOpen,
    setDimStyleOpen,
    dimText,
    setDimText,
    newTableAt,
    setNewTableAt,
    tableEditing,
    setTableEditing,
    fieldAsk,
    setFieldAsk,
    fieldTarget,
    setFieldTarget,
    setTextHeight,
    setTextStyle,
    textEditor,
    setTextEditor,
    textDialog,
    setTextDialog,
    currentTextHeight,
    currentTextStyle,
    placeText,
    openTextEdit,
    runDimStyle,
    openDimStyle,
    runText,
    runTable,
    insertField,
    updateFields,
    placeInPlace,
    findTexts,
    replaceTexts,
    showMatch,
  };
}

export type Annotate = ReturnType<typeof useAnnotate>;
