import { plainDashes } from "./content";
import { withoutBody } from "../shell/methodName";
import type { PlanPhase } from "../api";
import { translate, useT, type Lang } from "../i18n";
import "./plan.css";
import { DotLoader } from "../shell/DotLoader";

const MARK = { pending: "○", active: "●", done: "✓", skipped: "-" };

export function planPhaseTitle(phase: PlanPhase, t: (key: string) => string) {
  if (phase.id === "drawing-check" || phase.id === "drawing-check-review") return t("ai.reviewDrawingErrors");
  return plainDashes(withoutBody(phase.title));
}

/** The latest phase snapshot for one turn, kept above that turn's answer. */
export function PlanChecklist({ id, phases, stale = false, announceStale = true, busy = false, onApprove, onChange, language, showTechnicalDetails = false }: {
  id?: string;
  phases: PlanPhase[];
  stale?: boolean;
  announceStale?: boolean;
  busy?: boolean;
  language?: Lang;
  showTechnicalDetails?: boolean;
  onApprove?(): void;
  onChange?(): void;
}) {
  const uiT = useT();
  const t = language ? (key: string) => translate(language, key) : uiT;
  return (
    <section id={id} className="mizan-plan" aria-label={t("ai.plan")}>
      <strong className="heading">{t("ai.plan")}</strong>
      {stale && <p role={announceStale ? "status" : undefined}>{t("ai.plan.stale")}</p>}
      <ol>
        {phases.map((phase) => (
          <li key={phase.id} data-status={phase.status} aria-current={phase.status === "active" ? "step" : undefined}>
            <span className="plan-mark" aria-hidden="true">{busy && phase.status === "active" ? <DotLoader size="sm" /> : MARK[phase.status]}</span>
            <span className="plan-title" dir="auto">{planPhaseTitle(phase, t)}</span>
            {showTechnicalDetails && phase.id === "drawing-check-review" && <details className="technical-details"><summary>{t("technical.details")}</summary><pre>{plainDashes(phase.title)}</pre></details>}
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
