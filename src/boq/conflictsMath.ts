/** Grouping, filtering and counting for the Conflicts panel. Pure, so it is tested on its own. */
import type { Conflict, ConflictKind, ConflictStatus } from "../api";

/** In the order the panel lists them (`qiyas_core::ConflictKind::ALL`). */
export const KINDS: ConflictKind[] = [
  "quantity_mismatch",
  "missing_from_boq",
  "not_in_drawings",
  "schedule_vs_plan",
  "drawing_vs_drawing",
  "unit_doubt",
  "duplicate_item",
  "relation_mismatch",
  "document_mismatch",
];

export const STATUSES: ConflictStatus[] = ["open", "resolved", "ignored"];

export type StatusFilter = ConflictStatus | "all";

/** How many conflicts have each kind; every kind is present, at 0 when none. */
export function countByKind(conflicts: Conflict[]): Record<ConflictKind, number> {
  const counts = Object.fromEntries(KINDS.map((k) => [k, 0])) as Record<ConflictKind, number>;
  for (const c of conflicts) counts[c.kind] += 1;
  return counts;
}

export const openCount = (conflicts: Conflict[]) =>
  conflicts.filter((c) => c.status === "open").length;

/** The conflicts with `status` (any when "all") and `kind` (any when null). */
export const filterConflicts = (
  conflicts: Conflict[],
  status: StatusFilter,
  kind: ConflictKind | null,
) =>
  conflicts.filter(
    (c) => (status === "all" || c.status === status) && (kind === null || c.kind === kind),
  );

export type Group = { kind: ConflictKind; conflicts: Conflict[] };

/** One group per kind that has conflicts, in `KINDS` order; each keeps the conflicts' order. */
export function groupByKind(conflicts: Conflict[]): Group[] {
  return KINDS.map((kind) => ({ kind, conflicts: conflicts.filter((c) => c.kind === kind) })).filter(
    (g) => g.conflicts.length > 0,
  );
}

/** The reason in the UI language; the English one when the Arabic is missing. */
export const reasonIn = (c: Conflict, lang: "en" | "ar") =>
  lang === "ar" && c.reason_ar !== "" ? c.reason_ar : c.reason;
