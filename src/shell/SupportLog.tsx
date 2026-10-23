import { useState } from "react";
import { aiLog } from "../api";
import { useT } from "../i18n";
import { uiError, uiErrorLog } from "../i18n/errors";

/** Read on demand: diagnostics do not take over the conversation. */
export function SupportLog() {
  const t = useT();
  const [text, setText] = useState("");
  async function read() {
    try { setText([(await aiLog()).text, uiErrorLog()].filter(Boolean).join("\n\n")); }
    catch (error) { setText([uiError(error), uiErrorLog()].join("\n\n")); }
  }
  return <details className="technical-details support-log" onToggle={(e) => { if (e.currentTarget.open) void read(); }}>
    <summary>{t("support.details")}</summary>
    <button type="button" onClick={() => void read()}>{t("ai.logRefresh")}</button>
    <pre dir="ltr" tabIndex={0} aria-label={t("ai.log")}>{text || t("ai.logEmpty")}</pre>
  </details>;
}
