/** Blocks: BLOCK's name dialog, INSERT (the palette, the Design Center, attribute prompts),
 *  WBLOCK, ATTDEF and ATTEDIT, the attribute manager, the tool palette, and the end of an edit in
 *  place. */
import { useRef, useState, type RefObject } from "react";
import { save } from "../fileDialogs";
import {
  blocks as readBlocks,
  blockThumbnail,
  drawingInfo,
  getSettings,
  runCommand,
  setToolPalette,
  writeBlock,
  type BlockInfo,
  type Command,
  type DrawingInfo,
  type PaletteItem,
} from "../api";
import { promptsFor, type AttributeField } from "../shell/BlockDialogs";
import { isStale } from "../shell/switcher";
import { parse, type Action } from "../tools/commandParser";
import type { TextRequest } from "../tools/textTools";
import type { Point } from "../viewport/camera";
import type { T } from "./shared";
import type { Commands, ToolChoice } from "./useCommands";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { TakeoffState } from "./useTakeoff";

type Deps = Pick<Shell, "run" | "setError" | "setNotice" | "focusDrawing" | "commandAliases"> &
  Pick<Sheets, "redraw"> &
  Pick<TakeoffState, "refreshTakeoffAndBoq"> &
  Pick<ToolChoice, "chooseTool"> & {
    t: T;
    switchGen: RefObject<number>;
    info: DrawingInfo | null;
    setInfo(info: DrawingInfo): void;
    autosave(): Promise<void>;
    /** The root's `apply` and `onCommand`, filled after this hook runs (they call back into it). */
    apply: RefObject<Commands["apply"] | null>;
    onCommand: RefObject<((a: Action) => void) | null>;
  };

export function useBlocks({
  t,
  switchGen,
  info,
  setInfo,
  setError,
  setNotice,
  run,
  autosave,
  redraw,
  refreshTakeoffAndBoq,
  focusDrawing,
  chooseTool,
  commandAliases,
  apply,
  onCommand,
}: Deps) {
  /** BLOCK's base point is clicked: the command waiting for the new block's name, and the names
   *  taken. */
  const [createBlockAsk, setCreateBlockAsk] = useState<{
    cmd: Extract<Command, { type: "create_block" }>;
    names: string[];
  } | null>(null);
  /** The block INSERT places, and the drawing its definition comes from when this one lacks it. */
  const [insertChoice, setInsertChoice] = useState<{ block: BlockInfo; fromPath: string | null } | null>(null);
  /** Attribute values asked for (INSERT's prompts, ATTEDIT): the command they make, if any. */
  const [attributesAsk, setAttributesAsk] = useState<{
    heading: string;
    fields: AttributeField[];
    command(values: Record<string, string>): Command | null;
  } | null>(null);
  /** ATTDEF's click, waiting for the definition. */
  const [attDefAt, setAttDefAt] = useState<{ at: Point; height: number | null } | null>(null);
  const [blockPaletteOpen, setBlockPaletteOpen] = useState(false);
  /** The Design Center is open; the drawing's own block names (upper case). */
  const [designCenter, setDesignCenter] = useState<string[] | null>(null);
  /** The tool palette is open, with its buttons. */
  const [palette, setPalette] = useState<PaletteItem[] | null>(null);
  /** BATTMAN is open on these blocks. */
  const [managerBlocks, setManagerBlocks] = useState<BlockInfo[] | null>(null);
  /** The block palette's thumbnails, revoked when it closes. */
  const thumbUrls = useRef<string[]>([]);

  /** Runs a block command from a dialog or the edit bar, then draws it and reads the take-off
   *  (rows may lose their links) and the drawing's state (an edit in place may end) again; an
   *  error goes back to the caller. */
  async function runBlock(cmd: Command) {
    const gen = switchGen.current;
    const outcome = await runCommand(cmd);
    await redraw();
    await refreshTakeoffAndBoq();
    const next = await drawingInfo();
    if (!isStale(gen, switchGen.current)) setInfo(next);
    await autosave();
    return outcome;
  }

  /** A block's picture for the block palette, as a URL revoked when the palette closes. */
  async function blockPicture(name: string) {
    const png = await blockThumbnail(name, 96);
    const url = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    thumbUrls.current.push(url);
    return url;
  }

  function closeBlockPalette() {
    thumbUrls.current.forEach((url) => URL.revokeObjectURL(url));
    thumbUrls.current = [];
    setBlockPaletteOpen(false);
    focusDrawing();
  }

  /** Starts INSERT with `block`; with `fromPath` its definition is copied from that drawing when
   *  this one lacks it. */
  function startInsert(block: BlockInfo, fromPath: string | null = null) {
    setInsertChoice({ block, fromPath });
    chooseTool("insertBlock");
  }

  /** INSERT's click: the block goes there, after its attribute prompts when it has any. */
  function placeBlock(r: Extract<TextRequest, { kind: "insertBlock" }>) {
    const choice = insertChoice;
    if (!choice) {
      setBlockPaletteOpen(true);
      return;
    }
    const command = (values: Record<string, string>): Command => ({
      type: "insert_block",
      name: choice.block.name,
      at: r.at,
      scale: r.scale,
      rotation: r.rotation,
      attribute_values: values,
      from_path: choice.fromPath,
    });
    const fields = promptsFor(choice.block.attdefs);
    if (fields.length === 0) {
      apply.current?.(command({}));
      return;
    }
    setAttributesAsk({ heading: "block.attributesTitle", fields, command });
  }

  /** WBLOCK: writes the block to the .dwg the Save dialog names. */
  function writeBlockFile(name: string) {
    run(async () => {
      const path = await save({ defaultPath: `${name}.dwg`, filters: [{ name: "DWG", extensions: ["dwg"] }] });
      if (!path) return;
      await writeBlock(name, path);
      setNotice(t("block.written", { path }));
    });
  }

  /** REFCLOSE and the edit bar: ends the edit in place, saved into the block or discarded. */
  function closeBlockEdit(keep: boolean) {
    const name = info?.block_edit;
    if (!name) {
      setError(t("cmd.notNow", { name: "REFCLOSE" }));
      return;
    }
    run(async () => {
      const outcome = await runBlock({ type: keep ? "save_block_edits" : "discard_block_edits" });
      if (outcome.type !== "block_edit_closed") return;
      const notes = [t(keep ? "block.saved" : "block.discarded", { name })];
      if (outcome.unlinked > 0) notes.push(t("block.closedUnlinked", { n: outcome.unlinked }));
      if (outcome.dropped > 0) notes.push(t("block.closedDropped", { n: outcome.dropped }));
      setNotice(notes.join(" · "));
    });
  }

  function openAttributeManager() {
    run(async () => setManagerBlocks(await readBlocks()));
  }

  function openDesignCenter() {
    run(async () => setDesignCenter((await readBlocks()).map((b) => b.name.toUpperCase())));
  }

  function openToolPalettes() {
    run(async () => setPalette((await getSettings()).tool_palette ?? []));
  }

  /** Puts a button for the block `name` on the tool palette. */
  function addToPalette(name: string) {
    run(async () => {
      const now = (await getSettings()).tool_palette ?? [];
      await setToolPalette([...now, { kind: "block", label: name, value: name }]);
    });
  }

  /** A tool palette button: its block is inserted, its command line run. */
  function runPaletteItem(item: PaletteItem) {
    if (item.kind === "command") {
      const a = parse(item.value, { expect: "command", last: null, aliases: commandAliases });
      if (a.type === "error") setError(t(a.key, a.args));
      else onCommand.current?.(a);
      return;
    }
    run(async () => {
      const block = (await readBlocks()).find((b) => b.name.toUpperCase() === item.value.toUpperCase());
      if (block) startInsert(block);
      else setError(t("block.missing", { name: item.value }));
    });
  }

  return {
    createBlockAsk,
    setCreateBlockAsk,
    attributesAsk,
    setAttributesAsk,
    attDefAt,
    setAttDefAt,
    blockPaletteOpen,
    setBlockPaletteOpen,
    designCenter,
    setDesignCenter,
    palette,
    setPalette,
    managerBlocks,
    setManagerBlocks,
    runBlock,
    blockPicture,
    closeBlockPalette,
    startInsert,
    placeBlock,
    writeBlockFile,
    closeBlockEdit,
    openAttributeManager,
    openDesignCenter,
    openToolPalettes,
    addToPalette,
    runPaletteItem,
  };
}

export type Blocks = ReturnType<typeof useBlocks>;
