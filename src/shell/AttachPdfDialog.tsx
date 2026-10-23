import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { AttachMode, Units } from "../api";
import { useT } from "../i18n";
import { Progress } from "../ui/Progress";
import { SegmentedChoice } from "../ui/Segmented";
import type { Picture } from "../viewport/batch";
import type { Point } from "../viewport/camera";
import { DotLoader } from "./DotLoader";

/** Pages shown (and thumbnails fetched) at a time. */
export const PAGES_AT_ONCE = 20;
/** The drawing scales offered, as 1:N. */
export const SCALES = [50, 100, 200, 500];
/** ISO A sheets in millimetres, short side first. */
const SHEETS: [string, number, number][] = [
  ["A0", 841, 1189],
  ["A1", 594, 841],
  ["A2", 420, 594],
  ["A3", 297, 420],
  ["A4", 210, 297],
];
const MM_PER_POINT = 25.4 / 72;

/** Metres in one drawing unit; unknown units are millimetres, as everywhere in Qiyas. */
export function metresPerUnit(units: Units): number {
  return { inches: 0.0254, feet: 0.3048, millimetres: 0.001, unknown: 0.001, centimetres: 0.01, metres: 1 }[
    units
  ];
}

/** Drawing units per PDF point for a drawing at 1:`n` printed on the page. */
export function scaleFor(n: number, units: Units): number {
  return (MM_PER_POINT * n) / 1000 / metresPerUnit(units);
}

/** The ISO A sheet a page of `w` x `h` points is (within 2 percent), or its size in millimetres. */
export function sheetName(w: number, h: number): string {
  const [a, b] = [Math.min(w, h) * MM_PER_POINT, Math.max(w, h) * MM_PER_POINT];
  const sheet = SHEETS.find(([, s, l]) => Math.abs(a - s) <= 0.02 * s && Math.abs(b - l) <= 0.02 * l);
  return sheet ? sheet[0] : `${Math.round(w * MM_PER_POINT)} × ${Math.round(h * MM_PER_POINT)} mm`;
}

export type AttachRequest = {
  path: string;
  page: number;
  at: Point;
  scale: number;
  mode: AttachMode;
};

type Props = {
  units: Units;
  /** The point picked in the drawing for the page's bottom-left corner; null keeps the fields. */
  picked: Point | null;
  /** True while the window waits for that click: the dialog steps aside. */
  picking: boolean;
  /** Percent of the page drawn while attaching an underlay; null when not drawing. */
  progress: number | null;
  /** Asks for a PDF file; null when none was chosen. */
  chooseFile(): Promise<string | null>;
  pageCount(path: string): Promise<number>;
  pageSize(path: string, page: number): Promise<[number, number]>;
  /** Pages `first` to `first + count - 1`, small. */
  thumbnails(path: string, first: number, count: number): Promise<(Picture & { page: number })[]>;
  /** Starts the one click in the drawing that sets the insert point. */
  onPickPoint(): void;
  /** Attaches the page; a rejection is shown in the dialog, which stays open. */
  onAttach(request: AttachRequest): Promise<void>;
  onClose(): void;
};

/** A thumbnail drawn on a canvas. */
function Thumb({ picture }: { picture: Picture }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = canvas.current;
    const g = c?.getContext("2d");
    if (!c || !g) return;
    const { buffer, byteOffset, length } = picture.data;
    const data = new Uint8ClampedArray(buffer as ArrayBuffer, byteOffset, length);
    g.putImageData(new ImageData(data, picture.width, picture.height), 0, 0);
  }, [picture]);
  return <canvas ref={canvas} width={picture.width} height={picture.height} aria-hidden="true" />;
}

/** Attach PDF: the file, its page (thumbnails, 20 at a time), the scale (1:50 to 1:500 for the
 *  drawing's units, or drawing units per point), the insert point (typed, or clicked in the
 *  drawing), and underlay or import. */
export function AttachPdfDialog(props: Props) {
  const { units, picked, picking, progress } = props;
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  /** Set while the dialog closes to let the user click in the drawing, so that close is not the
   *  user closing it. */
  const stepping = useRef(false);
  const [path, setPath] = useState<string | null>(null);
  const [count, setCount] = useState(0);
  const [page, setPage] = useState(1);
  const [first, setFirst] = useState(1);
  const [thumbs, setThumbs] = useState(new Map<number, Picture>());
  const [size, setSize] = useState<[number, number] | null>(null);
  const [preset, setPreset] = useState<number | "custom">(100);
  const [custom, setCustom] = useState("1");
  const [at, setAt] = useState<[string, string]>(["0", "0"]);
  const [mode, setMode] = useState<AttachMode>("underlay");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const propsRef = useRef(props);
  propsRef.current = props;

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  // Out of the way while the user clicks in the drawing; back with the point after.
  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (picking && d.open) {
      stepping.current = true;
      d.close();
    } else if (!picking && !d.open) {
      d.showModal();
    }
  }, [picking]);

  useEffect(() => {
    if (picked) setAt([String(picked[0]), String(picked[1])]);
  }, [picked]);

  // The thumbnails of the pages in view, fetched once each (kept when the user pages on before
  // they arrive, unless another file was chosen meanwhile).
  const pathRef = useRef(path);
  pathRef.current = path;
  useEffect(() => {
    if (!path || count === 0 || first > count || thumbs.has(first)) return;
    propsRef.current
      .thumbnails(path, first, PAGES_AT_ONCE)
      .then((got) => {
        if (pathRef.current !== path) return;
        setThumbs((held) => {
          const next = new Map(held);
          for (const p of got) next.set(p.page, p);
          return next;
        });
      })
      .catch((e) => pathRef.current === path && setError(uiError(e)));
  }, [path, count, first]);

  useEffect(() => {
    if (!path || count === 0) return;
    let gone = false;
    propsRef.current
      .pageSize(path, page)
      .then((s) => !gone && setSize(s))
      .catch((e) => !gone && setError(uiError(e)));
    return () => {
      gone = true;
    };
  }, [path, count, page]);

  async function choose() {
    setError(null);
    try {
      const chosen = await props.chooseFile();
      if (!chosen) return;
      const pages = await props.pageCount(chosen);
      setPath(chosen);
      setCount(pages);
      setPage(1);
      setFirst(1);
      setThumbs(new Map());
      setSize(null);
    } catch (e) {
      setError(uiError(e));
    }
  }

  const scale = preset === "custom" ? Number(custom) : scaleFor(preset, units);
  const point: Point = [Number(at[0]), Number(at[1])];
  const valid =
    path !== null &&
    page >= 1 &&
    page <= count &&
    scale > 0 &&
    Number.isFinite(scale) &&
    point.every(Number.isFinite);

  async function attach() {
    if (!valid || !path) return;
    setBusy(true);
    setError(null);
    try {
      await props.onAttach({ path, page, at: point, scale, mode });
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const last = Math.min(count, first + PAGES_AT_ONCE - 1);
  const shown = Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i);

  return (
    <dialog
      ref={dialog}
      className="settings attach-pdf"
      aria-labelledby="attach-pdf-title"
      onClose={() => {
        if (stepping.current) {
          stepping.current = false;
          return;
        }
        props.onClose();
      }}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          attach();
        }}
      >
        <h2 id="attach-pdf-title" className="heading">
          {t("attachPdf.title")}
        </h2>
        <div className="field">
          <span>{t("attachPdf.file")}</span>
          <div className="row">
            <input readOnly dir="ltr" value={path ?? t("attachPdf.noFile")} title={path ?? undefined} />
            <button type="button" disabled={busy} onClick={choose}>
              {t("attachPdf.choose")}
            </button>
          </div>
        </div>
        {count > 0 && (
          <fieldset>
            <legend className="heading">{t("attachPdf.page")}</legend>
            <div className="row pages-head">
              <button
                type="button"
                aria-label={t("attachPdf.prevPages")}
                disabled={first <= 1}
                onClick={() => setFirst((f) => Math.max(1, f - PAGES_AT_ONCE))}
              >
                ‹
              </button>
              <span>{t("attachPdf.pages", { from: first, to: last, n: count })}</span>
              <button
                type="button"
                aria-label={t("attachPdf.nextPages")}
                disabled={last >= count}
                onClick={() => setFirst((f) => f + PAGES_AT_ONCE)}
              >
                ›
              </button>
              <label className="page-number">
                <span>{t("attachPdf.page")}</span>
                <input
                  type="number"
                  min={1}
                  max={count}
                  value={page}
                  onChange={(e) => {
                    const n = Math.round(Number(e.target.value));
                    setPage(n);
                    if (n >= 1 && n <= count) setFirst(Math.floor((n - 1) / PAGES_AT_ONCE) * PAGES_AT_ONCE + 1);
                  }}
                />
              </label>
            </div>
            <div className="thumbs" role="listbox" aria-label={t("attachPdf.page")}>
              {shown.map((n) => {
                const thumb = thumbs.get(n);
                return (
                  <button
                    key={n}
                    type="button"
                    role="option"
                    aria-selected={n === page}
                    aria-label={t("attachPdf.pageN", { n })}
                    onClick={() => setPage(n)}
                  >
                    {thumb ? <Thumb picture={thumb} /> : <span className="thumb-wait" />}
                    <span>{n}</span>
                  </button>
                );
              })}
            </div>
          </fieldset>
        )}
        <label>
          <span>
            {t("attachPdf.scale")}
            {size && ` · ${t("attachPdf.sheet", { size: sheetName(size[0], size[1]) })}`}
          </span>
          <select
            value={String(preset)}
            onChange={(e) =>
              setPreset(e.target.value === "custom" ? "custom" : Number(e.target.value))
            }
          >
            {SCALES.map((n) => (
              <option key={n} value={n}>
                1:{n}
              </option>
            ))}
            <option value="custom">{t("attachPdf.custom")}</option>
          </select>
        </label>
        {preset === "custom" && (
          <label>
            <span>{t("attachPdf.unitsPerPoint")}</span>
            <input
              type="number"
              min={0}
              step="any"
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
            />
          </label>
        )}
        <div className="field">
          <span>{t("attachPdf.insertAt")}</span>
          <div className="row" dir="ltr">
            <input
              type="number"
              step="any"
              aria-label="X"
              value={at[0]}
              onChange={(e) => setAt([e.target.value, at[1]])}
            />
            <input
              type="number"
              step="any"
              aria-label="Y"
              value={at[1]}
              onChange={(e) => setAt([at[0], e.target.value])}
            />
            <button type="button" disabled={busy} onClick={props.onPickPoint}>
              {t("attachPdf.pickPoint")}
            </button>
          </div>
        </div>
        <fieldset>
          <legend className="heading">{t("attachPdf.mode")}</legend>
          <SegmentedChoice<AttachMode>
            label={t("attachPdf.mode")}
            value={mode}
            onChange={setMode}
            segments={[
              { value: "underlay", label: t("attachPdf.underlay") },
              { value: "import", label: t("attachPdf.import") },
            ]}
          />
        </fieldset>
        {busy &&
          (progress === null ? (
            // No percentage yet (reading the PDF): the dots wait beside the words.
            <p className="status">
              <DotLoader pattern="pulse" />
              {t("attachPdf.working")}
            </p>
          ) : (
            <Progress value={progress} label={t("attachPdf.drawing")} />
          ))}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" disabled={busy} onClick={() => dialog.current?.close()}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!valid || busy}>
            {t("attachPdf.attach")}
          </button>
        </div>
      </form>
    </dialog>
  );
}

type CalibrateProps = {
  /** The two points picked, in drawing coordinates. */
  points: [Point, Point];
  units: Units;
  /** Rescales the underlay so the points are `distance` drawing units apart; a rejection is
   *  shown in the dialog. */
  onCalibrate(distance: number): Promise<void>;
  onClose(): void;
};

/** Asks how far apart the two points picked on a PDF underlay really are, in metres. */
export function CalibrateDialog({ points, units, onCalibrate, onClose }: CalibrateProps) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [real, setReal] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const perUnit = metresPerUnit(units);
  const [[x1, y1], [x2, y2]] = points;
  const measured = Math.hypot(x2 - x1, y2 - y1) * perUnit;
  const metres = Number(real);
  const valid = real.trim() !== "" && metres > 0 && Number.isFinite(metres);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function apply() {
    if (!valid) return;
    setBusy(true);
    setError(null);
    try {
      await onCalibrate(metres / perUnit);
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings"
      aria-labelledby="calibrate-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          apply();
        }}
      >
        <h2 id="calibrate-title" className="heading">
          {t("calibrate.title")}
        </h2>
        <p className="status">{t("calibrate.measured", { d: measured.toFixed(3) })}</p>
        <label>
          <span>{t("calibrate.real")}</span>
          <input
            type="number"
            min={0}
            step="any"
            autoFocus
            value={real}
            onChange={(e) => setReal(e.target.value)}
          />
        </label>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" disabled={busy} onClick={() => dialog.current?.close()}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!valid || busy}>
            {t("calibrate.apply")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
