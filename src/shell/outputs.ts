/** What Mizan produces and the right column can show in place of the conversation. */
import type { Sheet } from "../api";
import type { OutputFile } from "./outputFiles";

export type OutputId = "takeoff" | "boq" | "conflicts" | "review";

export type ShownOutput = OutputId | "file";

export const OUTPUT_IDS: OutputId[] = ["takeoff", "boq", "conflicts", "review"];

export type OutputInfo = {
  id: OutputId;
  /** How many rows it holds; null when it has no count. */
  count: number | null;
  /** Reports can be available before their rows are fetched. */
  available?: boolean;
};

export function hasOutput(output: OutputInfo): boolean {
  return output.available ?? (output.count ?? 0) > 0;
}

/** How many take-off outputs are available, independent of their row counts. */
export function filledOutputs(outputs: OutputInfo[]): number {
  return outputs.filter(hasOutput).length;
}

export function outputGroups(outputs: OutputInfo[], sheets: readonly Sheet[], files: readonly OutputFile[]) {
  const groups = {
    sheets: sheets.filter((sheet) => !sheet.is_model),
    files,
    takeoff: outputs.filter(hasOutput),
  };
  return { ...groups, count: groups.sheets.length + groups.files.length + groups.takeoff.length };
}

/** The outputs and how many rows each holds, from the take-off state. */
export function outputList(s: {
  items: readonly unknown[];
  bill: { items: readonly unknown[] } | null;
  generated: { items: readonly unknown[] } | null;
  conflicts: readonly unknown[];
}): OutputInfo[] {
  return [
    { id: "takeoff", count: s.items.length },
    { id: "boq", count: (s.bill?.items.length ?? 0) + (s.generated?.items.length ?? 0) },
    { id: "conflicts", count: s.conflicts.length },
    // The report fetches its own rows on opening; do not pass off the take-off's count as its own.
    { id: "review", count: null, available: s.items.length > 0 || (s.generated?.items.length ?? 0) > 0 },
  ];
}
