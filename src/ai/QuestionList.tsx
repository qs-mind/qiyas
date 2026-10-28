/** Mizan's questions (W1's `ask`, rendered as the founder chose in W4): a plain numbered list inside
 *  Mizan's message. No boxes, no cards. The recommended option is in the accent colour with
 *  "· recommended". Choosing an option puts it in the box (it is not sent: the founder can add to
 *  it); "Other" is the box itself, typed in the usual way. One question: its options are numbered
 *  1, 2, 3. Several: the questions are numbered and each one's options lettered. */
import type { Question } from "../api";
import { translate, useLanguage } from "../i18n";
import { contentLanguage, isolateRuns } from "./content";
import { withoutBody } from "../shell/methodName";

/** Latin words and sizes are isolated as in Mizan's answers, so they keep the Latin face. */
const shown = (text: string) => isolateRuns(withoutBody(text));

type Props = {
  questions: Question[];
  /** Puts the chosen answer in the box; absent once the question has been answered. */
  onPick?(text: string): void;
};

/** Options are lettered in the questions' own language: a, b, c or أ، ب، ج. */
const AR_LETTERS = "أبجدهوزح";
const letter = (i: number, ar: boolean) => ar ? AR_LETTERS[i] ?? String(i + 1) : String.fromCharCode(97 + i);

export function QuestionList({ questions, onPick }: Props) {
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
              const answer = many ? `${qi + 1}. ${o.label}` : o.label;
              return (
                <li key={o.label}>
                  <button
                    type="button"
                    className={o.recommended ? "qopt hot" : "qopt"}
                    disabled={!onPick}
                    onClick={() => onPick?.(answer)}
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
