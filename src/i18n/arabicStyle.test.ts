// @vitest-environment node
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import glossary from "./ar-glossary.json";

const read = (name: string): Record<string, string> =>
  JSON.parse(readFileSync(new URL(name, import.meta.url), "utf8"));
const ar = read("./ar.json");
const en = read("./en.json");
const inRanges = (c: string, ranges: number[][]) =>
  ranges.some(([a, b]) => c.codePointAt(0)! >= a && c.codePointAt(0)! <= b);
const arabic = (c: string) => inRanges(c, glossary.arabicLetterRanges);
const digitOrLatin = (c: string) => /[A-Za-z]/.test(c) || inRanges(c, glossary.digitRanges);
const wordChar = (c: string) => arabic(c) || digitOrLatin(c);
const placeholders = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
const banned = [...new Set([
  ...Object.values(glossary.concepts).flatMap(c => c.avoid),
  ...glossary.bannedPatterns.filter(p => p.kind === "word").map(p => p.value),
])];

function styleErrors(value: string): string[] {
  const errors: string[] = [];
  for (const term of banned) {
    for (const match of value.matchAll(new RegExp(term, "gu"))) {
      const i = match.index;
      if ((!i || !wordChar(value[i - 1])) &&
          (i + term.length === value.length || !wordChar(value[i + term.length]))) {
        errors.push(`banned term: ${term}`);
      }
    }
  }
  for (const c of value) {
    if (inRanges(c, glossary.diacriticRanges) &&
        !(glossary.allowedDiacritics as string[]).includes(c)) errors.push(`diacritic: ${c}`);
  }
  // Substitute a representative Latin/digit value to catch spaces missing at variable edges.
  const rendered = value.replace(/\{\w+\}/g, "A1");
  const chars = [...rendered];
  for (let i = 1; i < chars.length; i++) {
    const a = chars[i - 1], b = chars[i];
    if ((arabic(a) && digitOrLatin(b)) || (digitOrLatin(a) && arabic(b))) {
      errors.push(`missing space: ${a}${b}`);
    }
  }
  return errors;
}

function translationErrors(value: string, english: string): string[] {
  const errors = styleErrors(value);
  if (!value.trim()) errors.push("empty translation");
  if (JSON.stringify(placeholders(value)) !== JSON.stringify(placeholders(english))) {
    errors.push("placeholder mismatch");
  }
  if (![...value.replace(/\{\w+\}/g, "")].some(arabic) &&
      !Object.hasOwn(glossary.allowedLatinValues, value) &&
      !Object.hasOwn(glossary.allowedTemplateValues, value)) errors.push("untranslated value");
  return errors;
}

test("Arabic follows the shared glossary and preserves every English key and placeholder", () => {
  expect(Object.keys(ar)).toEqual(Object.keys(en));
  const errors = Object.entries(ar).flatMap(([key, value]) =>
    translationErrors(value, en[key]).map(error => `${key}: ${error} (${value})`));
  expect(errors).toEqual([]);
});

test("the style guard rejects the founder's regressions and accepts engineering notation", () => {
  for (const value of glossary.examples.valid) expect(styleErrors(value), value).toEqual([]);
  for (const value of glossary.examples.invalid) expect(styleErrors(value), value).not.toEqual([]);
  for (const [value, english] of [["", "Open"], ["Open drawing", "Open drawing"],
    ["عدد البنود: {count}", "{n} items"], ["{n} من {n}", "{n} of {total}"]]) {
    expect(translationErrors(value, english), value).not.toEqual([]);
  }
  expect(translationErrors("PDF export", "PDF export")).toContain("untranslated value");
  expect(translationErrors("Claude Code", "Claude Code")).toEqual([]);
  expect(translationErrors("PDF", "PDF")).toEqual([]);
});

test("count labels remain grammatical with unrestricted counts and substituted names", () => {
  for (const n of [0, 1, 2, 3, 10, 11, 19, 100, 102]) {
    for (const key of ["outputs.items", "layer.count", "ai.pdfPageCount", "activity.line.rows"] as const) {
      expect(styleErrors(ar[key].replace("{n}", String(n)).replace("{object}", "الجدول"))).toEqual([]);
    }
    expect(ar["outputs.items"].replace("{n}", String(n))).toBe(`عدد البنود: ${n}`);
  }
  expect(ar["activityFormat.numbered"]).toBe("{what} (العدد: {n})");
});
