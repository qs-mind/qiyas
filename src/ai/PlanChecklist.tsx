import { withoutBody } from "../shell/methodName";
import type { PlanPhase } from "../api";
import { translate, useT, type Lang } from "../i18n";
import "./plan.css";

const MARK = { pending: "○", active: "●", done: "✓", skipped: "–" };

/** The latest phase snapshot for one turn, kept above that turn's answer. */
export function PlanChecklist({ phases, stale = false, onApprove, onChange, language, showTechnicalDetails = false }: {
  phases: PlanPhase[];
  stale?: boolean;
  language?: Lang;
  showTechnicalDetails?: boolean;
  onApprove?(): void;
  onChange?(): void;
}) {
  const uiT = useT();
  const t = language ? (key: string) => translate(language, key) : uiT;
  function title(phase: PlanPhase) {
    if (phase.id === "drawing-check") return t("ai.reviewDrawingErrors");
    if (phase.id === "drawing-check-review") return t("ai.reviewDrawingErrors");
    return withoutBody(phase.title);
  }
  return (
    <section className="mizan-plan" aria-label={t("ai.plan")}>
      <strong className="heading">{t("ai.plan")}</strong>
      {stale && <p role="status">{t("ai.plan.stale")}</p>}
      <ol>
        {phases.map((phase) => (
          <li key={phase.id} data-status={phase.status} aria-current={phase.status === "active" ? "step" : undefined}>
            <span className="plan-mark" aria-hidden="true">{MARK[phase.status]}</span>
            <span className="plan-title" dir="auto">{title(phase)}</span>
            {showTechnicalDetails && phase.id === "drawing-check-review" && <details className="technical-details"><summary>{t("technical.details")}</summary><pre>{phase.title}</pre></details>}
            <span className="plan-status">{t(`ai.plan.${phase.status}`)}</span>
          </li>
        ))}
      </ol>
      {onApprove && <div className="plan-actions">
        <button type="button" onClick={onApprove}>{t("ai.approve")}</button>
        <button type="button" onClick={onChange}>{t("ai.change")}</button>
      </div>}
    </section>
  );
}
