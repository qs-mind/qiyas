import { uiError } from "../i18n/errors";
/** CUI: the ribbon's tabs, groups and buttons as a tree, to hide, show and reorder (up and down
 *  among their own), from the core's default (`get_default_ribbon`) and the saved layout
 *  (`settings.json` `ribbon`); "Reset to default" saves none, so the window's default is drawn. */
import { useEffect, useRef, useState } from "react";
import { getDefaultRibbon, getSettings, setSettings, type RibbonLayout, type Settings } from "../api";
import { useT } from "../i18n";
import { buttonKey, buttonKeys, DEFAULT_RIBBON, editTree, layoutOf, type EditTab } from "./ribbonLayout";

/** `list` with item `i` moved by `by` (-1 up, 1 down). */
function moved<T>(list: T[], i: number, by: number): T[] {
  const j = i + by;
  if (j < 0 || j >= list.length) return list;
  const next = [...list];
  [next[i], next[j]] = [next[j], next[i]];
  return next;
}

type Item = { hidden: boolean };

export function CustomizeUi({ onSaved, onClose }: { onSaved(ribbon: RibbonLayout | null): void; onClose(): void }) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [all, setAll] = useState<RibbonLayout | null>(null);
  const [tree, setTree] = useState<EditTab[]>([]);
  /** "Reset to default" pressed and nothing changed since: save no layout. */
  const [reset, setReset] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    Promise.all([getSettings(), getDefaultRibbon()])
      .then(([s, core]) => {
        setDraft(s);
        setAll(core);
        setTree(editTree(core, s.ribbon ?? null));
      })
      .catch((e) => setError(uiError(e)));
    return () => opener?.focus();
  }, []);

  const change = (next: EditTab[]) => {
    setTree(next);
    setReset(false);
    setError(null);
  };

  async function save() {
    if (!draft) return;
    const layout = reset ? null : layoutOf(tree);
    if (!reset && !layout) {
      setError(t("cui.noTab"));
      return;
    }
    try {
      await setSettings({ ...draft, ribbon: layout });
      onSaved(layout);
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    }
  }

  // The window's own buttons a saved layout can't hold (the core's default lacks them).
  const known = all ? buttonKeys(all) : null;
  const windowOnly = known
    ? DEFAULT_RIBBON.tabs.flatMap((x) => x.groups.flatMap((g) => g.buttons.filter((b) => !known.has(buttonKey(b)))))
    : [];

  /** One row: show or hide, and move up or down among its own. */
  const row = (name: string, item: Item, i: number, count: number, set: (patch: Partial<Item>) => void, move: (by: number) => void) => (
    <span className="cui-row">
      <label className="check">
        <input
          type="checkbox"
          checked={!item.hidden}
          aria-label={t("cui.show", { name })}
          onChange={(e) => set({ hidden: !e.target.checked })}
        />
        <bdi>{name}</bdi>
      </label>
      <button type="button" aria-label={t("cui.up", { name })} disabled={i === 0} onClick={() => move(-1)}>
        ↑
      </button>
      <button type="button" aria-label={t("cui.down", { name })} disabled={i === count - 1} onClick={() => move(1)}>
        ↓
      </button>
    </span>
  );

  const setTab = (ti: number, patch: Partial<EditTab>) => change(tree.map((x, k) => (k === ti ? { ...x, ...patch } : x)));

  return (
    <dialog
      ref={dialog}
      className="settings customize"
      aria-labelledby="cui-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 id="cui-title" className="heading">
          {t("cui.title")}
        </h2>
        {windowOnly.length > 0 && (
          <p className="status">{t("cui.windowOnly", { names: windowOnly.map((b) => t(b.label)).join(", ") })}</p>
        )}
        {draft && (
          <ul role="tree" aria-label={t("cui.tree")} className="cui-tree">
            {tree.map((tab, ti) => (
              <li key={tab.id} role="treeitem" aria-label={t(tab.label)} aria-expanded>
                {row(t(tab.label), tab, ti, tree.length, (p) => setTab(ti, p), (by) => change(moved(tree, ti, by)))}
                <ul role="group">
                  {tab.groups.map((group, gi) => {
                    const setGroups = (groups: EditTab["groups"]) => setTab(ti, { groups });
                    const setGroup = (patch: Partial<EditTab["groups"][number]>) =>
                      setGroups(tab.groups.map((x, k) => (k === gi ? { ...x, ...patch } : x)));
                    return (
                      <li key={group.id} role="treeitem" aria-label={t(group.label)} aria-expanded>
                        {row(t(group.label), group, gi, tab.groups.length, setGroup, (by) => setGroups(moved(tab.groups, gi, by)))}
                        <ul role="group">
                          {group.buttons.map((b, bi) => (
                            <li key={`${buttonKey(b)}-${bi}`} role="treeitem" aria-label={t(b.label)}>
                              {row(
                                t(b.label),
                                b,
                                bi,
                                group.buttons.length,
                                (p) => setGroup({ buttons: group.buttons.map((x, k) => (k === bi ? { ...x, ...p } : x)) }),
                                (by) => setGroup({ buttons: moved(group.buttons, bi, by) }),
                              )}
                            </li>
                          ))}
                        </ul>
                      </li>
                    );
                  })}
                </ul>
              </li>
            ))}
          </ul>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button
            type="button"
            disabled={!all}
            onClick={() => {
              if (!all) return;
              setTree(editTree(all, null));
              setReset(true);
              setError(null);
            }}
          >
            {t("cui.reset")}
          </button>
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!draft}>
            {t("settings.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
