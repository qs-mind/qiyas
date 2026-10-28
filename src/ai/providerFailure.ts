import { translate, type Lang } from "../i18n";
import en from "../i18n/en.json";

const setup = new Set([
  "auth", "billing", "model_not_found", "model_capability", "output_limit", "no_models",
  "no_api_key", "no_base_url", "no_model",
  "chatgpt_denied", "chatgpt_auth", "chatgpt_scope", "chatgpt_oauth", "chatgpt_keychain",
  "chatgpt_cancelled", "chatgpt_model", "chatgpt_setup", "chatgpt_access", "chatgpt_policy",
  "chatgpt_ineligible", "chatgpt_capability", "chatgpt_reauthorize", "chatgpt_account_mismatch",
  "chatgpt_revocation_unconfirmed", "claude_missing", "claude_auth", "claude_billing",
  "claude_hold", "claude_flags", "claude_approval", "claude_work_directory", "claude_platform",
]);

/** Old transcripts use bare codes; IPC uses a JSON envelope. Neither exposes diagnostics. */
export function providerFailure(error: string, lang: Lang) {
  let code = error;
  let reset: unknown;
  try {
    const value: unknown = JSON.parse(error);
    if (typeof value === "string") code = value;
    else if (value && typeof value === "object" && "code" in value && typeof value.code === "string") {
      code = value.code;
      const fields = value as Record<string, unknown>;
      reset = fields.resets_at ?? fields.reset_at;
    }
  } catch { /* Bare code from a saved stop reason. */ }
  const name = code.replace(/^ai\.error\./, "");
  const key = `ai.error.${name}`;
  if (!(key in en) && !code.startsWith("ai.error.")) return null;
  let text = translate(lang, key in en ? key : "ai.error.service_error");
  if (["chatgpt_limit", "claude_limit", "rate_limited"].includes(name) && reset != null) {
    // Accept an explicit timestamp only, never provider diagnostic text or a relative guess.
    const date = typeof reset === "number" ? new Date(reset < 1e12 ? reset * 1000 : reset)
      : typeof reset === "string" && /^\d{4}-\d\d-\d\dT/.test(reset) ? new Date(reset) : null;
    if (date && Number.isFinite(date.getTime())) {
      text = translate(lang, "ai.error.limitReset", { time: date.toLocaleString(lang) });
    }
  }
  return { text, canContinue: !setup.has(name) };
}
