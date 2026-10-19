/** What the canvas shows of the drawing on screen: the sheet and its hidden layers, the batch and
 *  its glyph atlas, underlay pictures, the layout's model-space inset, the split view, and the
 *  fits that follow a sheet or drawing change. */
import { useEffect, useRef, useState, type RefObject } from "react";
import {
  atlasPages,
  batchVersion,
  drawingInfo,
  paperViews,
  renderBatch,
  runCommand,
  setSheet,
  sheetBatch,
  sheetBatchVersion,
  underlayImage,
  type DrawingInfo,
  type Measurement,
  type PaperView,
} from "../api";
import { isStale } from "../shell/switcher";
import { matchBounds } from "../tools/overlay";
import { fetchPictures, parseAtlas, parseBatch, type AtlasPage, type Batch, type HeldPicture } from "../viewport/batch";
import type { Bounds } from "../viewport/camera";
import { refreshSheet, type HeldSheet } from "../viewport/inset";
import type { ViewportHandle } from "../viewport/Viewport";
import { MODEL, unionBounds } from "./shared";

type Deps = {
  switchGen: RefObject<number>;
  viewport: RefObject<ViewportHandle | null>;
  run(step: () => Promise<void>): Promise<void>;
  autosave(): Promise<void>;
  setInfo(info: DrawingInfo): void;
  /** Reads the file tabs again: a redraw may have changed a tab's unsaved dot. */
  refreshTabs(): Promise<void>;
  /** The take-off rows, for a fit to some of them (`fitIds`). */
  items: Measurement[];
  /** The tool in use, cancelled when another sheet comes on screen. Bound late: the tool hook
   *  takes this hook's state, so the root fills the ref after calling it. */
  measure: RefObject<{ cancel(): void } | null>;
};

const UCS_ICON_KEY = "qiyas.ucsIcon";

/** Whether the UCS icon shows: on unless it was turned off on this computer. */
function readUcsIcon(): boolean {
  try {
    return localStorage.getItem(UCS_ICON_KEY) !== "off";
  } catch {
    return true;
  }
}

export function useSheets({ switchGen, viewport, run, autosave, setInfo, refreshTabs, items, measure }: Deps) {
  const [batch, setBatch] = useState<Batch | null>(null);
  /** The batch in hand and the core's version of it (`batchVersion`), so a redraw that changed
   *  nothing on the sheet doesn't fetch the whole batch again. */
  const batchHeld = useRef<Batch | null>(null);
  const batchVersionRef = useRef<number | null>(null);
  /** On a layout: model space drawn inside its viewports, and the core's version of it. */
  const [inset, setInset] = useState<{ batch: Batch; views: PaperView[] } | null>(null);
  const insetHeld = useRef<HeldSheet | null>(null);
  /** VPORTS: a second view beside the main one. */
  const [split, setSplit] = useState(false);
  /** Bumped to run the fit effect when a redraw keeps the batch it has. */
  const [fitTick, setFitTick] = useState(0);
  /** Every glyph atlas page received for this drawing, and the atlas version they make up. */
  const [atlas, setAtlas] = useState<AtlasPage[]>([]);
  const atlasHeld = useRef(new Map<number, AtlasPage>());
  const atlasVersionRef = useRef<number | null>(null);
  /** True while glyphs are fetched for a new zoom, so zooming on doesn't stack requests. */
  const glyphsBusy = useRef(false);
  /** The underlay pictures held, by image id, and whether some are being fetched. */
  const [pictures, setPictures] = useState(new Map<number, HeldPicture>());
  const picturesHeld = useRef(new Map<number, HeldPicture>());
  const picturesBusy = useRef(false);
  /** Percent drawn of the PDF page pdfium is rendering; null when none is. */
  const [renderProgress, setRenderProgress] = useState<number | null>(null);
  /** The drawing holds its Arabic text backwards (View tab). */
  const [arabicReversed, setArabicReversed] = useState(false);
  /** The UCS icon on the canvas (UCSICON, View tab), kept on this computer. */
  const [ucsIcon, setUcsIcon] = useState(readUcsIcon);
  const [sheet, setSheetBlock] = useState("");
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  /** Set when the next batch is a new drawing or sheet, so the view fits it once it is drawn. */
  const fitNext = useRef(false);
  /** Measurements to fit instead of the whole sheet, when a conflict's Show changed the sheet. */
  const fitIds = useRef<string[] | null>(null);
  /** The find match the next fit shows (Find next on another sheet). */
  const fitMatch = useRef<number | null>(null);

  // Runs after the viewport has taken the new batch (child effects run first).
  useEffect(() => {
    if (!fitNext.current) return;
    fitNext.current = false;
    const ids = fitIds.current;
    fitIds.current = null;
    // A find's match on the sheet just shown wins over the take-off rows and the whole sheet.
    const match = fitMatch.current;
    fitMatch.current = null;
    const box =
      match !== null ? matchBounds(match, batch) : ids && unionBounds(items.filter((m) => ids.includes(m.id)), batch);
    if (box) viewport.current?.fitTo(box);
    else viewport.current?.fitExtents();
  }, [batch, fitTick]);

  /** Fetches the atlas pages `next` needs that the window doesn't hold yet. */
  async function syncAtlas(next: Batch) {
    if (next.atlasVersion === 0 || next.atlasVersion === atlasVersionRef.current) return;
    const gen = switchGen.current;
    const update = parseAtlas(await atlasPages(atlasVersionRef.current ?? undefined));
    // Another drawing's atlas: never mixed into this one's.
    if (isStale(gen, switchGen.current)) return;
    for (const page of update.pages) atlasHeld.current.set(page.index, page);
    atlasVersionRef.current = update.version;
    setAtlas([...atlasHeld.current.values()]);
  }

  /** Draws the core's current batch. When the core says the drawing, sheet and layers are as they
   *  were at the last draw, the batch in hand stays and only the fit (if asked) happens. A fit draws
   *  the glyphs for the core's own guess at the zoom; otherwise for the zoom on screen. */
  async function redraw(fit = false) {
    const gen = switchGen.current;
    // A drawing change, its undo or a save may have changed the tab's unsaved dot.
    const [version] = await Promise.all([batchVersion(), refreshTabs()]);
    if (isStale(gen, switchGen.current)) return;
    if (version === batchVersionRef.current && batchHeld.current) {
      // Nothing to send; the effect below still fits the view once the window has rendered.
      if (fit) {
        fitNext.current = true;
        setFitTick((n) => n + 1);
      }
      return;
    }
    const bytes = await renderBatch(undefined, fit ? undefined : viewport.current?.pxPerUnit());
    if (!bytes || isStale(gen, switchGen.current)) return;
    const next = parseBatch(bytes);
    await syncAtlas(next);
    if (isStale(gen, switchGen.current)) return;
    batchVersionRef.current = version;
    batchHeld.current = next;
    fitNext.current = fit;
    setBatch(next);
    await syncInset();
  }

  /** On a layout, fetches model space for its viewports (when the core's changed) and how each
   *  shows it; elsewhere drops it. Its glyphs are drawn for the largest viewport's zoom. */
  async function syncInset(pxPerUnit?: number) {
    const gen = switchGen.current;
    let views = await paperViews();
    if (isStale(gen, switchGen.current)) return;
    if (views.length === 0) {
      insetHeld.current = null;
      setInset(null);
      return;
    }
    const scale = Math.max(...views.map((v) => v.scale));
    const held = insetHeld.current;
    const next = await refreshSheet(held, pxPerUnit === undefined ? undefined : pxPerUnit * scale, {
      version: () => sheetBatchVersion(MODEL),
      fetch: async (px) => {
        const bytes = await sheetBatch(MODEL, undefined, px ?? (viewport.current?.pxPerUnit() ?? 1) * scale);
        if (!bytes || isStale(gen, switchGen.current)) return null;
        const batch = parseBatch(bytes);
        await syncAtlas(batch);
        return batch;
      },
    });
    if (!next || isStale(gen, switchGen.current)) return;
    // Glyphs drawn again move the viewports' text spans: ask for them again.
    if (next !== held) views = await paperViews();
    if (isStale(gen, switchGen.current)) return;
    insetHeld.current = next;
    setInset({ batch: next.batch, views });
  }

  /** Fetches the underlay pictures in `visible` that `next` needs for a zoom of `pxPerUnit` and
   *  the window doesn't hold at that version and about that zoom, and lets go of those out of view
   *  (their textures go too). One fetch runs at a time; the viewport asks again when the view
   *  settles. */
  async function syncPictures(next: Batch | null, pxPerUnit: number, visible: Bounds) {
    if (picturesBusy.current) return;
    picturesBusy.current = true;
    const gen = switchGen.current;
    const held = picturesHeld.current;
    // A drawing change replaces the map held; nothing fetched before it is kept.
    const stale = () => isStale(gen, switchGen.current) || picturesHeld.current !== held;
    try {
      const fetched = await fetchPictures(
        next,
        held,
        pxPerUnit,
        visible,
        (id) => underlayImage(id, pxPerUnit),
        stale,
      );
      if (fetched && !stale()) {
        picturesHeld.current = fetched;
        setPictures(new Map(fetched));
      }
    } finally {
      picturesBusy.current = false;
      setRenderProgress(null);
    }
  }

  /** Draws the text again for the zoom on screen (`pxPerUnit` screen pixels per drawing unit). The
   *  lines of the batch in hand stay when the core's batch is the same one. */
  async function redrawGlyphs(pxPerUnit: number) {
    if (!batchHeld.current || glyphsBusy.current) return;
    glyphsBusy.current = true;
    const gen = switchGen.current;
    await run(async () => {
      const version = await batchVersion();
      const bytes = await renderBatch(undefined, pxPerUnit);
      if (!bytes || isStale(gen, switchGen.current)) return;
      let next = parseBatch(bytes);
      await syncAtlas(next);
      if (isStale(gen, switchGen.current)) return;
      const held = batchHeld.current;
      if (held && version === batchVersionRef.current) {
        // The same lines and fills: keep the arrays the viewport has uploaded already.
        const { segments, colors, ranges, chains, fills, fillRanges } = held;
        next = { ...next, segments, colors, ranges, chains, fills, fillRanges };
      }
      batchVersionRef.current = version;
      batchHeld.current = next;
      setBatch(next);
      if (insetHeld.current) await syncInset(pxPerUnit);
    });
    glyphsBusy.current = false;
  }

  /** Shows or hides the UCS icon, and keeps the choice on this computer. */
  function chooseUcsIcon(on: boolean) {
    setUcsIcon(on);
    try {
      localStorage.setItem(UCS_ICON_KEY, on ? "on" : "off");
    } catch {
      // Kept for this run only.
    }
  }

  /** Says whether the drawing holds its Arabic text backwards, and redraws its text. */
  async function chooseArabicReversed(on: boolean) {
    let changed = false;
    await run(async () => {
      await runCommand({ type: "set_arabic_reversed", on });
      setArabicReversed(on);
      changed = true;
      await redraw();
    });
    if (changed) await autosave();
  }

  /** Shows the sheet `block`, fitted to the measurements `fit` when given, else to the whole sheet. */
  const chooseSheet = (block: string, fit?: string[]) =>
    run(async () => {
      await setSheet(block);
      setSheetBlock(block);
      // The layers panel's VP freeze column lists this sheet's viewports.
      setInfo(await drawingInfo());
      measure.current?.cancel();
      fitIds.current = fit ?? null;
      await redraw(true);
    });

  /** Saves the view on screen as the named view `name` (a drawing change). Errors go to the menu. */
  async function saveView(name: string) {
    const box = viewport.current?.visible();
    if (!box) return;
    await runCommand({ type: "save_view", name, bounds: box });
    await redraw();
  }

  /** Removes the named view `name` (a drawing change). Errors go to the menu. */
  async function deleteView(name: string) {
    await runCommand({ type: "delete_view", name });
    await redraw();
  }

  /** Forgets everything held about the drawing on screen: the batch, the glyph atlas, the pictures
   *  (all per drawing) and the inset; the second view showed another drawing's sheet. */
  function reset() {
    batchVersionRef.current = null;
    batchHeld.current = null;
    insetHeld.current = null;
    setInset(null);
    setSplit(false);
    atlasVersionRef.current = null;
    atlasHeld.current = new Map();
    setAtlas([]);
    picturesHeld.current = new Map();
    setPictures(new Map());
  }

  /** After the last tab closed: no sheet to show. */
  function clear() {
    setBatch(null);
    setSheetBlock("");
    setHidden(new Set());
  }

  return {
    batch,
    batchHeld,
    inset,
    insetHeld,
    split,
    setSplit,
    atlas,
    pictures,
    renderProgress,
    setRenderProgress,
    arabicReversed,
    setArabicReversed,
    ucsIcon,
    chooseUcsIcon,
    sheet,
    setSheetBlock,
    hidden,
    setHidden,
    fitMatch,
    syncAtlas,
    redraw,
    syncInset,
    syncPictures,
    redrawGlyphs,
    chooseArabicReversed,
    chooseSheet,
    saveView,
    deleteView,
    reset,
    clear,
  };
}

export type Sheets = ReturnType<typeof useSheets>;
