import { Explain } from "./Explain";
import type { MeasurementReport } from "../api";
import { useLanguage, useT } from "../i18n";

/** Every figure and dimension comes from the drawing's applied measurement method. */
export function GeneratedBoq({ report, onShow }: { report: MeasurementReport | null; onShow?(handles: number[], sheet: string): void }) {
  const t = useT();
  const { lang } = useLanguage();
  if (!report || (!report.items.length && !report.warnings.length)) return null;
  return <section className="generated-boq" aria-label={t("boq.generated")}>
    <h3>{t("boq.generated")}</h3>
    {report.confidence_summary && <p>{lang === "ar" ? "الثقة: منخفضة / متوسطة / عالية" : "Confidence: low / medium / high"}: {report.confidence_summary.low} / {report.confidence_summary.medium} / {report.confidence_summary.high}</p>}
    {report.items.map((item) => <details key={item.code}>
      <summary dir="auto">{item.code} · {lang === "ar" ? item.description_ar : item.description_en} · {item.quantity} {item.unit}</summary>
      <Explain item={`generated:${item.code}`} />
      <p>{lang === "ar" ? "الثقة" : "Confidence"}: {((item.confidence ?? 0.5) * 100).toFixed(0)}%</p>
      <p>{item.section} · {item.trade}</p>
      <ol>{item.lines.map((line, index) => <li key={index}>
        <span>{line.label} · {line.included ? t("boq.included") : t("boq.excluded")}</span>
        <div dir="ltr"><code>{line.expression} = {line.value} {line.unit}</code></div>
        <div>{line.method} · {line.citations.map((c) => `${c.source} / ${c.clause}`).join("; ")}</div>
        {line.decision && <div>{line.decision}</div>}
        {line.sources.map((source) => <button key={`${source.sheet}:${source.handle}`} type="button"
          disabled={!onShow} onClick={() => onShow?.([source.handle], source.sheet)}>
          {source.handle} · {source.sheet} · {source.grid ?? t("boq.noGrid")}
        </button>)}
      </li>)}</ol>
    </details>)}
    {report.warnings.length > 0 && <details><summary>{t("boq.measurementReview")} ({report.warnings.length})</summary>
      <ul>{report.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
    </details>}
  </section>;
}
