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

test("Arabic ranges do not join separately isolated placeholders with a dash or slash", () => {
  expect(Object.entries(ar).filter(([, value]) => /}[-–/]\{/.test(value))).toEqual([]);
});

// Inspect complete ASCII syntax runs so punctuation cannot escape its LTR isolate.
function unisolatedSyntax(value: string): string[] {
  if (![...value].some(arabic)) return [];
  let plain = "", isolate = 0, nextIsolate = 0;
  const owners: number[] = [], closed = new Set<number>();
  for (const c of value.split("")) {
    if (c === "\u2066") isolate = ++nextIsolate;
    else if (c === "\u2069") { closed.add(isolate); isolate = 0; }
    else { plain += c; owners.push(isolate); }
  }
  return [...plain.matchAll(/[A-Za-z0-9_@/<.,|\[\]\-]*[A-Za-z0-9_@/<|\[\]\-]/g)]
    .filter(match => /[@/<]|\.[A-Za-z]/.test(match[0]) && /[A-Za-z]/.test(match[0]))
    .filter(match => {
      const owner = owners[match.index];
      return !owner || !closed.has(owner) ||
        owners.slice(match.index, match.index + match[0].length).some(id => id !== owner);
    }).map(match => match[0]);
}

test("Arabic command syntax stays inside LRI and PDI", () => {
  expect(Object.entries(ar).flatMap(([key, value]) =>
    unisolatedSyntax(value).map(run => `${key}: ${run}`))).toEqual([]);
  for (const run of ["@dx,dy", "@distance<angle", "/login", ".bak", "path/file", "file.dxf"]) {
    expect(unisolatedSyntax(`اكتب ${run}`)).toEqual([run]);
    expect(unisolatedSyntax(`اكتب \u2066${run}\u2069`)).toEqual([]);
  }
  for (const value of ["اكتب @\u2066dx,dy\u2069", "اكتب \u2066/login", "اكتب \u2066@dx\u2069,dy"]) {
    expect(unisolatedSyntax(value)).not.toEqual([]);
  }
  expect(ar["cmd.badPoint"]).toContain("\u2066@distance<angle\u2069");
});

test("Arabic action buttons use verbal nouns", () => {
  const labels = {
    "ai.send": "إرسال",
    "ai.newConversation": "محادثة جديدة",
    "text.place": "وضع النص",
    "inquiry.place": "وضع النقاط",
    "ai.markArea": "تحديد منطقة لميزان",
    "attachPdf.noFile": "لم يحدد ملف",
  };
  for (const [key, value] of Object.entries(labels)) expect(ar[key], key).toBe(value);
});


const englishBanned = [...new Set(Object.values(glossary.concepts)
  .flatMap(c => "en_avoid" in c ? c.en_avoid : []))];
function englishErrors(value: string): string[] {
  // Substitution names are wire names, not displayed prose.
  const prose = value.replace(/\{\w+\}/g, "");
  return englishBanned.filter(term => new RegExp(
    `\\b${term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "i",
  ).test(prose));
}

test("English window text follows each concept's en_avoid terms", () => {
  expect(Object.entries(en).flatMap(([key, value]) =>
    englishErrors(value).map(term => `${key}: ${term} (${value})`))).toEqual([]);
});

test("English terms match whole words without changing identifiers or placeholders", () => {
  for (const value of ["Conflicts", "CONFLICT", "takeoff", "Take off quantities", "BOQ row"]) {
    expect(englishErrors(value), value).not.toEqual([]);
  }
  for (const value of ["Discrepancies", "entity", "Pick an object", "Select objects", "Object snap", "UCS from object", "take-off", "takeoff_check", "{object}", "conflicting"]) {
    expect(englishErrors(value), value).toEqual([]);
  }
});

test("Arabic drawing revisions and named parameters retain their distinct meanings", () => {
  const revision = glossary.concepts.revision.preferred;
  const parameter = glossary.concepts.parameter.preferred;
  expect(Object.entries(en).filter(([key, value]) => /\brevision\b|\brev\b/i.test(value) || /revcloud/i.test(key))
    .filter(([key]) => ar[key].includes(glossary.concepts.review.preferred))).toEqual([]);
  for (const key of ["draw.revcloud", "activity.line.object.revcloud.one", "activity.line.object.revcloud.many", "ai.packCheck.register"]) {
    expect(ar[key], key).toContain(revision);
  }
  for (const key of ["param.parameters", "param.manager", "activity.line.object.parameter.one", "activity.line.object.parameter.many"]) {
    expect(ar[key], key).toContain(parameter);
  }
  expect(ar["boq.measurementReview"]).toContain(glossary.concepts.review.preferred);
  expect(ar["modify.prompt.factor"]).toContain(glossary.concepts.factor.preferred);
});
