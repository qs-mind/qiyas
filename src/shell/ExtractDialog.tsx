import { entityKind } from "../i18n/entityKind";
import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { EntityFilter, Field } from "../api";
import { useT } from "../i18n";

/** The fields offered as checkboxes, in column order. */
export const FIELDS = [
  "handle",
  "kind",
  "layer",
  "block_name",
  "text",
  "length",
  "area",
  "x",
  "y",
  "sheet",
  "xref",
] as const;
type Plain = (typeof FIELDS)[number];

const DEFAULT_FIELDS: Plain[] = ["kind", "layer", "block_name", "text", "length", "area"];

/** Entity types offered in the type list; empty is any. */
export const KINDS = [
  "Line",
  "LwPolyline",
  "Polyline",
  "Arc",
  "Circle",
  "Ellipse",
  "Spline",
  "Hatch",
  "Insert",
  "Text",
  "MText",
  "Point",
  "Dimension",
];

/** Most rows one filter gives (the core's limit). */
const LIMIT = 5000;

/** `columns` and `rows` as tab-separated text, one line each, for pasting into a spreadsheet. Tabs
 *  and line breaks inside a cell become spaces. */
export function toTsv(columns: string[], rows: string[][]): string {
  const line = (cells: string[]) => cells.map((c) => c.replace(/[\t\r\n]+/g, " ")).join("\t");
  return [columns, ...rows].map(line).join("\n");
}

type Props = {
  /** The drawing's layer names. */
  layers: string[];
  /** Runs the extraction on the sheet shown: one filter per layer chosen (one without a layer
   *  when none is). Resolves to the rows of all of them. */
  onRun(filters: EntityFilter[], fields: Field[], progress?: (rows: number) => void): Promise<string[][]>;
  /** Writes the table to Excel (asks where first). */
  onExport(columns: string[], rows: string[][]): Promise<void>;
  onClose(): void;
};

/** Extract data: which entities (layers, type, block, text) and which fields, then a table of them
 *  to copy as tab-separated text or export to Excel. */
export function ExtractDialog({ layers, onRun, onExport, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [chosenLayers, setChosenLayers] = useState<string[]>([]);
  const [kind, setKind] = useState("");
  const [block, setBlock] = useState("");
  const [text, setText] = useState("");
  const [fields, setFields] = useState<Set<Plain>>(new Set(DEFAULT_FIELDS));
  const [tags, setTags] = useState("");
  const [table, setTable] = useState<{ columns: string[]; rows: string[][] } | null>(null);
  const [progress, setProgress] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  const attributeTags = tags
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s !== "");
  const plain = FIELDS.filter((f) => fields.has(f));
  const valid = plain.length + attributeTags.length > 0;

  function toggle(f: Plain) {
    const next = new Set(fields);
    if (next.has(f)) next.delete(f);
    else next.add(f);
    setFields(next);
  }

  async function run() {
    if (!valid) return;
    const base: EntityFilter = {
      kind: kind || null,
      block_name: block.trim() || null,
      text_contains: text.trim() || null,
      limit: LIMIT,
    };
    const filters =
      chosenLayers.length > 0 ? chosenLayers.map((layer) => ({ ...base, layer })) : [base];
    const chosen: Field[] = [...plain, ...attributeTags.map((attribute) => ({ attribute }))];
    const columns = [...plain.map((f) => t(`field.${f}`)), ...attributeTags];
    setBusy(true);
    setTable(null);
    setProgress(0);
    setError(null);
    setCopied(false);
    try {
      setTable({ columns, rows: await onRun(filters, chosen, setProgress) });
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  async function copy() {
    if (!table) return;
    try {
      await navigator.clipboard.writeText(toTsv(table.columns, table.rows));
      setCopied(true);
    } catch (e) {
      setError(uiError(e));
    }
  }

  async function exportTable() {
    if (!table) return;
    setError(null);
    try {
      await onExport(table.columns, table.rows);
    } catch (e) {
      setError(uiError(e));
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings extract"
      aria-labelledby="extract-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          run();
        }}
      >
        <h2 id="extract-title" className="heading">
          {t("extract.title")}
        </h2>
        {busy && <p role="status">{t("extract.progress", { count: progress })}</p>}
        <div className="extract-filters">
          <label>
            <span>{t("extract.layers")}</span>
            <select
              multiple
              size={Math.min(6, Math.max(2, layers.length))}
              value={chosenLayers}
              onChange={(e) =>
                setChosenLayers(Array.from(e.currentTarget.selectedOptions, (o) => o.value))
              }
            >
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <div>
            <label>
              <span>{t("extract.kind")}</span>
              <select value={kind} onChange={(e) => setKind(e.target.value)}>
                <option value="">{t("extract.anyKind")}</option>
                {KINDS.map((k) => (
                  <option key={k} value={k}>
                    {entityKind(k, t)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("extract.block")}</span>
              <input value={block} onChange={(e) => setBlock(e.target.value)} />
            </label>
            <label>
              <span>{t("extract.textContains")}</span>
              <input value={text} onChange={(e) => setText(e.target.value)} />
            </label>
          </div>
        </div>
        <fieldset>
          <legend>{t("extract.fields")}</legend>
          <div className="extract-fields">
            {FIELDS.map((f) => (
              <label key={f} className="check">
                <input type="checkbox" checked={fields.has(f)} onChange={() => toggle(f)} />
                <span>{t(`field.${f}`)}</span>
              </label>
            ))}
          </div>
          <label>
            <span>{t("extract.attributes")}</span>
            <input value={tags} placeholder="MARK" onChange={(e) => setTags(e.target.value)} />
          </label>
        </fieldset>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        {table && (
          <>
            <p className="status" role="status">
              {table.rows.length === 0 ? t("extract.none") : t("extract.rows", { n: table.rows.length })}
              {copied && ` · ${t("extract.copied")}`}
            </p>
            {table.rows.length > 0 && (
              <div className="extract-table">
                <table>
                  <thead>
                    <tr>
                      {table.columns.map((c, i) => (
                        <th key={i} scope="col">
                          {c}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {table.rows.map((r, i) => (
                      <tr key={i}>
                        {r.map((c, j) => (
                          <td key={j} dir="auto">
                            {c}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </>
        )}
        <div className="buttons">
          <button type="button" disabled={!table || table.rows.length === 0} onClick={copy}>
            {t("extract.copy")}
          </button>
          <button type="button" disabled={!table || table.rows.length === 0} onClick={exportTable}>
            {t("extract.export")}
          </button>
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("extract.close")}
          </button>
          <button type="submit" className="primary" disabled={!valid || busy}>
            {t("extract.run")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
