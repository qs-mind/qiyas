/** The data grid behind the take-off, BOQ and conflicts tables: TanStack Table keeps the columns'
 *  sorting and sizing, TanStack Virtual draws only the rows in view, and everything else is plain
 *  CSS (`theme.css`, "Data grid"). A header that stays on top while the rows scroll, columns you
 *  sort (click; a second click reverses, a third clears) and resize (drag the edge, double-click to
 *  reset, arrows on the edge from the keyboard; the sizes are remembered), numbers that read right
 *  to left of their unit with tabular figures, optional section heads that group the rows, a
 *  description-like column that takes the room left over, and the full text of every cell in its
 *  tooltip. Rows are reached with the arrow keys; Enter does what a click does. In Arabic the grid
 *  mirrors: the first column is on the right and the resize edges are on the left of a column. */
import {
  columnResizingFeature,
  columnSizingFeature,
  constructSortFn,
  createSortedRowModel,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnDef,
  type SortFn,
} from "@tanstack/react-table";
import { measureElement, observeElementRect, useVirtualizer } from "@tanstack/react-virtual";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { flushSync } from "react-dom";
import { clearTextWidths, textWidth } from "./textWidth";
import { useLanguage, useT } from "../i18n";

export type GridColumn<Row extends object> = {
  id: string;
  /** The column's title (text: it names the column for the screen reader and the resize edge). */
  header: string;
  /** Initial width until rendered content can be measured. */
  size: number;
  /** The narrowest the user can drag it to. */
  minSize?: number;
  /** Automatic text width cap; long text wraps. User widths can exceed it. */
  maxAutoSize?: number;
  /** Takes the room left over (one or two of a grid's columns: a description, a note). */
  grow?: boolean;
  /** Numbers: at the end of the cell, in tabular figures. */
  align?: "end";
  /** What the column sorts by; without it the column does not sort. */
  sortValue?(row: Row): string | number | null | undefined;
  cell(row: Row): ReactNode;
  /** The cell's tooltip: its full text. */
  title?(row: Row): string | undefined;
  /** A class on the cell: a muted value, a flagged difference. */
  cellClass?(row: Row): string | undefined;
  /** Explicitly marks prose; all cells can wrap without clipping. */
  wrap?: boolean;
  /** A click in the cell is not a click on the row (a cell that is edited in place). */
  isolate?: boolean;
  /** A double click in the cell (to edit it); it is not a click on the row either. */
  onDoubleClick?(row: Row): void;
};

export type GridProps<Row extends object> = {
  /** Names the table. */
  label: string;
  columns: GridColumn<Row>[];
  rows: Row[];
  rowKey(row: Row): string;
  /** The section a row belongs to: its rows are listed under a head with that name, the sections in
   *  the order they first appear and the rows sorted inside each. An empty name has no head. */
  section?(row: Row): string;
  /** The section heads say how many rows each holds. */
  sectionCount?: boolean;
  /** A click, or Enter on the focused row. */
  onRowClick?(row: Row): void;
  /** The row on show elsewhere (the one picked): highlighted. */
  isCurrent?(row: Row): boolean;
  /** A row that cannot be clicked (an item with no code to link). */
  isDisabled?(row: Row): boolean;
  rowClass?(row: Row): string | undefined;
  /** Buttons in a reserved final column, visible on hover, focus or the current row. */
  actions?(row: Row): ReactNode;
  /** Names the grid's remembered column widths; without it they last for this session. */
  storageKey?: string;
  /** What shows instead of the rows when there are none. */
  empty?: ReactNode;
  /** Initial row estimate and minimum; 27 px by default, then measured after wrapping. */
  rowHeight?: number;
  /** Retained for callers; all rows now measure after wrapping. */
  autoRowHeight?: boolean;
};

const ROW_HEIGHT = 27;
const KEY = "qiyas.grid.";

/** Numbers in numeric order, text in natural order ("A.2" before "A.10"), whatever the language. */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
const compare = constructSortFn({
  sort: (a: unknown, b: unknown) =>
    typeof a === "number" && typeof b === "number" ? a - b : collator.compare(String(a), String(b)),
});

// Statically, as the library asks: the features this grid turns on.
const features = tableFeatures({
  rowSortingFeature,
  columnSizingFeature,
  columnResizingFeature,
  sortedRowModel: createSortedRowModel(),
});

/** Widths dragged to earlier, by column id; nothing when none or storage is blocked. */
function storedSizes(key: string | undefined): Record<string, number> {
  if (!key) return {};
  try {
    const raw = JSON.parse(localStorage.getItem(KEY + key) ?? "{}");
    return Object.fromEntries(
      Object.entries(raw as Record<string, unknown>).filter(([, v]) => typeof v === "number" && Number.isFinite(v)),
    ) as Record<string, number>;
  } catch {
    return {};
  }
}

/** A grid in a box with no layout (jsdom, a hidden tab) measures 0 by 0: keep what it started with. */
const observeRect: typeof observeElementRect = (instance, cb) =>
  observeElementRect(instance, (rect) => {
    if (rect.width > 0 || rect.height > 0) cb(rect);
  });

type Item<Row extends object> = { kind: "section"; key: string; label: string; count: number } | { kind: "row"; key: string; row: Row };

export function DataGrid<Row extends object>(props: GridProps<Row>) {
  const { label, columns, rows, rowKey, section, sectionCount, onRowClick, isCurrent, isDisabled, rowClass, actions, storageKey, empty } = props;
  const rowHeight = props.rowHeight ?? ROW_HEIGHT;
  const t = useT();
  const { lang } = useLanguage();
  const rtl = lang === "ar";

  const [actionsWidth, setActionsWidth] = useState(actions ? 32 : 0);
  const [autoSizes, setAutoSizes] = useState<Record<string, number>>({});
  const defs = useMemo<ColumnDef<typeof features, Row>[]>(
    () =>
      columns.map((c) => ({
        id: c.id,
        accessorFn: (row: Row) => c.sortValue?.(row) ?? undefined,
        enableSorting: !!c.sortValue,
        // Numbers start with the largest, text with the first in the alphabet.
        sortDescFirst: c.align === "end",
        sortFn: compare as unknown as SortFn<typeof features, Row>,
        sortUndefined: "last" as const,
        size: autoSizes[c.id] ?? c.size,
        minSize: c.minSize ?? 40,
        maxSize: 1200,
      })),
    [columns, autoSizes],
  );
  const table = useTable({
    features,
    columns: defs,
    data: rows,
    getRowId: rowKey,
    columnResizeMode: "onChange",
    columnResizeDirection: rtl ? "rtl" : "ltr",
    initialState: { columnSizing: storedSizes(storageKey) },
  });
  const sizing = table.state.columnSizing;
  useEffect(() => {
    if (!storageKey) return;
    try {
      localStorage.setItem(KEY + storageKey, JSON.stringify(sizing));
    } catch {
      // Not remembered; the columns still change for this session.
    }
  }, [sizing, storageKey]);

  // The rows in the order shown: sorted, and gathered under their sections.
  const items = useMemo<Item<Row>[]>(() => {
    const sorted = table.getRowModel().rows.map((r) => r.original);
    if (!section) return sorted.map((row) => ({ kind: "row", key: rowKey(row), row }));
    const order: string[] = [];
    const by = new Map<string, Row[]>();
    // The sections keep the order of the data; sorting reorders the rows inside them.
    for (const row of rows) {
      const name = section(row);
      if (!by.has(name)) {
        by.set(name, []);
        order.push(name);
      }
    }
    for (const row of sorted) by.get(section(row))!.push(row);
    const out: Item<Row>[] = [];
    for (const name of order) {
      const inside = by.get(name)!;
      if (name !== "") out.push({ kind: "section", key: `§${name}`, label: name, count: inside.length });
      for (const row of inside) out.push({ kind: "row", key: rowKey(row), row });
    }
    return out;
    // Row order changes with the data or sorting, not with scroll position or column widths.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows, table.state.sorting, section, rowKey]);

  const scroller = useRef<HTMLDivElement>(null);
  // Stable keys let the virtualiser reuse its 10,000-row index while only the viewport changes.
  const itemKey = useCallback((i: number) => items[i].key, [items]);
  const virtual = useVirtualizer({
    count: items.length,
    getItemKey: itemKey,
    getScrollElement: () => scroller.current,
    // Section heads are always one line.
    estimateSize: (i) => (items[i]?.kind === "section" ? ROW_HEIGHT : rowHeight),
    overscan: 10,
    initialRect: { width: 600, height: 600 },
    observeElementRect: observeRect,
    measureElement: (element, entry, instance) => measureElement(element, entry, instance) || rowHeight,
  });

  // The columns' widths: the one that grows takes what is left (`1fr`); in a narrow panel it gives
  // way down to its `minSize` before the grid scrolls sideways.
  const headers = table.getHeaderGroups()[0]?.headers ?? [];
  const byId = new Map(columns.map((c) => [c.id, c]));
  const growing = headers.filter((h) => byId.get(h.column.id)?.grow && sizing[h.column.id] === undefined);
  const natural = headers.reduce((sum, h) => sum + h.getSize(), 0) + actionsWidth;
  const room = virtual.scrollRect?.width ?? 600;
  const shrinkable = growing.reduce((sum, h) => sum + Math.max(0, h.getSize() - (byId.get(h.column.id)?.minSize ?? h.getSize())), 0);
  const shrink = Math.min(shrinkable, Math.max(0, natural - room));
  const actual = (h: (typeof headers)[number]) => {
    const c = byId.get(h.column.id)!;
    const spare = Math.max(0, h.getSize() - (c.minSize ?? h.getSize()));
    return growing.includes(h) && shrinkable ? Math.ceil(h.getSize() - shrink * spare / shrinkable) : h.getSize();
  };
  const template = [...headers.map((h) => growing.includes(h) ? `minmax(${actual(h)}px, 1fr)` : `${h.getSize()}px`), ...(actions ? [`${actionsWidth}px`] : [])].join(" ");
  const total = headers.reduce((sum, h) => sum + actual(h), 0) + actionsWidth;
  const style = { "--cols": template, minWidth: total } as CSSProperties;

  // Text measurement only visits mounted cells, never the complete row set. The canvas cache
  // avoids reshaping the same strings while scrolling. Font loading invalidates those widths.
  useLayoutEffect(() => { setAutoSizes({}); }, [rows, lang]);
  useEffect(() => {
    const loaded = () => { clearTextWidths(); setAutoSizes({}); virtual.measure(); };
    document.fonts?.addEventListener("loadingdone", loaded);
    return () => document.fonts?.removeEventListener("loadingdone", loaded);
  }, [virtual]);
  useLayoutEffect(() => {
    if (actions) {
      const widths = Array.from(scroller.current?.querySelectorAll<HTMLElement>(".grid-actions") ?? [], (element) => element.offsetWidth);
      setActionsWidth((old) => Math.max(old, ...widths));
    }
    const measured: Record<string, number> = {};
    scroller.current?.querySelectorAll<HTMLElement>("[data-column]").forEach((element) => {
      const id = element.dataset.column!;
      const column = byId.get(id)!;
      const css = getComputedStyle(element);
      const control = element.querySelector("input, select, textarea");
      // Native controls keep their visible value outside textContent; unchosen options are hidden.
      const text = control instanceof HTMLSelectElement
        ? Array.from(control.selectedOptions, (option) => option.textContent ?? "").join(" ")
        : control instanceof HTMLInputElement || control instanceof HTMLTextAreaElement
          ? control.value : element.textContent ?? "";
      const width = textWidth(text, css);
      if (!width) return;
      const padding = (parseFloat(css.paddingLeft) || 6) + (parseFloat(css.paddingRight) || 6);
      const extra = element.getAttribute("role") === "columnheader" ? 14 : element.querySelector("select, svg") ? 22 : 2;
      measured[id] = Math.max(measured[id] ?? 0, Math.min(column.maxAutoSize ?? 360, Math.ceil(width + padding + extra)), column.minSize ?? 40);
    });
    setAutoSizes((old) => {
      const next = { ...old };
      let changed = false;
      for (const [id, width] of Object.entries(measured)) {
        if (old[id] === undefined || width > old[id]) { next[id] = width; changed = true; }
      }
      return changed ? next : old;
    });
  });

  // Rows by the keyboard: the arrows, Home and End move between them, even those scrolled out of
  // view (the list scrolls to them first); one row is the tab stop.
  const [stop, setStop] = useState<string | null>(null);
  const wanted = useRef<string | null>(null);
  const rowItems = useMemo(() => items.filter((i) => i.kind === "row"), [items]);
  const tabStop = stop !== null && rowItems.some((i) => i.key === stop) ? stop : (rowItems[0]?.key ?? null);
  useLayoutEffect(() => {
    const key = wanted.current;
    if (key === null) return;
    const el = Array.from(scroller.current?.querySelectorAll<HTMLElement>("[data-row-key]") ?? []).find(
      (e) => e.dataset.rowKey === key,
    );
    if (el) {
      wanted.current = null;
      el.focus();
    }
  });
  function move(to: number) {
    const target = rowItems[Math.max(0, Math.min(rowItems.length - 1, to))];
    if (!target) return;
    wanted.current = target.key;
    setStop(target.key);
    virtual.scrollToIndex(items.indexOf(target));
  }
  function onKeyDown(e: KeyboardEvent<HTMLDivElement>, row: Row, key: string) {
    // Only the row's own keys: a key typed in a field inside it is the field's.
    if (e.target !== e.currentTarget) return;
    const at = rowItems.findIndex((i) => i.key === key);
    if (e.key === "ArrowDown") move(at + 1);
    else if (e.key === "ArrowUp") move(at - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(rowItems.length - 1);
    else if ((e.key === "Enter" || e.key === " ") && onRowClick && !isDisabled?.(row)) onRowClick(row);
    else return;
    e.preventDefault();
  }

  const resizing = table.state.columnResizing.isResizingColumn;

  return (
    <div
      className={resizing ? "grid grid-resizing" : "grid"}
      ref={scroller}
      role="table"
      aria-label={label}
      aria-rowcount={items.length + 1}
      aria-colcount={columns.length + (actions ? 1 : 0)}
    >
      <div className="grid-inner" style={style}>
        <div className="grid-head" role="row" aria-rowindex={1}>
          {headers.map((h) => {
            const c = byId.get(h.column.id)!;
            const sort = h.column.getIsSorted();
            const sortable = h.column.getCanSort();
            return (
              <div
                key={h.id}
                role="columnheader"
                data-column={h.column.id}
                className={c.align === "end" ? "grid-th end" : "grid-th"}
                aria-sort={sort === "asc" ? "ascending" : sort === "desc" ? "descending" : sortable ? "none" : undefined}
              >
                {sortable ? (
                  <button type="button" className="grid-sort" onClick={h.column.getToggleSortingHandler()} title={c.header}>
                    <span>{c.header}</span>
                    <svg viewBox="0 0 10 10" aria-hidden="true" className={sort ? `grid-arrow ${sort}` : "grid-arrow"}>
                      <path d="M5 8V2M2.5 4.5 5 2l2.5 2.5" />
                    </svg>
                  </button>
                ) : (
                  <span className="grid-sort" title={c.header}>
                    <span>{c.header}</span>
                  </span>
                )}
                {h.column.getCanResize() && (
                  <span
                    className="grid-edge"
                    role="separator"
                    aria-orientation="vertical"
                    aria-label={t("grid.resize", { name: c.header })}
                    aria-valuenow={h.getSize()}
                    aria-valuemin={c.minSize ?? 40}
                    tabIndex={0}
                    onMouseDown={(event) => {
                      const width = event.currentTarget.parentElement?.getBoundingClientRect().width;
                      if (width) flushSync(() => table.setColumnSizing((old) => ({ ...old, [h.column.id]: width })));
                      h.getResizeHandler()(event);
                    }}
                    onTouchStart={(event) => {
                      const width = event.currentTarget.parentElement?.getBoundingClientRect().width;
                      if (width) flushSync(() => table.setColumnSizing((old) => ({ ...old, [h.column.id]: width })));
                      h.getResizeHandler()(event);
                    }}
                    onDoubleClick={() => table.setColumnSizing((old) => {
                      const next = { ...old }; delete next[h.column.id]; return next;
                    })}
                    onKeyDown={(e) => {
                      const grow = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
                      if (grow === 0) return;
                      e.preventDefault();
                      // Right widens a column in English; in Arabic the column is on the other side.
                      const by = (rtl ? -grow : grow) * 16;
                      table.setColumnSizing((was) => ({ ...was, [h.column.id]: Math.max(c.minSize ?? 40, (e.currentTarget.parentElement?.getBoundingClientRect().width || h.getSize()) + by) }));
                    }}
                  />
                )}
              </div>
            );
          })}
          {actions && <div role="columnheader" aria-label={t("grid.actions")} />}
        </div>
        {items.length === 0 ? (
          empty
        ) : (
          <div className="grid-body" style={{ height: virtual.getTotalSize() }}>
            {virtual.getVirtualItems().map((v) => {
              const item = items[v.index];
              const at = { transform: `translateY(${v.start}px)`, ...(item.kind === "row" ? { minHeight: rowHeight } : { height: v.size }) };
              if (item.kind === "section") {
                return (
                  <div key={item.key} className="grid-row grid-section" role="row" aria-rowindex={v.index + 2} style={at}>
                    <div role="rowheader" className="grid-sectionname" title={item.label} aria-colspan={columns.length + (actions ? 1 : 0)}>
                      {item.label}
                      {sectionCount && (
                        <>
                          {" "}
                          <span className="grid-count">{item.count}</span>
                        </>
                      )}
                    </div>
                  </div>
                );
              }
              const { row } = item;
              const disabled = isDisabled?.(row);
              const current = isCurrent?.(row);
              return (
                <div
                  key={item.key}
                  className={`grid-row${rowClass?.(row) ? ` ${rowClass(row)}` : ""}${onRowClick && !disabled ? " clickable" : ""}`}
                  role="row"
                  aria-rowindex={v.index + 2}
                  aria-current={current || undefined}
                  aria-disabled={disabled || undefined}
                  data-index={v.index}
                  ref={virtual.measureElement}
                  data-row-key={item.key}
                  tabIndex={item.key === tabStop ? 0 : -1}
                  style={at}
                  onClick={onRowClick && !disabled ? () => onRowClick(row) : undefined}
                  onKeyDown={(e) => onKeyDown(e, row, item.key)}
                  onFocus={(e) => e.target === e.currentTarget && setStop(item.key)}
                >
                  {headers.map((h) => {
                    const c = byId.get(h.column.id)!;
                    const extra = c.cellClass?.(row);
                    return (
                      <div
                        key={h.column.id}
                        role="cell"
                        data-column={h.column.id}
                        className={`grid-cell${c.align === "end" ? " end" : ""}${c.wrap ? " wrap" : ""}${extra ? ` ${extra}` : ""}`}
                        title={c.title?.(row)}
                        onClick={c.isolate || c.onDoubleClick ? (e) => e.stopPropagation() : undefined}
                        onDoubleClick={
                          c.onDoubleClick
                            ? (e) => {
                                e.stopPropagation();
                                c.onDoubleClick!(row);
                              }
                            : undefined
                        }
                      >
                        {c.cell(row)}
                      </div>
                    );
                  })}
                  {actions && (
                    <div className="grid-actions" role="cell">
                      {actions(row)}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
