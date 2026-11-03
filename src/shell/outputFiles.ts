import type { Transcript } from "../ai/messages";

export type OutputFile = { path: string; name: string };

/** Only completed tool results are evidence of a file being written, never prompts or inputs. */
export function transcriptFiles(transcript: Transcript): OutputFile[] {
  const paths = new Set<string>();
  function written(value: unknown) {
    if (typeof value === "string" && /\.(pdf|xlsx|dwg|dxf|svg|csv|txt|md|json|png|jpg|jpeg|webp|gif)$/i.test(value) && !/[\0\r\n]/.test(value)
      && (/^\//.test(value) || /^[A-Za-z]:[\\/]/.test(value) || /^\\\\[^\\]+\\/.test(value))) {
      paths.add(value);
    }
  }
  for (const item of transcript.items) {
    if (item.kind !== "tool" || item.status !== "done" || item.name !== "qiyas_execute") continue;
    try {
      const result: unknown = JSON.parse(item.summary);
      if (!result || typeof result !== "object" || Array.isArray(result) || !("written" in result)) continue;
      const value = result.written;
      if (Array.isArray(value)) value.forEach(written);
      else written(value);
    } catch { /* Older summaries contain no file result. */ }
  }
  return [...paths].map((path) => ({ path, name: path.split(/[\\/]/).pop()! }));
}
