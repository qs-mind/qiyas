/** Why Mizan stopped, or why a message could not go, in plain words. */
import { uiError } from "../i18n/errors";
import { translate, type Lang } from "../i18n";
import { providerFailure } from "./providerFailure";
import { TOO_LONG } from "./messages";

/** A failure Mizan can pick up again from where it was (a dropped connection): offer Continue. */
export function connectionLost(error: string): boolean {
  return providerFailure(error, "en")?.canContinue ?? false;
}

/** A turn's end that was not an answer. `lang` is the window's; `chatLang` the conversation's,
 *  for what Mizan itself decided. */
export function stopText(reason: string, lang: Lang, chatLang: Lang): string {
  const failure = providerFailure(reason, lang);
  if (failure) return failure.text;
  const t = (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars);
  if (reason === "cancelled") return t("ai.cancelled");
  if (reason === "repeated_ineffective_edit") return translate(chatLang, "ai.repeatedEdit");
  if (reason === "drawing_checks_unresolved") return translate(chatLang, "ai.unresolvedChecks");
  if (reason === TOO_LONG) return t("ai.tooLong");
  if (reason === "max_tokens") return t("ai.maxTokens");
  const steps = /^stopped after (\d+) steps$/.exec(reason);
  if (steps) return t("ai.steps", { n: steps[1] });
  return uiError(reason, lang);
}

/** A message that could not be sent or a turn that failed. */
export function errorText(error: string, lang: Lang): string {
  const failure = providerFailure(error, lang);
  if (failure) return failure.text;
  const t = (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars);
  const plain: Record<string, string> = {
    busy: "ai.busy", no_api_key: "ai.noKey", no_base_url: "ai.noBaseUrl", no_model: "ai.noModel",
    restarted: "ai.restarted", crashed: "ai.crashed",
  };
  if (plain[error]) return t(plain[error]);
  const missingModel = /^(.+) has no model (.+): choose one in Settings$/.exec(error);
  if (missingModel) return t("ai.modelNotFound", { service: missingModel[1], model: missingModel[2] });
  return uiError(error, lang);
}
