import type { PlanPhase } from "../api";
import { useT } from "../i18n";
import "./plan.css";

const MARK = { pending: "○", active: "●", done: "✓", skipped: "–" };

/** The latest phase snapshot for one turn, kept above that turn's answer. */
export function PlanChecklist({ phases, onApprove, onChange }: {
  phases: PlanPhase[];
  onApprove?(): void;
  onChange?(): void;
}) {
  const t = useT();
  function title(phase: PlanPhase) {
    if (phase.id === "drawing-check") return t("ai.reviewDrawingErrors");
    if (phase.id === "drawing-check-review") {
      const details = phase.title.replace(/^Review drawing errors:\s*/, "");
      return `${t("ai.reviewDrawingErrors")}: ${details}`;
    }
    return phase.title;
  }
  return (
    <section className="mizan-plan" aria-label={t("ai.plan")}>
      <strong className="heading">{t("ai.plan")}</strong>
      <ol>
        {phases.map((phase) => (
          <li key={phase.id} data-status={phase.status} aria-current={phase.status === "active" ? "step" : undefined}>
            <span className="plan-mark" aria-hidden="true">{MARK[phase.status]}</span>
            <span className="plan-title" dir="auto">{title(phase)}</span>
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
