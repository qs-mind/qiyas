/** The tool in use and what runs commands: the tool choice (with the command line's preset and
 *  last point), `apply` for the commands tools, panels and the command line make, and undo and
 *  redo (`history`). */
import { useRef, useState } from "react";
import { blocks as readBlocks, drawingInfo, redo, runCommand, type Command, type DrawingInfo } from "../api";
import type { Tool } from "../shell/Ribbon";
import type { Point } from "../viewport/camera";
import type { T } from "./shared";
import type { Layers } from "./useLayers";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { TakeoffState } from "./useTakeoff";

/** The tool chosen, and what the command line keeps for it. */
export function useToolChoice(focusDrawing: () => void) {
  const [tool, setTool] = useState<Tool>("select");
  /** The last point clicked or typed, for the command line's `@`. */
  const lastPoint = useRef<Point | null>(null);
  /** DIVIDE n or MEASURE step typed on the command line: the answer once the entity is clicked. */
  const preset = useRef<{ tool: Tool; value: number } | null>(null);
  /** The last tool chosen other than Select: Enter, with nothing under way, does it again (AutoCAD's
   *  repeat of the last command). */
  const lastTool = useRef<Tool | null>(null);

  function chooseTool(next: Tool) {
    preset.current = null;
    if (next !== "select") lastTool.current = next;
    setTool(next);
    focusDrawing();
  }

  return { tool, setTool, lastPoint, preset, chooseTool, lastTool };
}

export type ToolChoice = ReturnType<typeof useToolChoice>;

type Deps = Pick<Shell, "run" | "setNotice"> &
  Pick<Sheets, "redraw"> &
  Pick<TakeoffState, "refreshTakeoffAndBoq" | "setInfoCard" | "setSelectedId"> &
  Pick<Layers, "syncInfo" | "runDrawingCommand" | "setGroupAsk"> &
  Pick<ToolChoice, "chooseTool"> & {
    t: T;
    info: DrawingInfo | null;
    setInfo(info: DrawingInfo): void;
    autosave(): Promise<void>;
    /** BLOCK's name dialog: the command waiting for the name, and the names taken. */
    setCreateBlockAsk(ask: { cmd: Extract<Command, { type: "create_block" }>; names: string[] }): void;
    /** The height new text takes: plain text pastes as a text this tall. */
    currentTextHeight: number;
  };

export function useCommands({
  t,
  info,
  setInfo,
  setNotice,
  run,
  autosave,
  redraw,
  refreshTakeoffAndBoq,
  setInfoCard,
  setSelectedId,
  chooseTool,
  syncInfo,
  runDrawingCommand,
  setGroupAsk,
  setCreateBlockAsk,
  currentTextHeight,
}: Deps) {
  /** Runs a take-off command; a new measurement becomes the selected row. An inquiry's answer
   *  shows in the info card. */
  async function apply(given: Command, refreshAll = false) {
    // Plain text pastes as a text as tall as new text is.
    const cmd: Command =
      given.type === "paste_from_clipboard" ? { ...given, height: currentTextHeight } : given;
    // BLOCK's base point is clicked: the name comes from its dialog.
    if (cmd.type === "create_block" && cmd.name === "") {
      await run(async () => setCreateBlockAsk({ cmd, names: (await readBlocks()).map((b) => b.name) }));
      return;
    }
    // GROUP's entities are picked: the name comes from its dialog.
    if (cmd.type === "create_group" && cmd.name === "") {
      setGroupAsk(cmd);
      return;
    }
    // A copy goes onto the system clipboard and changes nothing; back to Select.
    if (cmd.type === "copy_to_clipboard") {
      await run(async () => {
        const outcome = await runCommand(cmd);
        if (outcome.type === "clipboard") setNotice(t("clip.copied", { n: outcome.count }));
        chooseTool("select");
      });
      return;
    }
    // The UCS is view state: the info carries it to the cursor, typed points and the grid; the
    // project file keeps it.
    if (cmd.type === "ucs" || cmd.type === "ucs_object") {
      let set = false;
      await run(async () => {
        await runCommand(cmd);
        await syncInfo();
        set = true;
        const world = cmd.type === "ucs" && cmd.origin[0] === 0 && cmd.origin[1] === 0 && cmd.x_axis[0] > 0 && cmd.x_axis[1] === 0;
        setNotice(t(world ? "ucs.worldSet" : "ucs.set"));
      });
      if (set) await autosave();
      return;
    }
    // Groups and matched properties change what the layers panel and the palette show.
    if (cmd.type === "ungroup" || cmd.type === "match_properties") {
      await run(async () => {
        await runDrawingCommand(cmd);
      });
      return;
    }
    let changed = false;
    await run(async () => {
      const outcome = await runCommand(cmd);
      // A registry addition may change any drawing part without a window-side command list.
      if (refreshAll) {
        await redraw();
        await syncInfo();
        changed = true;
      }
      if (outcome.type === "nothing") return;
      if (outcome.type === "info") {
        setInfoCard({ title: outcome.title, rows: outcome.rows });
        return;
      }
      changed = true;
      await refreshTakeoffAndBoq();
      // Divide, measure along, the draws and the modifies change the drawing itself.
      const drawingChanged = [
        "divide_along",
        "measure_along",
        "draw",
        "modify",
        "edit",
        "delete_entities",
        "text",
        "mtext",
        "edit_text",
        "find_replace",
        "dimension",
        "dim_edit",
        "dim_break",
        "dim_space",
        "dim_jog_line",
        "leader",
        "mleader",
        "tolerance",
        "insert_block",
        "edit_block_in_place",
        "paste_from_clipboard",
        "mview",
        "add_constraint",
        "delete_constraint",
        "delete_constraints_on",
        "auto_constrain",
        "add_dim_constraint",
        "set_parameter",
      ];
      if (!refreshAll && drawingChanged.includes(cmd.type)) {
        await redraw();
      }
      // A new viewport shows in the layers panel's VP freeze column.
      if (cmd.type === "mview") await syncInfo();
      if (cmd.type === "paste_from_clipboard" && outcome.type === "added") {
        setNotice(t("clip.pasted", { n: outcome.handles.length }));
      }
      // An edit in place began: the bar to end it shows, and Select works on the block.
      if (cmd.type === "edit_block_in_place") {
        setInfo(await drawingInfo());
        chooseTool("select");
      }
      if (outcome.type === "measured") setSelectedId(outcome.measurement.id);
      // AUTOCONSTRAIN and DELCONSTRAINT say how many.
      if (outcome.type === "constrained" && cmd.type === "auto_constrain") {
        setNotice(t("param.added", { n: outcome.ids.length }));
      }
      if (outcome.type === "constrained" && cmd.type === "delete_constraints_on") {
        setNotice(t("param.removed", { n: outcome.ids.length }));
      }
      // Delete says how many it erased, with what the take-off did about them.
      const erased = cmd.type === "delete_entities" ? t("modify.erased", { n: cmd.targets.length }) : null;
      if (erased && outcome.type !== "takeoff_followed") setNotice(erased);
      if (outcome.type === "exploded" || outcome.type === "takeoff_followed") {
        const notes: string[] = erased ? [erased] : [];
        if (outcome.type === "exploded" && outcome.left_alone > 0) {
          notes.push(t("modify.leftAlone", { n: outcome.left_alone }));
        }
        if (outcome.remeasured > 0) notes.push(t("modify.remeasured", { n: outcome.remeasured }));
        if (outcome.unlinked > 0) notes.push(t("modify.unlinked", { n: outcome.unlinked }));
        if (notes.length > 0) setNotice(notes.join(" · "));
      }
      if (outcome.type === "edited") {
        const notes: string[] = [];
        if (cmd.type === "edit" && cmd.kind === "overkill") {
          notes.push(t("edit.removed", { n: outcome.removed.length }));
        } else if (outcome.left_alone > 0) {
          notes.push(t("edit.leftAlone", { n: outcome.left_alone }));
        }
        if (outcome.unlinked > 0) notes.push(t("edit.unlinked", { n: outcome.unlinked }));
        if (outcome.joined_rows > 1) notes.push(t("edit.joinedRows", { n: outcome.joined_rows }));
        if (outcome.constraints_removed) notes.push(t("param.removed", { n: outcome.constraints_removed }));
        if (notes.length > 0) setNotice(notes.join(" · "));
      }
    });
    if (changed) await autosave();
  }

  async function history(step: () => Promise<boolean>) {
    let changed = false;
    // Every undo and redo says what it did (a key press otherwise shows nothing when it changes
    // nothing): done, or that there was nothing.
    const redoing = step === redo;
    await run(async () => {
      if (!info) return;
      if (!(await step())) {
        setNotice(t(redoing ? "history.nothingRedo" : "history.nothingUndo"));
        return;
      }
      setNotice(t(redoing ? "history.redone" : "history.undone"));
      changed = true;
      await refreshTakeoffAndBoq();
      // The step may have been a PDF import, an underlay or a calibration.
      await redraw();
      // Or a layer change: the panel shows the layers and hidden ones as they are now.
      await syncInfo();
    });
    if (changed) await autosave();
  }

  return { apply, history };
}

export type Commands = ReturnType<typeof useCommands>;
