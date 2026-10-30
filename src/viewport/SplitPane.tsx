import { isWorkspaceBusy } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import { paperViews, sheetBatch, sheetBatchVersion, type PaperView, type Sheet, type Ucs } from "../api";
import { useT } from "../i18n";
import { parseBatch, type AtlasPage, type Batch } from "./batch";
import type { CanvasColours } from "./colours";
import type { Theme } from "./gl";
import { refreshSheet, type HeldSheet } from "./inset";
import { Viewport, type ViewportHandle } from "./Viewport";

const MODEL = "*Model_Space";
const NO_TARGETS: never[] = [];

type Props = {
  /** The drawing's sheets. */
  sheets: Sheet[];
  /** The sheet on screen in the main view, its batch and what its viewports show: used as they
   *  are when this view shows the same sheet, and its change draws this one again. */
  mainSheet: string;
  mainBatch: Batch | null;
  mainInset: { batch: Batch; views: PaperView[] } | null;
  atlas: AtlasPage[];
  theme: Theme;
  /** The user's background and highlight colours (as the main view's). */
  colours?: CanvasColours | null;
  ucs?: Ucs | null;
  /** The UCS icon shows (as in the main view). */
  ucsIcon?: boolean;
  /** Fetches the glyph atlas pages `b` needs that the window doesn't hold yet. */
  syncAtlas(b: Batch): Promise<void>;
  onError(e: unknown): void;
  onClose(): void;
};

/** VPORTS: the second view of a split window, beside the main one, on the drawing on screen. It has
 *  its own camera and sheet (the main one's batch shared when it shows the same sheet) and only
 *  looks: pan and zoom here, tools work in the main view. */
export function SplitPane({
  sheets,
  mainSheet,
  mainBatch,
  mainInset,
  atlas,
  theme,
  colours = null,
  ucs = null,
  ucsIcon = true,
  syncAtlas,
  onError,
  onClose,
}: Props) {
  const t = useT();
  const view = useRef<ViewportHandle>(null);
  const [sheet, setSheet] = useState(mainSheet);
  const [own, setOwn] = useState<{ batch: Batch; inset: { batch: Batch; views: PaperView[] } | null } | null>(
    null,
  );
  const fit = useRef(true);
  const same = sheet.toLowerCase() === mainSheet.toLowerCase();

  /** This view's sheet and model space for its viewports, as held, with the core's versions. */
  const held = useRef<{ sheet: string; own: HeldSheet | null; model: HeldSheet | null }>({
    sheet: "",
    own: null,
    model: null,
  });

  // Another sheet, or the drawing may have changed (the main batch is new): fetched again only
  // when the core's version of this sheet, or of model space, changed.
  useEffect(() => {
    if (same) {
      setOwn(null);
      held.current = { sheet: "", own: null, model: null };
      return;
    }
    let gone = false;
    (async () => {
      const h = held.current.sheet === sheet ? held.current : { sheet, own: null, model: null };
      const fetchOf = (name: string) => async () => {
        const bytes = await sheetBatch(name);
        if (!bytes || gone) return null;
        const batch = parseBatch(bytes);
        await syncAtlas(batch);
        return batch;
      };
      const own = await refreshSheet(h.own, undefined, {
        version: () => sheetBatchVersion(sheet),
        fetch: fetchOf(sheet),
      });
      if (!own || gone) return;
      const views = await paperViews(sheet);
      const model =
        views.length === 0
          ? null
          : await refreshSheet(h.model, undefined, {
              version: () => sheetBatchVersion(MODEL),
              fetch: fetchOf(MODEL),
            });
      if (gone) return;
      const unchanged = own === h.own && model === h.model && held.current.sheet === sheet;
      held.current = { sheet, own, model };
      if (unchanged) return;
      setOwn({ batch: own.batch, inset: model && views.length > 0 ? { batch: model.batch, views } : null });
    })().catch(error => { if (!gone && !isWorkspaceBusy(error)) onError(error); });
    return () => {
      gone = true;
    };
  }, [sheet, same, mainBatch]);

  const batch = same ? mainBatch : (own?.batch ?? null);
  const inset = same ? mainInset : (own?.inset ?? null);

  // A new sheet here is fitted once its batch is in.
  useEffect(() => {
    if (fit.current && batch) {
      fit.current = false;
      view.current?.fitExtents();
    }
  }, [batch]);

  return (
    <div className="split-pane">
      <div className="split-bar">
        <select
          aria-label={t("vport.sheet")}
          value={sheet}
          onChange={(e) => {
            fit.current = true;
            setSheet(e.target.value);
          }}
        >
          {sheets.map((s) => (
            <option key={s.block} value={s.block}>
              {s.name}
            </option>
          ))}
        </select>
        <button type="button" onClick={onClose} aria-label={t("vport.one")} title={t("vport.one")}>
          ×
        </button>
      </div>
      <div className="split-view">
        <Viewport
          ref={view}
          batch={batch}
          atlas={atlas}
          highlight={NO_TARGETS}
          tool="select"
          theme={theme}
          colours={colours}
          ucs={ucs}
          ucsIcon={ucsIcon}
          inset={inset}
          onClick={() => {}}
          onMove={() => {}}
        />
      </div>
    </div>
  );
}
