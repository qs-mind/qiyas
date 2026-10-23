import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { Paper, PlotJob, PlotScale, PlotStyle, Sheet, Units } from "../api";
import { useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import type { Batch } from "../viewport/batch";
import type { Point } from "../viewport/camera";
import { metresPerUnit } from "./AttachPdfDialog";
import { DotLoader } from "./DotLoader";

/** What a sheet plots with, kept per drawing and sheet (`usePlot`). */
export type PageSetup = {
  area: "extents" | "window" | "layout";
  /** The window's corners in sheet coordinates, lower left first; null until one is picked. */
  window: [Point, Point] | null;
  /** A paper of the catalog, by name. */
  paper: string;
  orientation: "portrait" | "landscape";
  /** Fit to the paper, or paper millimetres per sheet millimetres (1:100 is [1, 100]). */
  scale: "fit" | [number, number];
  centre: boolean;
  lineweights: boolean;
};

/** The paper a setup without one takes, and a job whose paper is not in the catalog. */
export const DEFAULT_PAPER = "ISO A4";

/** The drawing scales offered besides fit and a custom pair. */
export const SCALE_PRESETS: [number, number][] = [
  [1, 1],
  [1, 20],
  [1, 50],
  [1, 100],
  [1, 200],
  [1, 500],
];

/** The plot's margin when fitted, in millimetres (as `qiyas-plot` leaves it). */
const FIT_MARGIN_MM = 5;

/** A sheet's setup before one is kept: A4 landscape, fitted and centred, with lineweights; a
 *  layout plots its own area. */
export function defaultSetup(isModel: boolean): PageSetup {
  return {
    area: isModel ? "extents" : "layout",
    window: null,
    paper: DEFAULT_PAPER,
    orientation: "landscape",
    scale: "fit",
    centre: true,
    lineweights: true,
  };
}

/** The plot job for `sheet` with `setup`: its paper from the catalog (A4, or the first, when
 *  the catalog has no such paper), and the extents when a window was never picked. */
export function jobFor(
  sheet: string,
  setup: PageSetup,
  papers: Paper[],
  style: PlotStyle | null,
  output: string | null,
): PlotJob {
  const paper =
    papers.find((p) => p.name === setup.paper) ?? papers.find((p) => p.name === DEFAULT_PAPER) ?? papers[0];
  if (!paper) throw new Error("No paper sizes");
  return {
    sheet,
    area: setup.area !== "window" ? setup.area : setup.window ? { window: setup.window } : "extents",
    paper,
    orientation: setup.orientation,
    scale: setup.scale === "fit" ? "fit" : { ratio: setup.scale },
    centre: setup.centre,
    style,
    lineweights: setup.lineweights,
    output,
  };
}

/** `<drawing>.pdf` beside the drawing (its extension swapped). */
export function pdfBeside(drawingPath: string): string {
  return drawingPath.replace(/\.[^.\\/]*$/, "") + ".pdf";
}

/** Two corners as lower left and upper right. */
export function normalized([a, b]: [Point, Point]): [Point, Point] {
  return [
    [Math.min(a[0], b[0]), Math.min(a[1], b[1])],
    [Math.max(a[0], b[0]), Math.max(a[1], b[1])],
  ];
}

export type PageLayout = {
  /** The page's width and height in millimetres, turned for landscape. */
  page: [number, number];
  /** Where the area lands on the page, in millimetres from its lower left corner. */
  rect: [Point, Point];
  /** The whole area is on the page. */
  fits: boolean;
};

/** Where `area` (sheet units, `mmPerUnit` sheet millimetres each) lands on the paper, as
 *  `qiyas-plot` puts it: fitted inside a 5 mm margin or at the ratio, centred or with its lower
 *  left at the page's (the margin's when fitted). Null when the area is empty. */
export function pageLayout(
  paper: Paper,
  orientation: PageSetup["orientation"],
  scale: PlotScale,
  centre: boolean,
  area: [Point, Point],
  mmPerUnit: number,
): PageLayout | null {
  const page: [number, number] =
    orientation === "portrait" ? [paper.w_mm, paper.h_mm] : [paper.h_mm, paper.w_mm];
  const [lo, hi] = normalized(area);
  const [aw, ah] = [hi[0] - lo[0], hi[1] - lo[1]];
  const usable = (v: number) => v > 0 && Number.isFinite(v);
  if (!usable(aw) && !usable(ah)) return null;
  let s: number;
  let margin = 0;
  if (scale === "fit") {
    margin = FIT_MARGIN_MM;
    const fit = (side: number, size: number) => (usable(size) ? (side - 2 * margin) / size : Infinity);
    s = Math.min(fit(page[0], aw), fit(page[1], ah));
  } else {
    const [num, den] = scale.ratio;
    s = (num * mmPerUnit) / den;
  }
  if (!usable(s)) return null;
  const [dx, dy] = centre
    ? [page[0] / 2 - ((lo[0] + hi[0]) / 2) * s, page[1] / 2 - ((lo[1] + hi[1]) / 2) * s]
    : [margin - lo[0] * s, margin - lo[1] * s];
  const rect: [Point, Point] = [
    [lo[0] * s + dx, lo[1] * s + dy],
    [hi[0] * s + dx, hi[1] * s + dy],
  ];
  const e = 1e-6;
  const fits = rect[0][0] >= -e && rect[0][1] >= -e && rect[1][0] <= page[0] + e && rect[1][1] <= page[1] + e;
  return { page, rect, fits };
}

/** What the preview needs of the sheet's batch: its lines. */
export type PreviewBatch = Pick<Batch, "segments" | "colors" | "origin" | "bounds">;

/** The preview's longest side, and the area picture's, in pixels. */
const PREVIEW_PX = 200;
const PICTURE_PX = 600;

/** The lines of `batch` inside `area` on a white picture `PICTURE_PX` along its longer side, as
 *  the plot draws them (white plots black); null where there is no 2D canvas. */
function areaPicture(batch: PreviewBatch, area: [Point, Point]): HTMLCanvasElement | null {
  const [lo, hi] = area;
  const [aw, ah] = [Math.max(hi[0] - lo[0], 1e-9), Math.max(hi[1] - lo[1], 1e-9)];
  const k = PICTURE_PX / Math.max(aw, ah);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(aw * k));
  canvas.height = Math.max(1, Math.round(ah * k));
  const g = canvas.getContext("2d");
  if (!g) return null;
  const { segments, colors, origin } = batch;
  const byColor = new Map<number, number[]>();
  for (let i = 0; i < colors.length; i++) {
    const x0 = segments[i * 4] + origin[0];
    const y0 = segments[i * 4 + 1] + origin[1];
    const x1 = segments[i * 4 + 2] + origin[0];
    const y1 = segments[i * 4 + 3] + origin[1];
    if (Math.max(x0, x1) < lo[0] || Math.min(x0, x1) > hi[0] || Math.max(y0, y1) < lo[1] || Math.min(y0, y1) > hi[1]) {
      continue;
    }
    const list = byColor.get(colors[i]) ?? [];
    list.push(i);
    byColor.set(colors[i], list);
  }
  g.lineWidth = 1;
  for (const [rgb, list] of byColor) {
    const ink = rgb === 0xffffff ? 0 : rgb;
    g.strokeStyle = `#${ink.toString(16).padStart(6, "0")}`;
    g.beginPath();
    for (const i of list) {
      g.moveTo((segments[i * 4] + origin[0] - lo[0]) * k, (hi[1] - segments[i * 4 + 1] - origin[1]) * k);
      g.lineTo((segments[i * 4 + 2] + origin[0] - lo[0]) * k, (hi[1] - segments[i * 4 + 3] - origin[1]) * k);
    }
    g.stroke();
  }
  return canvas;
}

/** The page as it will plot: the paper, and the area's lines where they land. The area's
 *  picture is drawn once per area and kept in `cache` for the dialog's life. */
function PlotPreview({
  batch,
  area,
  layout,
  cache,
}: {
  batch: PreviewBatch | null;
  area: [Point, Point] | null;
  layout: PageLayout | null;
  cache: Map<string, HTMLCanvasElement | null>;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const page = layout?.page ?? [210, 297];
  const k = PREVIEW_PX / Math.max(page[0], page[1]);
  const [w, h] = [Math.round(page[0] * k), Math.round(page[1] * k)];

  useEffect(() => {
    const g = canvas.current?.getContext("2d");
    if (!g) return;
    g.clearRect(0, 0, w, h);
    g.fillStyle = "#ffffff";
    g.fillRect(0, 0, w, h);
    g.strokeStyle = "#6e6a64";
    g.strokeRect(0.5, 0.5, w - 1, h - 1);
    if (!batch || !area || !layout) return;
    const key = area.flat().join(",");
    if (!cache.has(key)) cache.set(key, areaPicture(batch, area));
    const picture = cache.get(key);
    if (!picture) return;
    const [lo, hi] = layout.rect;
    g.save();
    g.beginPath();
    g.rect(0, 0, w, h);
    g.clip();
    // The page has y up from its lower left; the canvas y down from its top left.
    g.drawImage(picture, lo[0] * k, h - hi[1] * k, (hi[0] - lo[0]) * k, (hi[1] - lo[1]) * k);
    g.restore();
  });

  return <canvas ref={canvas} width={w} height={h} aria-hidden="true" style={{ alignSelf: "center" }} />;
}

/** A number as typed into a field: up to three decimals, no trailing zeros. */
const shown = (v: number) => String(Number(v.toFixed(3)));

type Props = {
  /** The sheet plotted: the one on screen. */
  sheet: Sheet;
  /** The drawing's file: the PDF is offered beside it. */
  drawingPath: string;
  units: Units;
  /** The sheet's lines for the preview; null shows the paper only. */
  batch: PreviewBatch | null;
  /** Where to start: the sheet's kept setup, else `defaultSetup`. */
  setup: PageSetup;
  /** The plot style file kept for the drawing; null for none. */
  stylePath: string | null;
  /** The window's two corners once picked in the drawing; null keeps the setup's. */
  picked: [Point, Point] | null;
  /** True while the window waits for those clicks: the dialog steps aside. */
  picking: boolean;
  /** The paper sizes (`paper_catalog`). */
  papers(): Promise<Paper[]>;
  /** The printers and the default one. */
  printers(): Promise<{ printers: string[]; default: string | null }>;
  /** Asks for a CTB or STB file; null when none was chosen. */
  chooseStyle(): Promise<string | null>;
  loadStyle(path: string): Promise<PlotStyle>;
  /** Asks where the PDF goes, offering `defaultPath`; null when cancelled. */
  choosePdf(defaultPath: string): Promise<string | null>;
  /** Starts the two clicks in the drawing that give the window. */
  onPickWindow(): void;
  /** Keeps the setup for the sheet and the plot style for the drawing (on every plot or print). */
  onKeep(setup: PageSetup, stylePath: string | null): void;
  /** Plots to the PDF at `path` (the Save dialog's); a rejection is shown in the dialog, which
   *  stays open. */
  onPlotPdf(job: PlotJob, path: string): Promise<void>;
  /** Plots and prints on `printer` (the default one when null); a rejection is shown here. */
  onPrint(job: PlotJob, printer: string | null): Promise<void>;
  onClose(): void;
};

/** Plot: the sheet on screen, what to plot (extents, a window clicked in the drawing, or the
 *  layout), the paper and its orientation, the scale (fit, 1:1 to 1:500, or a custom pair),
 *  centring, lineweights and a plot style; a preview of the page; then Plot to PDF… (Save
 *  dialog) or Print on a printer. */
export function PlotDialog(props: Props) {
  const { sheet, picked, picking } = props;
  const t = useT();
  /** The area pictures drawn while the dialog is open, by area. */
  const pictures = useRef(new Map<string, HTMLCanvasElement | null>());
  const [setup, setSetup] = useState<PageSetup>(props.setup);
  const [scaleChoice, setScaleChoice] = useState(() =>
    props.setup.scale === "fit" ? "fit" : props.setup.scale.join(":"),
  );
  const [custom, setCustom] = useState<[string, string]>(() =>
    props.setup.scale === "fit" ? ["1", "1"] : [String(props.setup.scale[0]), String(props.setup.scale[1])],
  );
  const [papers, setPapers] = useState<Paper[] | null>(null);
  const [printers, setPrinters] = useState<string[]>([]);
  const [printer, setPrinter] = useState("");
  const [stylePath, setStylePath] = useState<string | null>(props.stylePath);
  const [style, setStyle] = useState<PlotStyle | null>(null);
  const [styleBusy, setStyleBusy] = useState(props.stylePath !== null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    props
      .papers()
      .then((list) => {
        if (!live) return;
        setPapers(list);
        // A kept setup whose paper the catalog no longer has is not used at all.
        if (!list.some((p) => p.name === props.setup.paper)) {
          setSetup(defaultSetup(props.sheet.is_model));
          setScaleChoice("fit");
        }
      })
      .catch((e) => live && setError(uiError(e)));
    props
      .printers()
      .then((found) => {
        if (!live) return;
        const list = found.default && !found.printers.includes(found.default) ? [found.default, ...found.printers] : found.printers;
        setPrinters(list);
        setPrinter(found.default ?? list[0] ?? "");
      })
      .catch(() => {});
    const remembered = props.stylePath;
    if (remembered) {
      props
        .loadStyle(remembered)
        .then((s) => live && setStyle(s))
        .catch((e) => {
          if (!live) return;
          setStylePath(null);
          setError(t("plot.styleUnreadable", { name: baseName(remembered), error: uiError(e) }));
        })
        .finally(() => live && setStyleBusy(false));
    }
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    if (picked) setSetup((s) => ({ ...s, area: "window", window: normalized(picked) }));
  }, [picked]);

  /** The scale chosen, or null while the custom pair is not two numbers above 0. */
  const scale: PageSetup["scale"] | null = (() => {
    if (scaleChoice === "fit") return "fit";
    const pair = (scaleChoice === "custom" ? custom : scaleChoice.split(":")).map(Number);
    return pair.every((v) => v > 0 && Number.isFinite(v)) ? [pair[0], pair[1]] : null;
  })();
  const chosen: PageSetup | null = scale ? { ...setup, scale } : null;

  const paper = papers?.find((p) => p.name === setup.paper) ?? papers?.find((p) => p.name === DEFAULT_PAPER) ?? papers?.[0];

  const mmPerUnit = sheet.is_model ? metresPerUnit(props.units) * 1000 : 1;
  const previewArea: [Point, Point] | null =
    setup.area === "window"
      ? setup.window
      : props.batch && props.batch.colors.length > 0
        ? props.batch.bounds
        : null;
  const layout =
    paper && previewArea && scale
      ? pageLayout(paper, setup.orientation, scale === "fit" ? "fit" : { ratio: scale }, setup.centre, previewArea, mmPerUnit)
      : null;
  const ready = !!papers && !!chosen && !busy && !styleBusy && (setup.area !== "window" || setup.window !== null);

  async function chooseStyleFile() {
    setError(null);
    try {
      const path = await props.chooseStyle();
      if (!path) return;
      setStyleBusy(true);
      const loaded = await props.loadStyle(path);
      setStyle(loaded);
      setStylePath(path);
    } catch (e) {
      setError(uiError(e));
    } finally {
      setStyleBusy(false);
    }
  }

  /** Plots to a PDF, or prints with `printTo` (a printer name, "" for the default one). Busy
   *  from the first click, so a second one starts nothing. */
  async function plot(printTo: string | null) {
    if (!ready || !chosen || !papers) return;
    setError(null);
    setBusy(true);
    try {
      const job = jobFor(sheet.block, chosen, papers, stylePath ? style : null, null);
      if (printTo === null) {
        const path = await props.choosePdf(pdfBeside(props.drawingPath));
        if (!path) return;
        await props.onPlotPdf(job, path);
      } else {
        await props.onPrint(job, printTo || null);
      }
      props.onKeep(chosen, stylePath);
      props.onClose();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const corners = setup.window
    ? `${shown(setup.window[0][0])}, ${shown(setup.window[0][1])} – ${shown(setup.window[1][0])}, ${shown(setup.window[1][1])}`
    : t("plot.noWindow");

  return (
    // Out of the way (hidden, its state kept) while the user clicks in the drawing, and back after.
    // Escape while plotting would lose what the plot says when it ends.
    <ModalDialog
      className="plot-dialog"
      style={{ width: 560 }}
      labelledBy="plot-title"
      open={!picking}
      keepMounted
      dismissible={!busy}
      onClose={props.onClose}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          plot(null);
        }}
      >
        <h2 id="plot-title" className="heading">
          {t("plot.title")}
        </h2>
        <label>
          <span>{t("plot.sheet")}</span>
          <input readOnly value={sheet.name} />
        </label>
        <div className="field">
          <label htmlFor="plot-area">{t("plot.area")}</label>
          <div className="row">
            <select
              id="plot-area"
              value={setup.area}
              onChange={(e) => setSetup({ ...setup, area: e.target.value as PageSetup["area"] })}
            >
              <option value="extents">{t("plot.area.extents")}</option>
              <option value="window">{t("plot.area.window")}</option>
              {!sheet.is_model && <option value="layout">{t("plot.area.layout")}</option>}
            </select>
            {setup.area === "window" && (
              <button type="button" disabled={busy} onClick={props.onPickWindow}>
                {t("plot.pickWindow")}
              </button>
            )}
          </div>
          {setup.area === "window" && (
            <span className="status" dir={setup.window ? "ltr" : undefined}>
              {corners}
            </span>
          )}
        </div>
        <label>
          <span>{t("plot.paper")}</span>
          <select value={paper?.name ?? ""} onChange={(e) => setSetup({ ...setup, paper: e.target.value })}>
            {(papers ?? []).map((p) => (
              <option key={p.name} value={p.name}>
                {t("plot.paperSize", { name: p.name, w: shown(p.w_mm), h: shown(p.h_mm) })}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>{t("plot.orientation")}</span>
          <select
            value={setup.orientation}
            onChange={(e) => setSetup({ ...setup, orientation: e.target.value as PageSetup["orientation"] })}
          >
            <option value="portrait">{t("plot.portrait")}</option>
            <option value="landscape">{t("plot.landscape")}</option>
          </select>
        </label>
        <div className="field">
          <label htmlFor="plot-scale">{t("plot.scale")}</label>
          <div className="row">
            <select
              id="plot-scale"
              value={scaleChoice}
              onChange={(e) => {
                const next = e.target.value;
                if (next === "custom" && scale && scale !== "fit") setCustom([String(scale[0]), String(scale[1])]);
                setScaleChoice(next);
              }}
            >
              <option value="fit">{t("plot.fit")}</option>
              {SCALE_PRESETS.map(([n, d]) => (
                <option key={d} value={`${n}:${d}`} dir="ltr">
                  {n}:{d}
                </option>
              ))}
              <option value="custom">{t("plot.custom")}</option>
            </select>
            {scaleChoice === "custom" && (
              <span className="row" dir="ltr">
                <input
                  type="number"
                  min={0}
                  step="any"
                  aria-label={t("plot.customPaper")}
                  value={custom[0]}
                  onChange={(e) => setCustom([e.target.value, custom[1]])}
                />
                <span>:</span>
                <input
                  type="number"
                  min={0}
                  step="any"
                  aria-label={t("plot.customSheet")}
                  value={custom[1]}
                  onChange={(e) => setCustom([custom[0], e.target.value])}
                />
              </span>
            )}
          </div>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={setup.centre}
            onChange={(e) => setSetup({ ...setup, centre: e.target.checked })}
          />
          <span>{t("plot.centre")}</span>
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={setup.lineweights}
            onChange={(e) => setSetup({ ...setup, lineweights: e.target.checked })}
          />
          <span>{t("plot.lineweights")}</span>
        </label>
        <div className="field">
          <label htmlFor="plot-style">{t("plot.style")}</label>
          <div className="row">
            <select id="plot-style" value={stylePath ?? ""} onChange={(e) => setStylePath(e.target.value || null)}>
              <option value="">{t("plot.styleNone")}</option>
              {stylePath && (
                <option value={stylePath} title={stylePath}>
                  {baseName(stylePath)}
                </option>
              )}
            </select>
            <button type="button" disabled={busy || styleBusy} onClick={chooseStyleFile}>
              {t("plot.styleChoose")}
            </button>
          </div>
        </div>
        <div className="field">
          <span>{t("plot.preview")}</span>
          <PlotPreview batch={props.batch} area={previewArea} layout={layout} cache={pictures.current} />
          {setup.area === "layout" ? (
            // The layout plots its own paper, which the window doesn't know: the preview is its drawing.
            <span className="status">{t("plot.previewExtents")}</span>
          ) : (
            layout && !layout.fits && <span className="status">{t("plot.offPaper")}</span>
          )}
        </div>
        <div className="field">
          <label htmlFor="plot-printer">{t("plot.printer")}</label>
          <div className="row">
            <select id="plot-printer" value={printer} onChange={(e) => setPrinter(e.target.value)}>
              {printers.length === 0 && <option value="">{t("plot.printerDefault")}</option>}
              {printers.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
            <button type="button" disabled={!ready} onClick={() => plot(printer)}>
              {t("plot.print")}
            </button>
          </div>
        </div>
        {busy && (
          <p className="status">
            <DotLoader pattern="chase" />
            {t("plot.plotting")}
          </p>
        )}
        {error && (
          <p className="error" role="alert" dir="auto">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" disabled={busy} onClick={props.onClose}>
            {t("plot.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!ready}>
            {t("plot.toPdf")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}

/** A path's file name. */
export const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;
