import type { ProviderFailureKind as FailureKind, PublicProvider as FailureProvider, ProviderPage } from "../api";
import { locale } from "../i18n/locale";
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

export type FailureAction = { kind: "retry" | "open_settings" | "switch_provider" } | { kind: "open_provider_page"; page: ProviderPage };
const kinds = new Set<FailureKind>(["plan_limit", "spend_blocked", "out_of_credit", "rate_limited", "auth", "model_unavailable", "temporary", "offline", "context_too_long", "policy", "other"]);
const providers = new Set<FailureProvider>(["chatgpt_plan", "claude_code", "openai", "anthropic", "other"]);
const pages = new Set<ProviderPage>(["chatgpt_usage", "claude_usage", "openai_billing", "anthropic_billing"]);

function legacyKind(name: string): FailureKind {
  if (["chatgpt_limit", "claude_limit", "plan_limit"].includes(name)) return "plan_limit";
  if (["spend_blocked", "chatgpt_spend_blocked", "claude_billing"].includes(name)) return "spend_blocked";
  if (["billing", "out_of_credit"].includes(name)) return "out_of_credit";
  if (["rate_limited", "chatgpt_rate_limited"].includes(name)) return "rate_limited";
  if (["auth", "no_api_key", "chatgpt_auth", "chatgpt_access", "chatgpt_reauthorize", "chatgpt_scope", "chatgpt_denied", "chatgpt_account_mismatch", "claude_auth"].includes(name)) return "auth";
  if (["model_not_found", "model_capability", "no_model", "no_models", "chatgpt_model", "chatgpt_capability"].includes(name)) return "model_unavailable";
  if (["timeout", "stream", "service_error", "temporary", "chatgpt_temporary", "chatgpt_incomplete", "chatgpt_empty", "claude_stream"].includes(name)) return "temporary";
  if (["network", "offline", "chatgpt_offline"].includes(name)) return "offline";
  if (name === "context_too_long") return "context_too_long";
  if (["content_filter", "policy", "chatgpt_policy", "claude_hold"].includes(name)) return "policy";
  return "other";
}

function defaultActions(kind: FailureKind, provider: FailureProvider): FailureAction[] {
  const actions: FailureAction[] = [{ kind: "retry" }];
  if (["plan_limit", "spend_blocked", "out_of_credit"].includes(kind)) {
    const page: ProviderPage | undefined = provider === "chatgpt_plan" ? "chatgpt_usage" : provider === "claude_code" ? "claude_usage"
      : kind === "plan_limit" ? undefined : provider === "openai" ? "openai_billing" : provider === "anthropic" ? "anthropic_billing" : undefined;
    if (page) actions.push({ kind: "open_provider_page", page });
  }
  if (["auth", "model_unavailable", "out_of_credit", "other"].includes(kind)) actions.push({ kind: "open_settings" });
  actions.push({ kind: "switch_provider" });
  return actions;
}

function safeActions(value: unknown): FailureAction[] {
  if (!Array.isArray(value)) return [];
  const actions: FailureAction[] = [];
  for (const action of value) {
    if (!action || typeof action !== "object") continue;
    if (["retry", "open_settings", "switch_provider"].includes(action.kind)) actions.push({ kind: action.kind });
    else if (action.kind === "open_provider_page" && pages.has(action.page)) actions.push({ kind: action.kind, page: action.page });
  }
  return actions.filter((action, index) => actions.findIndex(other => JSON.stringify(other) === JSON.stringify(action)) === index);
}

function resetTime(reset: unknown, lang: Lang): string | null {
  // Accept explicit timestamps only; provider text must never become visible instructions.
  const date = typeof reset === "number" ? new Date(reset < 1e12 ? reset * 1000 : reset)
    : typeof reset === "string" && /^\d{4}-\d\d-\d\dT/.test(reset) ? new Date(reset) : null;
  return date && Number.isFinite(date.getTime()) ? date.toLocaleString(locale(lang)) : null;
}

/** Old transcripts use bare codes; IPC uses a JSON envelope. Neither exposes diagnostics. */
export function providerFailure(error: string, lang: Lang) {
  let code = error;
  let fields: Record<string, unknown> = {};
  try {
    const value: unknown = JSON.parse(error);
    if (typeof value === "string") code = value;
    else if (value && typeof value === "object" && "code" in value && typeof value.code === "string") {
      code = value.code;
      fields = value as Record<string, unknown>;
    }
  } catch { /* Bare code from a saved stop reason. */ }
  const name = code.replace(/^ai\.error\./, "");
  const key = `ai.error.${name}`;
  const structured = kinds.has(fields.kind as FailureKind);
  if (!(key in en) && !code.startsWith("ai.error.") && !structured) return null;
  const kind = structured ? fields.kind as FailureKind : legacyKind(name);
  const provider = providers.has(fields.provider as FailureProvider) ? fields.provider as FailureProvider
    : name.startsWith("chatgpt_") ? "chatgpt_plan" : name.startsWith("claude_") ? "claude_code" : "other";
  const canContinue = !setup.has(name);
  const reset = resetTime(fields.resets_at ?? fields.reset_at, lang);
  const actions = Array.isArray(fields.actions) ? safeActions(fields.actions) : defaultActions(kind, provider);
  let text = translate(lang, key in en ? key : "ai.error.service_error");
  if (structured || ["plan_limit", "spend_blocked", "out_of_credit", "rate_limited"].includes(kind)) {
    const specific = ["plan_limit", "spend_blocked"].includes(kind) && ["chatgpt_plan", "claude_code"].includes(provider);
    // Keep useful code-specific setup and timeout guidance; only usage categories replace it.
    if (!(key in en) || ["plan_limit", "spend_blocked", "out_of_credit", "rate_limited", "context_too_long"].includes(kind)) {
      text = translate(lang, `ai.failure.${specific ? `${provider}.${kind}` : kind}`);
    }
    if (reset && kind === "plan_limit" && provider === "chatgpt_plan") {
      text = translate(lang, "ai.failure.chatgpt_plan.plan_limit_until", { time: reset });
    } else if (reset && ["plan_limit", "spend_blocked", "rate_limited"].includes(kind)) {
      text += " " + translate(lang, "ai.failure.resetsAt", { time: reset });
    }
    if (typeof fields.retry_after === "number" && Number.isFinite(fields.retry_after) && fields.retry_after > 0 && ["rate_limited", "temporary"].includes(kind)) {
      text += " " + translate(lang, "ai.failure.retryAfter", { seconds: Math.ceil(fields.retry_after) });
    }
  }
  return { text, canContinue, kind, provider, actions, legacy: !structured };
}
