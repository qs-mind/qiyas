import type { PlanPhase } from "../api";
import { useT } from "../i18n";
import "./plan.css";

const MARK = { pending: "○", active: "●", done: "✓", skipped: "–" };

/** The latest phase snapshot for one turn, kept above that turn's answer. */
export function PlanChecklist({ phases }: { phases: PlanPhase[] }) {
  const t = useT();
  return (
    <section className="mizan-plan" aria-label={t("ai.plan")}>
      <strong className="heading">{t("ai.plan")}</strong>
      <ol>
        {phases.map((phase) => (
          <li key={phase.id} data-status={phase.status} aria-current={phase.status === "active" ? "step" : undefined}>
            <span className="plan-mark" aria-hidden="true">{MARK[phase.status]}</span>
            <span className="plan-title" dir="auto">{phase.title}</span>
            <span className="plan-status">{t(`ai.plan.${phase.status}`)}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}
