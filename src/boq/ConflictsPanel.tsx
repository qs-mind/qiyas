import { conflictFinding, findingText, findingQuantity, type DiscrepancyFinding } from "../findings";
import { readingDirection } from "../ai/content";
import { memo, useMemo, useRef, useState, type ReactNode } from "react";
import type { Command, Conflict, ConflictKind } from "../api";
import { useLanguage, useT } from "../i18n";
import { DataGrid, type GridColumn } from "../ui/DataGrid";
import { SegmentedChoice } from "../ui/Segmented";
import { qty, unitLabel } from "./boqMath";
import {
  KINDS,
  STATUSES,
  countByKind,
  filterConflicts,
  groupByKind,
  type StatusFilter,
} from "./conflictsMath";

export type ResolveCommand = Extract<Command, { type: "resolve_conflict" }>;

const conflictKey = (c: DiscrepancyFinding) => c.id;

type Props = {
  conflicts: Conflict[];
  /** A comparison ran for this drawing, so an empty list means none were found. */
  compared: boolean;
  hasBoq: boolean;
  /** Points the drawing (or the BOQ tab) at what the conflict is about. */
  onShow(conflict: Conflict): void;
  /** Sets a conflict's status and note. */
  onResolve(cmd: ResolveCommand): void;
  /** Opens the BOQ tab, from the "no BOQ" empty state. */
  onOpenBoq(): void;
};

/** The last comparison's conflicts, grouped by kind: filter them, show one on the drawing, set its
 *  status, write a note. Compare and Export are in the output's head (`ArtifactFrame`). Memoised: the app re-renders on every pointer move. */
export const ConflictsPanel = memo(function ConflictsPanel({
  conflicts,
  compared,
  hasBoq,
  onShow,
  onResolve,
  onOpenBoq,
}: Props) {
  const t = useT();
  const { lang } = useLanguage();
  const [status, setStatus] = useState<StatusFilter>("all");
  const [kind, setKind] = useState<ConflictKind | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  /** Set by Escape, so the blur that ends the edit doesn't save it. */
  const cancelled = useRef(false);

  function endEdit(c: DiscrepancyFinding) {
    const note = draft.trim();
    if (!cancelled.current && note !== c.details.note) {
      onResolve({ type: "resolve_conflict", id: c.id, status: c.details.status, note });
    }
    cancelled.current = false;
    setEditing(null);
  }

  const byStatus = filterConflicts(conflicts, status, null);
  const counts = countByKind(byStatus);
  // A chip stays while its kind has conflicts in any status, so a chosen one doesn't vanish.
  const present = countByKind(conflicts);
  const shown = filterConflicts(byStatus, "all", kind);
  const arrow = lang === "ar" ? "←" : "→";

  let empty: ReactNode = null;
  if (conflicts.length === 0) {
    if (!hasBoq && !compared) {
      empty = (
        <p className="empty">
          {t("conflicts.noBoq")}{" "}
          <button type="button" className="link" onClick={onOpenBoq}>
            {t("conflicts.toBoq")}
          </button>
        </p>
      );
    } else {
      empty = <p className="empty">{t(compared ? "conflicts.none" : "conflicts.notCompared")}</p>;
    }
  } else if (shown.length === 0) {
    empty = <p className="empty">{t("conflicts.noMatch")}</p>;
  }

  /** What a row can show on the drawing: a measurement, a BOQ item or a sheet. */
  const showable = (c: DiscrepancyFinding) => c.details.measurements.length > 0 || !!c.details.boq_code || c.details.sheets.length > 0;
  /** The rows in the order of the kinds, so the grid's sections come in that order. */
  const rows = useMemo(() => groupByKind(shown).flatMap((g) => g.conflicts.map(conflictFinding)), [shown]);
  const kindName = (c: DiscrepancyFinding) => t(`conflictKind.${c.details.kind}`);

  const columns = useMemo<GridColumn<DiscrepancyFinding>[]>(
    () => [
      {
        id: "query",
        header: t("conflicts.question"),
        size: 200,
        grow: true,
        wrap: true,
        cell: (c) => c.details.query ? <span {...readingDirection(lang === "ar" ? c.details.query.question_ar : c.details.query.question_en)}>{lang === "ar" ? c.details.query.question_ar : c.details.query.question_en}</span> : "—",
        title: (c) => c.citations.map((r) => `${r.source}: ${r.clause}`).join("; "),
      },
      {
        id: "severity", header: t("conflicts.severity"), size: 80,
        cell: (c) => c.details.query ? t(`querySeverity.${c.severity === "info" ? "information" : c.severity}`) : "—",
      },
      {
        id: "grid", header: t("conflicts.grid"), size: 100,
        cell: (c) => c.details.query?.references.map((r) => `${r.sheet} / ${r.grid ?? t("conflicts.unassigned")}`).join(", ") ?? "—",
      },
      {
        id: "reason",
        header: t("conflicts.reason"),
        size: 160,
        minSize: 80,
        grow: true,
        wrap: true,
        sortValue: (c) => findingText(c, lang),
        cell: (c) => <span {...readingDirection(findingText(c, lang))}>{findingText(c, lang)}</span>,
        title: (c) => findingText(c, lang),
        cellClass: () => "reason",
      },
      {
        id: "code",
        header: t("boq.code"),
        size: 52,
        sortValue: (c) => c.details.boq_code,
        cell: (c) => c.details.boq_code || "—",
        title: (c) => c.details.boq_code || undefined,
        cellClass: (c) => (c.details.boq_code ? "code" : "muted"),
      },
      {
        id: "numbers",
        header: t("conflicts.expectedFound"),
        size: 156,
        align: "end",
        sortValue: (c) => (findingQuantity(c, "expected")?.value ?? null),
        cell: (c) => {
          const expected = findingQuantity(c, "expected");
          const found = findingQuantity(c, "found");
          if (!expected && !found) return null;
          const unit = expected?.unit ?? found?.unit ?? "";
          return <>
            <bdi>{expected ? qty(expected.value, expected.unit) : "—"}</bdi> {arrow}{" "}
            <bdi>{found ? qty(found.value, found.unit) : "—"}</bdi>
            {unit && <> {unitLabel(t, unit)}</>}
          </>;
        },
        title: (c) => c.quantities.length ? t("conflicts.expectedFound") : undefined,
        cellClass: () => "numbers",
      },
      {
        id: "sheets",
        header: t("conflicts.sheets"),
        size: 64,
        minSize: 48,
        sortValue: (c) => c.details.sheets.join(", "),
        cell: (c) => <span {...readingDirection(c.details.sheets.join(", "))}>{c.details.sheets.join(", ")}</span>,
        title: (c) => (c.details.sheets.length > 0 ? c.details.sheets.join(", ") : undefined),
        cellClass: () => "muted",
      },
      {
        id: "status",
        header: t("conflicts.status"),
        size: 96,
        sortValue: (c) => STATUSES.indexOf(c.details.status),
        isolate: true,
        cell: (c) => (
          <select
            aria-label={t("conflicts.status")}
            value={c.details.status}
            onChange={(e) =>
              onResolve({
                type: "resolve_conflict",
                id: c.id,
                status: e.target.value as Conflict["status"],
                note: c.details.note,
              })
            }
          >
            {STATUSES.map((s) => (
              <option key={s} value={s}>
                {t(`conflictStatus.${s}`)}
              </option>
            ))}
          </select>
        ),
      },
      {
        id: "note",
        header: t("conflicts.noteColumn"),
        size: 96,
        minSize: 60,
        grow: true,
        sortValue: (c) => c.details.note,
        isolate: true,
        title: (c) => c.details.note || undefined,
        cell: (c) =>
          editing === c.id ? (
            <input
              type="text"
              {...readingDirection(draft, lang)}
              aria-label={t("conflicts.note")}
              placeholder={t("conflicts.note")}
              value={draft}
              autoFocus
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => endEdit(c)}
              onKeyDown={(e) => {
                if (e.key === "Escape") cancelled.current = true;
                if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
              }}
            />
          ) : (
            <button
              type="button"
              {...readingDirection(c.details.note || t("conflicts.note"))}
              aria-label={c.details.note ? undefined : t("conflicts.note")}
              className={c.details.note ? undefined : "muted"}
              onClick={() => {
                setDraft(c.details.note);
                setEditing(c.id);
              }}
            >
              {c.details.note || t("conflicts.note")}
            </button>
          ),
      },
    ],
    // A cell shows the note being typed; the rest changes with the language and the callbacks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, lang, editing, draft, onResolve],
  );

  const chip = (value: ConflictKind | null, label: string, n: number) => (
    <button
      key={value ?? "all"}
      type="button"
      aria-pressed={kind === value}
      onClick={() => setKind(kind === value || value === null ? null : value)}
    >
      {label} <span className="n">{n}</span>
    </button>
  );

  return (
    <aside className="conflicts">
      <p className="conflicts-hint">{t("conflicts.hint")}</p>
      {conflicts.length > 0 && (
        <div className="conflicts-filters">
          <SegmentedChoice<StatusFilter>
            label={t("conflicts.status")}
            value={status}
            onChange={setStatus}
            segments={(["all", ...STATUSES] as StatusFilter[]).map((s) => ({
              value: s,
              label: t(s === "all" ? "conflicts.all" : `conflictStatus.${s}`),
            }))}
          />
          <div className="chips" role="group" aria-label={t("conflicts.kind")}>
            {chip(null, t("conflicts.all"), byStatus.length)}
            {KINDS.filter((k) => present[k] > 0).map((k) => chip(k, t(`conflictKind.${k}`), counts[k]))}
          </div>
        </div>
      )}
      {empty ?? (
        <DataGrid
          label={t("conflicts")}
          columns={columns}
          rows={rows}
          rowKey={conflictKey}
          section={kindName}
          sectionCount
          storageKey="conflicts"
          rowHeight={40}
          onRowClick={(finding) => onShow(finding.details)}
          isDisabled={(c) => !showable(c)}
          rowClass={(c) => (c.details.status === "open" ? "conflict" : "conflict done")}
          actions={(c) => (
            <button type="button" disabled={!showable(c)} onClick={() => onShow(c.details)}>
              {t("conflicts.show")}
            </button>
          )}
        />
      )}
    </aside>
  );
});
