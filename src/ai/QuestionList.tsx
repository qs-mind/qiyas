/** One inline question at a time, with a separate free answer for each step. */
import { useState, useRef, useLayoutEffect, type KeyboardEvent } from "react";
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
  onAnswer?(text: string): void | boolean | Promise<void | boolean>;
  /** The following user message, also available when restoring a saved conversation. */
  answeredText?: string;
  active?: boolean;
  disabled?: boolean;
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

/** Hide only standalone input instructions, never an option that contains a substantive answer. */
export function isFreeAnswerHint(label: string): boolean {
  const text = label.trim().replace(/\s+/g, " ").replace(/[.!…؟:]+$/u, "").toLowerCase();
  return /^(?:or\s+)?(?:type|write|enter) (?:another|your own|a different|a free(?:[ -]text)?) answer(?: (?:in (?:the )?(?:box|field)(?: below)?|below))?$/.test(text)
    || /^(?:أو |او )?(?:اكتب|أدخل|ادخل) (?:إجابة أخرى|اجابة اخرى|إجابة تانية|إجابتك|اجابتك|رد تاني)(?: (?:في (?:الخانة|المربع|الصندوق)(?: (?:بالأسفل|أدناه|تحت))?|بالأسفل|أدناه))?$/.test(text);
}

export function QuestionList({ questions, picked = {}, onPick, onAnswer, answeredText, active = !!onPick, disabled = false }: Props) {
  const [step, setStep] = useState(0);
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [submitted, setSubmitted] = useState<string>();
  const [sending, setSending] = useState(false);
  const inFlight = useRef(false);
  const field = useRef<HTMLInputElement>(null);
  const previousStep = useRef(step);
  useLayoutEffect(() => {
    if (step !== previousStep.current) field.current?.focus();
    previousStep.current = step;
  }, [step]);
  const answers = Object.fromEntries(questions.map(q => [q.id, typed[q.id]?.trim() || picked[q.id] || ""]));
  const advance = async () => {
    if (disabled || inFlight.current || !answers[questions[step].id]) return;
    if (step < questions.length - 1) {
      setStep(step + 1);
    } else if (onAnswer && questions.every(q => answers[q.id])) {
      const text = answerText(questions, answers, "");
      inFlight.current = true;
      setSending(true);
      try {
        if (await onAnswer(text) !== false) setSubmitted(text);
      } finally {
        inFlight.current = false;
        setSending(false);
      }
    }
  };
  const choose = (id: string, label: string) => {
    setTyped(was => ({ ...was, [id]: "" }));
    onPick?.(id, label);
  };
  const optionKeys = (event: KeyboardEvent, q: Question) => {
    if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    const options = q.options.filter(o => !isFreeAnswerHint(o.label));
    if (!options.length || disabled) return;
    event.preventDefault();
    event.stopPropagation();
    const current = options.findIndex(o => o.label === picked[q.id]);
    const forward = event.key === "ArrowDown" || event.key === (ar ? "ArrowLeft" : "ArrowRight");
    const next = current < 0 ? (forward ? 0 : options.length - 1) : (current + (forward ? 1 : -1) + options.length) % options.length;
    if (next !== current) choose(q.id, options[next].label);
  };
  // The list reads in the questions' own language and direction, whatever the window's language
  // (an Arabic question with English "recommended" and its letters on the left once read wrongly).
  const { lang } = useLanguage();
  const language = contentLanguage(questions.map((q) => `${q.question} ${q.options.map((o) => o.label).join(" ")}`).join(" "), lang);
  const ar = language === "ar";
  const t = (key: string, vars?: Record<string, string | number>) => translate(language, key, vars);
  const many = questions.length > 1;
  if (submitted || (!active && answeredText)) return (
    <div className="msg assistant question-list" lang={language} dir={ar ? "rtl" : "ltr"}>
      {questions.map(q => <p key={q.id} className="qt">{shown(q.question)}</p>)}
      <p className="qsummary">{shown(answeredText ?? submitted ?? "")}</p>
    </div>
  );
  return (
    <div className="msg assistant question-list" lang={language} dir={ar ? "rtl" : "ltr"} role="group" aria-label={t(many ? "ai.nQuestions" : "ai.oneQuestion", { n: questions.length })}>
      {questions.map((q, qi) => (!active || qi === step) && (
        <div key={q.id} className="q">
          {active && many && <p className="qprogress">{t("ai.questionProgress", { n: qi + 1, total: questions.length })}</p>}
          <p className="qt"><bdi>{many ? `${qi + 1}. ` : ""}{shown(q.question)}</bdi></p>
          <ol className="qo" onKeyDown={active ? event => optionKeys(event, q) : undefined}>
            {q.options.filter(o => !isFreeAnswerHint(o.label)).map((o, oi) => {
              const marker = many ? letter(oi, ar) : String(oi + 1);
              const chosen = !typed[q.id]?.trim() && picked[q.id] === o.label;
              return (
                <li key={o.label}>
                  <button
                    type="button"
                    className={["qopt", o.recommended && "hot", chosen && "picked"].filter(Boolean).join(" ")}
                    aria-pressed={onPick ? chosen : undefined}
                    disabled={!active || disabled || sending}
                    onClick={() => choose(q.id, o.label)}
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
          </ol>
          {active ? <>
            <input ref={field} className="qanswer" aria-label={t("ai.questionAnswer")} placeholder={t("ai.questionAnswer")}
              value={typed[q.id] ?? ""} disabled={disabled || sending}
              onChange={event => setTyped(was => ({ ...was, [q.id]: event.target.value }))}
              onKeyDown={event => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); advance(); }
                else if (["ArrowUp", "ArrowDown"].includes(event.key)) optionKeys(event, q);
              }} />
            <div className="qactions">
              {many && <button type="button" disabled={disabled || sending || step === 0} onClick={() => setStep(step - 1)}>{t("ai.questionBack")}</button>}
              <button type="button" disabled={disabled || sending || !answers[q.id]} onClick={() => { void advance(); }}>{t(step === questions.length - 1 ? "ai.send" : "ai.questionNext")}</button>
            </div>
          </> : typed[q.id]?.trim() && <p>{shown(typed[q.id].trim())}</p>}
        </div>
      ))}
    </div>
  );
}
