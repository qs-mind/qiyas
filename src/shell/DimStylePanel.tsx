import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { DimStyleFields, DimStyleInfo } from "../api";
import { useT } from "../i18n";

/** The common annotation scales offered; any other "a:b" can be typed. */
export const SCALES = ["1:1", "1:5", "1:10", "1:20", "1:25", "1:50", "1:100", "1:200", "1:500", "1:1000"];

type Props = {
  styles: DimStyleInfo[];
  /** The style new dimensions take. */
  current: string;
  /** The annotation scale (CANNOSCALE). */
  annotationScale: string;
  /** Writes `fields` to the style `name` (made from the current one when new); `current` makes
   *  it the one new dimensions take. One undo step. */
  onSave(name: string, fields: DimStyleFields, current: boolean): Promise<void>;
  /** Sets the annotation scale. */
  onScale(name: string): Promise<void>;
  onClose(): void;
};

/** The number fields. */
type NumberKey = Exclude<keyof DimStyleFields, "show_units">;

/** The number fields as typed, by key. */
type Typed = Record<NumberKey, string>;

const KEYS: NumberKey[] = [
  "arrow_size",
  "text_height",
  "precision",
  "ext_offset",
  "ext_extension",
  "gap",
  "scale",
];

const typed = (f: DimStyleFields): Typed =>
  Object.fromEntries(KEYS.map((k) => [k, String(f[k])])) as Typed;

/** The typed fields as numbers (with the units switch), or null when one is not a valid value. */
function read(f: Typed, showUnits: boolean): DimStyleFields | null {
  const out = {
    ...(Object.fromEntries(KEYS.map((k) => [k, Number(f[k])])) as Record<NumberKey, number>),
    show_units: showUnits,
  };
  const ok =
    KEYS.every((k) => f[k].trim() !== "" && Number.isFinite(out[k]) && out[k] >= 0) &&
    out.text_height > 0 &&
    out.scale > 0 &&
    Number.isInteger(out.precision) &&
    out.precision <= 8;
  return ok ? out : null;
}

/** DIMSTYLE: the dimension styles, the current one first chosen. Edit the common fields of the
 *  chosen style and save them, make it current, or make a new style from the current one; and
 *  the annotation scale new annotations are sized by. */
export function DimStylePanel({ styles, current, annotationScale, onSave, onScale, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(current);
  const chosen = styles.find((s) => s.name === name) ?? styles.find((s) => s.name === current) ?? null;
  const [fields, setFields] = useState<Typed | null>(chosen ? typed(chosen.fields) : null);
  const [showUnits, setShowUnits] = useState(chosen?.fields.show_units ?? true);
  const [newName, setNewName] = useState("");
  const [scale, setScale] = useState(annotationScale);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const values = fields ? read(fields, showUnits) : null;

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  function choose(next: string) {
    setName(next);
    const style = styles.find((s) => s.name === next);
    if (style) {
      setFields(typed(style.fields));
      setShowUnits(style.fields.show_units);
    }
  }

  async function act(work: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const fromCurrent = styles.find((s) => s.name === current)?.fields ?? values;

  return (
    <dialog
      ref={dialog}
      className="settings dim-style"
      aria-labelledby="dim-style-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          if (values) act(() => onSave(name, values, false));
        }}
      >
        <h2 id="dim-style-title" className="heading">
          {t("dim.style.title")}
        </h2>
        <label>
          <span>{t("dim.style.name")}</span>
          <select value={name} onChange={(e) => choose(e.target.value)}>
            {styles.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name === current ? t("dim.style.isCurrent", { name: s.name }) : s.name}
              </option>
            ))}
          </select>
        </label>
        {fields &&
          KEYS.map((k) => (
            <label key={k}>
              <span>{t(`dim.style.${k}`)}</span>
              <input
                type="number"
                min={0}
                step={k === "precision" ? 1 : "any"}
                value={fields[k]}
                onChange={(e) => setFields({ ...fields, [k]: e.target.value })}
              />
            </label>
          ))}
        {fields && (
          <label className="check">
            <input type="checkbox" checked={showUnits} onChange={(e) => setShowUnits(e.target.checked)} />
            <span>{t("dim.style.show_units")}</span>
          </label>
        )}
        <div className="buttons">
          <button
            type="button"
            disabled={!values || busy || name === current}
            onClick={() => values && act(() => onSave(name, values, true))}
          >
            {t("dim.style.makeCurrent")}
          </button>
          <button type="submit" className="primary" disabled={!values || busy}>
            {t("dim.style.save")}
          </button>
        </div>
        <label>
          <span>{t("dim.style.newName")}</span>
          <input dir="auto" value={newName} onChange={(e) => setNewName(e.target.value)} />
        </label>
        <div className="buttons">
          <button
            type="button"
            disabled={newName.trim() === "" || !fromCurrent || busy}
            onClick={() =>
              fromCurrent &&
              act(async () => {
                const created = newName.trim();
                await onSave(created, fromCurrent, false);
                setNewName("");
                setName(created);
                setFields(typed(fromCurrent));
                setShowUnits(fromCurrent.show_units);
              })
            }
          >
            {t("dim.style.create")}
          </button>
        </div>
        <label>
          <span>{t("dim.style.annotationScale")}</span>
          <input
            list="dim-style-scales"
            value={scale}
            onChange={(e) => setScale(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                act(() => onScale(scale.trim()));
              }
            }}
          />
          <datalist id="dim-style-scales">
            {SCALES.map((s) => (
              <option key={s} value={s} />
            ))}
          </datalist>
        </label>
        <div className="buttons">
          <button
            type="button"
            disabled={busy || scale.trim() === "" || scale.trim() === annotationScale}
            onClick={() => act(() => onScale(scale.trim()))}
          >
            {t("dim.style.setScale")}
          </button>
        </div>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("dim.style.close")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
