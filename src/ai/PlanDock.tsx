import { useEffect, useId, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { translate, type Lang } from "../i18n";
import { DotLoader } from "../shell/DotLoader";
import { PlanChecklist, planPhaseTitle } from "./PlanChecklist";
import type { Item } from "./messages";
import "./plan.css";

type Plan = Extract<Item, { kind: "plan" }>;
const STORAGE_KEY = "qiyas.planDock.expanded";
const complete = (plan: Plan) => plan.phases.every((phase) => phase.status === "done" || phase.status === "skipped");

/** Keep unfinished work at hand across replies; a completed turn leaves when the next starts. */
export function latestDockPlan(items: Item[]) {
  const index = items.map((item) => item.kind).lastIndexOf("plan");
  const plan = items[index];
  if (plan?.kind !== "plan" || plan.phases.length < 2) return null;
  const answered = items.slice(index + 1).some((item) => item.kind === "user");
  if (answered && complete(plan)) return null;
  return { index, plan, awaitingApproval: !!plan.awaitingApproval && !answered };
}

type Props = {
  plan: Plan | null;
  busy: boolean;
  language: Lang;
  threadItemId: string;
  awaitingApproval?: boolean;
  showTechnicalDetails?: boolean;
  onApprove?(): void;
  onChange?(): void;
  onShowInChat?(): void;
};

export function PlanDock(props: Props) {
  if (!props.plan || props.plan.phases.length < 2) return null;
  return <Dock {...props} plan={props.plan} />;
}

function Dock({ plan, busy, language, threadItemId, awaitingApproval = plan.awaitingApproval,
  showTechnicalDetails, onApprove, onChange, onShowInChat }: Props & { plan: Plan }) {
  const t = (key: string, vars?: Record<string, string | number>) => translate(language, key, vars);
  const done = complete(plan) && !busy;
  const [expanded, setExpanded] = useState(() => {
    if (awaitingApproval) return true;
    if (done) return false;
    try { return localStorage.getItem(STORAGE_KEY) === "true"; } catch { return false; }
  });
  const button = useRef<HTMLButtonElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const bodyId = useId();
  const active = plan.phases.find((phase) => phase.status === "active");
  const count = plan.phases.filter((phase) => phase.status === "done" || phase.status === "skipped").length;
  // Explicit Arabic numerals for this compact count; no grouping in a phase count.
  const digits = new Intl.NumberFormat(language === "ar" ? "ar-EG" : "en", { useGrouping: false });

  useEffect(() => {
    if (awaitingApproval) setExpanded(true);
    else if (done) {
      if (body.current?.contains(document.activeElement)) button.current?.focus();
      setExpanded(false);
    }
  }, [awaitingApproval, done, threadItemId]);

  useEffect(() => {
    try { localStorage.setItem(STORAGE_KEY, String(expanded)); } catch { /* Storage may be blocked. */ }
  }, [expanded]);

  useEffect(() => {
    const scroll = body.current;
    const step = scroll?.querySelector<HTMLElement>('[aria-current="step"]');
    if (!expanded || !scroll || !step) return;
    // Move only the dock's scroll box, leaving the thread and the panel in place.
    scroll.scrollTop += step.getBoundingClientRect().top - scroll.getBoundingClientRect().top
      - (scroll.clientHeight - step.offsetHeight) / 2;
  }, [expanded, active?.id, threadItemId]);

  function toggle(next: boolean) {
    setExpanded(next);
    button.current?.focus();
  }

  return <div className="plan-dock" lang={language} dir={language === "ar" ? "rtl" : "ltr"}
    onKeyDown={(event) => {
      if (event.key === "Escape" && expanded) {
        event.preventDefault();
        event.stopPropagation();
        toggle(false);
      }
    }}>
    <button type="button" ref={button} className="plan-dock-toggle" aria-expanded={expanded} aria-controls={bodyId}
      onClick={() => toggle(!expanded)}>
      <span className="plan-mark" aria-hidden="true">{done ? "✓" : "☷"}</span>
      <span className="heading plan-dock-count">{t(done ? "ai.planDock.done" : "ai.planDock.count", {
        done: digits.format(count), total: digits.format(plan.phases.length),
      })}</span>
      {busy && active && <DotLoader size="sm" />}
      {active && <span className="plan-dock-title" dir="auto">{planPhaseTitle(active, t)}</span>}
      <span className="plan-dock-progress" aria-hidden="true"><span style={{ inlineSize: `${100 * count / plan.phases.length}%` }} /></span>
    </button>
    <div id={bodyId} ref={body} className="plan-dock-body" hidden={!expanded}>
      {expanded && <>
        <PlanChecklist phases={plan.phases} stale={plan.stale} announceStale={false} busy={busy}
          language={language} showTechnicalDetails={showTechnicalDetails} onApprove={onApprove} onChange={onChange} />
        <button type="button" className="link plan-dock-chat" onClick={() => {
          // Fold first, so the thread has its full height when the plan is scrolled to.
          flushSync(() => toggle(false));
          document.getElementById(threadItemId)?.scrollIntoView({ block: "nearest", behavior: "instant" });
          onShowInChat?.();
        }}>{t("ai.planDock.showInChat")}</button>
      </>}
    </div>
  </div>;
}

/** App facts are distinct from Mizan's words, in the language captured for that turn. */
export function StatusBlock({ item }: { item: Extract<Item, { kind: "status" }> }) {
  const { kind, items, language } = item.status;
  return <aside className="msg notice" role="status" lang={language} dir={language === "ar" ? "rtl" : "ltr"}
    style={{ fontSize: "0.85em", borderInlineStart: "2px solid currentColor", paddingInlineStart: "0.75em" }}>
    <strong>{translate(language, `ai.status.${kind}`)}</strong>
    <ul>{items.map((text, i) => <li key={i}>{text}</li>)}</ul>
  </aside>;
}
