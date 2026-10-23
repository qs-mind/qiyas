import { describe, expect, it } from "vitest";
import { extractAll } from "./extractRows";
describe("complete data extraction", () => {
  it("exports 5001 rows and reports both pages", async () => {
    const progress: number[] = [];
    const offsets: number[] = [];
    const rows = await extractAll([{}], ["handle"], async f => {
      offsets.push(f.offset ?? 0);
      return { type: "table", columns: ["handle"], rows: Array.from({ length: f.offset === 0 ? 5000 : 1 }, (_, i) => [String((f.offset ?? 0) + i)]) };
    }, n => progress.push(n));
    expect(rows).toHaveLength(5001);
    expect(rows[5000]).toEqual(["5000"]);
    expect(offsets).toEqual([0, 5000]); expect(progress).toEqual([5000, 5001]);
  });
  it("rejects a failed later page instead of returning a partial export", async () => {
    await expect(extractAll([{ limit: 1 }], ["handle"], async f => {
      if (f.offset) throw new Error("Page failed");
      return { type: "table", columns: [], rows: [["1"]] };
    })).rejects.toThrow("Page failed");
  });
});
