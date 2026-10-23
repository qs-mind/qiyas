/** What the canvas draws over the drawing: the highlighted entities (the selected row, a BOQ
 *  item's or a conflict's rows, the assistant's, the tool's pick and selection) and the overlay
 *  lines (the rows' outlines or count marks, the measure in progress, the snap marker). */
import { useMemo } from "react";
import type { Measurement } from "../api";
import type { Tool } from "../shell/Ribbon";
import { markSize, measurementTargets, overlaySegments, pointMarks } from "../tools/overlay";
import type { Batch, Target } from "../viewport/batch";
import type { Measure } from "./shared";

type Deps = {
  tool: Tool;
  measure: Pick<Measure, "highlighted" | "selection" | "preview" | "points" | "cursor" | "marker">;
  batch: Batch | null;
  /** The selected row on the sheet in view, if any, and the rows a BOQ item or a conflict lights. */
  selected: Measurement | undefined;
  boqLinked: Measurement[];
  conflictLinked: Measurement[];
  aiHighlight: number[];
  revealOverlay: Float64Array | null;
};

export function useOverlay({ tool, measure, batch, selected, boqLinked, conflictLinked, aiHighlight, revealOverlay }: Deps) {
  const highlight = useMemo(() => {
    const rows = [...(selected ? [selected] : []), ...boqLinked, ...conflictLinked];
    const targets: Target[] = [
      ...rows.flatMap(measurementTargets),
      ...aiHighlight.map((handle) => ({ handle, chain: [] })),
    ];
    if (measure.highlighted !== null) targets.push(measure.highlighted);
    for (const handle of measure.selection) targets.push({ handle, chain: [] });
    return targets;
  }, [selected, boqLinked, conflictLinked, measure.highlighted, measure.selection, aiHighlight]);

  const selectedOverlay = useMemo(() => {
    const rows = selected ? [selected] : [...boqLinked, ...conflictLinked];
    // A count's points are the counted places: small crosses, not a line.
    const size = markSize(batch?.bounds ?? null);
    const outlines = rows
      .filter((m) => m.points.length > 0)
      .map((m) =>
        m.kind === "count"
          ? pointMarks(m.points, size)
          : overlaySegments(m.points, null, m.kind === "area" ? "area" : "distance"),
      );
    if (outlines.length === 0) return null;
    const all = new Float64Array(outlines.reduce((n, o) => n + o.length, 0));
    outlines.reduce((at, o) => (all.set(o, at), at + o.length), 0);
    return all;
  }, [selected, boqLinked, conflictLinked, batch]);
  const rubberBand = useMemo(
    () =>
      measure.preview && measure.preview.length > 0
        ? measure.preview
        : measure.points.length > 0
          ? overlaySegments(measure.points, measure.cursor, tool)
          : null,
    [measure.preview, measure.points, measure.cursor, tool],
  );
  // A measure in progress hides the selected row's outline; the snap marker goes over either.
  const overlay = useMemo(() => {
    const lines = rubberBand ?? selectedOverlay;
    const parts = [lines, measure.marker, revealOverlay].filter((part) => part !== null);
    if (parts.length === 0) return null;
    if (parts.length === 1) return parts[0];
    const all = new Float64Array(parts.reduce((n, part) => n + part.length, 0));
    parts.reduce((at, part) => (all.set(part, at), at + part.length), 0);
    return all;
  }, [rubberBand, selectedOverlay, measure.marker, revealOverlay]);

  return { highlight, overlay };
}
