import type { Transcript } from "../ai/messages";

export type OutputFile = { path: string; name: string };

/** Only completed tool results are evidence of a file being written, never prompts or inputs. */
export function transcriptFiles(transcript: Transcript): OutputFile[] {
  const paths = new Set<string>();
  function written(value: unknown) {
    if (typeof value === "string" && /\.(pdf|xlsx|xlsm|xls|csv|txt|md|json|png|jpg|jpeg|webp|gif)$/i.test(value) && !/[\0\r\n]/.test(value)
      && (/^\//.test(value) || /^[A-Za-z]:[\\/]/.test(value) || /^\\\\[^\\]+\\/.test(value))) {
      paths.add(value);
    } else if (Array.isArray(value)) value.forEach(written);
  }
  function visit(value: unknown) {
    if (!value || typeof value !== "object") return;
    if (Array.isArray(value)) value.forEach(visit);
    else for (const [key, child] of Object.entries(value)) {
      if (key === "written") written(child);
      else visit(child);
    }
  }
  for (const item of transcript.items) {
    if (item.kind !== "tool" || item.status !== "done") continue;
    try { visit(JSON.parse(item.summary)); } catch { /* Older summaries contain no file result. */ }
  }
  return [...paths].map((path) => ({ path, name: path.split(/[\\/]/).pop()! }));
}
