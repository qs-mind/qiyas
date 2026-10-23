import { translate, type Lang } from ".";
import en from "./en.json";
import ar from "./ar.json";

// Keep diagnostic text out of panels. Support can read the latest failures in Settings.
const failures: string[] = [];
export const uiErrorLog = () => failures.join("\n\n");
export class UserFacingError extends Error { toString() { return this.message; } }
/** Preserve messages already translated by the window when they cross another catch. */
export const userError = (message: string) => new UserFacingError(message);
const known: [RegExp, string][] = [
  [/could not be read|can.?t (?:be opened|read)|unreadable|bad header|not a .*file/i, "err.open"],
  [/api key.*refused|unauthori[sz]ed|authentication/i, "err.auth"],
  [/signature.*match/i, "err.signature"],
  [/PDF support is not installed/i, "err.pdf"],
  [/Enter a number|no scale|0 to 100/i, "err.input"],
  [/no_api_key/, "ai.noKey"], [/no_base_url/, "ai.noBaseUrl"], [/no_model/, "ai.noModel"],
  [/^busy$|already running|in progress/i, "err.busy"],
  [/no drawing|no document|drawing.*not open/i, "err.noDrawing"],
  [/permission denied|access denied|read.only|locked/i, "err.locked"],
  [/not found|does not exist|no such|missing (entity|layer|file)/i, "err.missing"],
  [/invalid|expected|must be|out of (range|bounds)|cannot be empty|required/i, "err.input"],
  [/unsupported|not supported|cannot edit|generated|read.only/i, "err.unsupported"],
  [/nothing to undo|nothing to redo|no undo|no redo/i, "err.history"],
  [/cancelled|canceled/i, "ai.cancelled"],
  [/timed? ?out|timeout/i, "err.timeout"],
  [/network|connect|offline/i, "err.connection"],
  [/disk|space left|write|save/i, "err.save"],
];

/** The boundary for caught backend errors, including those with no structured code yet. */
export function uiError(error: unknown, lang: Lang = typeof document !== "undefined" && document.documentElement.lang === "ar" ? "ar" : "en"): string {
  if (error instanceof UserFacingError) return error.message;
  let raw: string;
  try {
    raw = typeof error === "object" && error !== null && !(error instanceof Error)
      ? JSON.stringify(error) : String(error);
  } catch {
    raw = String(error);
  }
  // Stable provider codes take precedence over the legacy diagnostic text patterns.
  try {
    const value = JSON.parse(raw);
    // SIWC eligibility comes from OpenAI; its pages list different eligible plans.
    if (value?.code === "ai.error.chatgpt_denied" && typeof value.message === "string") {
      return value.message;
    }
    if (typeof value?.code === "string" && value.code.startsWith("ai.error.") && value.code in en) {
      return translate(lang, value.code);
    }
  } catch { /* Older errors are plain strings. */ }
  if (raw.startsWith("ai.error.") && raw in en) return translate(lang, raw);
  // Already translated validation errors can pass through a caller's catch unchanged.
  if ([...Object.values(en), ...Object.values(ar)].includes(raw)) return raw;
  if (failures.at(-1) !== raw) failures.push(raw);
  if (failures.length > 50) failures.shift();
  const key = known.find(([pattern]) => pattern.test(raw))?.[1] ?? "err.unknown";
  return translate(lang, key);
}
