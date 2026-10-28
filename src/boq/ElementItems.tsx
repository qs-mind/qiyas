/** The reverse of a trace, in the properties palette: the BOQ items the picked element feeds, and
 *  what it gives each (or deducts from it, for an opening). A click traces that item. */
import { useEffect, useState } from "react";
import { elementItems, type ElementItems as Items } from "../api";
import { useLanguage, useT } from "../i18n";
import { num, unitText } from "./howCalculated";
import { trace } from "./trace";

export function ElementItems({ handle }: { handle: number }) {
  const t = useT();
  const { lang } = useLanguage();
  const [items, setItems] = useState<Items | null>(null);
  useEffect(() => {
    let live = true;
    setItems(null);
    elementItems(handle)
      .then((next) => live && setItems(next))
      // An entity that is no element, or a drawing with no method: nothing to list.
      .catch(() => live && setItems(null));
    return () => {
      live = false;
    };
  }, [handle]);
  if (!items) return null;
  const unmapped = items.quantities.filter((q) => !q.mapped);
  if (!items.items.length && !unmapped.length && !items.takeoff.length) return null;
  return (
    <div className="prop-section element-items">
      <h3>{t("trace.feeds")}</h3>
      <ul>
        {items.items.map((i) => (
          <li key={i.item}>
            <button type="button" onClick={() => trace({ item: i.item, line: i.lines[0], panel: true })}>
              <span dir="auto">
                {i.code} · {lang === "ar" ? i.description_ar || i.description_en : i.description_en || i.description_ar}
              </span>
              <bdi dir="ltr">
                {i.role === "deducted" ? "" : "+"}
                {num(i.value)} {unitText(i.unit)}
              </bdi>
            </button>
          </li>
        ))}
        {items.takeoff.map((m) => (
          <li key={m.item}>
            <button type="button" onClick={() => trace({ item: m.item, panel: true })}>
              <span dir="auto">{m.description}</span>
              <bdi dir="ltr">
                {num(m.quantity)} {unitText(m.unit)}
              </bdi>
            </button>
          </li>
        ))}
        {unmapped.map((q) => (
          <li key={q.item} className="trace-muted">
            <button type="button" onClick={() => trace({ item: q.item, panel: true })}>
              <span>{t("trace.unmapped", { rule: q.rule })}</span>
              <bdi dir="ltr">
                {num(q.quantity)} {unitText(q.unit)}
              </bdi>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
