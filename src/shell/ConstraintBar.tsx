import type { ConstraintInfo, ConstraintKind } from "../api";
import { useT } from "../i18n";

/** The glyph each kind shows in the bar (text, so it needs no icon files). */
export const GLYPHS: Record<ConstraintKind, string> = {
  coincident: "●",
  collinear: "⋯",
  concentric: "◎",
  fixed: "⊠",
  parallel: "∥",
  perpendicular: "⊥",
  horizontal: "―",
  vertical: "│",
  tangent: "⌒",
  smooth: "∿",
  symmetric: "⇌",
  equal: "=",
  linear: "↔",
  aligned: "⤢",
  angular: "∠",
  radius: "R",
  diameter: "⌀",
};

/** The constraints grouped by the entity they hold first, in order of first appearance. */
export function byEntity(constraints: ConstraintInfo[]): [number, ConstraintInfo[]][] {
  const groups = new Map<number, ConstraintInfo[]>();
  for (const c of constraints) {
    const first = c.handles[0];
    groups.set(first, [...(groups.get(first) ?? []), c]);
  }
  return [...groups.entries()];
}

type Props = {
  constraints: ConstraintInfo[];
  /** Deletes one constraint (a dimensional one with its Dimension). */
  onDelete(id: number): void;
  /** The entities a glyph holds, to draw highlighted while the pointer is on it (null: none). */
  onHover(handles: number[] | null): void;
  onHide(): void;
};

/** The constraint bar: for each constrained entity on the sheet, a glyph per constraint (a
 *  dimensional one shows its parameter's name). Pointing at a glyph highlights what it holds;
 *  clicking it deletes the constraint. Docked over the drawing's lower corner, not beside each
 *  entity (it does not follow pan and zoom). */
export function ConstraintBar({ constraints, onDelete, onHover, onHide }: Props) {
  const t = useT();
  if (constraints.length === 0) return null;
  return (
    <div className="constraint-bar" role="toolbar" aria-label={t("param.bar")}>
      {byEntity(constraints).map(([handle, list]) => (
        <span key={handle} className="constraint-group" role="group" aria-label={t("param.entity", { handle })}>
          {list.map((c) => {
            const label = c.name
              ? t("param.glyphNamed", { kind: t(`param.kind.${c.kind}`), name: c.name })
              : t("param.glyph", { kind: t(`param.kind.${c.kind}`) });
            return (
              <button
                key={c.id}
                type="button"
                className="constraint-glyph"
                title={label}
                aria-label={label}
                onClick={() => onDelete(c.id)}
                onPointerEnter={() => onHover(c.handles)}
                onPointerLeave={() => onHover(null)}
                onFocus={() => onHover(c.handles)}
                onBlur={() => onHover(null)}
              >
                {GLYPHS[c.kind]}
                {c.name && <bdi className="constraint-name">{c.name}</bdi>}
              </button>
            );
          })}
        </span>
      ))}
      <button type="button" className="constraint-hide" aria-label={t("param.hideBar")} title={t("param.hideBar")} onClick={onHide}>
        ×
      </button>
    </div>
  );
}
