/** Mizan's questions (W1's `ask`, rendered as the founder chose in W4): a plain numbered list inside
 *  Mizan's message. No boxes, no cards. The recommended option is in the accent colour with
 *  "· recommended". Choosing an option marks it on the question (not in the box; a second click
 *  takes it off); once every question has one the answers go at once. "Other" is the box itself:
 *  what is typed there, sent with Enter, answers the questions left open (founder, 28 Oct 2026).
 *  One question: its options are numbered 1, 2, 3. Several: the questions are numbered and each
 *  one's options lettered. */
import type { Question } from "../api";
import { translate, useLanguage } from "../i18n";
import { contentLanguage, isolateRuns, plainDashes } from "./content";
import { withoutBody } from "../shell/methodName";

/** Latin words and sizes are isolated as in Mizan's answers, so they keep the Latin face. */
const shown = (text: string) => isolateRuns(plainDashes(withoutBody(text)));

type Props = {
  questions: Question[];
  /** The option chosen for each question so far, by question id. */
  picked?: Record<string, string>;
  /** Chooses (or, the same again, takes off) an option; absent once the questions are answered. */
  onPick?(questionId: string, label: string): void;
};

/** The message that answers `questions`: each chosen option, numbered as its question when there
 *  are several, and what was typed in the box. Typed words answer the one question left open, when
 *  only one is; otherwise they follow the chosen options. Empty when nothing was chosen or typed. */
export function answerText(questions: Question[], picked: Record<string, string>, typed: string): string {
  const text = typed.trim();
  if (questions.length === 1) return [picked[questions[0].id], text].filter(Boolean).join("\n");
  const open = questions.filter((q) => !picked[q.id]);
  const fillsOpen = open.length === 1 && text !== "";
  const lines = questions.flatMap((q, i) =>
    picked[q.id] ? [`${i + 1}. ${picked[q.id]}`] : fillsOpen && q === open[0] ? [`${i + 1}. ${text}`] : []);
  return [...lines, ...(fillsOpen ? [] : [text])].filter(Boolean).join("\n");
}

/** Options are lettered in the questions' own language: a, b, c or أ، ب، ج. */
const AR_LETTERS = "أبجدهوزح";
const letter = (i: number, ar: boolean) => ar ? AR_LETTERS[i] ?? String(i + 1) : String.fromCharCode(97 + i);

export function QuestionList({ questions, picked = {}, onPick }: Props) {
  // The list reads in the questions' own language and direction, whatever the window's language
  // (an Arabic question with English "recommended" and its letters on the left once read wrongly).
  const { lang } = useLanguage();
  const language = contentLanguage(questions.map((q) => `${q.question} ${q.options.map((o) => o.label).join(" ")}`).join(" "), lang);
  const ar = language === "ar";
  const t = (key: string, vars?: Record<string, string | number>) => translate(language, key, vars);
  const many = questions.length > 1;
  return (
    <div className="msg assistant question-list" lang={language} dir={ar ? "rtl" : "ltr"} role="group" aria-label={t(many ? "ai.nQuestions" : "ai.oneQuestion", { n: questions.length })}>
      {questions.map((q, qi) => (
        <div key={q.id} className="q">
          <p className="qt"><bdi>{many ? `${qi + 1}. ` : ""}{shown(q.question)}</bdi></p>
          <ol className="qo">
            {q.options.map((o, oi) => {
              const marker = many ? letter(oi, ar) : String(oi + 1);
              const chosen = picked[q.id] === o.label;
              return (
                <li key={o.label}>
                  <button
                    type="button"
                    className={["qopt", o.recommended && "hot", chosen && "picked"].filter(Boolean).join(" ")}
                    aria-pressed={onPick ? chosen : undefined}
                    disabled={!onPick}
                    onClick={() => onPick?.(q.id, o.label)}
                  >
                    <span className="k" aria-hidden="true">{marker}</span>
                    <span className="tx">
                      <span className="l">
                        <bdi>{shown(o.label)}</bdi>
                        {o.recommended && <em>· {t("ai.recommended")}</em>}
                      </span>
                      {o.description && <span className="s"><bdi>{shown(o.description)}</bdi></span>}
                    </span>
                  </button>
                </li>
              );
            })}
            {q.allow_other && onPick && (
              <li className="qother">
                <span className="k" aria-hidden="true">{many ? letter(q.options.length, ar) : String(q.options.length + 1)}</span>
                <span className="l">{t("ai.questionOther")}</span>
              </li>
            )}
          </ol>
        </div>
      ))}
    </div>
  );
}
