/** Mizan's questions (W1's `ask`, rendered as the founder chose in W4): a plain numbered list inside
 *  Mizan's message. No boxes, no cards. The recommended option is in the accent colour with
 *  "· recommended". Choosing an option puts it in the box (it is not sent: the founder can add to
 *  it); "Other" is the box itself, typed in the usual way. One question: its options are numbered
 *  1, 2, 3. Several: the questions are numbered and each one's options lettered. */
import type { Question } from "../api";
import { useT } from "../i18n";

type Props = {
  questions: Question[];
  /** Puts the chosen answer in the box; absent once the question has been answered. */
  onPick?(text: string): void;
};

const letter = (i: number) => String.fromCharCode(97 + i);

export function QuestionList({ questions, onPick }: Props) {
  const t = useT();
  const many = questions.length > 1;
  return (
    <div className="msg assistant question-list" role="group" aria-label={t(many ? "ai.nQuestions" : "ai.oneQuestion", { n: questions.length })}>
      {questions.map((q, qi) => (
        <div key={q.id} className="q">
          <p className="qt"><bdi>{many ? `${qi + 1}. ` : ""}{q.question}</bdi></p>
          <ol className="qo">
            {q.options.map((o, oi) => {
              const marker = many ? letter(oi) : String(oi + 1);
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
                        <bdi>{o.label}</bdi>
                        {o.recommended && <em>· {t("ai.recommended")}</em>}
                      </span>
                      {o.description && <span className="s"><bdi>{o.description}</bdi></span>}
                    </span>
                  </button>
                </li>
              );
            })}
            {q.allow_other && onPick && (
              <li className="qother">
                <span className="k" aria-hidden="true">{many ? letter(q.options.length) : q.options.length + 1}</span>
                <span className="l">{t("ai.questionOther")}</span>
              </li>
            )}
          </ol>
        </div>
      ))}
    </div>
  );
}
