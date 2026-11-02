import { Fragment, type ReactNode } from "react";
import type { Lang } from "../i18n";

/** Layer names and code do not decide the direction of a surrounding Arabic sentence. */
export function contentLanguage(text: string, fallback: Lang = "en"): Lang {
  if (/(?=\p{Script_Extensions=Arabic})\p{Letter}/u.test(text)) return "ar";
  return /[a-z]/i.test(text) ? "en" : fallback;
}

/** Content direction does not depend on the first letter or the window language. */
export function readingDirection(text: string, fallback: Lang = "en") {
  const lang = contentLanguage(text, fallback);
  return { lang, dir: lang === "ar" ? "rtl" as const : "ltr" as const };
}

const PDF_MARKER = /\[PDF attachment:\s*("(?:\\.|[^"\\])*");\s*(\d+) pages; path: ("(?:\\.|[^"\\])*")\]/g;

/** Older conversations carry attachment transport metadata in their text. Render file chips. */
export function messageFiles(text: string): { name: string; pages: number }[] {
  return [...text.matchAll(PDF_MARKER)].map((match) => ({
    name: (JSON.parse(match[1]) as string).split(/[\\/]/).pop() ?? "PDF",
    pages: Number(match[2]),
  }));
}

/** Paths belong in support details; restored chats use the same display boundary. */
export function chatText(text: string): string {
  const fileName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
  return text.replace(PDF_MARKER, "").trim()
    .replace(/(["'`])((?:[A-Za-z]:[\\/]|\/(?!\/)|~[\\/])[^"'`]+)\1/g,
      (_, quote: string, path: string) => quote + fileName(path) + quote)
    .replace(/(?:(?<![\w:])[A-Za-z]:[\\/]|(?<![\w:/<])\/(?!\/)|~[\\/])[^\s"'<>`]+/g, fileName)
    .replace(/(?<![\w.:/<])(?:\.{1,2}[\\/]|\.?[A-Za-z_][\w.-]*[\\/])[^\s"'<>`]+\.[A-Za-z0-9]{1,8}/g, fileName);
}

/** Isolate Latin words, dimensions and numbers without changing their characters. */
export function isolateRuns(text: string): ReactNode {
  // Numeric separators and spaced operators join only digits; retain Latin/path runs.
  return text.split(/((?:[+\-−±(](?=\d)|[A-Za-z0-9])(?:(?<=\d)[,.](?=\d)|(?<=\d) ?[×x*/=+−-] ?(?=\d)|[A-Za-z0-9_.:/\\×+−%²³-]|[ \t]+(?:[+\-−±(](?=\d)|[A-Za-z0-9]))*)/g)
    .map((part, i) => i % 2 ? <bdi key={i} dir="ltr">{part}</bdi> : <Fragment key={i}>{part}</Fragment>);
}

/** Saved Mizan messages use the same plain dashes as new answers. */
export function plainDashes(text: string): string {
  return text.replace(/ [—–] /g, " - ").replace(/[—–―]/g, "-");
}
