/** What floats over the drawing while a quantity is traced: a small bar naming the item with
 *  "How was it calculated?", or that working itself. */
import type { Trace } from "../app/useTrace";
import { useLanguage, useT } from "../i18n";
import { HowCalculated } from "./HowCalculated";
import { itemTitle, num, unitText } from "./howCalculated";

export function TraceCard({ trace }: { trace: Trace }) {
  const t = useT();
  const { lang } = useLanguage();
  const traced = trace.traced;
  if (!traced) return null;
  const exp = traced.explanation;
  if (traced.panel)
    return (
      <HowCalculated
        explanation={exp}
        line={traced.target.line ?? null}
        focus={trace.focus}
        onFocus={trace.setFocus}
        onShowSource={trace.showSource}
        onClose={trace.close}
      />
    );
  return (
    <div className="trace-bar" role="status">
      <bdi dir="auto" className="trace-bar-item">
        {itemTitle(exp, lang)}
      </bdi>
      <bdi dir="ltr">
        {num(exp.quantity)} {unitText(exp.unit)}
      </bdi>
      <button type="button" onClick={trace.openPanel}>
        {t("trace.how")}
      </button>
      <button type="button" className="trace-close" aria-label={t("close")} onClick={trace.close}>
        ×
      </button>
    </div>
  );
}
