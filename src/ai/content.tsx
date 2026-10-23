import { Fragment, type ReactNode } from "react";
import type { Lang } from "../i18n";

/** Layer names and code do not decide the direction of a surrounding Arabic sentence. */
export function contentLanguage(text: string, fallback: Lang = "en"): Lang {
  const prose = text.replace(/\[PDF attachment:[^\]]*\]/g, "").replace(/`[^`]*`/g, "").replace(/\b[A-Z][A-Z0-9_-]*\b/g, "");
  const arabic = prose.match(/[\u0600-\u06ff\u0750-\u077f\u08a0-\u08ff]/g)?.length ?? 0;
  const latin = prose.match(/[a-z]/gi)?.length ?? 0;
  return arabic > 0 && arabic >= latin / 2 ? "ar" : latin > 0 ? "en" : fallback;
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
  return text.split(/([A-Za-z0-9][A-Za-z0-9_.:/\\×+−%²³-]*(?:[ \t]+[A-Za-z0-9][A-Za-z0-9_.:/\\×+−%²³-]*)*)/g)
    .map((part, i) => i % 2 ? <bdi key={i} dir="ltr">{part}</bdi> : <Fragment key={i}>{part}</Fragment>);
}
