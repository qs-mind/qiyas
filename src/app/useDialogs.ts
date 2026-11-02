import { createContext, createElement, useCallback, useContext, useMemo, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import type { BlockInfo, Command, PaletteItem, ReadReport, ScriptResult, Sheet, TableInfo } from "../api";
import type { AttributeField } from "../shell/BlockDialogs";
import type { PageSetup } from "../shell/PlotDialog";
import type { DimRequest } from "../tools/dimTools";
import type { TextRequest } from "../tools/textTools";
import type { Point } from "../viewport/camera";

type SimpleDialog =
  | "about" | "settings" | "connect" | "shortcuts"
  | "options" | "customize" | "aliases" | "purge" | "audit" | "drafting"
  | "layerStates" | "translator" | "attach" | "attachImage" | "blockPalette"
  | "find" | "dimStyle" | "parameters" | "sheetSet" | "xrefs" | "extract" | "new";

type DialogPayloads = Record<SimpleDialog, true> & {
  group: Extract<Command, { type: "create_group" }>;
  calibration: [Point, Point];
  createBlock: {
    cmd: Extract<Command, { type: "create_block" }>;
    names: string[];
  };
  attributes: {
    heading: string;
    fields: AttributeField[];
    command(values: Record<string, string>): Command | null;
  };
  attDefAt: { at: Point; height: number | null };
  designCenter: string[];
  palette: PaletteItem[];
  managerBlocks: BlockInfo[];
  dimText: DimRequest;
  newTableAt: Point;
  tableEditing: {
    handle: number;
    table: TableInfo;
    cell: [number, number];
  };
  field: { mode: "text"; at: Point; height: number | null } | { mode: "cell"; cell: [number, number] };
  textDialog:
    | { kind: "mtext"; request: Extract<TextRequest, { kind: "mtext" }> }
    | { kind: "edit"; handle: number; value: string; multiline: boolean };
  recoverReport: { path: string; report: ReadReport };
  scriptRun: { path: string; result: ScriptResult };
  plot: { sheet: Sheet; setup: PageSetup; stylePath: string | null };
  styles: { initialPath: string | null };
};
export type DialogId = keyof DialogPayloads;
export type DialogEntry = { [K in DialogId]: { openDialog: K; payload: DialogPayloads[K] } }[DialogId];
type DialogState = (DialogEntry | { openDialog: null; payload: null }) & { stacked: readonly DialogEntry[] };

const empty: DialogState = { openDialog: null, payload: null, stacked: [] };
const entries = ({ stacked, ...top }: DialogState): DialogEntry[] =>
  top.openDialog === null ? [...stacked] : [...stacked, top];
const fromEntries = (all: DialogEntry[]): DialogState => {
  const top = all.at(-1);
  return top ? { ...top, stacked: all.slice(0, -1) } : empty;
};

function useDialogStore() {
  const [state, setState] = useState<DialogState>(empty);
  const update = useCallback(<K extends DialogId>(id: K, next: SetStateAction<DialogPayloads[K] | null>) => {
    setState((was) => {
      const all = entries(was);
      const index = all.findIndex((entry) => entry.openDialog === id);
      const previous = index < 0 ? null : all[index].payload as DialogPayloads[K];
      const payload = typeof next === "function" ? next(previous) : next;
      if (Object.is(previous, payload)) return was;
      if (payload === null) return fromEntries(all.filter((entry) => entry.openDialog !== id));
      const entry = { openDialog: id, payload } as DialogEntry;
      if (index < 0) all.push(entry);
      else all[index] = entry;
      return fromEntries(all);
    });
  }, []);
  const close = useCallback((id: DialogId) => update(id, null), [update]);
  return useMemo(() => ({
    ...state,
    update,
    close,
    get<K extends DialogId>(id: K): DialogPayloads[K] | null {
      return entries(state).find((entry) => entry.openDialog === id)?.payload as DialogPayloads[K] ?? null;
    },
    isOpen(id: DialogId) { return entries(state).some((entry) => entry.openDialog === id); },
  }), [state, update, close]);
}

const DialogContext = createContext<ReturnType<typeof useDialogStore> | null>(null);

export function DialogsProvider({ children }: { children: ReactNode }) {
  const dialogs = useDialogStore();
  return createElement(DialogContext.Provider, { value: dialogs }, children);
}

/** Modeless dialogs and nested editors keep their existing layers until explicitly closed. */
export function useDialogs() {
  const dialogs = useContext(DialogContext);
  if (!dialogs) throw new Error("DialogsProvider is missing");
  return dialogs;
}

/** Dialog payloads live together; drawing and tool state stays in its owning hook. */
export function useDialogState<K extends DialogId>(id: K): [DialogPayloads[K] | null, Dispatch<SetStateAction<DialogPayloads[K] | null>>] {
  const { get, update } = useDialogs();
  const set = useCallback<Dispatch<SetStateAction<DialogPayloads[K] | null>>>((next) => update(id, next), [id, update]);
  return [get(id), set];
}

export function useDialogOpen(id: SimpleDialog): [boolean, Dispatch<SetStateAction<boolean>>] {
  const [payload, setPayload] = useDialogState(id);
  const setOpen = useCallback<Dispatch<SetStateAction<boolean>>>((next) => {
    setPayload((was) => (typeof next === "function" ? next(was === true) : next) ? true : null);
  }, [setPayload]);
  return [payload === true, setOpen];
}
