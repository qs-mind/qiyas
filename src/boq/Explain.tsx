import { useId, useState } from "react";
import { explainItem, type Explanation } from "../api";
import { useLanguage } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";

export const EXPLAIN_EVIDENCE = "qiyas:explain-evidence";
export type Evidence = { handle: number; sheet: string; drawing: string };

/** One read supplies the same evidence to the window and the automation tools. */
export function Explain({ item, project }: { item: string; project?: string }) {
  const { lang } = useLanguage();
  const ar = lang === "ar";
  const title = ar ? "كيف حسبت هذا الرقم؟" : "How did you get this number?";
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
      // Opening another drawing is an explicit evidence action, never a side effect of reading.
      if (!project && next.evidence.length) show(next.evidence.filter(e => e.sheet === next.evidence[0].sheet));
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  const evidence = result?.evidence ?? [];
  const groups = [...new Set(evidence.map(e => JSON.stringify([e.drawing, e.sheet])))];
  const explanations = result?.contributions?.map(c => c.explanation).filter(Boolean) ?? (result ? [result] : []);
  return <>
    <button type="button" title={title} aria-label={`${title} ${item}`} onClick={e => { e.stopPropagation(); void read(); }}>?</button>
    {open && <ModalDialog onClose={() => setOpen(false)} labelledBy={id}>
      <div onClick={event => event.stopPropagation()}>
      <h2 id={id}>{title}</h2>
      {busy && <p role="status">{ar ? "جارٍ قراءة الأدلة…" : "Reading evidence…"}</p>}
      {error && <p role="alert">{error}</p>}
      {result && <>
        <p dir="auto">{result.item}: {result.quantity} {result.unit}</p>
        <p>{ar ? "الثقة" : "Confidence"}: {(result.confidence * 100).toFixed(0)}% · {ar ? ({ low: "منخفضة", medium: "متوسطة", high: "عالية" }[result.confidence_band]) : result.confidence_band}</p>
        <p>{ar ? "أقل ثقة بين التعرف وقاعدة القياس؛ البيانات غير المقيمة تأخذ ٥٠٪. المراجعة اختيارية." : "Minimum recognition and measurement-rule confidence; unscored data uses 50%. Review is optional."}</p>
        {result.drawings.map(d => <p key={d}><bdi>{d}</bdi></p>)}
        <h3>{ar ? "سطور الأبعاد" : "Dimension lines"}</h3>
        <ol>{result.dimension_lines.map((line, i) => <li key={i}>
          <span dir="auto">{line.label}</span> · <code dir="ltr">{line.expression} = {line.value} {line.unit}</code>
          <span> · {line.included ? (ar ? "محسوب" : "Included") : (ar ? "مستبعد" : "Excluded")}</span>
          <p>{line.citations.map(c => `${c.source} / ${c.clause}`).join("; ")}</p>
          {line.decision && <p>{line.decision}</p>}
        </li>)}</ol>
        {explanations.map((exp, i) => <div key={i}>
          <h3>{ar ? "العناصر وقواعد التعرف والقياس ومراجعها" : "Elements, recognition and measurement rules with citations"}</h3>
          <EvidenceRecords title={ar ? "العناصر" : "Elements"} records={exp.elements} />
          <EvidenceRecords title={ar ? "أدلة التعرف" : "Recognition evidence"} records={exp.recognition} />
          <EvidenceRecords title={ar ? "قواعد التعرف" : "Recognition rules"} records={exp.recognition_rules} />
          <EvidenceRecords title={ar ? "قواعد القياس" : "Measurement rules"} records={exp.measurement_rules} />
          {exp.notes?.map(note => <p key={note}>{note}</p>)}
        </div>)}
        {groups.map(key => {
          const [drawing, sheet] = JSON.parse(key) as [string, string];
          const group = evidence.filter(e => e.drawing === drawing && e.sheet === sheet);
          return <button key={key} type="button" onClick={() => { show(group); setOpen(false); }}>
            {ar ? "إظهار الأدلة" : "Show evidence"} · <bdi>{drawing.split(/[\\/]/).pop()} · {sheet} · {group.map(e => e.handle.toString(16).toUpperCase()).join(", ")}</bdi>
          </button>;
        })}
      </>}
      <button type="button" onClick={() => setOpen(false)}>{ar ? "إغلاق" : "Close"}</button>
      </div>
    </ModalDialog>}
  </>;
}

/** Rule definitions are user-authored data; their names and source text stay verbatim. */
function EvidenceRecords({ title, records }: { title: string; records?: unknown[] }) {
  return <details><summary>{title} ({records?.length ?? 0})</summary>
    {records?.map((record, index) => <dl key={index}>
      {Object.entries(record as Record<string, unknown>).filter(([, value]) => value != null).map(([name, value]) => <div key={name}>
        <dt>{name.replaceAll("_", " ")}</dt>
        <dd dir="auto" style={{ overflowWrap: "anywhere" }}>{typeof value === "object" ? JSON.stringify(value) : String(value)}</dd>
      </div>)}
    </dl>)}
  </details>;
}
