import { memo, useMemo, useRef, useState } from "react";
import { GeneratedBoq } from "./GeneratedBoq";
import type { MeasurementReport, Boq, BoqItem, Measurement } from "../api";
import { useT } from "../i18n";
import { DataGrid, type GridColumn } from "../ui/DataGrid";
import {
  DEFAULT_TOLERANCE,
  difference,
  matches,
  measuredFor,
  qty,
  unitLabel,
  type Difference,
  type Measured,
} from "./boqMath";

type Props = {
  boq: Boq | null;
  generated?: MeasurementReport | null;
  onShowGenerated?(handles: number[], sheet: string): void;
  /** The take-off, for the measured quantities. */
  takeoff: Measurement[];
  selectedCode: string | null;
  /** Shows the item's linked measurements on the drawing. */
  onSelect(code: string): void;
  /** Missing while no drawing is open: the button is disabled. */
  onImport?(): void;
  /** A tolerance in percent, or null for the default. */
  onTolerance(code: string, percent: number | null): void;
};

const WARN = (
  <svg viewBox="0 0 16 16" aria-hidden="true">
    <path d="M8 2 15 14H1Z M8 6.5v3.5 M8 12v.01" />
  </svg>
);

/** "+12.5 %", Latin digits; "—" when there is nothing to compare. */
function percentText({ percent }: Difference) {
  if (percent === null) return "—";
  const p = Math.abs(percent) < 0.05 ? 0 : percent;
  return `${p > 0 ? "+" : ""}${p.toFixed(1)} %`;
}

/** An item with what the take-off measured for it, worked out once for the grid. */
type Line = { item: BoqItem; i: number; measured: Measured; diff: Difference };

const lineKey = (l: Line) => String(l.i);
const lineSection = (l: Line) => l.item.section;

/** The imported BOQ against the take-off: per item its quantity, what the linked measurements add
 *  up to, and the difference coloured by the item's tolerance. Click a row to show its
 *  measurements; double click the tolerance to change it; click a column's title to sort by it
 *  (inside each section). Memoised: the app re-renders on every pointer move. */
export const BoqPanel = memo(function BoqPanel({
  boq,
  generated = null,
  onShowGenerated,
  takeoff,
  selectedCode,
  onSelect,
  onImport,
  onTolerance,
}: Props) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  /** Set by Escape, so the blur that ends the edit doesn't save it. */
  const cancelled = useRef(false);

  function endEdit(item: BoqItem) {
    const text = draft.trim();
    const percent = text === "" ? null : Number(text);
    const valid = percent === null || (Number.isFinite(percent) && percent >= 0);
    if (!cancelled.current && valid && percent !== item.tolerance) onTolerance(item.code, percent);
    cancelled.current = false;
    setEditing(null);
  }

  const quantities = useMemo(() => {
    const derived = generated?.items ?? [];
    const owned = new Set(derived.map((item) => item.code));
    return [
      ...takeoff.filter((row) => !owned.has(row.boq_code ?? "")),
      ...derived.map((item) => ({ id: `generated:${item.code}`, boq_code: item.code, unit: item.unit, shown: item.quantity })),
    ];
  }, [takeoff, generated]);

  const lines = useMemo<Line[]>(
    () =>
      (boq?.items ?? [])
        .map((item, i) => ({ item, i }))
        .filter(({ item }) => matches(item, query))
        .map(({ item, i }) => {
          const measured = measuredFor(item, quantities);
          return { item, i, measured, diff: difference(item, measured.sum) };
        }),
    [boq, quantities, query],
  );

  /** What the measured cell says: the sum in the item's unit, then the sums in other units. */
  const shownOf = ({ item, measured }: Line) => {
    const others = measured.other.map((o) => `${qty(o.sum, o.unit)} ${unitLabel(t, o.unit)}`);
    const sum = measured.sum === null ? [] : [qty(measured.sum, item.unit)];
    return [...sum, ...others].join(" · ");
  };

  const columns = useMemo<GridColumn<Line>[]>(
    () => [
      {
        id: "code",
        header: t("boq.code"),
        size: 54,
        sortValue: (l) => l.item.code,
        cell: (l) => l.item.code,
        title: (l) => l.item.code,
      },
      {
        id: "description",
        header: t("description"),
        size: 130,
        minSize: 80,
        grow: true,
        sortValue: (l) => l.item.description,
        cell: (l) => <span dir="auto">{l.item.description}</span>,
        title: (l) => l.item.description,
      },
      {
        id: "unit",
        header: t("unit"),
        size: 48,
        sortValue: (l) => unitLabel(t, l.item.unit),
        cell: (l) => unitLabel(t, l.item.unit),
      },
      {
        id: "quantity",
        header: t("boq.quantity"),
        size: 72,
        align: "end",
        sortValue: (l) => l.item.quantity,
        cell: (l) => qty(l.item.quantity, l.item.unit),
      },
      {
        id: "measured",
        header: t("boq.measured"),
        size: 84,
        align: "end",
        sortValue: (l) => l.measured.sum,
        cell: (l) => (
          <span className="measured">
            {l.measured.other.length > 0 && (
              <span className="warn-icon" role="img" aria-label={t("boq.unitMismatch")} title={t("boq.unitMismatch")}>
                {WARN}
              </span>
            )}
            <span dir="ltr">{shownOf(l) || "—"}</span>
          </span>
        ),
        title: (l) => shownOf(l) || undefined,
        cellClass: (l) => (l.measured.links.length === 0 ? "muted" : undefined),
      },
      {
        id: "difference",
        header: t("boq.difference"),
        size: 74,
        align: "end",
        sortValue: (l) => l.diff.percent,
        cell: (l) => <span dir="ltr">{percentText(l.diff)}</span>,
        cellClass: (l) => `diff-${l.diff.status}`,
      },
      {
        id: "links",
        header: t("boq.links"),
        size: 44,
        align: "end",
        sortValue: (l) => l.measured.links.length,
        cell: (l) => l.measured.links.length,
      },
      {
        id: "tolerance",
        header: t("boq.tolerance"),
        size: 72,
        align: "end",
        sortValue: (l) => l.item.tolerance ?? DEFAULT_TOLERANCE,
        // Editing the tolerance doesn't select the row (and zoom) as well.
        isolate: true,
        onDoubleClick: (l) => {
          setDraft(l.item.tolerance === null ? "" : String(l.item.tolerance));
          setEditing(l.i);
        },
        cell: (l) =>
          editing === l.i ? (
            <input
              type="number"
              min={0}
              step="any"
              aria-label={t("boq.tolerance")}
              placeholder={String(DEFAULT_TOLERANCE)}
              value={draft}
              autoFocus
              onChange={(e) => setDraft(e.target.value)}
              onClick={(e) => e.stopPropagation()}
              onBlur={() => endEdit(l.item)}
              onKeyDown={(e) => {
                if (e.key === "Escape") cancelled.current = true;
                if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
              }}
            />
          ) : (
            <span dir="ltr" className={l.item.tolerance === null ? "muted" : undefined}>
              {l.item.tolerance ?? DEFAULT_TOLERANCE} %
            </span>
          ),
      },
    ],
    // The tolerance cell shows the field being edited; the others only change with the language.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, editing, draft],
  );

  const importButton = (
    <button type="button" className="boq-import" disabled={!onImport} onClick={onImport}>
      {t("importBoq")}
    </button>
  );

  if (!boq) {
    return (
      <aside className="boq">
        <div className="panel-head boq-head">
          <h2 className="heading">{t("boq")}</h2>
          {importButton}
        </div>
        <GeneratedBoq report={generated} onShow={onShowGenerated} />
        <p className="empty">{t("boq.empty")}</p>
      </aside>
    );
  }

  return (
    <aside className="boq">
      <div className="panel-head boq-head">
        <h2 className="heading">{t("boq")}</h2>
        <span className="source" title={boq.source}>
          {boq.source.split(/[\\/]/).pop()}
        </span>
        {importButton}
      </div>
      <GeneratedBoq report={generated} onShow={onShowGenerated} />
      {boq.skipped_rows > 0 && (
        <p className="boq-notice" role="status">
          {t("boq.skipped", { n: boq.skipped_rows })}
        </p>
      )}
      <input
        className="boq-search"
        type="search"
        aria-label={t("boq.search")}
        placeholder={t("boq.search")}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {lines.length === 0 ? (
        <p className="empty">{t("boq.noMatch")}</p>
      ) : (
        <DataGrid
          label={t("boq")}
          columns={columns}
          rows={lines}
          rowKey={lineKey}
          section={lineSection}
          storageKey="boq"
          // An item without a code can't be linked, so there is nothing to show for it.
          onRowClick={(l) => onSelect(l.item.code)}
          isDisabled={(l) => l.item.code === ""}
          isCurrent={(l) => l.item.code !== "" && l.item.code === selectedCode}
        />
      )}
    </aside>
  );
});
