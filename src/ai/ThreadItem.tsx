/** One entry of Mizan's thread: the user's message, Mizan's answer, its questions, plan and steps,
 *  and the notices between them. */
import type { ReactNode } from "react";
import { translate, useLanguage, useT, type Lang } from "../i18n";
import { chatText, contentLanguage, isolateRuns, messageFiles, plainDashes } from "./content";
import { connectionLost, stopText } from "./chatErrors";
import { Markdown } from "./Markdown";
import { PlanChecklist } from "./PlanChecklist";
import { QuestionList } from "./QuestionList";
import { ToolCard } from "./ToolCard";
import type { Item, ToolCardItem } from "./messages";
import { providerFailure } from "./providerFailure";

/** What every entry needs to know about the thread around it. */
export type ThreadContext = {
  items: Item[];
  busy: boolean;
  /** Busy, or the box can't send now (a history read, an attachment on its way). */
  unavailable: boolean;
  chatLang: Lang;
  showTechnicalDetails: boolean;
  /** The open questions' item and what is chosen on them. */
  openIndex: number | null;
  picks: Record<string, string>;
  onPick(questionId: string, label: string): void;
  /** Steps folded into a run's first line (`readRuns`), and failures Mizan put right (`recoveryGroups`). */
  runs: { counts: Map<number, number>; hidden: Set<number> };
  recovery: { hidden: Set<number>; heads: Map<number, { indices: number[]; pending: boolean }> };
  undoable: ToolCardItem | null;
  onShow(card: ToolCardItem): void;
  onUndo(card: ToolCardItem): void;
  onSettings(): void;
  send(text: string): void;
  focusBox(): void;
};

export function ThreadItem({ item, i, cx }: { item: Item; i: number; cx: ThreadContext }): ReactNode {
  const t = useT();
  const { lang } = useLanguage();
  const said = (key: string, vars?: Record<string, string | number>) => translate(cx.chatLang, key, vars);
  switch (item.kind) {
    case "provider_status":
      return <p className="msg notice" role="status">{t(item.code)}</p>;
    case "retrying":
      return <p className="msg notice" role="status">
        {["ai.error.context_retry", "ai.error.finish_gate", "ai.error.language_retry"].includes(item.code) ? t(item.code) : t("ai.error.retrying", { seconds: item.seconds })}
      </p>;
    case "cache_usage":
      return cx.showTechnicalDetails ? <details className="technical-details">
        <summary>{t("technical.details")}</summary>
        <p>{t("ai.cacheUsage", { percent: (100 * item.cached / item.total).toFixed(1), cached: item.cached, total: item.total })}</p>
      </details> : null;
    case "plan": {
      const answered = cx.items.slice(i + 1).some((next) => next.kind === "user");
      return <PlanChecklist phases={item.phases} stale={item.stale} language={cx.chatLang} showTechnicalDetails={cx.showTechnicalDetails}
        onApprove={item.awaitingApproval && !cx.unavailable && !answered ? () => cx.send(said("ai.approvedMessage")) : undefined}
        onChange={cx.focusBox} />;
    }
    case "question": {
      // Plain numbered list in Mizan's message (design B): options are chosen on it, the box answers
      // the rest. Once the user has replied it is only a record.
      const open = i === cx.openIndex;
      return <QuestionList questions={item.questions} picked={open ? cx.picks : undefined}
        onPick={open && !cx.unavailable ? cx.onPick : undefined} />;
    }
    case "user": {
      const own = contentLanguage(item.text, lang);
      return (
        <div className="msg user" lang={own} dir={own === "ar" ? "rtl" : "ltr"}>
          {((item.pointed?.length ?? 0) > 0 || (item.images?.length ?? 0) > 0) && (
            <div className="msg-attached">
              {item.pointed?.map((label) => <span key={label} className="chip">{label}</span>)}
              {item.images?.map((url, k) => url === "[picture removed]"
                ? <span key={k} className="chip">{t("ai.pictureRemoved")}</span>
                : <img key={k} src={url} alt={t("ai.picture")} className="thumb" />)}
            </div>
          )}
          {messageFiles(item.text).map((file, k) => <span key={k} className="chip"><bdi>{file.name}</bdi> · {t("ai.pdfPageCount", { n: file.pages })}</span>)}
          {isolateRuns(chatText(item.text))}
        </div>
      );
    }
    case "assistant":
      return <div className="msg assistant"><Markdown text={plainDashes(item.text)} /></div>;
    case "open_findings":
      return <div className="msg notice" role="note" lang={cx.chatLang} dir={cx.chatLang === "ar" ? "rtl" : "ltr"}>
        <strong className="heading">{said("ai.openFindings")}</strong>
        <ul>{[...new Set(item.findings.map(finding => finding.rule))].map(rule =>
          <li key={rule}>{said(`ai.finding.${rule}`)} ({item.findings.filter(finding => finding.rule === rule).length})</li>,
        )}</ul>
        {cx.showTechnicalDetails && <details className="technical-details"><summary>{t("technical.details")}</summary><pre dir="ltr">{JSON.stringify(item.findings, null, 2)}</pre></details>}
      </div>;
    case "thinking":
      return null;
    case "tool": {
      if (cx.recovery.hidden.has(i)) {
        const group = cx.recovery.heads.get(i);
        if (!group) return null;
        return <div className="msg notice recovery-summary" role="note" lang={cx.chatLang} dir={cx.chatLang === "ar" ? "rtl" : "ltr"}>
          {said(group.pending ? "activity.fixing" : "activity.recovered")}
          {cx.showTechnicalDetails && <details className="technical-details">
            <summary>{t("technical.details")}</summary>
            <pre dir="ltr">{group.indices.map((index) => {
              const failed = cx.items[index] as ToolCardItem;
              return `${failed.label ?? failed.name}\n${JSON.stringify(failed.input, null, 2)}\n${failed.summary}`;
            }).join("\n\n")}</pre>
          </details>}
        </div>;
      }
      // The plan's own call shows as the plan; a run of the same read is its first line.
      if (item.name === "qiyas_execute" && item.label === "plan" && !item.activity?.detail && item.status !== "error") return null;
      if (cx.runs.hidden.has(i)) return null;
      const canShow = item.handles.length > 0 || !!item.ui?.bounds;
      const canUndo = !cx.unavailable && item === cx.undoable;
      return <ToolCard card={item} language={cx.chatLang} repeat={cx.runs.counts.get(i)} showTechnicalDetails={cx.showTechnicalDetails}
        onShow={canShow ? () => cx.onShow(item) : undefined} onUndo={canUndo ? () => cx.onUndo(item) : undefined} />;
    }
    case "refused":
      return (
        <div className="msg notice refused" role="note">
          <strong className="heading">{t("ai.refused")}</strong>
          {item.text && <Markdown text={plainDashes(item.text)} />}
          {cx.showTechnicalDetails && item.category && <details className="technical-details"><summary>{t("technical.details")}</summary><pre>{item.category}</pre></details>}
        </div>
      );
    case "stopped":
      return (
        <div className="msg notice stopped" role="note">
          {stopText(item.reason, lang, cx.chatLang)}
          {connectionLost(item.reason) ? <button type="button" disabled={cx.busy}
            onClick={() => cx.send(t("ai.continueWork"))}>{t("ai.continueWorkButton")}</button>
            : providerFailure(item.reason, lang) && <button type="button" className="link" onClick={cx.onSettings}>{t("settings")}</button>}
        </div>
      );
  }
  return null;
}

