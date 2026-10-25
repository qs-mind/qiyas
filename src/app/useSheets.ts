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
import { handlesBounds, matchBounds } from "../tools/overlay";
import { fetchPictures, parseAtlas, parseBatch, type AtlasPage, type Batch, type HeldPicture } from "../viewport/batch";
import type { Bounds } from "../viewport/camera";
import { refreshSheet, type HeldSheet } from "../viewport/inset";
import { planReveal, revealFrame } from "../viewport/reveal";
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
  /** The first content needs a useful view even after an empty drawing was fitted. */
  const contentShown = useRef(false);
  /** Measurements to fit instead of the whole sheet, when a conflict's Show changed the sheet. */
  const fitIds = useRef<string[] | null>(null);
  /** The find match the next fit shows (Find next on another sheet). */
  const fitMatch = useRef<number | null>(null);
  /** Mizan's batches are fetched at once but shown in order, after their overlay finishes. */
  const [revealOverlay, setRevealOverlay] = useState<Float64Array | null>(null);
  const revealEpoch = useRef(0);
  const revealCount = useRef(0);
  const revealTail = useRef(Promise.resolve());
  const revealQueue = useRef<{ handles: number[] }[]>([]);
  const completedReveal = useRef<Float64Array | null>(null);
  const stopFrame = useRef<(() => void) | null>(null);
  /** A later redraw or cancellation makes an earlier fetch obsolete. */
  const drawRequest = useRef(0);

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

  /** Fetches a batch and its glyphs without putting it on screen yet. */
  async function fetchBatch(fit = false): Promise<HeldSheet | null> {
    const gen = switchGen.current;
    // A drawing change, its undo or a save may have changed the tab's unsaved dot.
    const [version] = await Promise.all([batchVersion(), refreshTabs()]);
    if (isStale(gen, switchGen.current)) return null;
    if (version === batchVersionRef.current && batchHeld.current) {
      return { version, batch: batchHeld.current };
    }
    const bytes = await renderBatch(undefined, fit ? undefined : viewport.current?.pxPerUnit());
    if (!bytes || isStale(gen, switchGen.current)) return null;
    const next = parseBatch(bytes);
    await syncAtlas(next);
    if (isStale(gen, switchGen.current)) return null;
    return { version, batch: next };
  }

  async function showBatch(next: HeldSheet, fit = false) {
    if (next.batch.ranges.length > 0 && !contentShown.current) {
      contentShown.current = true;
      fit = true;
    }
    if (next.batch === batchHeld.current) {
      if (fit) {
        fitNext.current = true;
        setFitTick((n) => n + 1);
      }
      return;
    }
    batchVersionRef.current = next.version;
    batchHeld.current = next.batch;
    fitNext.current = fit;
    setBatch(next.batch);
    if (fit) setFitTick((n) => n + 1);
    await syncInset();
  }

  /** Redraws wait for all reserved reveals, including ones still fetching their batch. */
  async function redraw(fit = false) {
    const gen = switchGen.current;
    while (revealCount.current > 0) await revealTail.current;
    if (isStale(gen, switchGen.current)) return;
    const request = ++drawRequest.current;
    const epoch = revealEpoch.current;
    const next = await fetchBatch(fit);
    if (isStale(gen, switchGen.current) || request !== drawRequest.current) return;
    // A tool result may have reserved a reveal while this fetch was in flight.
    if (epoch !== revealEpoch.current || revealCount.current > 0) return redraw(fit);
    if (next) await showBatch(next, fit);
  }

  /** Stops the animation and discards queued snapshots; the current core drawing wins. */
  function cancelReveal(showFinal = true) {
    if (revealCount.current === 0) return;
    drawRequest.current += 1;
    revealEpoch.current += 1;
    revealCount.current = 0;
    revealQueue.current = [];
    completedReveal.current = null;
    revealTail.current = Promise.resolve();
    stopFrame.current?.();
    stopFrame.current = null;
    setRevealOverlay(null);
    if (showFinal) void run(() => redraw());
  }

  /** Reserves before returning, so onMutated cannot show the new drawing early. Fetch now:
   *  waiting for the previous animation would let later changes enter this step's batch. */
  function queueReveal(handles: number[]) {
    const gen = switchGen.current;
    const epoch = revealEpoch.current;
    const stale = () => gen !== switchGen.current || epoch !== revealEpoch.current;
    revealCount.current += 1;
    drawRequest.current += 1;
    const entry = { handles };
    revealQueue.current.push(entry);
    const previous = revealTail.current;
    const fetched = fetchBatch();
    // A queued fetch can fail before the earlier animation finishes.
    void fetched.catch(() => {});
    revealTail.current = run(async () => {
      try {
        await previous;
        const next = await fetched;
        if (!next || stale()) return;
        const plan = planReveal(next.batch, handles);
        // Only the visible sheet reaches this queue. Fit before the overlay starts, including
        // reduced motion; fitExtents here would still see the previous (possibly empty) batch.
        const revealedBounds = handlesBounds(handles, next.batch);
        if (revealedBounds) {
          viewport.current?.fitTo(revealedBounds);
          contentShown.current = true;
        }
        if (plan && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
          await new Promise<void>((resolve) => {
            let frame = 0;
            let start: number | null = null;
            const finish = () => {
              cancelAnimationFrame(frame);
              stopFrame.current = null;
              resolve();
            };
            stopFrame.current = finish;
            const tick = (now: number) => {
              if (stale()) {
                finish();
                return;
              }
              start ??= now;
              const elapsed = now - start;
              if (elapsed >= plan.total) {
                finish();
                return;
              }
              setRevealOverlay(withCompleted(revealFrame(plan, elapsed)));
              frame = requestAnimationFrame(tick);
            };
            frame = requestAnimationFrame(tick);
          });
        }
        if (stale()) return;
        // The core can run the next tool before this batch was fetched. Keep completed
        // strokes in the overlay if showing it would expose a queued entity too early.
        const waiting = new Set(revealQueue.current.slice(1).flatMap((job) => job.handles).map(BigInt));
        const ranges = next.batch.ranges;
        let includesWaiting = false;
        for (let i = 0; i < ranges.length && !includesWaiting; i++) {
          includesWaiting = waiting.has(ranges.top(i));
        }
        if (plan && includesWaiting) {
          completedReveal.current = withCompleted(plan.segments);
          setRevealOverlay(completedReveal.current);
          return;
        }
        completedReveal.current = null;
        setRevealOverlay(null);
        await showBatch(next);
      } finally {
        if (epoch === revealEpoch.current) {
          if (gen !== switchGen.current) cancelReveal(false);
          else {
            revealCount.current -= 1;
            revealQueue.current = revealQueue.current.filter((job) => job !== entry);
            if (revealCount.current === 0) {
              completedReveal.current = null;
              setRevealOverlay(null);
            }
          }
        }
      }
    });
  }

  function withCompleted(frame: Float64Array): Float64Array {
    const completed = completedReveal.current;
    if (!completed) return frame;
    const all = new Float64Array(completed.length + frame.length);
    all.set(completed);
    all.set(frame, completed.length);
    return all;
  }

  useEffect(() => {
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => {
      if (motion.matches) cancelReveal();
    };
    motion.addEventListener("change", changed);
    return () => {
      motion.removeEventListener("change", changed);
      cancelReveal(false);
    };
  }, []);

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
    if (!batchHeld.current || glyphsBusy.current || revealCount.current > 0) return;
    glyphsBusy.current = true;
    const gen = switchGen.current;
    const epoch = revealEpoch.current;
    const request = ++drawRequest.current;
    await run(async () => {
      const version = await batchVersion();
      const bytes = await renderBatch(undefined, pxPerUnit);
      if (!bytes || isStale(gen, switchGen.current)) return;
      let next = parseBatch(bytes);
      await syncAtlas(next);
      if (isStale(gen, switchGen.current) || epoch !== revealEpoch.current ||
          request !== drawRequest.current || revealCount.current > 0) return;
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
    cancelReveal(false);
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
      cancelReveal(false);
      await setSheet(block);
      // Read back the canonical window sheet; automation restores it after every tool call.
      const next = await drawingInfo();
      setSheetBlock(next.sheet);
      contentShown.current = false;
      // The layers panel's VP freeze column lists this sheet's viewports.
      setInfo(next);
      measure.current?.cancel();
      fitIds.current = fit ?? null;
      await redraw(true);
    });

  /** Saves the view on screen as the named view `name` (a drawing change). Errors go to the menu. */
  async function saveView(name: string) {
    cancelReveal();
    const box = viewport.current?.visible();
    if (!box) return;
    await runCommand({ type: "save_view", name, bounds: box });
    await redraw();
  }

  /** Removes the named view `name` (a drawing change). Errors go to the menu. */
  async function deleteView(name: string) {
    cancelReveal();
    await runCommand({ type: "delete_view", name });
    await redraw();
  }

  /** Forgets everything held about the drawing on screen: the batch, the glyph atlas, the pictures
   *  (all per drawing) and the inset; the second view showed another drawing's sheet. */
  function reset() {
    cancelReveal(false);
    drawRequest.current += 1;
    batchVersionRef.current = null;
    batchHeld.current = null;
    contentShown.current = false;
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
    cancelReveal(false);
    setBatch(null);
    setSheetBlock("");
    setHidden(new Set());
  }

  return {
    batch,
    batchHeld,
    revealOverlay,
    queueReveal,
    cancelReveal,
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
