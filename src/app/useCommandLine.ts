/** The command line and the keyboard: what a typed command or a key does (through the same
 *  handlers as the ribbon), a click in the drawing, Escape, the DIVIDE and MEASURE presets, and
 *  what the command line and the status bar say. */
import { useEffect, useRef, useState, type RefObject } from "react";
import { cmdlineStep, EMPTY_MODEL, pick, redo, undo, type CmdlineStep, type DrawingInfo } from "../api";
import type { Assistant } from "../ai/useAssistant";
import { metresPerUnit } from "../shell/AttachPdfDialog";
import type { DotPattern } from "../shell/DotLoader";
import type { Tool } from "../shell/Ribbon";
import type { Action, Expect, ParseState } from "../tools/commandParser";
import { shortcutFor, userShortcut } from "../tools/shortcuts";
import { toolLabel } from "../shell/toolLabel";
import type { Point } from "../viewport/camera";
import { type Measure, type T, typing } from "./shared";
import type { Annotate } from "./useAnnotate";
import type { Attach } from "./useAttach";
import type { Blocks } from "./useBlocks";
import type { Commands, ToolChoice } from "./useCommands";
import type { DraftingState } from "./useDrafting";
import type { Drawings } from "./useDrawings";
import type { Layers } from "./useLayers";
import type { Options } from "./useOptions";
import type { Parametric } from "./useParametric";
import type { Plot } from "./usePlot";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { Takeoff, TakeoffState } from "./useTakeoff";

type Deps = {
  t: T;
  switchGen: RefObject<number>;
  info: DrawingInfo | null;
  measure: Measure;
  assistant: Pick<Assistant, "send">;
  shell: Pick<Shell, "run" | "setError" | "setNotice" | "setSideTab" | "viewport" | "commandAliases" | "askMizan" | "setShortcutsOpen">;
  drafting: Pick<DraftingState, "toggleDrafting">;
  toolChoice: ToolChoice;
  commands: Commands;
  sheets: Pick<
    Sheets,
    "batch" | "renderProgress" | "split" | "setSplit" | "hidden" | "chooseSheet" | "ucsIcon" | "chooseUcsIcon" | "cancelReveal"
  >;
  layers: Pick<
    Layers,
    "applyHidden" | "isolateLayer" | "unisolateLayers" | "showAll" | "setLayerStatesOpen" | "setTranslatorOpen"
  >;
  attach: Pick<
    Attach,
    "attachOpen" | "setAttachOpen" | "setAttachImageOpen" | "pending" | "setPending" | "setPickedInsert" | "pendingPoint"
  >;
  blocks: Pick<Blocks, "setBlockPaletteOpen" | "closeBlockEdit" | "openAttributeManager">;
  annotate: Pick<Annotate, "setFindOpen" | "openDimStyle" | "updateFields" | "setTextEditor" | "setFieldTarget">;
  drawings: Pick<
    Drawings,
    | "opening"
    | "switching"
    | "openFile"
    | "setNewOpen"
    | "recoverFile"
    | "runScriptFile"
    | "scriptLine"
    | "setSheetSetShown"
    | "saveFile"
    | "saveFileAs"
    | "exportFile"
    | "setXrefsOpen"
    | "setExtractOpen"
    | "closeFileTab"
    | "cycleTabs"
  >;
  takeoffState: Pick<
    TakeoffState,
    "setInfoCard" | "setSelectedId" | "setBoqCode" | "setAiHighlight" | "setConflictIds" | "comparing"
  >;
  takeoff: Pick<Takeoff, "importBoqFile" | "compareNow">;
  plot: Pick<Plot, "windowPick" | "pickPoint" | "cancelPick" | "openPlot" | "quickPrint" | "printAll" | "openStyles">;
  options: Pick<
    Options,
    "shortcuts" | "setOptionsOpen" | "setCustomizeOpen" | "setAliasesOpen" | "openPurge" | "openAudit"
  >;
  parametric: Pick<Parametric, "openParameters">;
};

export function useCommandLine({
  t,
  switchGen,
  info,
  measure,
  assistant,
  shell,
  drafting,
  toolChoice,
  commands,
  sheets,
  layers,
  attach,
  blocks,
  annotate,
  drawings,
  takeoffState,
  takeoff,
  plot,
  options,
  parametric,
}: Deps) {
  const { run, setError, setNotice, setSideTab, viewport, commandAliases, askMizan, setShortcutsOpen } = shell;
  const { toggleDrafting } = drafting;
  const { tool, setTool, lastPoint, preset, chooseTool, lastTool } = toolChoice;
  const { apply, history } = commands;
  const { batch, renderProgress, split, setSplit, hidden, chooseSheet, ucsIcon, chooseUcsIcon } = sheets;
  const { applyHidden, isolateLayer, unisolateLayers, showAll, setLayerStatesOpen, setTranslatorOpen } = layers;
  const { attachOpen, setAttachOpen, setAttachImageOpen, pending, setPending, setPickedInsert, pendingPoint } = attach;
  const { setBlockPaletteOpen, closeBlockEdit, openAttributeManager } = blocks;
  const { setFindOpen, openDimStyle, updateFields, setTextEditor, setFieldTarget } = annotate;
  const {
    opening,
    switching,
    openFile,
    setNewOpen,
    recoverFile,
    runScriptFile,
    scriptLine,
    setSheetSetShown,
    saveFile,
    saveFileAs,
    exportFile,
    setXrefsOpen,
    setExtractOpen,
    closeFileTab,
    cycleTabs,
  } = drawings;
  const { setInfoCard, setSelectedId, setBoqCode, setAiHighlight, setConflictIds, comparing } = takeoffState;
  const { importBoqFile, compareNow } = takeoff;
  const { windowPick, pickPoint, cancelPick, openPlot, quickPrint, printAll, openStyles } = plot;
  /** The command line shows (Ctrl+9), its input, and whether to focus it once shown. */
  const [commandLineOn, setCommandLineOn] = useState(true);
  const commandInput = useRef<HTMLInputElement>(null);
  const focusCommandLine = useRef(false);
  const [typedPrompt, setTypedPrompt] = useState<string | null>(null);
  const typedActive = useRef(false);
  const typedGeneration = useRef(0);
  const typedCalls = useRef<Promise<unknown>>(Promise.resolve());

  /** A cancellation must finish before the next line starts its machine. */
  function readTyped(line: string, last?: Point | null): Promise<CmdlineStep> {
    const next = typedCalls.current.then(() => cmdlineStep(line, last));
    typedCalls.current = next.catch(() => {});
    return next;
  }

  function cancelTyped() {
    typedGeneration.current++;
    if (typedActive.current) void readTyped("ESC").catch(() => {});
    typedActive.current = false;
    setTypedPrompt(null);
  }

  useEffect(() => {
    cancelTyped();
  }, [info?.path, info?.sheet, tool]);

  /** Registry commands use the same refresh and save path as drawing tools. */
  async function typeCommand(line: string): Promise<CmdlineStep> {
    if (scriptLine !== null) throw new Error(t("script.busy"));
    if (switching.current > 0) return { type: "cancelled" };
    const last = measure.lastDrawPoint ?? measure.points[measure.points.length - 1] ?? lastPoint.current;
    if (!typedActive.current) {
      measure.cancel();
      setPending(null);
      cancelPick();
    }
    const generation = typedGeneration.current;
    const drawingGeneration = switchGen.current;
    const wasActive = typedActive.current;
    typedActive.current = true;
    let result: CmdlineStep;
    try {
      result = await readTyped(line, last);
    } catch (error) {
      if (generation === typedGeneration.current) typedActive.current = wasActive;
      throw error;
    }
    if (generation !== typedGeneration.current || drawingGeneration !== switchGen.current || switching.current > 0) {
      return { type: "cancelled" };
    }
    if (result.type !== "cancelled") lastPoint.current = result.last;
    typedActive.current = result.type === "prompt";
    setTypedPrompt(result.type === "prompt" ? result.prompt : null);
    if (result.type === "command") {
      sheets.cancelReveal();
      await apply(result.command, true);
    }
    return result;
  }

  async function startCommand(line: string): Promise<void> {
    cancelTyped();
    setCommandLineOn(true);
    await typeCommand(line);
    commandInput.current?.focus();
  }

  /** A click in the drawing: the one a plot window, a PDF or image insert point or a calibration
   *  waits for, else the tool's. */
  function clickDrawing(world: Point, screen: Point, worldPerPixel: number, shift: boolean) {
    // The drawing under the click may be about to go.
    if (switching.current > 0) return;
    if (typedActive.current) cancelTyped();
    lastPoint.current = world;
    if (pickPoint(world)) return;
    if (pending?.kind === "fieldTarget") {
      // The entity at the top of the sheet a field reads.
      run(async () => {
        const picked = await pick(world[0], world[1], 6 * worldPerPixel);
        if (!picked) return;
        setFieldTarget({ handle: picked.top, kind: picked.kind ?? "" });
        setPending(null);
      });
      return;
    }
    if (!pendingPoint(world)) measure.onClick(world, screen, worldPerPixel, shift);
  }

  /** Escape: drops what is waiting (an insert point, a measure, a prompt) and every highlight.
   *  `escKey` for the Esc key itself (or ESC typed), which keeps a polyline drawn so far. */
  /** The tool and time of the last Esc: a second one soon after, in the same tool, ends the tool
   *  (AutoCAD: Esc cancels the input, Esc again leaves the command). */
  const lastEsc = useRef<{ tool: Tool; at: number } | null>(null);
  function cancelAll(escKey = false) {
    if (escKey) {
      const now = Date.now();
      const again = lastEsc.current;
      lastEsc.current = tool !== "select" ? { tool, at: now } : null;
      if (again && again.tool === tool && tool !== "select" && now - again.at < 1500) {
        lastEsc.current = null;
        chooseTool("select");
      }
    }
    cancelTyped();
    preset.current = null;
    setPending(null);
    cancelPick();
    setTextEditor(null);
    setInfoCard(null);
    if (escKey) measure.escape();
    else measure.cancel();
    setSelectedId(null);
    setBoqCode(null);
    setAiHighlight([]);
    setConflictIds([]);
  }

  /** Answers divide's number of parts or measure along's step (metres). */
  function answerAlong(value: number) {
    if (measure.asking === "divide") measure.answer(value);
    else if (measure.asking === "measureAlong" && info) measure.answer(value / metresPerUnit(info.units));
  }

  // DIVIDE n and MEASURE step: the number typed with the command answers once the entity is
  // clicked. It is used once, and dropped when the prompt closes or another tool is chosen, so it
  // never answers a later prompt.
  useEffect(() => {
    const typed = preset.current;
    preset.current = null;
    if (typed && measure.asking && typed.tool === tool) answerAlong(typed.value);
  }, [measure.asking]);
  useEffect(() => {
    if (preset.current?.tool !== tool) preset.current = null;
  }, [tool]);
  useEffect(() => {
    if (commandLineOn && focusCommandLine.current) commandInput.current?.focus();
    focusCommandLine.current = false;
  }, [commandLineOn]);

  /** The layer named `name` in any case, as the drawing spells it; null (and an error) when none. */
  function layerNamed(name: string): string | null {
    const found = info?.layers.find((l) => l.name.toUpperCase() === name.toUpperCase());
    if (!found) setError(t("cmd.noLayer", { name }));
    return found?.name ?? null;
  }

  /** Runs what the command line asks for, through the same handlers as the ribbon and keys. */
  function onCommand(a: Action) {
    if (a.type !== "repeat") cancelTyped();
    sheets.cancelReveal();
    setNotice(null);
    // A running script: nothing else runs until it ends or is stopped.
    if (scriptLine !== null) {
      setError(t("script.busy"));
      return;
    }
    const needDrawing = () => {
      if (!info) setError(t("cmd.noDrawing"));
      return !info;
    };
    switch (a.type) {
      case "repeat":
        return;
      case "finish":
        measure.finish();
        return;
      case "cancel":
        cancelAll(true);
        return;
      case "point":
        if (switching.current > 0) return;
        lastPoint.current = a.p;
        if (!pickPoint(a.p) && !pendingPoint(a.p)) measure.typePoint(a.p);
        return;
      case "number":
        answerAlong(a.value);
        return;
      case "option":
        measure.drawInput({ type: "option", option: a.option });
        return;
      case "distance":
        measure.drawInput({ type: "number", value: a.value });
        return;
      case "draw":
        if (!needDrawing()) apply({ type: "draw", ...a.spec });
        return;
      case "tool":
        if (needDrawing()) return;
        // DIVIDE n while divide already asks for its number: that is the answer.
        if (a.preset !== undefined && a.tool === tool && measure.asking === tool) {
          answerAlong(a.preset);
          return;
        }
        preset.current = a.preset !== undefined ? { tool: a.tool, value: a.preset } : null;
        setTool(a.tool);
        return;
      case "count":
        if (!needDrawing()) apply({ type: "count_block", block_name: a.block, layer: null });
        return;
      case "layers": {
        if (needDrawing()) return;
        const names = a.names.map(layerNamed);
        if (names.includes(null)) return;
        const next = new Set(hidden);
        for (const n of names as string[]) {
          if (a.hide) next.add(n);
          else next.delete(n);
        }
        applyHidden(next);
        return;
      }
      case "isolate": {
        const name = needDrawing() ? null : layerNamed(a.name);
        if (name) isolateLayer(name);
        return;
      }
      case "unisolate":
        if (!needDrawing()) unisolateLayers();
        return;
      case "showAll":
        if (!needDrawing()) showAll();
        return;
      case "zoom":
        if (a.how === "extents") viewport.current?.fitExtents();
        else viewport.current?.zoomBy(a.how === "in" ? 0.5 : 2);
        return;
      case "toggle":
        if (!needDrawing()) toggleDrafting(a.toggle);
        return;
      case "ucsIcon":
        chooseUcsIcon(a.on ?? !ucsIcon);
        return;
      case "sheet": {
        if (needDrawing()) return;
        const want = a.name.toUpperCase();
        const found = info!.sheets.find(
          (s) => s.name.toUpperCase() === want || s.block.toUpperCase() === want,
        );
        if (found) chooseSheet(found.block);
        else setError(t("cmd.noSheet", { name: a.name }));
        return;
      }
      case "ai":
        setSideTab("assistant");
        assistant.send(a.text);
        return;
      case "run":
        runCommandLine(a.what);
    }
  }

  /** The command line's file, view and BOQ commands: what the ribbon's buttons do, when they
   *  are enabled. */
  function runCommandLine(what: Extract<Action, { type: "run" }>["what"]) {
    if (what === "open") {
      openFile();
      return;
    }
    if (what === "new") {
      setNewOpen(true);
      return;
    }
    // A plot style file needs no drawing.
    if (what === "plotStyles") {
      openStyles();
      return;
    }
    // Both open drawings: none needs to be open.
    if (what === "recover") {
      recoverFile();
      return;
    }
    if (what === "sheetSet") {
      setSheetSetShown(true);
      return;
    }
    // A script may OPEN its drawing.
    if (what === "script") {
      runScriptFile();
      return;
    }
    // The window's own settings need no drawing.
    if (what === "shortcuts") return setShortcutsOpen(true);
    if (what === "options") return options.setOptionsOpen(true);
    if (what === "customize") return options.setCustomizeOpen(true);
    if (what === "aliasEdit") return options.setAliasesOpen(true);
    if (!info) {
      setError(t("cmd.noDrawing"));
      return;
    }
    switch (what) {
      case "save":
        return saveFile();
      case "saveAs":
        return saveFileAs();
      case "export":
        return exportFile();
      case "importBoq":
        return importBoqFile();
      case "compare":
        return compareNow();
      case "undo":
        return history(undo);
      case "redo":
        return history(redo);
      case "attachPdf":
        setPickedInsert(null);
        return setAttachOpen(true);
      case "attachImage":
        setPickedInsert(null);
        return setAttachImageOpen(true);
      case "xrefs":
        return setXrefsOpen(true);
      case "extract":
        return setExtractOpen(true);
      case "plot":
        return openPlot();
      case "purge":
        return options.openPurge();
      case "audit":
        return options.openAudit();
      case "quickPrint":
        return quickPrint();
      case "printAll":
        return printAll();
      case "find":
        return setFindOpen(true);
      case "dimStyle":
        return openDimStyle();
      case "updateFields":
        return updateFields();
      case "parameters":
        if (info) parametric.openParameters();
        return;
      case "insertBlock":
      case "writeBlock":
        return setBlockPaletteOpen(true);
      case "refClose":
        return closeBlockEdit(true);
      case "attributeManager":
        return openAttributeManager();
      case "layers":
        document.querySelector<HTMLElement>(".layers button, .layers input")?.focus();
        return;
      case "layerStates":
        return setLayerStatesOpen(true);
      case "layerTranslate":
        return setTranslatorOpen(true);
      case "vports":
        return setSplit(!split);
      case "vports1":
        return setSplit(false);
      case "vports2":
        return setSplit(true);
      case "ucsWorld":
        return apply({ type: "ucs", origin: [0, 0], x_axis: [1, 0] });
      case "vpLayer": {
        const box = document.querySelector<HTMLElement>(".layers .vp-freeze");
        if (box) box.focus();
        else setError(t("layer.vpNone"));
        return;
      }
      case "properties":
        document.querySelector<HTMLElement>(".properties h2 button")?.focus();
        return;
      case "calibrate":
        if (!batch || batch.imageTable.length === 0) {
          setError(t("cmd.notNow", { name: "CALIBRATE" }));
          return;
        }
        measure.cancel();
        setPending({ kind: "calibrate", first: null });
    }
  }

  /** Enter (or a tap of Space) with nothing under way does the last tool again, as AutoCAD repeats
   *  its last command; it says which, or that there is none. False when something is under way. */
  function repeatLast(): boolean {
    if (tool !== "select" || measure.highlighted !== null || measure.picks.length > 0) return false;
    if (!info) return false;
    const again = lastTool.current;
    if (!again) {
      setNotice(t("repeat.none"));
      return true;
    }
    setNotice(t("repeat.last", { name: toolLabel(again, t) }));
    chooseTool(again);
    return true;
  }

  /** Select all (Ctrl+A): the Select tool's selection set takes every entity of the sheet on screen. */
  const selectAllNext = useRef(false);
  const announceSelection = useRef(false);
  function selectAll() {
    if (!info || !batch) {
      setNotice(t("select.none"));
      return;
    }
    if (tool !== "select") {
      // The Select tool first; the selection is made once it is on (the effect below).
      selectAllNext.current = true;
      chooseTool("select");
      return;
    }
    announceSelection.current = true;
    void measure.onBox(batch.bounds, true, false);
  }
  useEffect(() => {
    if (tool === "select" && selectAllNext.current) {
      selectAllNext.current = false;
      selectAll();
    }
  }, [tool]);
  useEffect(() => {
    if (!announceSelection.current) return;
    announceSelection.current = false;
    setNotice(measure.picks.length > 0 ? t("select.all", { n: measure.picks.length }) : t("select.none"));
  }, [measure.picks]);

  /** Cut (Ctrl+X): what is selected goes onto the clipboard and is erased, as CUTCLIP. */
  async function cutSelection() {
    const handles = tool === "select" ? measure.picks : measure.selection;
    if (!info || handles.length === 0) {
      setNotice(t("clip.nothingSelected"));
      return;
    }
    const targets = handles.map((handle) => ({ handle, chain: [] as number[] }));
    await apply({ type: "copy_to_clipboard", targets, base: null });
    await apply({ type: "delete_entities", targets });
    setNotice(t("clip.cut", { n: targets.length }));
  }

  // Keyboard: tool letters, Escape, Enter, undo, redo and save. Re-bound each render, so it sees fresh state.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const action = shortcutFor(e, options.shortcuts);
      // Ctrl+9 works from the command line too.
      if (action === "commandLine") {
        e.preventDefault();
        if (e.repeat) return;
        focusCommandLine.current = !commandLineOn;
        setCommandLineOn(!commandLineOn);
        return;
      }
      // A new drawing, from anywhere: Ctrl+N types nothing (nor opens a browser window).
      if (action === "new") {
        e.preventDefault();
        if (!e.repeat) setNewOpen(true);
        return;
      }
      // Files and Mizan, from anywhere (a prompt being typed too: Ctrl+S saves the drawing): the
      // window answers with a dialog, a toast or the box, never silence.
      if (action === "save" || action === "saveAs" || action === "open" || action === "plot" || action === "find") {
        e.preventDefault();
        if (!e.repeat && switching.current === 0) runCommandLine(action);
        return;
      }
      if (action === "cheatsheet" || (action === "askMizan" && e.ctrlKey)) {
        e.preventDefault();
        if (e.repeat) return;
        if (action === "cheatsheet") setShortcutsOpen(true);
        else askMizan();
        return;
      }
      // The file tabs, from anywhere: Ctrl+Tab and Ctrl+W type nothing.
      if (action === "nextTab" || action === "prevTab" || action === "closeTab") {
        e.preventDefault();
        if (e.repeat) return;
        // From the core's tabs at the time the change runs, never from what this render saw.
        if (action === "closeTab") closeFileTab(null);
        else cycleTabs(action === "nextTab" ? 1 : -1);
        return;
      }
      // A drawing change is under way: tools wait for the drawing it brings.
      if (switching.current > 0) return;
      if (typing(e)) return;
      // A key of the user's is the window's only (F5 or Ctrl+R never reload the web view).
      if (userShortcut(e, options.shortcuts)) e.preventDefault();
      if (action === "osnap" || action === "grid" || action === "ortho" || action === "polar") {
        // F7 and F10 also mean something to the web view; not here.
        e.preventDefault();
        if (!e.repeat) toggleDrafting(action);
      } else if (action === "undo" || action === "redo") {
        if (action === "undo") history(undo);
        else history(redo);
        e.preventDefault();
      } else if (action === "askMizan") {
        // "/" opens Mizan's box with a slash typed, which lists his commands.
        e.preventDefault();
        if (!e.repeat) askMizan("/");
      } else if (action === "selectAll") {
        e.preventDefault();
        if (!e.repeat) selectAll();
      } else if (action === "cut") {
        // Text selected in a panel: Ctrl+X is the text's, as Ctrl+C.
        if ((window.getSelection()?.toString() ?? "") !== "") return;
        e.preventDefault();
        if (!e.repeat) void cutSelection();
      } else if (action === "escape") {
        cancelAll(true);
      } else if (action === "finish") {
        // Enter on a focused button presses it; don't also finish the measure.
        if (e.target instanceof HTMLButtonElement) return;
        // Nor on a menu item, a grid or tree row or a list option: they are divs, and Enter is theirs.
        if ((e.target as Element | null)?.closest?.("[role^='menuitem'],[role=row],[role=treeitem],[role=option]")) return;
        // Nothing under way: Enter does the last tool again.
        if (!e.repeat && repeatLast()) return;
        measure.finish();
      } else if (action === "erase" && (e.target as Element | null)?.closest?.("dialog[open], [role=dialog], [role=menu]")) {
        // Delete in a dialog or a menu is not for the drawing behind it.
        return;
      } else if (action === "erase" && tool === "erase") {
        // Delete again: ERASE is on already, so choosing it changes nothing; erase what it picked.
        if (!e.repeat) measure.finish();
      } else if (action && !e.repeat) {
        // Text selected in a panel: Ctrl+C copies it, as anywhere else.
        const text = window.getSelection()?.toString() ?? "";
        if ((action === "copyclip" || action === "copybase") && text !== "") return;
        chooseTool(action);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  // A modify tool left dimensions or tables out of what was picked: say so.
  useEffect(() => {
    if (measure.skipped > 0) setNotice(t("modify.skipped", { n: measure.skipped }));
  }, [measure.skipped]);

  // An undo or a redo may take selected entities away: keep only those still drawn.
  useEffect(() => {
    if (!batch || measure.selection.length === 0) return;
    const drawn = new Set<number>();
    for (let i = 0; i < batch.ranges.length; i++) drawn.add(Number(batch.ranges.top(i)));
    measure.prune((h) => drawn.has(h));
  }, [batch]);

  const fileName = info?.path.split(/[\\/]/).pop() ?? "";
  // An empty model is said next to the file name, not counted as a reading problem.
  const emptyModel = info?.problems.includes(EMPTY_MODEL) ?? false;
  // What the command line waits for, and its prompt.
  let expect: Expect = "command";
  let promptKey = "cmd.prompt.command";
  if (windowPick) {
    expect = "point";
    promptKey = windowPick.first ? "cmd.prompt.otherCorner" : "cmd.prompt.firstCorner";
  } else if (pending) {
    expect = "point";
    promptKey =
      pending.kind === "insert"
        ? "cmd.prompt.insert"
        : pending.kind === "fieldTarget"
          ? "cmd.prompt.select"
          : pending.first
          ? "cmd.prompt.calibrateSecond"
          : "cmd.prompt.calibrateFirst";
  } else if (measure.asking) {
    expect = measure.asking === "divide" ? "count" : "step";
    promptKey = `cmd.prompt.${expect}`;
  } else if (measure.draw || measure.modify || measure.edit || measure.text || measure.dim) {
    expect = "point";
    promptKey = measure.hint;
  } else if (tool === "distance" || tool === "area" || tool === "mizanArea") {
    expect = "point";
    promptKey = measure.points.length > 0 ? "cmd.prompt.nextPoint" : "cmd.prompt.firstPoint";
  } else if (tool === "idPoint") {
    expect = "point";
    promptKey = "cmd.prompt.point";
  } else if (tool === "zoomWindow") {
    expect = "point";
    promptKey = measure.corner ? "cmd.prompt.otherCorner" : "cmd.prompt.firstCorner";
  } else if (tool === "pasteclip" || tool === "pasteblock" || tool === "mview" || tool === "ucs") {
    expect = "point";
    promptKey = measure.hint;
  } else if (tool === "ucsObject") {
    promptKey = measure.hint;
  } else if (
    tool === "count" ||
    tool === "geometry" ||
    tool === "divide" ||
    tool === "measureAlong" ||
    tool === "textEdit"
  ) {
    promptKey = "cmd.prompt.select";
  }
  const commandState: ParseState = {
    expect,
    last: measure.lastDrawPoint ?? measure.points[measure.points.length - 1] ?? lastPoint.current,
    aliases: commandAliases,
    options: measure.drawOptions,
    numberToo: measure.takesNumber,
    ucs: info?.ucs,
  };
  const prompt = typedPrompt ?? t(promptKey, promptKey === measure.hint ? (measure.hintArgs ?? undefined) : undefined);

  // The command line already asks the tool's own prompt: the status bar does not say it again (it
  // keeps the file name; what was done is a toast). A hint the command line does not show (COUNT's "not a
  // block") stays, and with the command line hidden the status bar says it all.
  const promptShown = commandLineOn && promptKey === measure.hint;
  const message = opening
    ? t("opening")
    : comparing
      ? t("compare.running")
    : windowPick
      ? t(windowPick.first ? "plot.pickSecond" : "plot.pickFirst")
    : pending
      ? t(
          pending.kind === "insert"
            ? pending.of === "image"
              ? "attachImage.pickHint"
              : "attachPdf.pickHint"
            : pending.kind === "fieldTarget"
              ? "insertField.pickHint"
              : pending.first
              ? "calibrate.second"
              : "calibrate.first",
        )
      : renderProgress !== null && !attachOpen
        ? t("pdf.drawingLabel")
        : measure.hint && !promptShown
      ? t(measure.hint, measure.hintArgs ?? undefined)
      : info
        ? emptyModel
          ? `${fileName} — ${t("problem.emptyModel")}`
          : fileName
        : t("noDrawing");

  // What the status bar waits for, as the dots to show beside the message: the same order as the
  // message above, so the dots never stand beside a text about something else. A PDF page being
  // drawn knows how far it is: that is a bar (`progress`), not dots.
  const waiting: DotPattern | null = opening ? "scan" : comparing ? "chase" : null;
  const progress =
    !opening && !comparing && !windowPick && !pending && renderProgress !== null && !attachOpen ? renderProgress : null;

  return { commandLineOn, commandInput, onCommand, typeCommand, startCommand, repeatLast, coreActive: typedPrompt !== null, clickDrawing, commandState, prompt, message, waiting, progress };
}
