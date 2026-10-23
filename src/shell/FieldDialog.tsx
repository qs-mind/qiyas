import { entityKind } from "../i18n/entityKind";
import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { FieldKind } from "../api";
import { useT } from "../i18n";

type Kind = FieldKind["kind"];
const KINDS: Kind[] = ["date", "file_name", "area", "length", "count"];

/** A field's code as the core writes it (`qiyas_core::field_code`): AutoCAD's field codes, the
 *  entity's handle in hex. Typed into a cell, it makes the cell a field. */
export function fieldCode(field: FieldKind): string {
  switch (field.kind) {
    case "date":
      return '%<\\AcVar Date \\f "yyyy-MM-dd">%';
    case "file_name":
      return "%<\\AcVar Filename>%";
    case "area":
    case "length": {
      const property = field.kind === "area" ? "Area" : "Length";
      const id = field.target.handle.toString(16).toUpperCase();
      return `%<\\AcObjProp Object(%<\\_ObjId ${id}>%).${property} \\f "%lu2%pr3">%`;
    }
    case "count":
      return `%<\\AcVar QiyasCount "${field.block.replace(/"/g, "")}">%`;
  }
}

type Props = {
  /** Into a new text where the user clicked, or into the table cell being edited. */
  mode: "text" | "cell";
  /** The entity an area or a length reads, once picked. */
  target: { handle: number; kind: string } | null;
  /** Out of the way while the user picks in the drawing. */
  picking: boolean;
  /** Asks the user to pick the entity in the drawing. */
  onPick(): void;
  /** Inserts the field; an error stays in the dialog. */
  onSubmit(field: FieldKind): Promise<void>;
  onClose(): void;
};

/** FIELD: today's date, the file name, an entity's area or length (picked in the drawing; it
 *  follows the entity's edits), or how many references of a block the sheet holds. */
export function FieldDialog(props: Props) {
  const { target, picking } = props;
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const stepping = useRef(false);
  const [kind, setKind] = useState<Kind>("date");
  const [block, setBlock] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

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

  const reads = kind === "area" || kind === "length";
  const field: FieldKind | null =
    kind === "date" || kind === "file_name"
      ? { kind }
      : kind === "count"
        ? block.trim()
          ? { kind, block: block.trim() }
          : null
        : target
          ? { kind, target: { handle: target.handle, chain: [] } }
          : null;

  async function submit() {
    if (!field || busy) return;
    setBusy(true);
    setError(null);
    try {
      await props.onSubmit(field);
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
      aria-labelledby="field-title"
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
          submit();
        }}
      >
        <h2 id="field-title" className="heading">
          {t(props.mode === "cell" ? "insertField.cellTitle" : "insertField.title")}
        </h2>
        <label>
          <span>{t("insertField.kind")}</span>
          <select value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
            {KINDS.map((k) => (
              <option key={k} value={k}>
                {t(`insertField.${k}`)}
              </option>
            ))}
          </select>
        </label>
        {reads && (
          <div className="field-target">
            <span>
              {target
                ? t("insertField.object", { kind: entityKind(target.kind, t) })
                : t("insertField.noObject")}
            </span>
            <button type="button" onClick={props.onPick}>
              {t("insertField.pick")}
            </button>
          </div>
        )}
        {kind === "count" && (
          <label>
            <span>{t("insertField.block")}</span>
            <input dir="auto" value={block} onChange={(e) => setBlock(e.target.value)} />
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
          <button type="submit" className="primary" disabled={!field || busy}>
            {t("table.insert")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
