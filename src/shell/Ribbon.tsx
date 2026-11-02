import { Menu } from "@base-ui/react/menu";
import { Tabs } from "@base-ui/react/tabs";
import { memo, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { NamedView, Recent, RibbonButton, RibbonGroup, RibbonLayout } from "../api";
import { useT } from "../i18n";
import type { Tool } from "../viewport/Viewport";
import { Glyph } from "./glyphs";
import { RecentsMenu } from "./RecentsMenu";
import { RibbonFlyout } from "./RibbonFlyout";
import { RibbonIcon } from "./ribbonIcons";
import { useStableCallbacks } from "./stableCallbacks";
import { aliasesFor, isPrimaryGroup } from "./ribbonInfo";
import { DEFAULT_RIBBON, QUICK_ACCESS, fittingTabs, isRibbonCommand, type RibbonCommand } from "./ribbonLayout";
import { Tip, TooltipLayer, type TipInfo } from "./Tooltip";
import { Toolbar } from "@base-ui/react/toolbar";
import type { BaseUIEvent } from "@base-ui/react/types";
import { useToolbarRoving } from "./useRoving";
import { ViewsMenu } from "./ViewsMenu";

export type { Tool };

type Props = {
  onAction(command: RibbonCommand): void;
  isEnabled(command: RibbonCommand): boolean;
  isPressed(command: RibbonCommand): boolean;
  /** Commands added through Customize use the core's prompt machine. */
  onCommandLine?(line: string): void;
  loadRecents?(): Promise<Recent[]>;
  onOpenRecent?(path: string): void;
  onClearRecents?(): Promise<void>;
  loadViews?(): Promise<NamedView[]>;
  onGoToView?(view: NamedView): void;
  onSaveView?(name: string): Promise<void>;
  onDeleteView?(name: string): Promise<void>;
  layout?: RibbonLayout | null;
  aliases?: Record<string, string>;
};

// Space is the viewport's pan key: it must never press a focused ribbon button.
const ignoreSpace = (e: KeyboardEvent & Partial<Pick<BaseUIEvent<KeyboardEvent>, "preventBaseUIHandler">>) => {
  if (e.key === " ") {
    e.preventDefault();
    e.preventBaseUIHandler?.();
  }
};

/** The ribbon's collapsed state is kept on this computer. Collapsed, only the tabs' row shows; a tab
 *  clicked shows its panel over the drawing until something else is done (Esc, a click elsewhere, a
 *  tool). Double-click a tab, the arrow at the row's end or Ctrl+F1 collapse and expand it. */
const COLLAPSED_KEY = "qiyas.ribbon.collapsed";

function storedCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) !== "0";
  } catch {
    return true;
  }
}

/** The tab that is the "More" button: it is never the chosen tab itself, but stands for the one
 *  chosen from its menu. */
const MORE = "\u0000more";

export function Ribbon(props: Props) {
  // The handlers are the app's, new on every render of it: the body re-renders for a changed value.
  const { isEnabled, isPressed, ...callbacks } = props;
  const stable = useStableCallbacks(callbacks);
  const commands = [...QUICK_ACCESS, ...(props.layout ?? DEFAULT_RIBBON).tabs.flatMap((t) => t.groups.flatMap((g) => g.buttons))];
  const enabled = commands.map((b) => isRibbonCommand(b.command) && isEnabled(b.command)).join(",");
  const pressed = commands.map((b) => isRibbonCommand(b.command) && isPressed(b.command)).join(",");
  const lookups = useStableCallbacks({ isEnabled, isPressed });
  return (
    <TooltipLayer>
      <RibbonBodyMemo {...stable} {...lookups} enabled={enabled} pressed={pressed} />
    </TooltipLayer>
  );
}

function RibbonBody(props: Props & { enabled: string; pressed: string }) {
  const t = useT();
  const layout = props.layout ?? DEFAULT_RIBBON;
  const tabs = layout.tabs;
  const [chosen, setTab] = useState<string>(tabs[0]?.id ?? "");
  // A layout without the tab chosen (saved meanwhile) shows its first.
  const tab = tabs.find((x) => x.id === chosen) ?? tabs[0];
  const [tabCount, setTabCount] = useState<number | null>(null);
  const tabRow = useRef<HTMLDivElement>(null);
  const tabSizer = useRef<HTMLDivElement>(null);
  const fallbackCount = tabs.length > 4 ? 3 : tabs.length;
  const lead = tabs.slice(0, tabCount ?? fallbackCount);
  const more = tabs.slice(lead.length);
  const inMore = tab !== undefined && more.includes(tab);
  useLayoutEffect(() => {
    const row = tabRow.current;
    const sizer = tabSizer.current;
    if (!row || !sizer) return;
    const measure = () => {
      if (!row.clientWidth) { setTabCount(null); return; }
      const style = getComputedStyle(row);
      const gap = parseFloat(style.columnGap) || 0;
      const fixed = Array.from(row.children).filter((child) =>
        !child.classList.contains("ribbon-tablist") && child !== sizer);
      const occupied = fixed.reduce((sum, child) => {
        const css = getComputedStyle(child);
        return sum + child.getBoundingClientRect().width + (parseFloat(css.marginInlineStart) || 0) + (parseFloat(css.marginInlineEnd) || 0);
      }, 0);
      const widths = Array.from(sizer.children).map((child) => child.getBoundingClientRect().width);
      const moreWidth = widths.pop() ?? 0;
      if (!moreWidth || widths.some((width) => width <= 0)) { setTabCount(null); return; }
      const available = row.clientWidth - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0) - occupied - fixed.length * gap;
      setTabCount(fittingTabs(widths, available, moreWidth));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(row);
    observer.observe(sizer);
    for (const child of row.children) if (!child.classList.contains("ribbon-tablist")) observer.observe(child);
    let alive = true;
    void document.fonts?.ready.then(() => { if (alive) measure(); });
    return () => { alive = false; observer.disconnect(); };
  }, [tabs, t, tab?.label]);
  const quick = useToolbarRoving();
  /** The flyout open: the key of a group's "⋯" list, or `MORE` for the tabs' menu. */
  const [open, setOpen] = useState<string | null>(null);
  const [collapsed, setCollapsedNow] = useState(storedCollapsed);
  /** A collapsed ribbon's panel shown over the drawing for a moment. */
  const [peek, setPeek] = useState(false);
  const showPanel = !collapsed || peek;
  const setCollapsed = (next: boolean) => {
    setCollapsedNow(next);
    setPeek(false);
    setOpen(null);
    try {
      localStorage.setItem(COLLAPSED_KEY, next ? "1" : "0");
    } catch {
      // Not kept; it still collapses for this session.
    }
  };

  // How many buttons each group of the tab on screen shows in this width (`main`, or fewer when
  // the panel would need to scroll), by tab and group.
  const [fit, setFit] = useState<Record<string, number>>({});
  const [width, setWidth] = useState(0);
  const panel = useRef<HTMLDivElement>(null);

  /** What a button is and does now; no `run` is a disabled button ("coming soon"). */
  function resolve(b: RibbonButton): { label: string; run?: () => void; pressed?: boolean } {
    if (b.kind === "command") {
      return { label: b.label, run: props.onCommandLine && (() => props.onCommandLine!(b.command)) };
    }
    if (!isRibbonCommand(b.command)) return { label: b.label };
    const command = b.command;
    const pressed = props.isPressed(command);
    const label = command === "split" && b.label === "vport.two" && pressed ? "vport.one" : b.label;
    return {
      label,
      pressed,
      run: props.isEnabled(command) ? () => {
        props.onAction(b.kind === "tool" && pressed && command !== "select" ? "select" : command);
        if (b.kind === "tool" && collapsed) setPeek(false);
      } : undefined,
    };
  }

  /** The tooltip of a command: its name, what it does, its alias; "coming soon" when disabled. */
  function tipFor(command: string, name: string, disabled: boolean): TipInfo {
    const key = `tip.${command}`;
    const text = t(key);
    const aliases = aliasesFor(command, props.aliases);
    return {
      name: name.replace(/…$/, ""),
      text: text === key ? undefined : text,
      alias: aliases.length > 0 ? `${t("ribbon.alias")} · ${aliases.join(" · ")}` : undefined,
      note: disabled ? t("comingSoon") : undefined,
    };
  }

  /** Headline tools show their names; compact tools keep the name in their tooltip. */
  function iconButton(b: RibbonButton, key: string, primary: boolean, toolbar: boolean) {
    const r = resolve(b);
    const name = t(r.label);
    const Button = toolbar ? Toolbar.Button : "button";
    return (
      <Tip key={key} info={tipFor(b.command, name, !r.run)}>
        {(describedBy) => (
          <Button
            {...(toolbar ? { focusableWhenDisabled: false } : {})}
            type="button"
            className={primary ? "rb primary" : "rb"}
            aria-label={name}
            aria-pressed={b.kind === "action" ? undefined : r.pressed}
            aria-describedby={describedBy}
            disabled={!r.run}
            onClick={r.run}
            onKeyDown={ignoreSpace}
            onKeyUp={ignoreSpace}
          >
            <RibbonIcon name={b.kind === "command" ? "aliases" : b.command} />
            {primary && <span className="rb-label">{name}</span>}
          </Button>
        )}
      </Tip>
    );
  }

  /** One button of the layout, as its kind and command say. */
  function draw(b: RibbonButton, key: string, primary = false, toolbar = false) {
    if (b.command === "open" && props.loadRecents && props.onOpenRecent && props.onClearRecents) {
      return (
        <RecentsMenu
          key={key}
          toolbar={toolbar}
          onOpen={() => props.onAction("open")}
          load={props.loadRecents}
          onOpenRecent={props.onOpenRecent}
          onClear={props.onClearRecents}
          onKey={ignoreSpace}
          tip={tipFor("open", t("open"), false)}
        />
      );
    }
    if (b.command === "views" && props.loadViews && props.onGoToView && props.onSaveView && props.onDeleteView) {
      return (
        <ViewsMenu
          key={key}
          load={props.loadViews}
          onGo={props.onGoToView}
          onSave={props.onSaveView}
          onDelete={props.onDeleteView}
          onKey={ignoreSpace}
          tip={tipFor("views", t("views"), false)}
        />
      );
    }
    return iconButton(b, key, primary, toolbar);
  }

  /** A group's buttons: those on show, and those behind its "⋯" button. Views, a menu of its own,
   *  always stays on show. */
  function parts(tabId: string, g: RibbonGroup) {
    const key = `${tabId}/${g.id}`;
    const n = g.buttons.length;
    const cap = Math.min(g.main ?? n, n);
    const shown = Math.min(cap, fit[key] ?? cap);
    const on = (b: RibbonButton, i: number) => i < shown || b.command === "views";
    return {
      key,
      n,
      shown,
      visible: g.buttons.filter(on),
      rest: g.buttons.filter((b, i) => !on(b, i)),
    };
  }

  // The panel never needs a scrollbar if it can help it: too wide, the group with the most
  // buttons on show puts more of them behind its "⋯" button, until it fits (or nothing is left to
  // put away). A new width, tab or layout starts again from what the groups want to show.
  const seen = useRef({ at: "", layout });
  useLayoutEffect(() => {
    const el = panel.current;
    if (!el || !tab) return;
    const at = `${tab.id}|${width}`;
    const changed = seen.current.at !== at || seen.current.layout !== layout;
    seen.current = { at, layout };
    if (changed && Object.keys(fit).length > 0) {
      setFit({});
      return;
    }
    if (el.scrollWidth <= el.clientWidth) return;
    let pick: { key: string; shown: number; n: number } | null = null;
    for (const g of tab.groups) {
      const p = parts(tab.id, g);
      if (p.shown > 0 && !(p.shown === p.n && p.n < 2) && (!pick || p.shown >= pick.shown)) pick = p;
    }
    // The first button put away is not worth it (the "⋯" takes its place): put two.
    if (pick) {
      const { key, shown, n } = pick;
      setFit((f) => ({ ...f, [key]: shown === n ? Math.max(0, n - 2) : shown - 1 }));
    }
  });
  useEffect(() => {
    const el = panel.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(() => setWidth(el.clientWidth));
    watch.observe(el);
    return () => watch.disconnect();
  }, [showPanel]);

  // Ctrl+F1 collapses and expands, as in Office; Escape and a click elsewhere put a peeking panel away.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.key === "F1") {
        e.preventDefault();
        if (!e.repeat) setCollapsed(!collapsed);
      } else if (e.key === "Escape" && peek) {
        setPeek(false);
      }
    };
    const onPointer = (e: PointerEvent) => {
      if (peek && !(e.target as Element | null)?.closest?.(".ribbon, .rb-flyout-pos, .rb-tooltip")) setPeek(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });
  // Another tab: its flyouts are closed.
  useEffect(() => setOpen(null), [tab?.id]);

  /** One row of a flyout: the icon and the name. */
  function flyoutItem(b: RibbonButton, key: string) {
    const r = resolve(b);
    const checkable = b.kind !== "action";
    const content = (
      <>
        <RibbonIcon name={b.kind === "command" ? "aliases" : b.command} />
        <span>{t(r.label)}</span>
      </>
    );
    const shared = {
      className: checkable && r.pressed ? "rb-item on" : "rb-item",
      disabled: !r.run,
      title: r.run ? undefined : t("comingSoon"),
      onKeyDown: ignoreSpace,
      onKeyUp: ignoreSpace,
    };
    return checkable ? (
      <Menu.CheckboxItem key={key} {...shared} checked={!!r.pressed} onCheckedChange={() => r.run?.()} closeOnClick>
        {content}
      </Menu.CheckboxItem>
    ) : (
      <Menu.Item key={key} {...shared} onClick={() => r.run?.()}>
        {content}
      </Menu.Item>
    );
  }

  const groupBox = (g: RibbonGroup) => {
    const name = t(g.label);
    const p = parts(tab!.id, g);
    const label = t("ribbon.groupMore", { name });
    const primary = isPrimaryGroup(g);
    const columns = p.rest.length > 8 ? 3 : p.rest.length > 4 ? 2 : 1;
    return (
      <div key={g.id} className="ribbon-group" role="group" aria-label={name}>
        <div className={primary ? "ribbon-group-buttons primary" : "ribbon-group-buttons"}>
          {p.visible.map((b, i) => draw(b, `${i}`, primary))}
          {p.rest.length > 0 && (
            <RibbonFlyout
              open={open === p.key}
              onOpenChange={(on) => setOpen(on ? p.key : null)}
              label={name}
              rows={Math.ceil(p.rest.length / columns)}
              items={p.rest.map((b, i) => flyoutItem(b, `${i}`))}
            >
              <Tip info={{ name: label }}>
                {(describedBy) => (
                  <Menu.Trigger
                    className={primary ? "rb primary" : "rb"}
                    aria-label={label}
                    aria-describedby={describedBy}
                    onKeyDown={ignoreSpace}
                    onKeyUp={ignoreSpace}
                  >
                    <RibbonIcon name="more" />
                  </Menu.Trigger>
                )}
              </Tip>
            </RibbonFlyout>
          )}
        </div>
        <div className="ribbon-group-label" aria-hidden="true">
          {name}
        </div>
      </div>
    );
  };

  // Measure first: the tabs' row (the app name, the quick-access bar and the tabs that fit),
  // then the panel of the tab chosen, each group a box of icon buttons with its name under them.
  // The panel does not wrap: a group puts buttons behind its "⋯" as the window narrows, and only
  // when even that is not enough does it scroll sideways (`theme.css`).
  const chosenValue = inMore ? MORE : (tab?.id ?? null);
  return (
    <Tabs.Root
      className={collapsed ? "ribbon collapsed" : "ribbon"}
      value={chosenValue}
      onValueChange={(value) => {
        // "More" opens its menu; the tab it stands for is chosen there.
        if (value !== MORE) {
          setTab(String(value));
          if (collapsed) setPeek(true);
        }
      }}
    >
      <div className="ribbon-tabs" ref={tabRow}>
        <span className="app-name heading">{t("appName")}</span>
        <Toolbar.Root
          {...quick.props}
          className="quick-access"
          aria-label={t("ribbon.quickAccess")}
        >
          {QUICK_ACCESS.map((b) => draw(b, b.command, false, true))}
        </Toolbar.Root>
        <Tabs.List className="ribbon-tablist">
          {lead.map((x) => (
            <Tabs.Tab
              key={x.id}
              value={x.id}
              className="ribbon-tab"
              // A tab clicked again while the ribbon is collapsed shows its panel too; two quick
              // clicks collapse or expand it (Ctrl+F1 and the arrow do the same from the keyboard).
              onClick={() => collapsed && setPeek(true)}
              onDoubleClick={() => setCollapsed(!collapsed)}
            >
              {t(x.label)}
            </Tabs.Tab>
          ))}
          {more.length > 0 && (
            <RibbonFlyout
              open={open === MORE}
              onOpenChange={(on) => setOpen(on ? MORE : null)}
              label={t("ribbon.more")}
              rows={more.length}
              items={
                <Menu.RadioGroup value={tab?.id ?? ""} onValueChange={(value) => setTab(String(value))}>
                  {more.map((x) => (
                    <Menu.RadioItem
                      key={x.id}
                      value={x.id}
                      closeOnClick
                      className={x === tab ? "rb-item on" : "rb-item"}
                      onKeyDown={ignoreSpace}
                      onKeyUp={ignoreSpace}
                    >
                      <span>{t(x.label)}</span>
                    </Menu.RadioItem>
                  ))}
                </Menu.RadioGroup>
              }
            >
              <Menu.Trigger render={<Tabs.Tab value={MORE} className="ribbon-tab ribbon-more" />}>
                {inMore ? t(tab.label) : t("ribbon.more")}
                <RibbonIcon name="chevron" />
              </Menu.Trigger>
            </RibbonFlyout>
          )}
          <Tabs.Indicator className="tab-indicator" />
        </Tabs.List>
        <button
          type="button"
          className="rb ribbon-collapse"
          aria-expanded={!collapsed}
          aria-label={t(collapsed ? "ribbon.expand" : "ribbon.collapse")}
          title={`${t(collapsed ? "ribbon.expand" : "ribbon.collapse")} (Ctrl+F1)`}
          onClick={() => setCollapsed(!collapsed)}
          onKeyDown={ignoreSpace}
          onKeyUp={ignoreSpace}
        >
          <Glyph name="down" size={14} />
        </button>
        <div className="ribbon-tab-sizer" ref={tabSizer} aria-hidden="true">
          {tabs.map((x) => <span key={x.id} className="ribbon-tab" style={x === tab ? { fontWeight: 500 } : undefined}>{t(x.label)}</span>)}
          <span className="ribbon-tab ribbon-more" style={inMore ? { fontWeight: 500 } : undefined}>{inMore ? t(tab.label) : t("ribbon.more")}<RibbonIcon name="chevron" /></span>
        </div>
      </div>
      {tab && showPanel && (
        <Tabs.Panel value={chosenValue} className="ribbon-panel" ref={panel} tabIndex={-1}>
          {tab.groups.map(groupBox)}
        </Tabs.Panel>
      )}
    </Tabs.Root>
  );
}

const RibbonBodyMemo = memo(RibbonBody);
