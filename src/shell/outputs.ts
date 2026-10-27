/** What Mizan produces and the right column can show in place of the conversation. */
export type OutputId = "takeoff" | "boq" | "conflicts" | "review";

export const OUTPUT_IDS: OutputId[] = ["takeoff", "boq", "conflicts", "review"];

export type OutputInfo = {
  id: OutputId;
  /** How many rows it holds; null when it has no count. */
  count: number | null;
};

/** How many outputs have something in them (the number beside "Outputs"). */
export function filledOutputs(outputs: OutputInfo[]): number {
  return outputs.filter((o) => (o.count ?? 0) > 0).length;
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
    { id: "review", count: null },
  ];
}
