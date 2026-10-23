import { uiError, userError } from "../i18n/errors";
/** The side panel: its state (`useTakeoffState`: the take-off rows and totals, the BOQ, the
 *  conflicts, the inquiry info card, and what each highlights on the drawing) and its handlers
 *  (`useTakeoff`: BOQ import, compare, export, selecting rows, BOQ items and conflicts). */
import { useCallback, useMemo, useRef, useState, type RefObject } from "react";
import { open, save } from "../fileDialogs";
import {
  boq as readBoq,
  measurementReport,
  type MeasurementReport,
  compare,
  conflicts as readConflicts,
  exportConflicts,
  importBoq,
  takeoff,
  totals,
  type Boq,
  type Conflict,
  type DrawingInfo,
  type Measurement,
  type Total,
} from "../api";
import type { ResolveCommand } from "../boq/ConflictsPanel";
import type { Info } from "../shell/InfoCard";
import { isStale } from "../shell/switcher";
import { measurementBounds } from "../tools/overlay";
import { type T, unionBounds } from "./shared";
import type { Commands } from "./useCommands";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";

export function useTakeoffState(switchGen: RefObject<number>) {
  const [items, setItems] = useState<Measurement[]>([]);
  const [itemTotals, setItemTotals] = useState<Total[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  /** The drawing's imported BOQ. */
  const [generated, setGenerated] = useState<MeasurementReport | null>(null);
  const [bill, setBill] = useState<Boq | null>(null);
  /** The BOQ item whose linked measurements are shown. */
  const [boqCode, setBoqCode] = useState<string | null>(null);
  /** The last comparison's conflicts. */
  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  /** A comparison ran for this drawing, so no conflicts means none were found. */
  const [compared, setCompared] = useState(false);
  /** A comparison is running (the status bar says so). */
  const [comparing, setComparing] = useState(false);
  /** The measurements a conflict's Show pointed at. */
  const [conflictIds, setConflictIds] = useState<string[]>([]);
  /** Entities the assistant pointed at (Show on a tool card, or a zoom it asked for). */
  const [aiHighlight, setAiHighlight] = useState<number[]>([]);
  /** What the last inquiry (geometry, ID point) found, shown until closed. */
  const [infoCard, setInfoCard] = useState<Info | null>(null);

  /** Re-reads the take-off, the BOQ and the conflicts, after anything that may have changed them. */
  async function refreshTakeoffAndBoq() {
    const gen = switchGen.current;
    const [nextItems, nextTotals, nextBoq, nextConflicts, nextGenerated] = await Promise.all([
      takeoff(),
      totals(),
      readBoq(),
      readConflicts(),
      measurementReport(),
    ]);
    if (isStale(gen, switchGen.current)) return;
    setItems(nextItems);
    setItemTotals(nextTotals);
    setBill(nextBoq);
    setGenerated(nextGenerated);
    setConflicts(nextConflicts);
    // Conflicts kept in the project file come from an earlier comparison.
    if (nextConflicts.length > 0) setCompared(true);
  }

  /** Forgets the selection and highlights of the drawing on screen (another one is coming). */
  function reset() {
    setSelectedId(null);
    setBoqCode(null);
    setAiHighlight([]);
    setConflictIds([]);
    setCompared(false);
    setInfoCard(null);
  }

  /** After the last tab closed: nothing to show. */
  function clear() {
    setItems([]);
    setItemTotals([]);
    setBill(null);
    setGenerated(null);
    setConflicts([]);
  }

  return {
    items,
    itemTotals,
    selectedId,
    setSelectedId,
    bill,
    generated,
    boqCode,
    setBoqCode,
    conflicts,
    setConflicts,
    compared,
    setCompared,
    comparing,
    setComparing,
    conflictIds,
    setConflictIds,
    aiHighlight,
    setAiHighlight,
    infoCard,
    setInfoCard,
    refreshTakeoffAndBoq,
    reset,
    clear,
  };
}

export type TakeoffState = ReturnType<typeof useTakeoffState>;

type Deps = Pick<Shell, "run" | "setNotice" | "setSideTab" | "viewport"> &
  Pick<Sheets, "sheet" | "batch" | "chooseSheet" | "redraw"> &
  Pick<Commands, "apply"> & {
    t: T;
    info: DrawingInfo | null;
    autosave(): Promise<void>;
    takeoffState: TakeoffState;
  };

/** The side panel's handlers. Those handed to the memoised panels are stable between pointer
 *  moves; the ones with no dependencies keep the first `apply` they were given, as before. */
export function useTakeoff({
  t,
  info,
  setNotice,
  run,
  autosave,
  setSideTab,
  viewport,
  sheet,
  batch,
  chooseSheet,
  redraw,
  takeoffState,
  apply,
}: Deps) {
  const {
    items,
    selectedId,
    setSelectedId,
    bill,
    generated,
    boqCode,
    setBoqCode,
    setConflicts,
    setCompared,
    setComparing,
    conflictIds,
    setConflictIds,
    setAiHighlight,
    refreshTakeoffAndBoq,
  } = takeoffState;

  /** Asks for an Excel BOQ and makes it the drawing's BOQ (an undo step), then shows it. */
  async function importBoqFile() {
    if (!info) return;
    const path = await open({ filters: [{ name: "Excel", extensions: ["xlsx", "xlsm"] }] });
    if (typeof path !== "string") return;
    let changed = false;
    await run(async () => {
      await importBoq(path);
      changed = true;
      setBoqCode(null);
      setSideTab("boq");
      await refreshTakeoffAndBoq();
    });
    if (changed) await autosave();
  }

  // Stable for the memoised BOQ panel, which would otherwise re-render on every pointer move.
  const importBoqRef = useRef(importBoqFile);
  importBoqRef.current = importBoqFile;
  const onImportBoq = useCallback(() => importBoqRef.current(), []);

  /** Compares the BOQ and the take-off with the drawings (an undo step) and shows the conflicts. */
  async function compareNow() {
    if (!info) return;
    let changed = false;
    setComparing(true);
    await run(async () => {
      let found: Conflict[];
      try {
        found = await compare();
      } finally {
        setComparing(false);
      }
      changed = true;
      await redraw();
      setConflicts(found);
      setCompared(true);
      setSideTab("conflicts");
      setNotice(
        found.length === 0 ? t("conflicts.none") : t("conflicts.found", { n: found.length }),
      );
    });
    if (changed) await autosave();
  }

  /** Asks where to save the conflicts workbook, next to the drawing by default, and writes it. */
  async function exportConflictsFile() {
    if (!info) return;
    const path = await save({
      defaultPath: `${info.path.replace(/\.[^.\\/]*$/, "")}-conflicts.xlsx`,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
    });
    if (!path) return;
    await run(async () => {
      await exportConflicts(path).catch((e) => {
        throw userError(t("export.failed", { error: uiError(e) }));
      });
      setNotice(t("export.done", { path }));
    });
  }

  // Stable for the memoised conflicts panel.
  const compareRef = useRef(compareNow);
  compareRef.current = compareNow;
  const onCompare = useCallback(() => compareRef.current(), []);
  const exportConflictsRef = useRef(exportConflictsFile);
  exportConflictsRef.current = exportConflictsFile;
  const onExportConflicts = useCallback(() => exportConflictsRef.current(), []);

  /** The selected row, when it was measured on the sheet in view. */
  const selected = items.find(
    (m) => m.id === selectedId && m.sheet.toLowerCase() === sheet.toLowerCase(),
  );

  const onSheet = (m: Measurement) => m.sheet.toLowerCase() === sheet.toLowerCase();

  /** The measurements on the sheet in view linked to the selected BOQ item. */
  const boqLinked = useMemo(
    () =>
      boqCode !== null && bill?.items.some((i) => i.code === boqCode)
        ? items.filter((m) => m.boq_code === boqCode && onSheet(m))
        : [],
    [boqCode, bill, items, sheet],
  );

  /** The measurements on the sheet in view that a conflict's Show pointed at. */
  const conflictLinked = useMemo(
    () => items.filter((m) => conflictIds.includes(m.id) && onSheet(m)),
    [conflictIds, items, sheet],
  );

  const selectRow = useCallback(
    (id: string) => {
      setSelectedId(id);
      setBoqCode(null);
      setConflictIds([]);
      const m = items.find((i) => i.id === id);
      if (m?.sheet.toLowerCase() !== sheet.toLowerCase()) return;
      const bounds = measurementBounds(m, batch);
      if (bounds) viewport.current?.fitTo(bounds);
    },
    [items, sheet, batch],
  );

  function showGeneratedItem(code: string): boolean {
    const sources = generated?.items.find((item) => item.code === code)?.lines.flatMap((line) => line.sources) ?? [];
    const target = sources.find((source) => source.sheet.toLowerCase() === sheet.toLowerCase())?.sheet ?? sources[0]?.sheet;
    if (!target) return false;
    showGenerated([...new Set(sources.filter((source) => source.sheet === target).map((source) => source.handle))], target);
    return true;
  }
  /** Selects a BOQ item: its linked measurements on this sheet light up and fill the view. */
  const selectBoq = useCallback(
    (code: string) => {
      setBoqCode(code);
      setSelectedId(null);
      setAiHighlight([]);
      setConflictIds([]);
      if (showGeneratedItem(code)) return;
      const linked = items.filter((m) => code !== "" && m.boq_code === code && onSheet(m));
      const box = unionBounds(linked, batch);
      if (box) viewport.current?.fitTo(box);
    },
    [items, sheet, batch, generated],
  );
  /** Shows what a conflict is about: its measurements lit up and filling the view (moving to their
   *  sheet when none is on this one); else its BOQ item in the BOQ tab; else its first sheet. */
  const showConflict = useCallback(
    (c: Conflict) => {
      if (c.query?.clouds.length) {
        const target = c.query.references[0]?.sheet ?? c.sheets[0];
        const block = info?.sheets.find((s) => s.block === target || s.name === target)?.block;
        showGenerated(c.query.clouds, block ?? target ?? sheet);
        return;
      }
      const derived = generated?.items.find((item) => c.measurements.includes(`generated:${item.code}`));
      if (derived && showGeneratedItem(derived.code)) {
        setSelectedId(null);
        setBoqCode(null);
        setConflictIds([]);
        return;
      }
      const involved = items.filter((m) => c.measurements.includes(m.id));
      if (involved.length > 0) {
        setSelectedId(null);
        setBoqCode(null);
        setAiHighlight([]);
        setConflictIds(involved.map((m) => m.id));
        const here = involved.filter(onSheet);
        if (here.length > 0) {
          const box = unionBounds(here, batch);
          if (box) viewport.current?.fitTo(box);
        } else {
          const ids = involved.filter((m) => m.sheet === involved[0].sheet).map((m) => m.id);
          chooseSheet(involved[0].sheet, ids);
        }
      } else if (c.boq_code) {
        setSideTab("boq");
        selectBoq(c.boq_code);
      } else {
        const target = info?.sheets.find((s) => c.sheets.includes(s.name));
        if (target && target.block.toLowerCase() !== sheet.toLowerCase()) chooseSheet(target.block);
      }
    },
    [items, sheet, batch, info, selectBoq, generated],
  );
  const resolveConflict = useCallback((cmd: ResolveCommand) => apply(cmd), []);
  const openBoqTab = useCallback(() => setSideTab("boq"), []);
  const linkRow = useCallback(
    (id: string, code: string | null) => apply({ type: "link_boq", id, code }),
    [],
  );
  const setTolerance = useCallback(
    (code: string, percent: number | null) => apply({ type: "set_boq_tolerance", code, percent }),
    [],
  );
  const removeRow = useCallback((id: string) => apply({ type: "remove_measurement", id }), []);
  const renameRow = useCallback(
    (id: string, text: string) => apply({ type: "set_description", id, text }),
    [],
  );

  function showGenerated(handles: number[], sourceSheet: string) {
    void run(async () => {
      if (sourceSheet.toLowerCase() !== sheet.toLowerCase()) await chooseSheet(sourceSheet);
      setAiHighlight(handles);
    });
  }

  return {
    showGenerated,
    importBoqFile,
    onImportBoq,
    compareNow,
    onCompare,
    exportConflictsFile,
    onExportConflicts,
    selected,
    boqLinked,
    conflictLinked,
    selectRow,
    selectBoq,
    showConflict,
    resolveConflict,
    openBoqTab,
    linkRow,
    setTolerance,
    removeRow,
    renameRow,
  };
}

export type Takeoff = ReturnType<typeof useTakeoff>;
