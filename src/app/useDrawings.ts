import { refreshInBackground } from "./background";
import { displayedWorkspace } from "../workspaceIpc";
import { extractAll } from "./extractRows";
import { isWorkspaceBusy, uiError, userError } from "../i18n/errors";
/** Drawings as files: the open drawings' tabs, the project autosave switch, and opening, showing,
 *  switching, closing, saving, recovering and exporting them (`useDrawings`). */
import { useEffect, useRef, useState, type RefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, message as messageDialog, open, save } from "../fileDialogs";
import {
  activateTab,
  aiCancel,
  workspaceAttach,
  workspaceViewState,
  autosaveDrawings,
  closeTab,
  discardAutosave,
  DRAWING_CLOSE,
  DRAWING_SAVE,
  DRAWING_SHOWN,
  drawingInfo,
  exportTable,
  exportTakeoff,
  getDrafting,
  getSettings,
  isDirty,
  newDrawing as makeNewDrawing,
  openDrawing,
  recents as readRecents,
  recoverableDrawings,
  recoverAutosave,
  recoverDrawing,
  runCommand,
  runScript,
  SCRIPT_LINE,
  stopScript,
  saveDrawing,
  saveDrawingAs,
  saveProject,
  setXrefPath,
  sheetSetEntry,
  sheetSetFromTabs,
  sheetSetOpen,
  sheetSetSave,
  tabs as readTabs,
  untitledAutosaves,
  type Drafting,
  type ConversationSummary,
  type DrawingInfo,
  type EntityFilter,
  type Field,
  type NewUnits,
  type ReadReport,
  type ScriptResult,
  type TabInfo,
  type WorkspaceEvent,
} from "../api";
import { useLanguage } from "../i18n";
import { cycleTab } from "../shell/FileTabs";
import { readNameTexts, suggestDrawingName } from "./saveName";
import { useOpenRequests } from "../shell/openRequests";
import { atLaunch, drop, enqueue, forgetThen, laterOpens, localTime, offered, type RecoveryAsk } from "../shell/recoveryQueue";
import type { SheetSetApi } from "../shell/SheetSetManager";
import { isStale, serial } from "../shell/switcher";
import { leaveDrawing, readChoice } from "../shell/unsaved";
import { readProblems, type T } from "./shared";
import type { Attach } from "./useAttach";
import type { ToolChoice } from "./useCommands";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { TakeoffState } from "./useTakeoff";

/** The open drawings' file tabs, and the one on screen. */
export function useFileTabs() {
  const [fileTabs, setFileTabs] = useState<TabInfo[]>([]);
  const [activeTab, setActiveTab] = useState<number | null>(null);

  /** Reads the file tabs again (titles, unsaved dots, the active one). */
  async function refreshTabs() {
    try {
      const next = await readTabs();
      setFileTabs(next.tabs);
      setActiveTab(next.active);
    } catch {
      // The tabs keep what they showed; the next refresh tries again.
    }
  }

  useEffect(() => {
    let gone = false;
    let off: (() => void) | undefined;
    void listen<WorkspaceEvent>("workspace:event", ({payload}) => {
      if (["queued", "working", "end", "clear", "awaiting_approval", "tool_result"].includes(payload.event.type)) void refreshTabs();
    }).then(unlisten => { if (gone) unlisten(); else off = unlisten; });
    void refreshTabs();
    return () => { gone = true; off?.(); };
  }, []);
  return { fileTabs, setFileTabs, activeTab, setActiveTab, refreshTabs };
}

export type FileTabsState = ReturnType<typeof useFileTabs>;

/** The core's answer when no drawing is open (the same words `i18n/errors.ts` reads). */
const NO_DRAWING = /no drawing|no document|drawing.*not open/i;

/** Saving the take-off next to the drawing after every change. */
export function useAutosave(t: T, setError: (error: string | null) => void) {
  /** Set when autosave failed for this drawing, so the error shows once, not after every click. A new
   *  open or a successful save turns autosave back on. */
  const autosaveOff = useRef(false);

  /** Saves the take-off next to the drawing. A failure is shown once and turns autosave off, but the
   *  work goes on. */
  async function autosave() {
    if (autosaveOff.current) return;
    try {
      await saveProject();
    } catch (e) {
      // With no drawing open there is nothing to save: not a failure (it once showed as one).
      if (NO_DRAWING.test(String(e)) || isWorkspaceBusy(e)) return;
      autosaveOff.current = true;
      setError(t("project.autosaveOff", { error: uiError(e) }));
    }
  }

  return { autosaveOff, autosave };
}

type Deps = Pick<Shell, "run" | "setError" | "setNotice" | "settingsOpen"> &
  Pick<FileTabsState, "fileTabs" | "setFileTabs" | "setActiveTab" | "refreshTabs"> &
  Pick<Sheets, "setArabicReversed" | "setSheetBlock" | "setHidden" | "redraw"> &
  Pick<TakeoffState, "refreshTakeoffAndBoq"> &
  Pick<ToolChoice, "preset" | "lastPoint"> & {
    t: T;
    switchGen: RefObject<number>;
    info: DrawingInfo | null;
    setInfo(info: DrawingInfo | null): void;
    autosaveOff: RefObject<boolean>;
    autosave(): Promise<void>;
    setDraftingState(drafting: Drafting | null): void;
    /** What `resetDrawingView` and `showNoDrawing` forget, per hook. */
    sheets: Pick<Sheets, "reset" | "clear"> & Partial<Pick<Sheets, "restoreView">>;
    takeoffState: Pick<TakeoffState, "reset" | "clear">;
    attach: Pick<Attach, "reset">;
    /** The tool in use: another drawing cancels it. */
    measure: { cancel(): void; picks?: number[]; select?(handles: number[]): void };
    viewport?: Shell["viewport"];
    shownWorkspace?: RefObject<number | null>;
  };

/** Opening, showing, switching and closing drawings (one at a time, in the order asked), saving
 *  them, the autosaves offered back, RECOVER, sheet sets, xrefs and the Excel exports. */
export function useDrawings({
  t,
  switchGen,
  info,
  setInfo,
  setError,
  setNotice,
  run,
  settingsOpen,
  fileTabs,
  setFileTabs,
  setActiveTab,
  refreshTabs,
  autosaveOff,
  autosave,
  setDraftingState,
  sheets,
  setArabicReversed,
  setSheetBlock,
  setHidden,
  redraw,
  takeoffState,
  refreshTakeoffAndBoq,
  attach,
  preset,
  lastPoint,
  measure,
  viewport,
  shownWorkspace: sharedShownWorkspace,
}: Deps) {
  /** Autosaves to offer back (on launch, or before opening `then`), one question at a time. */
  const localShownWorkspace = useRef<number | null>(null);
  const shownWorkspace = sharedShownWorkspace ?? localShownWorkspace;
  const [recoveries, setRecoveries] = useState<RecoveryAsk[]>([]);
  /** The app's language: the recovery notice writes its time in it. */
  const { lang } = useLanguage();
  const recovery = recoveries[0] ?? null;
  /** What RECOVER read and dropped from a damaged drawing. */
  const [recoverReport, setRecoverReport] = useState<{ path: string; report: ReadReport } | null>(null);
  /** What the last Run script did, for its dialog. */
  const [scriptRun, setScriptRun] = useState<{ path: string; result: ScriptResult } | null>(null);
  /** The line the running script is on (0 before its first), null when none runs: the ribbon and
   *  the command line wait meanwhile, and Stop ends it. */
  const [scriptLine, setScriptLine] = useState<number | null>(null);
  const [sheetSetShown, setSheetSetShown] = useState(false);
  const [opening, setOpening] = useState(false);
  const [xrefsOpen, setXrefsOpen] = useState(false);
  const [extractOpen, setExtractOpen] = useState(false);
  const [newOpen, setNewOpen] = useState(false);
  /** Drawing changes waiting or running; while any is, clicks and tool input on the canvas are
   *  ignored (they would land on a drawing about to go). */
  const switching = useRef(0);
  /** Opens, switches and closes reach the core one at a time, in the order asked. */
  const inTurn = useRef(serial()).current;
  /** The drawing on screen now, for checks inside queued work. */
  const infoRef = useRef(info);
  infoRef.current = info;
  /** A resumed chat waits until the usual autosave question has selected its drawing. */
  const conversationOpen = useRef(new Map<string, (opened: boolean) => void>());
  function finishConversationOpen(path: string, opened: boolean) {
    conversationOpen.current.get(path)?.(opened);
    conversationOpen.current.delete(path);
  }
  useEffect(() => () => {
    conversationOpen.current.forEach((finish) => finish(false));
    conversationOpen.current.clear();
  }, []);

  /** Before the open drawing is left: asks "Save changes to the drawing?" when the drawing itself
   *  has unsaved changes, and saves when told to. False when the user cancels. */
  async function leaveOpenDrawing(): Promise<boolean> {
    if (!info) return true;
    const workspace = displayedWorkspace();
    let saveTo: string | null = null;
    const labels = { yes: t("unsaved.yes"), no: t("unsaved.no"), cancel: t("unsaved.cancel") };
    return leaveDrawing({
      isDirty,
      ask: async () => {
        const choice = readChoice(
          await messageDialog(t("unsaved.question"), {
            title: t("appName"),
            kind: "warning",
            buttons: labels,
          }),
          labels,
        );
        // Don't Save: its autosave goes too (the window closing, or the tab). The drawing on
        // screen now: closing the window shows each drawing before asking.
        if (choice === "discard") await discardAutosave((await drawingInfo(workspace)).path).catch(() => {});
        return choice;
      },
      // The drawing on screen now: closing the window shows each drawing before asking. A new
      // drawing has no file yet: it asks where to save it, and cancelling that keeps it open.
      confirmSave: async () => {
        const now = await drawingInfo(workspace);
        if (!now.untitled) return confirmRecoveredSave(now);
        saveTo = await askSavePath(now);
        return saveTo !== null;
      },
      save: () => (saveTo ? saveDrawingAs(saveTo, workspace) : saveDrawing(workspace)),
    });
  }
  /** Before the window closes: asks about each open drawing with unsaved changes in turn, showing
   *  it first. False when the user cancels one. */
  async function leaveAllDrawings(): Promise<boolean> {
    const { tabs: open } = await readTabs();
    for (const tab of open) if (!(await stopForClose(tab))) return false;
    const settled = await readTabs();
    for (const tab of settled.tabs.filter((f) => f.dirty)) {
      if (!(await switchTab(tab.id))) return false;
      if (!(await leaveOpenDrawing())) return false;
    }
    return true;
  }
  const leaveRef = useRef(leaveAllDrawings);
  leaveRef.current = leaveAllDrawings;

  // Closing the window with unsaved drawing changes asks first; a failed save keeps it open.
  useEffect(() => {
    let gone = false;
    let off: (() => void) | null = null;
    getCurrentWindow()
      .onCloseRequested(async (event) => {
        try {
          if (!(await leaveRef.current())) event.preventDefault();
        } catch (e) {
          event.preventDefault();
          setError(t("save.failed", { error: uiError(e) }));
        }
      })
      .then((unlisten) => (gone ? unlisten() : (off = unlisten)))
      .catch(() => {});
    return () => {
      gone = true;
      off?.();
    };
  }, []);

  // On launch: the recent drawings' autosaves newer than them, or left by a crash, are offered back,
  // and those of untitled drawings Qiyas closed on before their first Save As.
  useEffect(() => {
    (async () => {
      const untitled = (await untitledAutosaves().catch(() => null)) ?? [];
      const paths = [...(await readRecents()).map((r) => r.path), ...untitled];
      const found = atLaunch(await recoverableDrawings(paths));
      if (found.length > 0) setRecoveries((q) => enqueue(q, found, null, true));
    })().catch(() => {});
  }, []);

  useEffect(() => {
    let gone = false;
    void readTabs().then(async now => {
      if (gone || now?.active == null) return;
      const next = await drawingInfo();
      if (!gone) await showDrawing(next, beginShow());
    }).catch(() => {});
    return () => { gone = true; };
  }, []);

  // Autosave: every few minutes (settings, 5 by default, 0 off) the drawings with unsaved changes
  // are written to <name>.sv$ beside them, with their project.
  useEffect(() => {
    let timer = 0;
    let gone = false;
    getSettings()
      .then((s) => {
        const minutes = s.autosave_minutes ?? 5;
        if (gone || !(minutes > 0)) return;
        timer = window.setInterval(() => {
          autosaveDrawings()
            .then((done) => {
              if (done.failed.length > 0) setError(t("autosave.failed", { error: done.failed.join("; ") }));
            })
            .catch((e) => { if (!isWorkspaceBusy(e)) setError(t("autosave.failed", { error: uiError(e) })); });
        }, minutes * 60_000);
      })
      .catch(() => {});
    return () => {
      gone = true;
      clearInterval(timer);
    };
  }, [settingsOpen]);

  /** Forgets everything the window holds about the drawing on screen: the batch, the glyph atlas
   *  and the pictures (all per drawing), the selection, the tool's points, prompts and dialogs. */
  function resetDrawingView() {
    sheets.reset();
    autosaveOff.current = false;
    takeoffState.reset();
    attach.reset();
    setXrefsOpen(false);
    setExtractOpen(false);
    preset.current = null;
    measure.cancel();
    lastPoint.current = null;
  }

  /** Another drawing comes on screen: earlier work for the one there goes stale. */
  function beginShow(): number {
    switchGen.current += 1;
    return switchGen.current;
  }

  /** Shows the drawing the core has on screen now (just opened, or its tab chosen) as its session
   *  keeps it: its sheet, hidden layers, take-off, BOQ and conflicts. `gen` is `beginShow`'s; a
   *  later drawing change stops it at the next step. */
  async function showDrawing(next: DrawingInfo, gen: number) {
    shownWorkspace.current = null;
    resetDrawingView();
    setArabicReversed(next.arabic_reversed);
    setInfo(next);
    setSheetBlock(next.sheet);
    setHidden(new Set(next.hidden_layers));
    const current = await readTabs();
    if (current?.active != null) {
      const saved = await workspaceAttach(current.active).catch(() => null);
      if (isStale(gen, switchGen.current)) return;
      if (saved?.context?.view) sheets.restoreView?.(saved.context.view);
      if (saved?.context?.selection) measure.select?.(saved.context.selection.map(item => item.handle));
    }
    const drafting = await getDrafting();
    if (isStale(gen, switchGen.current)) return;
    setDraftingState(drafting);
    await refreshTakeoffAndBoq();
    if (isStale(gen, switchGen.current)) return;
    await redraw(true);
    if (!isStale(gen, switchGen.current)) shownWorkspace.current = current?.active ?? null;
  }

  /** After the last tab closed: an empty window. */
  function showNoDrawing() {
    shownWorkspace.current = null;
    beginShow();
    resetDrawingView();
    sheets.clear();
    setInfo(null);
    setDraftingState(null);
    takeoffState.clear();
  }

  /** Runs a drawing change (open, tab switch, close) in turn after those asked before it; canvas
   *  input is ignored until all are done. */
  async function drawingChange<T>(job: () => Promise<T>): Promise<T> {
    switching.current += 1;
    try {
      return await inTurn(async () => {
        if (viewport?.current && infoRef.current) {
          const outgoing = shownWorkspace.current;
          if (outgoing != null) await workspaceViewState(outgoing, {
            selection: (measure.picks ?? []).map(handle => ({ handle, chain: [] })),
            region: null, view: viewport.current.visible(), picture: false,
          }).catch(() => {});
        }
        return job();
      });
    } finally {
      switching.current -= 1;
    }
  }

  /** Opens the drawing at `path` in a new tab, or shows its tab when it is open already; nothing
   *  happens when it is the drawing on screen. An autosave newer than the drawing is offered first
   *  (`recover` false skips that: the user chose to open the drawing itself). */
  async function openPath(path: string, recover = true) {
    if (recover) {
      const found = offered(await recoverableDrawings([path]).catch(() => []));
      const open = fileTabs.some((f) => f.path === path);
      if (found.length > 0 && !open) {
        setRecoveries((q) => enqueue(q, found, path));
        return;
      }
    }
    setOpening(true);
    let opened = false;
    await drawingChange(() =>
      run(async () => {
        const next = await openDrawing(path);
        opened = true;
        // The drawing on screen asked for again: the core changed nothing.
        if (infoRef.current && next.path === infoRef.current.path) return;
        await showDrawing(next, beginShow());
      }),
    );
    setOpening(false);
    finishConversationOpen(path, opened);
  }

  async function prepareConversation(chat: ConversationSummary): Promise<boolean> {
    const now = await readTabs();
    const tab = chat.drawing && now.tabs.find((item) => item.path === chat.drawing);
    if (tab) return now.active === tab.id || drawingChange(() => activate(tab.id, true));
    if (!chat.drawing) {
      setNotice(t("ai.drawingMissing"));
      return false;
    }
    const path = chat.drawing;
    if (!chat.drawing_exists) {
      // Untitled drawings have a stable path even before their first Save As.
      // The file may be absent while its autosave is the only surviving drawing.
      const found = await recoverableDrawings([path]);
      const recoverable = found.find((item) => item.drawing === path && (item.autosave || item.older_copies.length > 0));
      if (recoverable) {
        await drawingChange(async () => {
          const next = await recoverAutosave(recoverable.autosave ? path : recoverable.older_copies[0].path);
          await showDrawing(next, beginShow());
          await refreshTabs();
        });
        return true;
      }
      setNotice(t("ai.drawingMissing"));
      return false;
    }
    const opened = new Promise<boolean>((resolve) => conversationOpen.current.set(path, resolve));
    try {
      await openPath(path);
    } catch (e) {
      finishConversationOpen(path, false);
      throw e;
    }
    return opened;
  }

  function dismissRecovery(ask: RecoveryAsk) {
    const then = laterOpens(ask);
    setRecoveries((q) => q.filter((item) => item.id !== ask.id));
    if (then) void openPath(then, false);
    else if (ask.then) finishConversationOpen(ask.then, false);
  }

  // Drawings the OS asks to open (a double click on one, a second launch) come the same way.
  useOpenRequests(openPath);

  // Mizan (or an MCP client) chose the drawing on screen, or asked the window to save or close
  // one where the user decides: the listeners are set once and call the latest code through this.
  const fromMizan = useRef({ shown: async () => {}, save: async (_id: number) => {}, close: async (_id: number) => {} });
  fromMizan.current = {
    shown: () =>
      drawingChange(() =>
        refreshInBackground(async () => {
          const now = await readTabs();
          setFileTabs(now.tabs);
          setActiveTab(now.active);
          if (now.active === null) showNoDrawing();
          else await showDrawing(await drawingInfo(), beginShow());
        }),
      ),
    save: async (id) => {
      if (!(await switchTab(id))) return;
      const drawing = await drawingInfo(id);
      if (drawing.untitled) await saveAs(drawing, id);
      else if (await confirmRecoveredSave(drawing)) await saveWith(() => saveDrawing(id), id);
    },
    close: (id) => closeFileTab(id, true),
  };
  useEffect(() => {
    let gone = false;
    const offs: (() => void)[] = [];
    const keep = (unlisten: () => void) => (gone ? unlisten() : offs.push(unlisten));
    listen(DRAWING_SHOWN, () => fromMizan.current.shown()).then(keep, () => {});
    listen<number>(DRAWING_SAVE, (e) => fromMizan.current.save(e.payload)).then(keep, () => {});
    listen<number>(DRAWING_CLOSE, (e) => fromMizan.current.close(e.payload)).then(keep, () => {});
    return () => {
      gone = true;
      offs.forEach((off) => off());
    };
  }, []);

  /** A new, empty drawing in `units` in a new tab (NEW, Ctrl+N, the quick-access bar, after the
   *  units dialog). */
  async function newDrawing(units: NewUnits) {
    setNewOpen(false);
    await drawingChange(() =>
      run(async () => {
        await showDrawing(await makeNewDrawing(units), beginShow());
        await refreshTabs();
      }),
    );
  }

  async function openFile() {
    const path = await open({ filters: [{ name: "Drawings", extensions: ["dwg", "dxf"] }] });
    if (typeof path !== "string") return;
    await openPath(path);
  }

  /** Recover: the autosave of `drawing` opens in a new tab as a new drawing beside it, and Save As
   *  asks where it goes; then the status bar says which autosave it came from. */
  async function recoverFrom(ask: RecoveryAsk, drawing: string, copy?: string) {
    const item = ask.items.find((i) => i.drawing === drawing);
    const savedAt = copy ? item?.older_copies.find((c) => c.path === copy)?.saved_at : item?.saved_at;
    const recovered = await drawingChange(async () => {
      const next = await recoverAutosave(copy ?? drawing);
      await showDrawing(next, beginShow());
      await refreshTabs();
      return next;
    });
    // Its older copies stay listed; Later no longer opens the original beside the recovered one.
    setRecoveries((q) => forgetThen(drop(q, ask.id, drawing, copy), ask.id, drawing));
    // The recovered drawing itself: this render's `info` is the one before it (none at launch).
    const savedTo = await saveAs(recovered);
    finishConversationOpen(drawing, true);
    // Times and paths isolated (FSI … PDI): they keep their own order inside Arabic. One notice,
    // so where it came from never hides where it went.
    const iso = (text: string) => `\u2068${text}\u2069`;
    if (savedAt == null) return;
    const time = iso(localTime(savedAt, lang));
    setNotice(savedTo ? t("recovery.doneSaved", { time, path: iso(savedTo) }) : t("recovery.done", { time }));
  }

  /** Discard: the autosave of `drawing` (or its older `copy`) is deleted; the drawing about to
   *  open opens once nothing is left to ask about it (its older copies too). */
  async function discardFrom(ask: RecoveryAsk, drawing: string, copy?: string) {
    await discardAutosave(copy ?? drawing);
    const done = !drop([ask], ask.id, drawing, copy)[0]?.items.some((i) => i.drawing === drawing);
    setRecoveries((q) => drop(q, ask.id, drawing, copy));
    if (done && ask.then === drawing) await openPath(drawing, false);
  }

  /** Open anyway: a drawing open in another Qiyas opens here (once, even when it is the drawing
   *  about to open); its autosave is that Qiyas's and stays. */
  async function openAnyway(ask: RecoveryAsk, drawing: string) {
    setRecoveries((q) => forgetThen(drop(q, ask.id, drawing), ask.id, drawing));
    await openPath(drawing, false);
  }

  /** RECOVER: a damaged drawing opens in a new tab through the failsafe reader, and the dialog
   *  says what was dropped. */
  async function recoverFile() {
    const path = await open({ filters: [{ name: "Drawings", extensions: ["dwg", "dxf"] }] });
    if (typeof path !== "string") return;
    await drawingChange(() =>
      run(async () => {
        const { info: next, report } = await recoverDrawing(path);
        await showDrawing(next, beginShow());
        await refreshTabs();
        setRecoverReport({ path, report });
      }),
    );
  }

  /** Run script (SCRIPT): asks for a `.scr` command script and runs it on the drawing on screen,
   *  then shows what each line did. A drawing its OPEN put on screen is shown as a tab switch is;
   *  else the drawing on screen is read and drawn again. Either way the take-off is saved. */
  async function runScriptFile() {
    if (infoRef.current?.block_edit) {
      setError(t("script.blockEdit"));
      return;
    }
    const path = await open({ filters: [{ name: t("script.file"), extensions: ["scr"] }] });
    if (typeof path !== "string") return;
    let changed = false;
    await drawingChange(() =>
      run(async () => {
        const before = (await readTabs()).active;
        setScriptLine(0);
        const off = await listen<number>(SCRIPT_LINE, (e) => setScriptLine(e.payload)).catch(() => null);
        let result: ScriptResult;
        try {
          result = await runScript(path);
        } finally {
          off?.();
          setScriptLine(null);
        }
        setScriptRun({ path, result });
        if (!result.changed) return;
        changed = true;
        const now = await readTabs();
        const gen = now.active === before ? switchGen.current : beginShow();
        const next = await drawingInfo();
        if (isStale(gen, switchGen.current)) return;
        if (now.active !== before) {
          await showDrawing(next, gen);
          return;
        }
        setInfo(next);
        setHidden(new Set(next.hidden_layers));
        await refreshTakeoffAndBoq();
        await redraw();
      }),
    );
    if (changed) await autosave();
  }

  /** The sheet set manager's file dialogs and core commands. */
  const sheetSetApi: SheetSetApi = {
    open: sheetSetOpen,
    save: sheetSetSave,
    fromTabs: sheetSetFromTabs,
    entry: sheetSetEntry,
    pickNew: async () =>
      (await save({ filters: [{ name: t("sheetSet.title"), extensions: ["qss"] }] })) ?? null,
    pickOpen: async () => {
      const path = await open({ filters: [{ name: t("sheetSet.title"), extensions: ["qss"] }] });
      return typeof path === "string" ? path : null;
    },
    pickDrawing: async () => {
      const path = await open({ filters: [{ name: "Drawings", extensions: ["dwg", "dxf"] }] });
      return typeof path === "string" ? path : null;
    },
  };

  /** Opens every drawing of a sheet set, a tab each, one after another. */
  async function openAll(paths: string[]) {
    for (const path of paths) await openPath(path);
  }

  /** Shows the drawing of tab `id` (call inside `drawingChange`); false when that failed (the
   *  error shows). `keepConversation`: Mizan asked for it, its conversation goes on. */
  async function activate(id: number, keepConversation = false): Promise<boolean> {
    let ok = false;
    await run(async () => {
      const gen = beginShow();
      const next = await activateTab(id, keepConversation);
      if (isStale(gen, switchGen.current)) return;
      // Work started while the core was switching (a zoom's glyphs) may have read the old drawing
      // under the new generation: bump it again now that the new one is in place.
      await showDrawing(next, beginShow());
      ok = true;
    });
    return ok;
  }

  /** Shows the drawing of tab `id`, after any drawing change already asked for. */
  const switchTab = (id: number) => drawingChange(() => activate(id));

  /** Shows the tab after (`step` 1) or before (-1) the one on screen, as the core has them. */
  const cycleTabs = (step: 1 | -1) =>
    drawingChange(async () => {
      const now = await readTabs().catch(() => null);
      const next = now && cycleTab(now.tabs, now.active, step);
      if (now && next !== null && next !== now.active) await activate(next);
    });

  /** Closes tab `id` (the one on screen when null). A drawing with unsaved changes is shown first
   *  and asks "Save changes to the drawing?"; Cancel keeps it open. `keepConversation`: Mizan
   *  asked to close it, its conversation goes on. */
  async function stopForClose(tab: TabInfo): Promise<boolean> {
    if (!tab.busy) return true;
    if (!(await ask(t("workspace.closeBusy"), { title: t("appName"), kind: "warning" }))) return false;
    await aiCancel(tab.id);
    // Waiting for a committing tool does not occupy the tab-switch queue.
    while ((await readTabs()).tabs.find(item => item.id === tab.id)?.busy) {
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    return true;
  }

  async function closeFileTab(id: number | null, keepConversation = false) {
    const requested = await readTabs().catch(() => null);
    const target = requested?.tabs.find(tab => tab.id === (id ?? requested.active));
    if (!target || !(await stopForClose(target))) return;
    await drawingChange(async () => {
      const now = await readTabs();
      const tab = now.tabs.find(tab => tab.id === target.id);
      if (!tab) return;
      try {
        if (tab.dirty && (!(await activate(tab.id, keepConversation)) || !(await leaveOpenDrawing()))) return;
      } catch (e) {
        setError(t("save.failed", { error: uiError(e) }));
        return;
      }
      await run(async () => {
        const shown = tab.dirty ? tab.id : now.active;
        const left = await closeTab(tab.id, tab.dirty, keepConversation);
        setFileTabs(left.tabs);
        setActiveTab(left.active);
        if (left.active === null) showNoDrawing();
        else if (left.active !== shown) {
          const gen = beginShow();
          const next = await drawingInfo();
          if (!isStale(gen, switchGen.current)) await showDrawing(next, gen);
        }
      });
    });
  }

  /** Writes the drawing with `write`, then shows its new layers and entities. */
  async function saveWith(write: () => Promise<void>, workspace = displayedWorkspace()): Promise<boolean> {
    const gen = switchGen.current;
    let saved = false;
    await run(async () => {
      let next: DrawingInfo | null = null;
      try {
        await write().catch((e) => {
          throw userError(t("save.failed", { error: uiError(e) }));
        });
      } finally {
        // Even a failed save may have written the drawing and moved to its new path.
        next = await drawingInfo(workspace).catch(() => null);
        if (next && workspace === displayedWorkspace() && !isStale(gen, switchGen.current)) setInfo(next);
      }
      // The save wrote the project file too: autosave can try again.
      autosaveOff.current = false;
      if (workspace === displayedWorkspace()) await redraw();
      setNotice(t("save.done", { name: (next?.path ?? "").split(/[\\/]/).pop() ?? "" }));
      saved = true;
    });
    return saved;
  }

  /** A drawing that opened with problems is rewritten with only what Qiyas could read, so a save
   *  over its own file asks first; the core keeps the original as a `.bak` either way. True to
   *  save. */
  async function confirmRecoveredSave(of: DrawingInfo): Promise<boolean> {
    // An xref's problems are not the drawing's own: saving never touches the xref. Nor is an
    // empty model something the reader lost.
    if (readProblems(of).length <= of.xrefs.reduce((n, x) => n + x.problems.length, 0)) return true;
    return ask(t("save.confirmRecovered"), { title: t("appName"), kind: "warning" });
  }

  /** Saves over the drawing's own file, after `confirmRecoveredSave`; a new drawing has none yet,
   *  so it asks where (Save As). */
  async function saveFile() {
    if (!info) return;
    const workspace = displayedWorkspace();
    if (info.untitled) {
      await saveAs(info, workspace);
      return;
    }
    if (!(await confirmRecoveredSave(info))) return;
    await saveWith(() => saveDrawing(workspace), workspace);
  }

  /** Shows the xrefs as the core now has them (their layers, problems and drawing) and saves the
   *  project, which remembers a file chosen for one. Errors go to the xrefs dialog. */
  async function xrefsChanged(next: DrawingInfo) {
    setInfo(next);
    await redraw();
    await autosave();
  }

  /** Asks for the file of the xref `name` and loads it from there. */
  async function locateXref(name: string) {
    const path = await open({ filters: [{ name: "Drawings", extensions: ["dwg", "dxf"] }] });
    if (typeof path !== "string") return;
    await xrefsChanged(await setXrefPath(name, path));
  }

  /** Asks where to save the drawing, as DWG or DXF, and saves it there. */
  async function saveFileAs() {
    if (info) await saveAs(info);
  }

  /** Save As for `of`, the drawing on screen (given, as one just recovered is not in `info` yet). */
  async function saveAs(of: DrawingInfo, workspace = displayedWorkspace()): Promise<string | null> {
    const path = await askSavePath(of);
    if (!path) {
      // Cancelled: the "Choose where to save" toast goes with it.
      setNotice(null);
      return null;
    }
    return (await saveWith(() => saveDrawingAs(path, workspace), workspace)) ? path : null;
  }

  /** Asks where to save. The native dialog can open where the user is not looking (a compositor may
   *  put it on another workspace), so a toast says it is coming; an untitled drawing is offered a
   *  real name (its title block's project or title, else its first text, else "Qiyas drawing <date>")
   *  and the shared picker's real folder (the last one used, else Documents), never the placeholder's. */
  async function askSavePath(of: DrawingInfo): Promise<string | null> {
    setNotice(t("save.choose"));
    let offered = of.path;
    if (of.untitled) {
      const now = new Date();
      const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
      const name = suggestDrawingName(await readNameTexts(of.sheets.map((s) => s.block)), t("save.defaultName", { date }));
      offered = `${name}.dwg`;
    }
    return save({
      defaultPath: offered,
      filters: [
        { name: "DWG", extensions: ["dwg"] },
        { name: "DXF", extensions: ["dxf"] },
      ],
    }, of);
  }

  /** Asks where to save the Excel take-off, next to the drawing by default, and writes it. */
  async function exportFile() {
    if (!info) return;
    const path = await save({
      defaultPath: `${info.path.replace(/\.[^.\\/]*$/, "")}-takeoff.xlsx`,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
    });
    if (!path) return;
    await run(async () => {
      await exportTakeoff(path).catch((e) => {
        throw userError(t("export.failed", { error: uiError(e) }));
      });
      setNotice(t("export.done", { path }));
    });
  }

  /** Extract data: runs each filter on the sheet shown and gathers the rows. */
  async function extractRows(filters: EntityFilter[], fields: Field[], progress?: (rows: number) => void): Promise<string[][]> {
    return extractAll(filters, fields, (filter, fields) => runCommand({ type: "extract_data", filter, fields }), progress);
  }

  /** Asks where to save the extracted table, next to the drawing by default, and writes it. */
  async function exportExtract(columns: string[], rows: string[][]) {
    if (!info) return;
    const path = await save({
      defaultPath: `${info.path.replace(/\.[^.\\/]*$/, "")}-data.xlsx`,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
    });
    if (!path) return;
    await exportTable(path, columns, rows).catch((e) => {
      throw userError(t("export.failed", { error: uiError(e) }));
    });
    setNotice(t("extract.done", { path }));
  }

  return {
    opening,
    recovery,
    setRecoveries,
    recoverReport,
    setRecoverReport,
    scriptRun,
    setScriptRun,
    scriptLine,
    stopRunningScript: () => stopScript().catch(() => {}),
    sheetSetShown,
    setSheetSetShown,
    xrefsOpen,
    setXrefsOpen,
    extractOpen,
    setExtractOpen,
    switching,
    openPath,
    prepareConversation,
    dismissRecovery,
    openFile,
    newOpen,
    setNewOpen,
    newDrawing,
    recoverFrom,
    discardFrom,
    openAnyway,
    recoverFile,
    runScriptFile,
    sheetSetApi,
    openAll,
    switchTab,
    cycleTabs,
    closeFileTab,
    saveFile,
    saveFileAs,
    xrefsChanged,
    locateXref,
    exportFile,
    extractRows,
    exportExtract,
  };
}

export type Drawings = ReturnType<typeof useDrawings>;
