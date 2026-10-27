import { useEffect, useLayoutEffect, useState } from "react";
import type { Sheet } from "../api";
import { useT } from "../i18n";
import { Glyph } from "./glyphs";
import { useRoving } from "./useRoving";

type Props = { sheets: Sheet[]; active: string; onSelect(block: string): void };

/** One tab per sheet: the model first as the drawing lists it, then the layouts. The strip never
 *  shows a scrollbar (it once lay over the tab names, note 47): with more tabs than fit, the edge
 *  that has more fades and an arrow there scrolls to them, and the chosen tab is kept in view. */
export function SheetTabs({ sheets, active, onSelect }: Props) {
  const t = useT();
  const roving = useRoving(
    sheets.length,
    sheets.findIndex((s) => s.block === active),
  );
  // The tab list is the roving hook's element too.
  const list = roving.props.ref;
  /** More tabs lie before the first one in view (toward the start edge), or after the last. */
  const [more, setMore] = useState({ start: false, end: false });

  /** Reads which edges have more; the scroll position is logical, so Arabic reads the same. */
  const measure = () => {
    const el = list.current;
    if (!el) return;
    const at = Math.abs(el.scrollLeft);
    const start = at > 1;
    const end = at + el.clientWidth < el.scrollWidth - 1;
    setMore((was) => (was.start === start && was.end === end ? was : { start, end }));
  };

  useLayoutEffect(measure, [sheets]);
  useEffect(() => {
    const el = list.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  // The chosen sheet stays in view.
  useEffect(() => {
    list.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView?.({ inline: "nearest", block: "nearest" });
    measure();
  }, [active]);

  /** Scrolls most of a screenful toward the end edge (`1`) or the start edge (`-1`). */
  const scroll = (toward: 1 | -1) => {
    const el = list.current;
    if (!el) return;
    const rtl = getComputedStyle(el).direction === "rtl";
    el.scrollBy?.({ left: (rtl ? -toward : toward) * Math.max(80, el.clientWidth * 0.8), behavior: "smooth" });
  };

  return (
    <div className={`sheet-strip${more.start ? " more-start" : ""}${more.end ? " more-end" : ""}`}>
      {more.start && (
        <button type="button" className="sheet-arrow start" tabIndex={-1} aria-label={t("sheets.earlier")} title={t("sheets.earlier")} onClick={() => scroll(-1)}>
          <Glyph name="next" mirror size={14} />
        </button>
      )}
      <div className="sheet-tabs" role="tablist" aria-label={t("sheets")} onScroll={measure} {...roving.props}>
        {sheets.map((s, i) => (
          <button
            key={s.block}
            type="button"
            role="tab"
            aria-selected={s.block === active}
            tabIndex={roving.tabIndex(i)}
            title={s.is_model ? undefined : s.name}
            onClick={() => onSelect(s.block)}
          >
            {s.is_model ? t("model") : <bdi>{s.name}</bdi>}
          </button>
        ))}
      </div>
      {more.end && (
        <button type="button" className="sheet-arrow end" tabIndex={-1} aria-label={t("sheets.later")} title={t("sheets.later")} onClick={() => scroll(1)}>
          <Glyph name="next" mirror size={14} />
        </button>
      )}
    </div>
  );
}
