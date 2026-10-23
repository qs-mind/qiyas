import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import { useT } from "../i18n";

/** What a text editor gives back: the text (plain, lines split by "\n"), and for a new text its
 *  style and height (drawing units). */
export type TextDraft = { value: string; style: string; height: number };

type InPlaceProps = {
  /** Where the text starts, in CSS pixels over the drawing. */
  at: [number, number];
  styles: string[];
  style: string;
  height: number;
  onSubmit(draft: TextDraft): void;
  onCancel(): void;
};

/** TEXT's editor, over the drawing where the text starts: type, then Enter places it (Esc drops
 *  it). Clicking elsewhere places it too, as AutoCAD does, when something was typed. The style and
 *  height sit beside it. */
export function InPlaceText(props: InPlaceProps) {
  const { at, styles, onSubmit, onCancel } = props;
  const t = useT();
  const [value, setValue] = useState("");
  const [style, setStyle] = useState(props.style);
  const [height, setHeight] = useState(String(props.height));
  const done = useRef(false);
  const h = Number(height);
  const valid = value.trim() !== "" && h > 0 && Number.isFinite(h);

  function submit() {
    if (done.current) return;
    done.current = true;
    if (valid) onSubmit({ value, style, height: h });
    else onCancel();
  }

  return (
    <form
      className="text-in-place"
      style={{ left: at[0], top: at[1] }}
      aria-label={t("text.text")}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") {
          done.current = true;
          onCancel();
        }
      }}
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      onBlur={(e) => {
        // Focus moving to the style or the height stays here; a click on the drawing (or
        // anywhere else in the window) places the text. Leaving the window does not.
        if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
        if (document.hasFocus()) submit();
      }}
    >
      <input
        className="text-value"
        aria-label={t("text.value")}
        dir="auto"
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <select aria-label={t("text.style")} value={style} onChange={(e) => setStyle(e.target.value)}>
        {styles.map((s) => (
          <option key={s} value={s}>
            {s}
          </option>
        ))}
      </select>
      <input
        className="text-height"
        aria-label={t("text.height")}
        type="number"
        min={0}
        step="any"
        value={height}
        onChange={(e) => setHeight(e.target.value)}
      />
    </form>
  );
}

type DialogProps = {
  /** An i18n key: "text.newMtext" or "text.editTitle". */
  heading: string;
  /** The text to start from (plain, lines split by "\n"). */
  value: string;
  /** A textarea (MText) rather than one line. */
  multiline: boolean;
  /** For a new text: the styles to choose from, the one chosen, and the height. */
  styles?: string[];
  style?: string;
  height?: number;
  /** The text (and for a new one its style and height). */
  onSubmit(draft: TextDraft): Promise<void>;
  onClose(): void;
};

/** The MText dialog (a new MText's text, style and height) and the text edit dialog (a text or
 *  an MText; a block's attributes have `AttributesDialog`). Ctrl+Enter submits a textarea; an error from
 *  the drawing stays in the dialog. */
export function TextDialog(props: DialogProps) {
  const { heading, multiline, styles, onSubmit, onClose } = props;
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [value, setValue] = useState(props.value);
  const [style, setStyle] = useState(props.style ?? "");
  const [height, setHeight] = useState(String(props.height ?? 1));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const h = Number(height);
  const valid = value.trim() !== "" && (props.height === undefined || (h > 0 && Number.isFinite(h)));

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function submit() {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit({ value, style, height: h });
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
      className="settings text-dialog"
      aria-labelledby="text-dialog-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <h2 id="text-dialog-title" className="heading">
          {t(heading)}
        </h2>
        <label>
          <span>{t("text.value")}</span>
          {multiline ? (
            <textarea
              dir="auto"
              rows={6}
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                  e.preventDefault();
                  submit();
                }
              }}
            />
          ) : (
            <input dir="auto" autoFocus value={value} onChange={(e) => setValue(e.target.value)} />
          )}
        </label>
        {styles && (
          <label>
            <span>{t("text.style")}</span>
            <select value={style} onChange={(e) => setStyle(e.target.value)}>
              {styles.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
          </label>
        )}
        {props.height !== undefined && (
          <label>
            <span>{t("text.height")}</span>
            <input type="number" min={0} step="any" value={height} onChange={(e) => setHeight(e.target.value)} />
          </label>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("text.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!valid || busy}>
            {t(styles ? "text.place" : "text.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
