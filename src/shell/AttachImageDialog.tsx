import { useEffect, useRef, useState } from "react";
import type { ImageDisplay, Units } from "../api";
import { useT } from "../i18n";
import type { WorldBounds } from "../viewport/batch";
import type { Point } from "../viewport/camera";

/** The width a new image gets: a tenth of the sheet's extents, or 10 m on an empty sheet (in
 *  drawing units, `metresPerUnit` metres each). */
export function defaultImageWidth(bounds: WorldBounds | null, metresPerUnit = 1): number {
  const width = bounds ? bounds[1][0] - bounds[0][0] : 0;
  return width > 0 && Number.isFinite(width) ? width / 10 : 10 / metresPerUnit;
}

export type AttachImageRequest = { path: string; at: Point; width: number };

type Props = {
  units: Units;
  /** The width offered first, in drawing units (`defaultImageWidth`). */
  defaultWidth: number;
  /** The point picked in the drawing for the image's bottom-left corner; null keeps the fields. */
  picked: Point | null;
  /** True while the window waits for that click: the dialog steps aside. */
  picking: boolean;
  /** Asks for an image file; null when none was chosen. */
  chooseFile(): Promise<string | null>;
  /** The image's width and height in pixels. */
  imageSize(path: string): Promise<[number, number]>;
  /** Starts the one click in the drawing that sets the insert point. */
  onPickPoint(): void;
  /** Attaches the image; a rejection is shown in the dialog, which stays open. */
  onAttach(request: AttachImageRequest): Promise<void>;
  onClose(): void;
};

/** Attach image: the file (its size in pixels shown), its width in drawing units, and the insert
 *  point (typed, or clicked in the drawing). Calibrate scales it afterwards. */
export function AttachImageDialog(props: Props) {
  const { units, picked, picking } = props;
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  /** Set while the dialog closes to let the user click in the drawing. */
  const stepping = useRef(false);
  const [path, setPath] = useState<string | null>(null);
  const [pixels, setPixels] = useState<[number, number] | null>(null);
  const [width, setWidth] = useState(String(Number(props.defaultWidth.toPrecision(6))));
  const [at, setAt] = useState<[string, string]>(["0", "0"]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  async function choose() {
    setError(null);
    try {
      const chosen = await props.chooseFile();
      if (!chosen) return;
      const size = await props.imageSize(chosen);
      setPath(chosen);
      setPixels(size);
    } catch (e) {
      setError(String(e));
    }
  }

  const w = Number(width);
  const point: Point = [Number(at[0]), Number(at[1])];
  const valid = path !== null && w > 0 && Number.isFinite(w) && point.every(Number.isFinite);
  const unit = t(`unit.${units === "unknown" ? "millimetres" : units}`);

  async function attach() {
    if (!valid || !path) return;
    setBusy(true);
    setError(null);
    try {
      await props.onAttach({ path, at: point, width: w });
      dialog.current?.close();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings"
      aria-labelledby="attach-image-title"
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
        <h2 id="attach-image-title" className="heading">
          {t("attachImage.title")}
        </h2>
        <div className="field">
          <span>{t("attachImage.file")}</span>
          <div className="row">
            <input readOnly dir="ltr" value={path ?? t("attachPdf.noFile")} title={path ?? undefined} />
            <button type="button" disabled={busy} onClick={choose}>
              {t("attachPdf.choose")}
            </button>
          </div>
          {pixels && (
            <span className="status">{t("attachImage.pixels", { w: pixels[0], h: pixels[1] })}</span>
          )}
        </div>
        <label>
          <span>{t("attachImage.width", { unit })}</span>
          <input
            type="number"
            min={0}
            step="any"
            value={width}
            onChange={(e) => setWidth(e.target.value)}
          />
        </label>
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
        <p className="status">{t("attachImage.calibrateHint")}</p>
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

/** Neutral: the image as it is. */
export const NEUTRAL_DISPLAY: ImageDisplay = { brightness: 50, contrast: 50, fade: 0 };

type DisplayProps = {
  /** The image's settings now. */
  value: ImageDisplay;
  /** Sets them (one undo step each time); a rejection is shown in the panel. */
  onCommit(next: ImageDisplay): Promise<void>;
  onClose(): void;
};

const SLIDERS: (keyof ImageDisplay)[] = ["brightness", "contrast", "fade"];

/** The selected image's brightness, contrast and fade, 0 to 100 each. A slider is set when it is
 *  let go (pointer up, key up, or leaving it), so dragging makes one undo step. */
export function ImageDisplayPanel({ value, onCommit, onClose }: DisplayProps) {
  const t = useT();
  const [draft, setDraft] = useState(value);
  const committed = useRef(value);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    committed.current = value;
    setDraft(value);
  }, [value.brightness, value.contrast, value.fade]);

  async function commit(next: ImageDisplay) {
    const was = committed.current;
    if (SLIDERS.every((k) => next[k] === was[k])) return;
    committed.current = next;
    setError(null);
    try {
      await onCommit(next);
    } catch (e) {
      committed.current = was;
      setError(String(e));
    }
  }

  return (
    <section
      className="image-display"
      role="dialog"
      aria-labelledby="image-display-title"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onClose();
      }}
    >
      <h2 id="image-display-title" className="heading">
        {t("imageDisplay.title")}
      </h2>
      {SLIDERS.map((key) => (
        <label key={key}>
          <span>
            {t(`imageDisplay.${key}`)} <span className="value">{draft[key]}</span>
          </span>
          <input
            type="range"
            min={0}
            max={100}
            step={1}
            value={draft[key]}
            onChange={(e) => setDraft({ ...draft, [key]: Number(e.target.value) })}
            onPointerUp={() => commit(draft)}
            onKeyUp={() => commit(draft)}
            onBlur={() => commit(draft)}
          />
        </label>
      ))}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="buttons">
        <button
          type="button"
          onClick={() => {
            setDraft(NEUTRAL_DISPLAY);
            commit(NEUTRAL_DISPLAY);
          }}
        >
          {t("imageDisplay.reset")}
        </button>
        <button type="button" onClick={onClose}>
          {t("imageDisplay.close")}
        </button>
      </div>
    </section>
  );
}
