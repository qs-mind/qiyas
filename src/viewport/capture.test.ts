import { describe, expect, it } from "vitest";
import { base64, fitSize, flipRows } from "./capture";

describe("capture", () => {
  it("turns WebGL's rows the right way up and makes every pixel opaque", () => {
    // Two rows of one pixel: bottom red, top blue, half transparent.
    const read = new Uint8Array([255, 0, 0, 128, 0, 0, 255, 128]);
    expect([...flipRows(read, 1, 2)]).toEqual([0, 0, 255, 255, 255, 0, 0, 255]);
  });

  it("fits the longer side and never grows a picture", () => {
    expect(fitSize(3200, 1600, 1600)).toEqual([1600, 800]);
    expect(fitSize(800, 600, 1600)).toEqual([800, 600]);
    expect(fitSize(1000, 4000, 1600)).toEqual([400, 1600]);
  });

  it("writes base64 of large pictures", () => {
    const bytes = new Uint8Array(100_000).map((_, i) => i % 256);
    const text = base64(bytes);
    expect(text.slice(0, 8)).toBe(btoa(String.fromCharCode(0, 1, 2, 3, 4, 5)).slice(0, 8));
    expect(atob(text).length).toBe(100_000);
  });
});
