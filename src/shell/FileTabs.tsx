import type { MouseEvent } from "react";
import type { TabInfo } from "../api";
import { useT } from "../i18n";
import { useRoving } from "./useRoving";

type Props = {
  tabs: TabInfo[];
  active: number | null;
  onSelect(id: number): void;
  onClose(id: number): void;
};

/** The tab `step` places after `active` (-1: before), going round; null with no tabs. */
export function cycleTab(tabs: TabInfo[], active: number | null, step: 1 | -1): number | null {
  if (tabs.length === 0) return null;
  const at = tabs.findIndex((t) => t.id === active);
  return tabs[(at + step + tabs.length) % tabs.length].id;
}

/** One tab per open drawing, above the drawing: its file name, a dot while it has unsaved changes,
 *  and × to close it (a middle click closes too). */
export function FileTabs({ tabs, active, onSelect, onClose }: Props) {
  const t = useT();
  const roving = useRoving(
    tabs.length,
    tabs.findIndex((tab) => tab.id === active),
  );
  if (tabs.length === 0) return null;
  // The middle button closes, as in a browser; it must not start the web view's autoscroll.
  const middle = (id: number) => (e: MouseEvent) => {
    if (e.button !== 1) return;
    e.preventDefault();
    onClose(id);
  };
  return (
    <div className="file-tabs" role="tablist" aria-label={t("fileTabs")} {...roving.props}>
      {tabs.map((tab, i) => (
        <div
          key={tab.id}
          className="file-tab"
          data-active={tab.id === active || undefined}
          title={tab.path}
          onMouseDown={(e) => e.button === 1 && e.preventDefault()}
          onAuxClick={middle(tab.id)}
        >
          <button
            type="button"
            role="tab"
            aria-selected={tab.id === active}
            tabIndex={roving.tabIndex(i)}
            onClick={() => onSelect(tab.id)}
          >
            <bdi>{tab.title}</bdi>
            {tab.status && tab.status !== "idle" && <span className={`workspace-status ${tab.status}`} title={t(`workspace.${tab.status}`)} aria-label={t(`workspace.${tab.status}`)}>●</span>}
            {tab.busy && tab.id === active && <small className="workspace-caption" aria-hidden="true">{t(tab.status === "queued" ? "workspace.queued" : "workspace.working")}</small>}
            {tab.dirty && (
              <span className="dirty" aria-label={t("tab.unsaved")}>
                {" •"}
              </span>
            )}
          </button>
          <button
            type="button"
            className="close"
            aria-label={t("tab.close", { name: tab.title })}
            title={t("tab.close", { name: tab.title })}
            onClick={() => onClose(tab.id)}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
