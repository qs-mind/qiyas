import { expect, test } from "vitest";
import { fileNameFrom, suggestDrawingName, type NameText } from "./saveName";

const FALLBACK = "Qiyas drawing 2026-10-27";
const at = (layer: string, text: string, y = 0, sheet = "*Model_Space"): NameText => ({ sheet, layer, text, y });

test("the project in the title block is the name, the title when there is no project", () => {
  // The Title block command writes the title first, the project under it.
  const both = [
    at("A-ANNO-TTLB", "Ground floor plan", 290),
    at("A-ANNO-TTLB", "Cairo Villa", 282),
    at("A-ANNO-TTLB", "A-101", 274),
    at("A-ANNO-TTLB", "1:100", 266),
    at("A-ANNO-TTLB", "2026-10-27", 258),
    at("A-ANNO-TEXT", "Majlis", 0),
  ];
  expect(suggestDrawingName(both, FALLBACK)).toBe("Cairo Villa");
  expect(suggestDrawingName([at("A-ANNO-TTLB", "Ground floor plan", 290), at("A-ANNO-TTLB", "A-101", 274)], FALLBACK)).toBe("Ground floor plan");
});

test("a title block line that is not a name is skipped: numbers, scales, dates, sheet numbers, status", () => {
  const texts = ["1:100", "A-101", "1 of 10", "27/10/2026", "FOR REVIEW", "للمراجعة", "Area 17.81 m²", "12.5", "KS"].map((t, i) => at("TTLB", t, 100 - i));
  expect(suggestDrawingName(texts, FALLBACK)).toBe(FALLBACK);
});

test("with no title block, the first text that reads as a name: a room name", () => {
  const texts = [at("0", "12.50"), at("A-AREA", "Area 28.00 m²"), at("A-ROOM", "Living room"), at("A-ROOM", "Kitchen")];
  expect(suggestDrawingName(texts, FALLBACK)).toBe("Living room");
  expect(suggestDrawingName([at("A-ROOM", "المجلس")], FALLBACK)).toBe("المجلس");
});

test("an empty drawing is named by the date, never Untitled", () => {
  expect(suggestDrawingName([], FALLBACK)).toBe(FALLBACK);
  expect(FALLBACK).not.toMatch(/untitled/i);
});

test("a name is safe for a file: no separators or reserved characters, one line, not too long, no leading dot", () => {
  expect(fileNameFrom('Villa: "A/B" <final>?')).toBe("Villa A B final");
  expect(fileNameFrom("line one\nline two")).toBe("line one line two");
  expect(fileNameFrom("..hidden. ")).toBe("hidden");
  expect(fileNameFrom("x".repeat(100)).length).toBe(60);
  expect(fileNameFrom('<>:"/\\|?*')).toBe("");
});

test("a drawing of several layouts repeats its project on each: the line most sheets agree on is the name", () => {
  const sheet = (block: string, title: string) => [
    at("A-ANNO-TTLB", title, 290, block),
    at("A-ANNO-TTLB", "Cairo Villa", 282, block),
  ];
  const texts = [...sheet("*Paper_Space", "Ground floor plan"), ...sheet("*Paper_Space0", "First floor plan"), ...sheet("*Paper_Space1", "Roof plan")];
  expect(suggestDrawingName(texts, FALLBACK)).toBe("Cairo Villa");
});
