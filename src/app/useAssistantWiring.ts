/** Mizan (the in-app assistant) and outside AI apps over MCP: what their tools ask of the window
 *  (another sheet, other layers, a zoom, a picture of the screen, a selection), the take-off and
 *  drawing they changed, the tool cards' Show and Undo, and what the user points Mizan at (the
 *  selection, an area marked with the `mizanArea` tool) with each message. */
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AI_CAPTURE,
  AI_POINTED,
  AI_SELECT,
  aiUndo,
  aiWindowReply,
  drawingInfo,
  PDF_PROGRESS,
  pickPolygon,
  SESSION_CHANGED,
  TAKEOFF_CHANGED,
  type AiContext,
  type AiImage,
  type AiRegion,
  type AiUi,
  type DrawingInfo,
} from "../api";
import type { ToolCardItem } from "../ai/messages";
import { kindsLabel, pointedKey, pointedTargets, regionBounds, regionRing } from "../ai/pointing";
import { useAssistant } from "../ai/useAssistant";
import type { Lang } from "../i18n";
import { metresPerUnit } from "../shell/AttachPdfDialog";
import type { Tool } from "../shell/Ribbon";
import { isStale } from "../shell/switcher";
import { handlesBounds } from "../tools/overlay";
import type { Measure, T } from "./shared";
import type { Layers } from "./useLayers";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { TakeoffState } from "./useTakeoff";

type Deps = Pick<Shell, "run" | "setSettingsOpen" | "viewport" | "setSideTab"> &
  Pick<Sheets, "batch" | "sheet" | "chooseSheet" | "queueReveal" | "cancelReveal" | "setSheetBlock" | "setHidden" | "setRenderProgress" | "redraw"> &
  Pick<TakeoffState, "refreshTakeoffAndBoq" | "setCompared" | "setAiHighlight"> &
  Pick<Layers, "syncInfo"> & {
    t: T;
    lang: Lang;
    switchGen: RefObject<number>;
    info: DrawingInfo | null;
    setInfo(info: DrawingInfo): void;
    autosave(): Promise<void>;
    /** The tool in use (another sheet cancels it), what it has selected, and Mizan's selecting. */
    measure: Pick<Measure, "cancel" | "picks" | "highlighted" | "kinds" | "select">;
    /** The tool chosen, and choosing another (marking an area, selecting for Mizan). */
    tool: Tool;
    chooseTool(tool: Tool): void;
  };

/** What the chips above Mizan's composer show, and what they do. */
export type Pointing = {
  /** The selection that goes with the next message: how many, and of which kinds. */
  selection: { count: number; kinds: string } | null;
  /** The area marked: its size in metres, and how many entities are in it (null while counted). */
  region: { width: number; height: number; count: number | null } | null;
  /** Leaves the selection out until it changes. */
  detachSelection(): void;
  clearRegion(): void;
  /** Starts the area tool. */
  markArea(): void;
  /** Sends `text` with what is pointed at and `images`; resolves false when it was not sent. */
  send(text: string, images: (AiImage & { url: string })[]): Promise<boolean>;
  /** Counts up when the composer should take the focus ("Ask Mizan" on the canvas menu). */
  focusKey: number;
  /** "Ask Mizan" on the canvas menu: the selection goes along, the composer takes the focus. */
  ask(): void;
};

export function useAssistantWiring({
  t,
  lang,
  switchGen,
  info,
  setInfo,
  run,
  autosave,
  setSettingsOpen,
  viewport,
  batch,
  sheet,
  chooseSheet,
  queueReveal,
  cancelReveal,
  setSheetBlock,
  setHidden,
  setRenderProgress,
  redraw,
  refreshTakeoffAndBoq,
  setCompared,
  setAiHighlight,
  syncInfo,
  measure,
  tool,
  chooseTool,
  setSideTab,
}: Deps) {
  // Refreshes after mutations (including new_layout) follow the session, never select a tab.
  useEffect(() => {
    if (info?.sheet) setSheetBlock(info.sheet);
  }, [info?.sheet]);

  /** Refresh shared drawing data without moving the user's sheet or camera. */
  function applyAiUi(ui: AiUi) {
    if (ui.action === "reveal") {
      if (ui.sheet?.toLowerCase() === sheet.toLowerCase() &&
          !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        queueReveal(ui.handles ?? []);
      } else {
        cancelReveal(false);
        void run(() => redraw());
      }
      return;
    }
    if (ui.hidden) {
      const names = new Set(ui.hidden.map(name => name.toUpperCase()));
      setHidden(new Set(info?.layers.map(layer => layer.name).filter(name => names.has(name.toUpperCase()))));
    }
    if (ui.action === "redraw" || ui.action === "show_sheet" || ui.action === "zoom") {
      void run(() => redraw());
    }
  }

  const assistant = useAssistant({
    lang,
    onMutated: async (label) => {
      if (label === "compare") setCompared(true);
      await run(async () => {
        await refreshTakeoffAndBoq();
        await syncInfo();
        await redraw();
      });
      await autosave();
    },
    onUi: applyAiUi,
    onNeedSettings: () => setSettingsOpen(true),
  });

  // An outside AI app changed the take-off through the MCP server: show it and save it, as after
  // the assistant's changes. The listener is set up once and calls the latest code through this.
  const onMcpChange = useRef<() => Promise<void>>(async () => {});
  onMcpChange.current = async () => {
    await run(async () => {
      await refreshTakeoffAndBoq();
      await syncInfo();
      await redraw();
    });
    await autosave();
  };
  // External tools refresh shared drawing data; the session retains the user's visible sheet.
  const onMcpUi = useRef<(ui: AiUi) => void>(() => {});
  onMcpUi.current = (ui) => {
    run(async () => {
      const gen = switchGen.current;
      const next = await drawingInfo();
      if (isStale(gen, switchGen.current)) return;
      setInfo(next);
      applyAiUi(ui);
    });
  };
  useEffect(() => {
    let gone = false;
    const offs: (() => void)[] = [];
    const keep = (unlisten: () => void) => {
      if (gone) unlisten();
      else offs.push(unlisten);
    };
    listen(TAKEOFF_CHANGED, () => onMcpChange.current()).then(keep);
    listen("measurement:changed", () => onMcpChange.current()).then(keep);
    listen<number>(PDF_PROGRESS, (e) => setRenderProgress(e.payload)).then(keep);
    listen<AiUi>(SESSION_CHANGED, (e) => onMcpUi.current(e.payload)).then(keep);
    return () => {
      gone = true;
      offs.forEach((off) => off());
    };
  }, []);

  const showCard = useCallback(
    (card: ToolCardItem) => {
      void run(async () => {
        const otherSheet = card.ui?.sheet && card.ui.sheet.toLowerCase() !== sheet.toLowerCase();
        if (otherSheet) await chooseSheet(card.ui!.sheet!);
        setAiHighlight(card.handles);
        const bounds = card.ui?.bounds ?? (otherSheet ? null : handlesBounds(card.handles, batch));
        if (bounds) viewport.current?.fitTo(bounds);
      });
    },
    [batch, sheet, chooseSheet, run, setAiHighlight, viewport],
  );

  const { markUndone, markUndoStale } = assistant;
  /** Undoes the card's change, unless the take-off changed since (then the card says so). */
  const undoCard = useCallback(
    async (card: ToolCardItem) => {
      cancelReveal();
      let undone = false;
      await run(async () => {
        undone = await aiUndo();
        if (undone) {
          await refreshTakeoffAndBoq();
          await redraw();
          await syncInfo();
        }
      });
      if (undone) {
        markUndone(card.id);
        await autosave();
      } else {
        markUndoStale(card.id);
      }
    },
    [markUndone, markUndoStale],
  );

  const openSettings = useCallback(() => setSettingsOpen(true), []);

  // What the user points at: the select tool's selection (unless its chip was removed) and an
  // area marked with the area tool.
  const selection = useMemo(
    () => (tool === "select" ? pointedTargets(measure.picks, measure.highlighted) : []),
    [tool, measure.picks, measure.highlighted],
  );
  const selectionKey = JSON.stringify(selection);
  const [detached, setDetached] = useState<string | null>(null);
  const attached = selection.length > 0 && selectionKey !== detached ? selection : [];
  const [region, setRegion] = useState<AiRegion | null>(null);
  const [regionCount, setRegionCount] = useState<number | null>(null);
  /** The tool in use before the area tool, chosen again once the area is marked. */
  const toolBefore = useRef<Tool>("select");
  /** What the last message pointed at: a picture goes only with something new. */
  const sentKey = useRef("");
  const [focusKey, setFocusKey] = useState(0);

  // Another drawing comes with nothing pointed at.
  const gen = switchGen.current;
  useEffect(() => {
    setRegion(null);
    setDetached(null);
    sentKey.current = "";
  }, [gen, info?.path]);

  const markRegion = useCallback((marked: AiRegion) => {
    setRegion(marked);
    setRegionCount(null);
    chooseTool(toolBefore.current === "mizanArea" ? "select" : toolBefore.current);
    setSideTab("assistant");
    setFocusKey((n) => n + 1);
    pickPolygon(regionRing(marked), true).then(
      (picked) => setRegionCount(picked.length),
      () => setRegionCount(null),
    );
  }, []);

  const regionKey = JSON.stringify(region);
  const attachedKey = JSON.stringify(attached);

  const { send: sendMessage } = assistant;
  const send = useCallback(
    async (text: string, images: (AiImage & { url: string })[]) => {
      const key = pointedKey(attached, region);
      const pointed = attached.length > 0 || region !== null;
      const shown: string[] = [];
      if (attached.length > 0) shown.push(t("ai.chip.selection", { n: String(attached.length) }));
      if (region) shown.push(t("ai.chip.areaShort"));
      const ok = await sendMessage(text, {
        context: {
          selection: attached,
          region,
          view: viewport.current?.visible() ?? null,
          picture: pointed && key !== sentKey.current,
        },
        images: images.map(({ media_type, data }) => ({ media_type, data })),
        shown: { pointed: shown, images: images.map((i) => i.url) },
      });
      if (ok) sentKey.current = key;
      return ok;
    },
    [attachedKey, regionKey, sendMessage, t],
  );

  const size = region ? regionBounds(region) : null;
  const toMetres = metresPerUnit(info?.units ?? "unknown");
  const kinds = kindsLabel(attached, measure.kinds, t);
  const pointing: Pointing = useMemo(
    () => ({
      selection: attached.length > 0 ? { count: attached.length, kinds } : null,
      region: size
        ? {
            width: (size[1][0] - size[0][0]) * toMetres,
            height: (size[1][1] - size[0][1]) * toMetres,
            count: regionCount,
          }
        : null,
      detachSelection: () => setDetached(selectionKey),
      clearRegion: () => setRegion(null),
      markArea: () => {
        toolBefore.current = tool;
        chooseTool("mizanArea");
      },
      send,
      focusKey,
      ask: () => {
        setDetached(null);
        setSideTab("assistant");
        setFocusKey((n) => n + 1);
      },
    }),
    [attachedKey, kinds, regionKey, regionCount, toMetres, selectionKey, tool, send, focusKey],
  );

  // Mizan's window tools (and outside AI apps'): a picture of the screen, what the user points
  // at, and a selection made for the user. The listeners are set up once and call the latest code
  // through this.
  const onWindowAsk = useRef({
    capture: async (_id: number, _maxSide: number) => {},
    pointed: (): Omit<AiContext, "picture"> => ({ selection: [], region: null, view: null }),
    select: (_handles: number[], _zoom: boolean) => {},
  });
  onWindowAsk.current = {
    capture: async (id, maxSide) => {
      const view = viewport.current;
      if (!view || !info) {
        await aiWindowReply(id, { error: "No drawing is on screen" });
        return;
      }
      try {
        await aiWindowReply(id, { value: await view.capture(maxSide) });
      } catch (e) {
        await aiWindowReply(id, { error: String(e) });
      }
    },
    pointed: () => ({ selection: attached, region, view: viewport.current?.visible() ?? null }),
    select: (handles, zoom) => {
      chooseTool("select");
      measure.select(handles);
      setDetached(null);
      if (zoom) {
        const bounds = handlesBounds(handles, batch);
        if (bounds) viewport.current?.fitTo(bounds);
      }
    },
  };
  useEffect(() => {
    let gone = false;
    const offs: (() => void)[] = [];
    const keep = (unlisten: () => void) => {
      if (gone) unlisten();
      else offs.push(unlisten);
    };
    listen<{ id: number; maxSide: number }>(AI_CAPTURE, (e) =>
      onWindowAsk.current.capture(e.payload.id, e.payload.maxSide).catch(() => {}),
    ).then(keep);
    listen<{ id: number }>(AI_POINTED, (e) =>
      aiWindowReply(e.payload.id, { value: onWindowAsk.current.pointed() }).catch(() => {}),
    ).then(keep);
    listen<{ handles: number[]; zoom: boolean }>(AI_SELECT, (e) =>
      onWindowAsk.current.select(e.payload.handles, e.payload.zoom),
    ).then(keep);
    return () => {
      gone = true;
      offs.forEach((off) => off());
    };
  }, []);

  return { assistant, showCard, undoCard, openSettings, pointing, markRegion };
}

export type AssistantWiring = ReturnType<typeof useAssistantWiring>;
