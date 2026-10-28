/** "How was it calculated": a side card beside the drawing with the item's working in engineering
 *  terms (Length × Thickness × Height = m³), each value with where it came from, and each element
 *  it measures. Nothing here edits; a source on the drawing is shown there with one click. */
import { useEffect, useRef, useState } from "react";
import type { DimensionLine, Explanation, Operand } from "../api";
import { useLanguage, useT } from "../i18n";
import { TechnicalDetails } from "../shell/TechnicalDetails";
import { elementName, itemTitle, lineOperands, linesOf, num, operandText, unitText } from "./howCalculated";

export type HowCalculatedProps = {
  explanation: Explanation;
  /** The line asked for (a cell of the exported sheet), scrolled to and marked. */
  line: string | null;
  /** The element shown alone, or null for all. */
  focus: number | null;
  onFocus(handle: number | null): void;
  /** Lights one entity on the drawing: a note, a table, a block a value was read from. */
  onShowSource(handle: number): void;
  onClose(): void;
};

export function HowCalculated({ explanation, line, focus, onFocus, onShowSource, onClose }: HowCalculatedProps) {
  const t = useT();
  const { lang } = useLanguage();
  const shares = explanation.by_element ?? [];
  const lines = linesOf(explanation, focus);
  const unit = unitText(explanation.unit);
  return (
    <aside className="trace-panel" aria-label={t("trace.title")}>
      <header className="trace-head">
        <h2>{t("trace.title")}</h2>
        <button type="button" className="trace-close" aria-label={t("close")} onClick={onClose}>
          ×
        </button>
      </header>
      <p className="trace-item">
        <bdi dir="auto">{itemTitle(explanation, lang)}</bdi>
      </p>
      <p className="trace-total">
        <bdi dir="ltr">
          {num(explanation.quantity)} {unit}
        </bdi>
      </p>
      {shares.length > 1 && (
        <section>
          <h3>{t("trace.elements", { n: shares.length })}</h3>
          <ul className="trace-elements">
            {shares.map((share, i) => (
              <li key={share.handle}>
                <button
                  type="button"
                  aria-pressed={focus === share.handle}
                  onClick={() => onFocus(focus === share.handle ? null : share.handle)}
                >
                  <span dir="auto">{elementName(explanation, share.handle) || t("trace.element", { n: i + 1 })}</span>
                  <bdi dir="ltr">
                    {num(share.value)} {unitText(share.unit)}
                  </bdi>
                  {share.voids.length > 0 && <span className="trace-muted">{t("trace.voids", { n: share.voids.length })}</span>}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      <section>
        <h3>{t("trace.working")}</h3>
        <ol className="trace-lines">
          {lines.map((l, i) => (
            <Line key={l.id || i} line={l} marked={!!line && l.id === line} onShowSource={onShowSource} />
          ))}
        </ol>
      </section>
      {!!explanation.notes?.length && (
        <TechnicalDetails>
          <ul>
            {explanation.notes.map((note, i) => (
              <li key={i} dir="auto">
                {note}
              </li>
            ))}
          </ul>
        </TechnicalDetails>
      )}
    </aside>
  );
}

function Line({ line, marked, onShowSource }: { line: DimensionLine; marked: boolean; onShowSource(handle: number): void }) {
  const t = useT();
  const ref = useRef<HTMLLIElement>(null);
  const [open, setOpen] = useState<number | null>(null);
  useEffect(() => {
    if (marked) ref.current?.scrollIntoView({ block: "nearest" });
  }, [marked]);
  // The core names the timesing "Times"; the reader's language says it.
  const operands = lineOperands(line).map((op) => (op.label === "Times" ? { ...op, label: t("trace.times") } : op));
  const deduct = line.operation === "deduct";
  return (
    <li ref={ref} className={`trace-line${marked ? " trace-line-marked" : ""}${line.included ? "" : " trace-line-excluded"}`}>
      <p className="trace-label" dir="auto">
        {deduct ? `${t("trace.deduct")}: ` : ""}
        {line.label}
        {!line.included && <span className="trace-muted"> · {t("boq.excluded")}</span>}
      </p>
      <p className="trace-equation" dir="ltr">
        {operands.map((op, i) => (
          <span key={i}>
            {i > 0 && <span className="trace-op"> × </span>}
            <button
              type="button"
              className={`trace-operand trace-kind-${kindOf(op)}`}
              aria-expanded={open === i}
              title={kindOf(op) === "none" ? undefined : t(`trace.kind.${kindOf(op)}`)}
              onClick={() => setOpen(open === i ? null : i)}
            >
              <bdi dir="auto">{op.label ? operandText(op) : num(op.value)}</bdi>
            </button>
          </span>
        ))}
        <span className="trace-op"> = </span>
        <strong>
          {deduct && line.value > 0 ? "−" : ""}
          {num(line.value)} {unitText(line.unit)}
        </strong>
      </p>
      {open !== null && operands[open] && <Sources operand={operands[open]} onShowSource={onShowSource} />}
      {line.citations.length > 0 && (
        <p className="trace-cite" dir="auto">
          {line.citations.map((c) => [c.source, c.clause].filter(Boolean).join(" / ")).join("; ")}
        </p>
      )}
      {line.decision && (
        <TechnicalDetails>
          <pre dir="ltr">
            {line.expression}
            {"\n"}
            {line.decision}
          </pre>
        </TechnicalDetails>
      )}
    </li>
  );
}

/** The weakest source of an operand names its colour: an assumed value is never shown as read. */
function kindOf(op: Operand): string {
  const order = ["unknown", "mizan", "type_default", "engineer", "derived", "rule", "document", "drawing_block", "drawing_attribute", "drawing_table", "drawing_note", "measured"];
  if (!op.sources.length) return "none";
  return op.sources.map((s) => s.kind).sort((a, b) => order.indexOf(a) - order.indexOf(b))[0];
}

function Sources({ operand, onShowSource }: { operand: Operand; onShowSource(handle: number): void }) {
  const t = useT();
  return (
    <ul className="trace-sources">
      <li className="trace-muted" dir="ltr">
        {operand.expression}
      </li>
      {operand.sources.map((s, i) => (
        <li key={i}>
          <span className={`trace-kind trace-kind-${s.kind}`}>{t(`trace.kind.${s.kind}`)}</span>
          {s.text && (
            <span dir="auto">
              {" "}
              · {s.text}
            </span>
          )}
          {s.handle != null && (
            <button type="button" className="trace-show" onClick={() => onShowSource(s.handle!)}>
              {t("trace.showSource")}
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

/** One line's working on one row: "Length 5.2 m × Height 3 m = 15.6 m²", for lists. */
export function LineFormula({ line }: { line: DimensionLine }) {
  const t = useT();
  const operands = lineOperands(line).map((op) => (op.label === "Times" ? { ...op, label: t("trace.times") } : op));
  return (
    <bdi dir="ltr">
      {operands.map((op) => (op.label ? operandText(op) : num(op.value))).join(" × ")} = {num(line.value)} {unitText(line.unit)}
    </bdi>
  );
}
