/** What the thread shows, worked out from the transcript alone (no React): the questions still
 *  waiting for an answer, the conversation's language, and the failed steps Mizan put right. */
import type { Question } from "../api";
import type { Lang } from "../i18n";
import { contentLanguage } from "./content";
import { recoveringCard, type Item } from "./messages";

/** Mizan's last questions while nothing was sent after them: their item's index and the questions. */
export function openQuestions(items: Item[]): { index: number; questions: Question[] } | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.kind === "user") return null;
    if (item.kind === "question") return { index: i, questions: item.questions };
  }
  return null;
}

/** The conversation's language: the latest message the user wrote, else the window's. Everything
 *  Mizan shows in the thread (its steps, plan, waiting line) speaks it. */
export function chatLanguage(items: Item[], fallback: Lang): Lang {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.kind === "user") return contentLanguage(item.text, fallback);
  }
  return fallback;
}

/** Failed steps Mizan retried successfully in the same turn, grouped by turn: they show as one calm
 *  line at the first of them (`heads`), the rest hidden (`hidden`). `pending` while the retry has not
 *  come yet. Stored cards and the support log keep every failure. */
export function recoveryGroups(items: Item[], busy: boolean) {
  const groups = new Map<number, { indices: number[]; pending: boolean }>();
  let turn = 0;
  items.forEach((item, i) => {
    if (item.kind === "user") turn = i;
    if (item.kind !== "tool" || item.status !== "error" || !recoveringCard(items, i, busy)) return;
    const group = groups.get(turn) ?? { indices: [], pending: false };
    group.indices.push(i);
    group.pending ||= !recoveringCard(items, i, false);
    groups.set(turn, group);
  });
  return {
    hidden: new Set([...groups.values()].flatMap((group) => group.indices)),
    heads: new Map([...groups.values()].map((group) => [group.indices[0], group])),
  };
}
