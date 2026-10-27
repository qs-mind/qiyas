/** The name Save As offers for a drawing that has none (founder's note 68, 26 October 2026): never
 *  the placeholder "Untitled-5", but the project or title written in the title block, else the first
 *  plain text in the drawing (a room name), else "Qiyas drawing <date>". */
import { runCommand } from "../api";

export type NameText = { sheet: string; layer: string; text: string; y: number };

/** A line of a title block that is not a name: a number, a scale ("1:100"), a sheet number ("A-101",
 *  "1 of 10"), a date, an area, or the issue status Mizan writes ("For review"). */
const NOT_A_NAME = [
  /^[\s\d.,:;/\\×x%+\-–—_()[\]m²³]*$/i,
  /^[A-Za-z]{1,3}[-.\s]?\d[\w.\-]*$/,
  /^\d+\s*(of|\/|من)\s*\d+$/i,
  /\b\d{1,4}[-/.]\d{1,2}[-/.]\d{1,4}\b/,
  /^(area|المساحة)\b/i,
  /(for review|not for construction|draft|issued? for|approved|للمراجعة|ليس للتنفيذ|مسودة)/i,
];

/** A name for a file from `text`: no characters a file system refuses, one line, 60 characters at
 *  most, no leading dot. Empty when nothing is left. */
export function fileNameFrom(text: string): string {
  const cleaned = text
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001f\\/:*?"<>|]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^\.+/, "")
    .replace(/[. ]+$/, "");
  return cleaned.length > 60 ? cleaned.slice(0, 60).trim() : cleaned;
}

const letters = (s: string) => (s.match(/\p{L}/gu) ?? []).length;
const isName = (s: string) => letters(s) >= 3 && !NOT_A_NAME.some((pattern) => pattern.test(s.trim()));

/** The name to offer: the project or title in the title block, else the first text of the drawing
 *  that reads as a name, else `fallback`. The Title block command writes the title first and the
 *  project under it, so a sheet's block gives its second line (its only one when there is one); a
 *  drawing of several layouts repeats its project on each, so the line most sheets agree on wins. */
export function suggestDrawingName(texts: NameText[], fallback: string): string {
  const votes = new Map<string, number>();
  const sheets = [...new Set(texts.map((t) => t.sheet))];
  for (const sheet of sheets) {
    const block = texts
      .filter((t) => t.sheet === sheet && /TTLB|TITLE/i.test(t.layer) && isName(t.text))
      .sort((a, b) => b.y - a.y)
      .map((t) => fileNameFrom(t.text))
      .filter(Boolean);
    if (block.length > 0) {
      const line = block[Math.min(1, block.length - 1)];
      votes.set(line, (votes.get(line) ?? 0) + 1);
    }
  }
  let best: string | null = null;
  for (const [line, n] of votes) if (best === null || n > (votes.get(best) ?? 0)) best = line;
  if (best) return best;
  for (const t of texts) {
    const name = isName(t.text) ? fileNameFrom(t.text) : "";
    if (name) return name;
  }
  return fallback;
}

/** The texts of the drawing's sheets (at most `limit` each), best effort: a failed read is no text. */
export async function readNameTexts(sheets: string[], limit = 200): Promise<NameText[]> {
  const found: NameText[] = [];
  for (const sheet of sheets.slice(0, 8)) {
    for (const kind of ["Text", "MText"]) {
      try {
        const out = await runCommand({ type: "extract_data", filter: { sheet, kind, limit }, fields: ["layer", "text", "y"] });
        if (out.type !== "table") continue;
        for (const [layer, text, y] of out.rows) found.push({ sheet, layer, text, y: Number(y) || 0 });
      } catch {
        // A drawing that cannot be read for names still saves under the date.
      }
    }
  }
  return found;
}
