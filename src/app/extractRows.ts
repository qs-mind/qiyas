import type { EntityFilter, Field, Outcome } from "../api";

/** Read every page. A failed page rejects the whole extraction, so partial rows cannot export. */
export async function extractAll(
  filters: EntityFilter[], fields: Field[],
  read: (filter: EntityFilter, fields: Field[]) => Promise<Outcome>,
  progress?: (rows: number) => void,
): Promise<string[][]> {
  const rows: string[][] = [];
  for (const filter of filters) {
    const limit = Math.min(filter.limit ?? 5000, 5000);
    if (limit <= 0) throw new Error("Extraction page size must be positive");
    let offset = 0;
    for (;;) {
      const page = await read({ ...filter, limit, offset }, fields);
      if (page.type !== "table") throw new Error("Extraction did not return a table");
      rows.push(...page.rows);
      progress?.(rows.length);
      offset += page.rows.length;
      if (page.rows.length < limit) break;
    }
  }
  return rows;
}
