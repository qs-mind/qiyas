import { TechnicalDetails, ReviewNotes } from "../shell/TechnicalDetails";
import { plainMethod } from "../shell/methodName";
import { Explain, Formula, quantity } from "./Explain";
import type { MeasurementReport } from "../api";
import { useLanguage, useT } from "../i18n";

/** Every figure and dimension comes from the drawing's applied measurement method. */
export function GeneratedBoq({ report, onShow }: { report: MeasurementReport | null; onShow?(handles: number[], sheet: string): void }) {
  const t = useT();
  const { lang } = useLanguage();
  if (!report || (!report.items.length && !report.warnings.length)) return null;
  return <section className="generated-boq" aria-label={t("boq.generated")}>
    <h3>{t("boq.generated")}</h3>
    {report.items.map((item) => <details key={item.code}>
      <summary dir="auto">{item.code} · {lang === "ar" ? item.description_ar : item.description_en} · {quantity(item.quantity)} {item.unit}</summary>
      <Explain item={`generated:${item.code}`} />
      <ol>{item.lines.map((line, index) => <li key={index}>
        <span>{line.label} · {line.included ? t("boq.included") : t("boq.excluded")}</span>
        <div><Formula {...line} /></div>
        <div>{line.citations.map((c) => `${c.source} / ${c.clause}`).join("; ")}</div>
        <TechnicalDetails><pre>{line.expression}{"\n"}{plainMethod(line.method)}{"\n"}{line.decision}</pre></TechnicalDetails>
        {line.sources.map((source) => <button key={`${source.sheet}:${source.handle}`} type="button"
          disabled={!onShow} onClick={() => onShow?.([source.handle], source.sheet)}>
          {t("evidence.show", { n: 1 })} · <bdi>{source.sheet} · {source.grid ?? t("boq.noGrid")}</bdi>
        </button>)}
      </li>)}</ol>
    </details>)}
    <ReviewNotes notes={report.warnings} />
  </section>;
}
