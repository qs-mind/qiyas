import { uiError } from "../i18n/errors";
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import type { Command, TableInfo } from "../api";
import { useT } from "../i18n";

type Cell = [number, number];

const key = ([r, c]: Cell) => `${r},${c}`;

/** For each cell, the merged cell it is part of: its first cell and how far it reaches. */
function mergedCells(table: TableInfo) {
  const owner = new Map<string, Cell>();
  const span = new Map<string, [number, number]>();
  for (const [top, left, bottom, right] of table.merges) {
    span.set(key([top, left]), [bottom - top + 1, right - left + 1]);
    for (let r = top; r <= bottom; r++) {
      for (let c = left; c <= right; c++) owner.set(key([r, c]), [top, left]);
    }
  }
  return {
    owner: (cell: Cell): Cell => owner.get(key(cell)) ?? cell,
    span: (cell: Cell): [number, number] => span.get(key(cell)) ?? [1, 1],
  };
}

type EditorProps = {
  /** The table's handle (at the top of the sheet). */
  handle: number;
  table: TableInfo;
  /** The cell chosen first (the one double-clicked). */
  cell: Cell;
  /** Runs one table command; the app then hands the table back as it is now. */
  onRun(cmd: Command): Promise<void>;
  /** FIELD into the chosen cell. */
  onField(cell: Cell): void;
  /** Out of the way while the user picks in the drawing. */
  picking?: boolean;
  onClose(): void;
};

/** TABLEDIT: the table's cells as a grid in the drawing's own column order (left to right, in
 *  Arabic too: the grid mirrors the drawing; each cell's text runs its own way). A cell is written
 *  when Enter is pressed or it loses focus; rows and columns go in and out beside the chosen cell;
 *  Shift+click chooses cells to merge. Every change is one undo step. */
export function TableEditor(props: EditorProps) {
  const { table, picking } = props;
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const stepping = useRef(false);
  const extending = useRef(false);
  const pending = useRef<Promise<boolean> | null>(null);
  const merged = useMemo(() => mergedCells(table), [table]);
  const [anchor, setAnchor] = useState<Cell>(props.cell);
  const [end, setEnd] = useState<Cell>(props.cell);
  /** Cells typed into and not written yet, by "row,col". */
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  /** What was last written to each cell, so a blur after Enter writes nothing twice. */
  const sent = useRef<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const rows = table.cells.length;
  const cols = table.widths.length;

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

  // The table as it is now, after the writes sent before it: a draft that was sent goes (the cell
  // may show something else than was typed, such as a field's value); text typed meanwhile into
  // another cell is kept. When rows or columns came or went, cells moved: every draft goes.
  const shape = `${rows}x${cols}`;
  const lastShape = useRef(shape);
  useEffect(() => {
    const reshaped = lastShape.current !== shape;
    lastShape.current = shape;
    const kept: Record<string, string> = {};
    if (!reshaped) {
      for (const [k, v] of Object.entries(drafts)) {
        if (sent.current[k] !== v) kept[k] = v;
      }
    }
    setDrafts(kept);
    for (const k of Object.keys(sent.current)) {
      if (reshaped || !(k in kept)) delete sent.current[k];
    }
    const inside = (c: Cell) => [Math.min(c[0], rows - 1), Math.min(c[1], cols - 1)] as Cell;
    setAnchor((a) => inside(a));
    setEnd((e) => inside(e));
  }, [table]);

  const target = { handle: props.handle, chain: [] };
  const chosen = merged.owner(anchor);
  const [rowSpan, colSpan] = merged.span(chosen);
  const range = {
    first: [Math.min(anchor[0], end[0]), Math.min(anchor[1], end[1])] as Cell,
    last: [Math.max(anchor[0], end[0]), Math.max(anchor[1], end[1])] as Cell,
  };
  const manyCells = range.first[0] !== range.last[0] || range.first[1] !== range.last[1];
  const isMerged = rowSpan > 1 || colSpan > 1;
  const isField = (cell: Cell) => table.fields.some(([r, c]) => r === cell[0] && c === cell[1]);

  function run(cmd: Command): Promise<boolean> {
    const execute = async () => {
      setError(null);
      try {
        await props.onRun(cmd);
        return true;
      } catch (e) {
        setError(uiError(e));
        return false;
      }
    };
    // A blur saves the cell before the following merge/row/column click. Wait for that save;
    // if it failed, keep the draft and the error instead of reshaping the table underneath it.
    const job = pending.current
      ? pending.current.then((ok) => !ok && cmd.type !== "edit_cell" ? false : execute())
      : execute();
    pending.current = job;
    void job.then(() => { if (pending.current === job) pending.current = null; });
    return job;
  }

  function commit(cell: Cell) {
    const k = key(cell);
    const value = drafts[k];
    if (value === undefined || value === table.cells[cell[0]][cell[1]] || sent.current[k] === value) return;
    sent.current[k] = value;
    void run({ type: "edit_cell", target, row: cell[0], col: cell[1], text: value }).then((ok) => {
      if (!ok && sent.current[k] === value) delete sent.current[k];
    });
  }

  function choose(cell: Cell, e?: MouseEvent) {
    if (e?.shiftKey) {
      e.preventDefault();
      setEnd(cell);
      return;
    }
    setAnchor(cell);
    setEnd(cell);
  }

  return (
    <dialog
      ref={dialog}
      className="settings table-editor"
      aria-labelledby="table-editor-title"
      onClose={() => {
        if (stepping.current) {
          stepping.current = false;
          return;
        }
        props.onClose();
      }}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <h2 id="table-editor-title" className="heading">
        {t("table.editTitle")}
      </h2>
      <p>{t("table.rangeHint")}</p>
      <div className="table-grid">
        <table role="grid" dir="ltr">
          <tbody>
            {table.cells.map((line, r) => (
              <tr key={r}>
                {line.map((text, c) => {
                  const cell: Cell = [r, c];
                  const owner = merged.owner(cell);
                  if (owner[0] !== r || owner[1] !== c) return null;
                  const [down, across] = merged.span(cell);
                  const inRange =
                    r >= range.first[0] && r <= range.last[0] && c >= range.first[1] && c <= range.last[1];
                  return (
                    <td
                      key={c}
                      rowSpan={down > 1 ? down : undefined}
                      colSpan={across > 1 ? across : undefined}
                      aria-selected={inRange}
                    >
                      <input
                        dir="auto"
                        aria-label={t("table.cell", { row: r + 1, col: c + 1 })}
                        title={isField(cell) ? t("table.isField") : undefined}
                        className={isField(cell) ? "field" : undefined}
                        value={drafts[key(cell)] ?? text}
                        onChange={(e) => setDrafts({ ...drafts, [key(cell)]: e.target.value })}
                        onMouseDown={(e) => e.shiftKey && choose(cell, e)}
                        onClick={(e) => e.shiftKey && choose(cell, e)}
                        data-cell={key(cell)}
                        onFocus={() => { if (!extending.current) choose(cell); }}
                        onBlur={() => commit(cell)}
                        onKeyDown={(e) => {
                          if (e.shiftKey && ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) {
                            e.preventDefault();
                            const next: Cell = [
                              Math.max(0, Math.min(rows - 1, end[0] + (e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0))),
                              Math.max(0, Math.min(cols - 1, end[1] + (e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0))),
                            ];
                            setEnd(next);
                            extending.current = true;
                            dialog.current?.querySelector<HTMLInputElement>(`[data-cell="${key(merged.owner(next))}"]`)?.focus();
                            extending.current = false;
                          } else if (e.key === "Enter") {
                            e.preventDefault();
                            commit(cell);
                          }
                        }}
                      />
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="table-tools" role="group" aria-label={t("table.rowsColumns")}>
        <button type="button" onClick={() => run({ type: "insert_row", target, at: chosen[0] })}>
          {t("table.rowAbove")}
        </button>
        <button type="button" onClick={() => run({ type: "insert_row", target, at: chosen[0] + rowSpan })}>
          {t("table.rowBelow")}
        </button>
        <button type="button" onClick={() => run({ type: "insert_col", target, at: chosen[1] })}>
          {t("table.colLeft")}
        </button>
        <button type="button" onClick={() => run({ type: "insert_col", target, at: chosen[1] + colSpan })}>
          {t("table.colRight")}
        </button>
        <button
          type="button"
          disabled={rows <= 1}
          onClick={() => run({ type: "delete_row", target, row: chosen[0] })}
        >
          {t("table.deleteRow")}
        </button>
        <button
          type="button"
          disabled={cols <= 1}
          onClick={() => run({ type: "delete_col", target, col: chosen[1] })}
        >
          {t("table.deleteCol")}
        </button>
        <button
          type="button"
          disabled={!manyCells}
          onClick={() => run({ type: "merge_cells", target, first: range.first, last: range.last })}
        >
          {t("table.merge")}
        </button>
        <button
          type="button"
          disabled={!isMerged}
          onClick={() => run({ type: "merge_cells", target, first: chosen, last: chosen })}
        >
          {t("table.unmerge")}
        </button>
        <button type="button" onClick={() => props.onField(chosen)}>
          {t("table.field")}
        </button>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="buttons">
        <button type="button" className="primary" onClick={() => dialog.current?.close()}>
          {t("table.close")}
        </button>
      </div>
    </dialog>
  );
}

/** What TABLE asks for: rows, columns and their sizes in drawing units. */
export type NewTable = { rows: number; cols: number; col_width: number; row_height: number };

const fmt = (v: number) => String(Number(v.toPrecision(6)));

/** TABLE's dialog after the click: rows, columns, column width and row height (twice and ten
 *  times the text height to start with). */
export function NewTableDialog(props: {
  /** The height new text takes, in drawing units. */
  textHeight: number;
  /** Adds the table; an error stays in the dialog. */
  onSubmit(table: NewTable): Promise<void>;
  onClose(): void;
}) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [rows, setRows] = useState("3");
  const [cols, setCols] = useState("3");
  const [rowHeight, setRowHeight] = useState(fmt(props.textHeight * 2));
  const [colWidth, setColWidth] = useState(fmt(props.textHeight * 10));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  const table: NewTable = {
    rows: Math.round(Number(rows)),
    cols: Math.round(Number(cols)),
    col_width: Number(colWidth),
    row_height: Number(rowHeight),
  };
  const valid = table.rows >= 1 && table.cols >= 1 && table.col_width > 0 && table.row_height > 0;

  async function submit() {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      await props.onSubmit(table);
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const number = (label: string, value: string, set: (v: string) => void, step: string) => (
    <label>
      <span>{t(label)}</span>
      <input type="number" min="0" step={step} value={value} onChange={(e) => set(e.target.value)} />
    </label>
  );

  return (
    <dialog
      ref={dialog}
      className="settings"
      aria-labelledby="new-table-title"
      onClose={props.onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <h2 id="new-table-title" className="heading">
          {t("table.newTitle")}
        </h2>
        {number("table.rows", rows, setRows, "1")}
        {number("table.cols", cols, setCols, "1")}
        {number("table.rowHeight", rowHeight, setRowHeight, "any")}
        {number("table.colWidth", colWidth, setColWidth, "any")}
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
            {t("table.insert")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
