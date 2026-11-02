/** A provider pause keeps the drawing and offers only the actions supplied by Qiyas. */
import { useState } from "react";
import { subscriptionLink } from "../api";
import { translate, type Lang } from "../i18n";
import { isolateRuns } from "./content";
import { providerFailure, type FailureAction } from "./providerFailure";
import "./ProviderFailureCard.css";

export function ProviderFailureCard({ failure, lang, busy, onRetry, onSettings, role = "note" }: {
  failure: NonNullable<ReturnType<typeof providerFailure>>;
  lang: Lang;
  busy: boolean;
  onRetry(): void;
  onSettings(): void;
  role?: "note" | "alert";
}) {
  const [linkFailed, setLinkFailed] = useState(false);
  const t = (key: string) => translate(lang, key);
  const label = (action: FailureAction) => {
    if (failure.legacy && failure.canContinue && action.kind === "retry" && !["plan_limit", "spend_blocked", "out_of_credit"].includes(failure.kind)) return t("ai.continueWorkButton");
    if (failure.legacy && action.kind === "open_settings") return t("settings");
    if (action.kind === "open_provider_page") return t(action.page.endsWith("billing") ? "ai.failure.billing" : "ai.failure.usage");
    return t(action.kind === "retry" ? "ai.failure.retry" : action.kind === "switch_provider" ? "ai.failure.switchProvider" : "ai.failure.settings");
  };
  const run = async (action: FailureAction) => {
    if (action.kind === "retry") onRetry();
    else if (action.kind === "open_provider_page") {
      setLinkFailed(false);
      try { await subscriptionLink(action.page); } catch { setLinkFailed(true); }
    } else onSettings();
  };
  return <div className="msg notice stopped provider-failure" role={role} lang={lang} dir={lang === "ar" ? "rtl" : "ltr"}>
    <p>{isolateRuns(failure.text)}</p>
    <div className="provider-failure-actions">
      {failure.actions.map((action, index) => <button key={index} type="button" disabled={busy && action.kind === "retry"}
        onClick={() => void run(action)}>{label(action)}</button>)}
    </div>
    {linkFailed && <p role="status">{t("ai.failure.linkFailed")}</p>}
  </div>;
}
