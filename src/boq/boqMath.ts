/** The sums and differences of the BOQ panel: what the take-off measured for a BOQ item, and how
 *  far that is from the item's quantity. Pure, so it is tested on its own. */
import type { BoqItem, Measurement } from "../api";

/** Allowed difference, in percent, when an item has none of its own. */
export const DEFAULT_TOLERANCE = 5;

/** Linked rows whose unit is not the item's: their sum in their own unit. */
export type OtherUnit = { unit: string; sum: number };
export type QuantityLink = Pick<Measurement, "id" | "boq_code" | "unit" | "shown">;

export type Measured = {
  /** The take-off rows linked to the item, whatever their unit. */
  links: QuantityLink[];
  /** Sum of `shown` over the linked rows in the item's unit; null when there is none. */
  sum: number | null;
  /** Linked rows in another unit, per unit: a unit mismatch. */
  other: OtherUnit[];
};

/** Units compare without case or spaces, so "M2" is "m2". */
const unitKey = (unit: string) => unit.toLowerCase().replace(/\s+/g, "");

/** The take-off rows linked to `item` (by code) and their sum. A row counts towards the sum only
 *  when its unit is the item's (m with m, m2 with m2, m3 with m3, pcs with pcs, or the same own
 *  unit); the others are summed per unit as a mismatch. An item without a code has no links. */
export function measuredFor(item: BoqItem, takeoff: QuantityLink[]): Measured {
  const links = item.code === "" ? [] : takeoff.filter((m) => m.boq_code === item.code);
  let sum: number | null = null;
  const other = new Map<string, number>();
  for (const m of links) {
    if (unitKey(m.unit) === unitKey(item.unit)) sum = (sum ?? 0) + m.shown;
    else other.set(m.unit, (other.get(m.unit) ?? 0) + m.shown);
  }
  return { links, sum, other: [...other].map(([unit, s]) => ({ unit, sum: s })) };
}

export type Status = "within" | "over" | "under" | "none";

export type Difference = {
  /** (measured − BOQ) / BOQ in percent; null when there is nothing to compare, or the BOQ
   *  quantity is 0. */
  percent: number | null;
  status: Status;
};

/** How far `measured` is from the item's quantity, against its tolerance (or 5 %). `none` when
 *  nothing in the item's unit is linked. */
export function difference(item: BoqItem, measured: number | null): Difference {
  if (measured === null) return { percent: null, status: "none" };
  const tolerance = item.tolerance ?? DEFAULT_TOLERANCE;
  if (item.quantity === 0) {
    return { percent: null, status: measured === 0 ? "within" : "over" };
  }
  const percent = ((measured - item.quantity) / Math.abs(item.quantity)) * 100;
  // A hair over the line from rounding is still within it.
  if (Math.abs(percent) <= tolerance + 1e-9) return { percent, status: "within" };
  return { percent, status: percent > 0 ? "over" : "under" };
}

/** Text for searching: lower case, Arabic diacritics and tatweel removed, alef forms, ى and ة
 *  folded, so "أعمال" finds "اعمال". */
export function searchKey(text: string): string {
  return text
    .toLowerCase()
    .replace(/[ً-ٰٟـ]/g, "")
    .replace(/[أإآٱ]/g, "ا")
    .replace(/ى/g, "ي")
    .replace(/ة/g, "ه");
}

/** The item matches the search `query` on its code or description. */
export const matches = (item: BoqItem, query: string) => {
  const q = searchKey(query.trim());
  return q === "" || searchKey(item.code).includes(q) || searchKey(item.description).includes(q);
};

/** Quantities keep Latin digits in both languages, with three decimals; whole pieces show as
 *  whole numbers. */
export const qty = (n: number, unit: string) =>
  unitKey(unit) === "pcs" && Number.isInteger(n) ? n.toFixed(0) : n.toFixed(3);

/** The unit as the UI says it ("m²", "م²"); a unit it doesn't know is shown as written. */
export function unitLabel(t: (key: string) => string, unit: string): string {
  const key = `qty.${unitKey(unit)}`;
  const label = t(key);
  return label === key ? unit : label;
}
