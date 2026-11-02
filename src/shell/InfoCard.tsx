import { readingDirection } from "../ai/content";
import { useState } from "react";
import type { Units } from "../api";
import { useT } from "../i18n";
import { metresPerUnit } from "./AttachPdfDialog";

export type Info = { title: string; rows: [string, string][] };

/** What an inquiry found (an entity's geometry, a point's coordinates): label and value rows,
 *  with a close button. Labels are the core's keys, shown in the current language. */
export function InfoCard({ info, onClose }: { info: Info; onClose(): void }) {
  const t = useT();
  /** The translation of `key`, or `fallback` when there is none. */
  const tr = (key: string, fallback: string) => {
    const text = t(key);
    return text === key ? fallback : text;
  };
  const title = info.title === "Point" ? t("inquiry.point") : info.title;
  return (
    <section className="info-card" aria-labelledby="info-card-title">
      <header>
        <h2 id="info-card-title" className="heading">
          {t("inquiry.info")}: <bdi>{title}</bdi>
        </h2>
        <button type="button" aria-label={t("inquiry.close")} title={t("inquiry.close")} onClick={onClose}>
          ×
        </button>
      </header>
      <dl>
        {info.rows.map(([label, value], i) => (
          <div key={i}>
            <dt>{tr(`info.${label}`, label)}</dt>
            {/* Numbers and coordinates read left to right in both languages. */}
            <dd {...(label === "text" || label === "layer" ? readingDirection(value) : { dir: "ltr" })}>
              {label === "snap" ? tr(`snap.${value}`, value) : value}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

type PromptProps = {
  kind: "divide" | "measureAlong";
  units: Units;
  /** The number of parts, or the step in drawing units. */
  onAnswer(value: number): void;
  onCancel(): void;
};

/** Asks how many parts to divide the clicked entity into (2 to 1000, 2 to start with), or the
 *  step in metres to place points along it. */
export function AlongPrompt({ kind, units, onAnswer, onCancel }: PromptProps) {
  const t = useT();
  const [text, setText] = useState(kind === "divide" ? "2" : "");
  const n = Number(text);
  const valid =
    text.trim() !== "" &&
    Number.isFinite(n) &&
    (kind === "divide" ? Number.isInteger(n) && n >= 2 && n <= 1000 : n > 0);
  return (
    <form
      className="along-prompt"
      aria-label={t(kind)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onCancel();
      }}
      onSubmit={(e) => {
        e.preventDefault();
        if (valid) onAnswer(kind === "divide" ? n : n / metresPerUnit(units));
      }}
    >
      <label>
        <span>{t(kind === "divide" ? "inquiry.parts" : "inquiry.step")}</span>
        <input
          type="number"
          min={kind === "divide" ? 2 : 0}
          max={kind === "divide" ? 1000 : undefined}
          step={kind === "divide" ? 1 : "any"}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <div className="buttons">
        <button type="button" onClick={onCancel}>
          {t("inquiry.cancel")}
        </button>
        <button type="submit" className="primary" disabled={!valid}>
          {t("inquiry.place")}
        </button>
      </div>
    </form>
  );
}
