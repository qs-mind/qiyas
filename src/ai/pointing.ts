/** What the user points Mizan at: the select tool's selection, or an area they marked. Pure
 *  helpers for the chips above the composer and the context sent with a message. */
import type { AiRegion, EntityTarget } from "../api";
import type { Bounds, Point } from "../viewport/camera";

/** What the select tool points at: its selection set (whole entities on the sheet), or the one
 *  entity it picked inside a block (with its chain). */
export function pointedTargets(picks: number[], picked: EntityTarget | null): EntityTarget[] {
  if (picked && picked.chain.length > 0 && (picks.length === 0 || (picks.length === 1 && picks[0] === picked.chain[0]))) {
    return [picked];
  }
  if (picks.length > 0) return picks.map((handle) => ({ handle, chain: [] }));
  return picked ? [picked] : [];
}

/** The kinds in a selection, most first: "2 LwPolyline, 1 Insert" (unknown kinds left out). */
export function kindsLabel(selection: EntityTarget[], kinds: Record<number, string>): string {
  const counts = new Map<string, number>();
  for (const { handle, chain } of selection) {
    const kind = kinds[chain.length > 0 ? chain[0] : handle];
    if (kind) counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  return [...counts]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([kind, n]) => `${n} ${kind}`)
    .join(", ");
}

/** A region's corners in order round it: a box's four, a polygon's own. */
export function regionRing(region: AiRegion): Point[] {
  if (region.kind === "polygon") return region.points;
  const [[x0, y0], [x1, y1]] = region.points;
  const [lx, hx, ly, hy] = [Math.min(x0, x1), Math.max(x0, x1), Math.min(y0, y1), Math.max(y0, y1)];
  return [
    [lx, ly],
    [hx, ly],
    [hx, hy],
    [lx, hy],
  ];
}

/** The box round a region. */
export function regionBounds(region: AiRegion): Bounds {
  const ring = regionRing(region);
  const xs = ring.map((p) => p[0]);
  const ys = ring.map((p) => p[1]);
  return [
    [Math.min(...xs), Math.min(...ys)],
    [Math.max(...xs), Math.max(...ys)],
  ];
}

/** What changes the picture sent with a message: the selection and the region. */
export const pointedKey = (selection: EntityTarget[], region: AiRegion | null) =>
  JSON.stringify([selection, region]);
