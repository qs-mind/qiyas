import { memo, useCallback, useMemo, useRef, useState } from "react";
import type { Boq, Kind, Measurement, Total } from "../api";
import { LinkPicker } from "../boq/LinkPicker";
import { useT } from "../i18n";
import { DataGrid, type GridColumn } from "../ui/DataGrid";
import { shortLayerName } from "./layerName";

type Props = {
  items: Measurement[];
  totals: Total[];
  selectedId: string | null;
  onSelect(id: string): void;
  onRemove(id: string): void;
  onRename(id: string, text: string): void;
  /** The BOQ to link rows to; null disables the link buttons. */
  boq: Boq | null;
  /** Links the row to the BOQ item `code`, or unlinks it (null). */
  onLink(id: string, code: string | null): void;
  /** Decimals of lengths, areas and volumes (2, 3 or 4). */
  precision?: number;
};

/** Quantities keep Latin digits in both languages: counts as whole numbers, the rest with
 *  `precision` decimals. */
const qty = (n: number, kind: Kind, precision: number) =>
  kind === "count" ? n.toFixed(0) : n.toFixed(precision);

/** The take-off: one row per measurement (click to show it on the drawing, double click or the pen
 *  to rename, the chain to link it to a BOQ item, the cross to remove) and the totals per kind
 *  underneath. Memoised: the app re-renders on every pointer move. */
export const TakeoffPanel = memo(function TakeoffPanel({
  items,
  totals,
  selectedId,
  onSelect,
  onRemove,
  onRename,
  boq,
  onLink,
  precision = 3,
}: Props) {
  const t = useT();
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  /** Set by Escape, so the blur that ends the edit doesn't save it. */
  const cancelled = useRef(false);
  /** The row whose BOQ link is being picked. */
  const [linking, setLinking] = useState<string | null>(null);
  /** The link button that opened the picker; it gets the focus back when the picker closes. */
  const opener = useRef<HTMLButtonElement | null>(null);
  const closePicker = useCallback(() => {
    setLinking(null);
    opener.current?.focus();
  }, []);
  const linked = items.find((m) => m.id === linking);

  function startEdit(m: Measurement) {
    setDraft(m.description);
    setEditing(m.id);
  }

  function endEdit(m: Measurement) {
    const text = draft.trim();
    if (!cancelled.current && text && text !== m.description) onRename(m.id, text);
    cancelled.current = false;
    setEditing(null);
  }

  // The columns are made once per language and precision, so the grid's own state (the sorting,
  // the widths) is not asked to start again on every render.
  const columns = useMemo<GridColumn<Measurement>[]>(
    () => [
      {
        id: "description",
        header: t("description"),
        size: 120,
        minSize: 80,
        grow: true,
        cellClass: () => "takeoff-description",
        sortValue: (m) => m.description,
        cell: (m) => (
          <div className="desc">
            {editing === m.id ? (
              <input
                aria-label={t("description")}
                value={draft}
                autoFocus
                onChange={(e) => setDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                onBlur={() => endEdit(m)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") cancelled.current = true;
                  if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
                }}
              />
            ) : (
              <button
                type="button"
                title={
                  m.block_path.length > 0
                    ? `${m.description}\n${t("takeoff.inBlock", { path: blockPath(m) })}`
                    : m.description
                }
                onDoubleClick={() => startEdit(m)}
              >
                <bdi>{m.description}</bdi>
              </button>
            )}
            {(m.kind === "area" || m.kind === "volume") && (
              <small>{t(`takeoff.origin.${m.area_origin ?? "unspecified"}`)} · {t(`takeoff.basis.${m.measurement_basis ?? "unspecified"}`)}</small>
            )}
            {m.kind === "count" && <small title={m.note}>{t(m.count_scope ? "takeoff.insertScope" : "takeoff.unknownScope")}</small>}
            {m.self_intersecting && (
              <span className="warn-icon" role="img" aria-label={t("selfCrossing")} title={t("selfCrossing")}>
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <path d="M8 2 15 14H1Z M8 6.5v3.5 M8 12v.01" />
                </svg>
              </span>
            )}
          </div>
        ),
      },
      {
        id: "quantity",
        header: t("quantity"),
        size: 64,
        align: "end",
        sortValue: (m) => m.shown,
        cell: (m) => qty(m.shown, m.kind, precision),
      },
      {
        id: "unit",
        header: t("unit"),
        size: 44,
        sortValue: (m) => t(`qty.${m.unit}`),
        cell: (m) => t(`qty.${m.unit}`),
      },
      {
        id: "boq",
        header: t("boq.column"),
        size: 46,
        sortValue: (m) => m.boq_code,
        cell: (m) => m.boq_code ?? "—",
        title: (m) => m.boq_code ?? undefined,
        cellClass: (m) => (m.boq_code ? undefined : "muted"),
      },
      {
        id: "layer",
        header: t("layer"),
        size: 70,
        minSize: 48,
        sortValue: (m) => shortLayerName(m.layer),
        cell: (m) => <bdi>{shortLayerName(m.layer)}</bdi>,
        title: (m) => m.layer,
      },
    ],
    // `editing` and `draft` change what the description cell shows: they are the cells' own state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, precision, editing, draft],
  );

  return (
    <aside className="takeoff">
      <h2 className="heading panel-head">{t("takeoff")}</h2>
      {items.length === 0 ? (
        <p className="empty">{t("takeoffEmpty")}</p>
      ) : (
        <DataGrid
          label={t("takeoff")}
          columns={columns}
          rows={items}
          rowKey={rowId}
          storageKey="takeoff"
          rowHeight={72}
          autoRowHeight
          onRowClick={(m) => onSelect(m.id)}
          isCurrent={(m) => m.id === selectedId}
          actions={(m) => {
            const renameLabel = `${t("rename")}: ${m.description}`;
            const removeLabel = `${t("remove")}: ${m.description}`;
            const linkLabel = boq ? `${t("boq.link")}: ${m.description}` : t("boq.importFirst");
            return (
              <>
                <button
                  type="button"
                  className="icon"
                  aria-label={renameLabel}
                  title={renameLabel}
                  onClick={(e) => {
                    e.stopPropagation();
                    startEdit(m);
                  }}
                >
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path d="M3 13l.6-2.6L10.8 3.2l2 2-7.2 7.2Z" />
                  </svg>
                </button>
                <button
                  type="button"
                  className="icon"
                  aria-label={linkLabel}
                  title={linkLabel}
                  aria-haspopup="dialog"
                  disabled={!boq}
                  onClick={(e) => {
                    e.stopPropagation();
                    opener.current = e.currentTarget;
                    setLinking(m.id);
                  }}
                >
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path d="M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 0 1 3.5 3.5l-1 1M9 11.5l-1 1a2.5 2.5 0 0 1-3.5-3.5l1-1" />
                  </svg>
                </button>
                <button
                  type="button"
                  className="icon"
                  aria-label={removeLabel}
                  title={removeLabel}
                  onClick={(e) => {
                    e.stopPropagation();
                    onRemove(m.id);
                  }}
                >
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path d="M4 4l8 8M12 4l-8 8" />
                  </svg>
                </button>
              </>
            );
          }}
        />
      )}
      {boq && linked && (
        <LinkPicker
          title={linked.description}
          items={boq.items}
          current={linked.boq_code}
          onPick={(code) => {
            closePicker();
            if (code !== linked.boq_code) onLink(linked.id, code);
          }}
          onClose={closePicker}
        />
      )}
      <h2 className="heading panel-head">{t("totals")}</h2>
      <ul className="totals" aria-label={t("totals")}>
        {totals.map((total) => (
          <li key={`${total.kind}-${total.unit}`}>
            <span className="kind">{t(total.kind)}</span>
            <span className="num">{total.count} ×</span>
            <span className="num">
              {qty(total.sum, total.kind, precision)} {t(`qty.${total.unit}`)}
            </span>
          </li>
        ))}
      </ul>
    </aside>
  );
});

const rowId = (m: Measurement) => m.id;

/** The blocks a row's entity sits in, outermost first. */
const blockPath = (m: Measurement) => m.block_path.join(" / ");
