import { uiError } from "../i18n/errors";
import { useState, type CSSProperties, type FormEvent, type KeyboardEvent } from "react";
import type { NamedView } from "../api";
import { useT } from "../i18n";
import { RibbonIcon } from "./ribbonIcons";
import { Tip, type TipInfo } from "./Tooltip";
import { useMenu } from "./useMenu";

type Props = {
  /** Reads the drawing's named views, each time the menu opens. */
  load(): Promise<NamedView[]>;
  onGo(view: NamedView): void;
  /** Saves the view on screen as `name`. */
  onSave(name: string): Promise<void>;
  onDelete(name: string): Promise<void>;
  onKey?(e: KeyboardEvent): void;
  /** The button's tooltip. */
  tip?: TipInfo;
};

/** The View tab's Views drop-down: the drawing's named views (choose one to show it, × to delete
 *  it), and Save current view… with a name. */
export function ViewsMenu({ load, onGo, onSave, onDelete, onKey, tip }: Props) {
  const t = useT();
  const menu = useMenu<HTMLDivElement>();
  const [list, setList] = useState<NamedView[]>([]);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The ribbon's panel scrolls sideways, which would clip a menu inside it: the menu is placed
  // on the window under the button instead (its start edge under the button's, RTL too).
  const [at, setAt] = useState<CSSProperties>();

  async function toggle() {
    if (menu.open) {
      menu.setOpen(false);
      return;
    }
    setNaming(false);
    setError(null);
    setList(await load().catch(() => []));
    const r = menu.ref.current?.getBoundingClientRect();
    if (r) {
      const rtl = document.documentElement.dir === "rtl";
      setAt({ position: "fixed", top: r.bottom, ...(rtl ? { right: window.innerWidth - r.right } : { left: r.left }) });
    }
    menu.setOpen(true);
  }

  async function save(e: FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    try {
      await onSave(name.trim());
      setName("");
      menu.setOpen(false);
    } catch (err) {
      setError(uiError(err));
    }
  }

  async function remove(view: string) {
    try {
      await onDelete(view);
      setList(await load());
    } catch (err) {
      setError(uiError(err));
    }
  }

  return (
    <div className="split" ref={menu.ref}>
      <Tip info={tip ?? { name: t("views") }}>
        {(describedBy) => (
          <button
            type="button"
            className="rb"
            aria-label={t("views")}
            aria-haspopup="menu"
            aria-expanded={menu.open}
            aria-describedby={describedBy}
            onClick={toggle}
            onKeyDown={onKey}
            onKeyUp={onKey}
          >
            <RibbonIcon name="views" />
          </button>
        )}
      </Tip>
      {menu.open && (
        <div className="menu" role="menu" aria-label={t("views")} style={at}>
          {list.length === 0 && <div className="menu-note">{t("views.none")}</div>}
          {list.map((v) => (
            <div key={v.name} className="menu-row">
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  menu.setOpen(false);
                  onGo(v);
                }}
              >
                <bdi>{v.name}</bdi>
              </button>
              <button
                type="button"
                className="close"
                aria-label={t("views.deleteNamed", { name: v.name })}
                title={t("views.delete")}
                onClick={() => remove(v.name)}
              >
                ×
              </button>
            </div>
          ))}
          <hr />
          {naming ? (
            <form className="menu-form" onSubmit={save}>
              <label>
                {t("views.name")}
                <input
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.currentTarget.value)}
                />
              </label>
              <button type="submit" disabled={!name.trim()}>
                {t("save")}
              </button>
            </form>
          ) : (
            <button type="button" role="menuitem" onClick={() => setNaming(true)}>
              {t("views.save")}
            </button>
          )}
          {error && (
            <div className="menu-note error-text" role="alert">
              {error}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
