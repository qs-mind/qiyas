import { useId, useState } from "react";
import { explainItem, type Explanation } from "../api";
import { useT } from "../i18n";
import { uiError } from "../i18n/errors";
import { ModalDialog } from "../ui/ModalDialog";
import { TechnicalDetails, ReviewNotes } from "../shell/TechnicalDetails";

export const EXPLAIN_EVIDENCE = "qiyas:explain-evidence";
export type Evidence = { handle: number; sheet: string; drawing: string };
export const quantity = (value: number) => Number.isFinite(value) ? String(Number(value.toFixed(3))) : "—";

/** A formula is shown only when it contains numbers and arithmetic, never internal fields. */
export function Formula({ expression, value, unit }: { expression: string; value: number; unit: string }) {
  const arithmetic = /^[\d\s.()+*/×÷−-]+$/.test(expression);
  return <bdi dir="ltr">{arithmetic ? `${expression.replaceAll("*", "×").replaceAll("/", "÷")} = ` : ""}{quantity(value)} {unit}</bdi>;
}

/** One read supplies the same evidence to the window and the automation tools. */
export function Explain({ item, project }: { item: string; project?: string }) {
  const t = useT();
  const title = t("evidence.title");
  const id = useId();
  const [result, setResult] = useState<Explanation | null>(null);
  const [error, setError] = useState("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  function show(evidence: Evidence[]) {
    window.dispatchEvent(new CustomEvent(EXPLAIN_EVIDENCE, { detail: evidence }));
  }
  async function read() {
    setOpen(true); setBusy(true); setError(""); setResult(null);
    try {
      const next = await explainItem(item, project);
      setResult(next);
      if (!project && next.evidence.length) show(next.evidence.filter(e => e.sheet === next.evidence[0].sheet));
    } catch (error) { setError(uiError(error)); }
    finally { setBusy(false); }
  }
  const evidence = result?.evidence ?? [];
  const groups = [...new Set(evidence.map(e => JSON.stringify([e.drawing, e.sheet])))];
  const explanations = result?.contributions?.map(c => c.explanation).filter(Boolean) ?? (result ? [result] : []);
  return <>
    <button type="button" aria-label={title} onClick={e => { e.stopPropagation(); void read(); }}>{t("evidence.explain")}</button>
    {open && <ModalDialog onClose={() => setOpen(false)} labelledBy={id}>
      <div onClick={event => event.stopPropagation()}>
      <h2 id={id}>{title}</h2>
      {busy && <p role="status">{t("evidence.reading")}</p>}
      {error && <p role="alert">{error}</p>}
      {result && <>
        <p><bdi>{quantity(result.quantity)} {result.unit}</bdi></p>
        <p>{t("evidence.confidence")}: {(result.confidence * 100).toFixed(0)}% · {t(`evidence.confidence.${result.confidence_band}`)}</p>
        <p>{t("evidence.confidenceHelp")}</p>
        {result.drawings.map(d => <p key={d}><bdi>{d.split(/[\\/]/).pop()}</bdi></p>)}
        <h3>{t("evidence.dimensions")}</h3>
        <ol>{result.dimension_lines.map((line, i) => <li key={i}>
          <span dir="auto">{line.label}</span> · <Formula {...line} />
          <span> · {t(line.included ? "boq.included" : "boq.excluded")}</span>
          <p>{line.citations.map(c => `${c.source} / ${c.clause}`).join("; ")}</p>
          <TechnicalDetails><pre>{line.expression}{"\n"}{line.decision}</pre></TechnicalDetails>
        </li>)}</ol>
        {explanations.map((exp, i) => <div key={i}>
          <h3>{t("evidence.records")}</h3>
          <EvidenceRecords title={t("evidence.elements")} records={exp.elements} />
          <EvidenceRecords title={t("evidence.recognition")} records={exp.recognition} />
          <EvidenceRecords title={t("evidence.recognitionRules")} records={exp.recognition_rules} />
          <EvidenceRecords title={t("evidence.measurementRules")} records={exp.measurement_rules} />
          <ReviewNotes notes={exp.notes ?? []} />
        </div>)}
        {groups.map(key => {
          const [drawing, sheet] = JSON.parse(key) as [string, string];
          const group = evidence.filter(e => e.drawing === drawing && e.sheet === sheet);
          return <button key={key} type="button" onClick={() => { show(group); setOpen(false); }}>
            {t("evidence.show", { n: group.length })} · <bdi>{drawing.split(/[\\/]/).pop()} · {sheet}</bdi>
          </button>;
        })}
      </>}
      <button type="button" onClick={() => setOpen(false)}>{t("close")}</button>
      </div>
    </ModalDialog>}
  </>;
}

function EvidenceRecords({ title, records }: { title: string; records?: unknown[] }) {
  return <div><p>{title}: {records?.length ?? 0}</p>
    {!!records?.length && <TechnicalDetails><pre dir="ltr">{JSON.stringify(records, null, 2)}</pre></TechnicalDetails>}
  </div>;
}
