/** Side tabs keep the active panel and pinned tabs visible; the rest fold into a menu. */
import { Menu } from "@base-ui/react/menu";
import { Tabs } from "@base-ui/react/tabs";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useT } from "../i18n";

export type SideTabItem<Id extends string> = {
  id: Id;
  label: string;
  /** What follows the label: a count, a loader. */
  extra?: ReactNode;
  /** Always visible, including when another panel is active. */
  pinned?: boolean;
};

type Props<Id extends string> = {
  tabs: SideTabItem<Id>[];
  value: Id;
  onChange(id: Id): void;
  /** The class of the box holding the tabs and the panels (`side`, or `side wide`). */
  className: string;
  /** What is under the tab row: the panels (`Tabs.Panel`). */
  children: ReactNode;
};

export function SideTabs<Id extends string>({ tabs, value, onChange, className, children }: Props<Id>) {
  const t = useT();
  const list = useRef<HTMLDivElement>(null);
  const more = useRef<HTMLButtonElement>(null);
  const [visible, setVisible] = useState<string[]>(tabs.map((tab) => tab.id));
  const [open, setOpen] = useState(false);
  const shown = new Set([...visible, value, ...tabs.filter((tab) => tab.pinned).map((tab) => tab.id)]);
  const folded = tabs.filter((tab) => !shown.has(tab.id));

  const measure = () => {
    const row = list.current;
    if (!row || row.clientWidth === 0) return;
    const css = getComputedStyle(row);
    const gap = parseFloat(css.columnGap) || 0;
    const widths = Array.from(row.querySelectorAll<HTMLElement>("[data-side-tab]")).map((el) => el.offsetWidth);
    const room = row.clientWidth - (parseFloat(css.paddingLeft) || 0) - (parseFloat(css.paddingRight) || 0);
    const total = widths.reduce((sum, width) => sum + width + gap, -gap);
    const keep = new Set(tabs.filter((tab) => tab.id === value || tab.pinned).map((tab) => tab.id));
    if (total <= room) tabs.forEach((tab) => keep.add(tab.id));
    else {
      let used = (more.current?.offsetWidth || 28) + tabs.reduce((sum, tab, i) => sum + (keep.has(tab.id) ? widths[i] + gap : 0), 0);
      tabs.forEach((tab, i) => {
        if (!keep.has(tab.id) && used + widths[i] + gap <= room) {
          keep.add(tab.id);
          used += widths[i] + gap;
        }
      });
    }
    const next = tabs.filter((tab) => keep.has(tab.id)).map((tab) => tab.id);
    setVisible((old) => old.join("|") === next.join("|") ? old : next);
  };
  const labels = tabs.map((x) => `${x.id}:${x.label}:${x.pinned}`).join("|");
  // Before paint: a new language or tab set. After it: the row, or a tab (a count), changed size.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useLayoutEffect(measure, [labels, value]);
  useLayoutEffect(() => {
    const row = list.current;
    if (!row || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(measure);
    watch.observe(row);
    row.querySelectorAll("[data-side-tab]").forEach((el) => watch.observe(el));
    return () => watch.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [labels, value]);

  return (
    <Tabs.Root
      className={className}
      value={value}
      onValueChange={(next) => onChange(next as Id)}
    >
      <Tabs.List className="side-tabs" ref={list}>
        {tabs.map((x) => (
          <Tabs.Tab
            key={x.id}
            value={x.id}
            id={`tab-${x.id}`}
            className={shown.has(x.id) ? "side-tab" : "side-tab folded"}
            disabled={!shown.has(x.id)}
            aria-hidden={!shown.has(x.id) || undefined}
            data-side-tab=""
          >
            {x.label}
            {x.extra}
          </Tabs.Tab>
        ))}
        {folded.length > 0 && (
        <Menu.Root open={open} onOpenChange={setOpen} modal={false}>
          <Menu.Trigger
            ref={more}
            className="side-tab side-tab-more"
            aria-label={t("panels.moreTabs")}
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M3.5 8h.01M8 8h.01M12.5 8h.01" />
            </svg>
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Positioner className="rb-flyout-pos" side="bottom" align="end" sideOffset={4} collisionPadding={8}>
              <Menu.Popup
                className="rb-flyout side-more-menu"
                aria-label={t("panels.moreTabs")}
                onKeyDown={(e) => e.key === "Escape" && e.stopPropagation()}
              >
                <Menu.RadioGroup value={value} onValueChange={(next) => onChange(next as Id)}>
                  {folded.map((x) => (
                    <Menu.RadioItem key={x.id} value={x.id} closeOnClick className="rb-item">
                      {x.label}
                      {x.extra}
                    </Menu.RadioItem>
                  ))}
                </Menu.RadioGroup>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>
        )}
        <Tabs.Indicator className="tab-indicator side-indicator" />
      </Tabs.List>
      {children}
    </Tabs.Root>
  );
}
