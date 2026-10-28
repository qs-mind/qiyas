/** Pure helpers for "How was it calculated": numbers, units and names as an engineer reads them. */
import type { DimensionLine, Explanation, Operand } from "../api";

/** Units as written on a dimension sheet: m², m³. */
export function unitText(unit: string | undefined): string {
  if (!unit) return "";
  return unit.replace(/^m2$/, "m²").replace(/^m3$/, "m³").replace(/^nr$/, "nr");
}

/** A value to three decimals at most, trailing zeros dropped. */
export function num(value: number): string {
  return Number.isFinite(value) ? String(Number(value.toFixed(3))) : "-";
}

/** "Length 5.2 m": an operand's name, value and unit. */
export function operandText(op: Operand): string {
  const unit = unitText(op.unit);
  return `${op.label} ${num(op.value)}${unit ? ` ${unit}` : ""}`;
}

/** The operands of a line, or one plain operand per factor for lines written before operands. */
export function lineOperands(line: DimensionLine): Operand[] {
  if (line.operands?.length) return line.operands;
  const out: Operand[] = [];
  if (line.timesing !== 1) out.push({ label: "×", expression: String(line.timesing), value: line.timesing, sources: [] });
  for (const factor of line.factors) out.push({ label: "", expression: String(factor), value: factor, sources: [] });
  return out;
}

/** The item's own description in the language shown, else its code. */
export function itemTitle(exp: Explanation, lang: "en" | "ar"): string {
  const original = (exp.original ?? {}) as Record<string, unknown>;
  const pick = (key: string) => (typeof original[key] === "string" ? (original[key] as string) : "");
  const text = lang === "ar" ? pick("description_ar") || pick("description_en") : pick("description_en") || pick("description_ar");
  return text || pick("description") || pick("code") || exp.item;
}

/** A short name for an element: a mark or label it carries, else its type, as the drawing names it. */
export function elementName(exp: Explanation, handle: number): string {
  const record = (exp.elements ?? []).find((e) => (e as { id?: number }).id === handle) as
    | { type?: string; props?: Record<string, unknown> }
    | undefined;
  const props = record?.props ?? {};
  for (const key of ["mark", "label", "tag", "name", "id"]) {
    const value = props[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return record?.type ?? "";
}

/** The lines of one element (its own terms and the voids deducted from it), or all of them. */
export function linesOf(exp: Explanation, handle: number | null): DimensionLine[] {
  if (handle == null) return exp.dimension_lines;
  return exp.dimension_lines.filter((l) => l.sources[0]?.handle === handle);
}
