import { findingText, findingFix, type PackFindings } from "../findings";
import { useEffect, useState } from "react";
import { measurementPackCheck } from "../api";
import { useLanguage, useT } from "../i18n";
import { isWorkspaceBusy, uiError } from "../i18n/errors";
import { DotLoader } from "../shell/DotLoader";
import { isolateRuns } from "./content";
import "./packCheck.css";

/** A live read of the drawing pack, before any take-off is requested. */
export function PackCheckCard({ drawing, workspace, revision, busy = false }: {
  drawing: string;
  workspace?: number | null;
  revision: unknown;
  busy?: boolean;
}) {
  const t = useT();
  const { lang } = useLanguage();
  const [card, setCard] = useState<PackFindings | null>(null);
  const [error, setError] = useState<{ cause: unknown } | null>(null);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    let gone = false;
    setCard(null);
    setError(null);
    // PDF checks take the session lock. Read once the turn ends, not after each AI edit.
    if (busy) return;
    measurementPackCheck(workspace).then((result) => {
      if (!gone) setCard(result);
    }).catch((cause: unknown) => {
      if (!gone && !isWorkspaceBusy(cause)) setError({ cause });
    });
    return () => { gone = true; };
  }, [drawing, workspace, revision, busy, reload]);

  const loading = !card && !error;
  return <section className="pack-check" aria-label={t("ai.packCheck.title")} dir={lang === "ar" ? "rtl" : "ltr"}>
    <header>
      <strong className="heading">{t("ai.packCheck.title")}</strong>
      <button type="button" onClick={() => setReload((n) => n + 1)} disabled={busy || loading}>
        {t("ai.packCheck.refresh")}
      </button>
    </header>
    {busy ? <p role="status">{t("ai.packCheck.waiting")}</p>
      : loading ? <p className="pack-check-loading" role="status"><DotLoader size="sm" pattern="scan" />{t("ai.packCheck.loading")}</p>
      : error ? <p role="alert">{uiError(error.cause, lang)}</p>
      : card && <>
        <p className="pack-check-summary" role="status">{t(card.ready ? "ai.packCheck.ready" : "ai.packCheck.fix")}</p>
        <ul>
          {card.findings.map((line) => {
            const labelKey = `ai.packCheck.${line.rule}`;
            const label = t(labelKey);
            const found = findingText(line, lang);
            const fix = findingFix(line, lang);
            return <li key={line.id} data-passes={line.passed}>
              <span className="pack-check-mark" aria-hidden="true">{line.passed ? "✓" : "!"}</span>
              <span className="pack-check-text">
                <span className="sr-only">{t(line.passed ? "ai.packCheck.passed" : "ai.packCheck.needsFix")}{": "}</span>
                {label !== labelKey && <strong>{label}{": "}</strong>}
                {isolateRuns(found)}
                {!line.passed && fix && <> <span className="pack-check-fix">{isolateRuns(fix)}</span></>}
              </span>
            </li>;
          })}
        </ul>
      </>}
  </section>;
}
