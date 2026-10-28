import { Toolbar } from "@base-ui/react/toolbar";
import { useState, type CSSProperties, type KeyboardEvent } from "react";
import type { Recent } from "../api";
import { useT } from "../i18n";
import { RibbonIcon } from "./ribbonIcons";
import { Tip, type TipInfo } from "./Tooltip";
import { useMenu } from "./useMenu";

type Props = {
  /** Open…: the file dialog. */
  onOpen(): void;
  /** Reads the recent list, each time the menu opens. */
  load(): Promise<Recent[]>;
  onOpenRecent(path: string): void;
  onClear(): Promise<void>;
  onKey?(e: KeyboardEvent): void;
  /** The tooltip of the Open button. */
  tip?: TipInfo;
  /** Both halves are stops of the quick-access toolbar. */
  toolbar?: boolean;
};

/** The ribbon's Open split button: Open… and, beside it, the recent drawings (file name, its
 *  folder muted; greyed with "(missing)" when the file is gone), then Clear recents. */
export function RecentsMenu({ onOpen, load, onOpenRecent, onClear, onKey, tip, toolbar = false }: Props) {
  const t = useT();
  const Button = toolbar ? Toolbar.Button : "button";
  const menu = useMenu<HTMLDivElement>();
  const [list, setList] = useState<Recent[]>([]);
  // The tabs' row scrolls sideways, which would clip a menu inside it: the menu is placed on the
  // window under the button instead (its start edge under the button's, RTL too).
  const [at, setAt] = useState<CSSProperties>();

  async function toggle() {
    if (menu.open) {
      menu.setOpen(false);
      return;
    }
    setList(await load().catch(() => []));
    const r = menu.ref.current?.getBoundingClientRect();
    if (r) {
      const rtl = document.documentElement.dir === "rtl";
      setAt({ position: "fixed", top: r.bottom, ...(rtl ? { right: window.innerWidth - r.right } : { left: r.left }) });
    }
    menu.setOpen(true);
  }

  return (
    <div className="split" ref={menu.ref}>
      <Tip info={tip ?? { name: t("open") }}>
        {(describedBy) => (
          <Button type="button" className="rb" aria-label={t("open")} aria-describedby={describedBy} onClick={onOpen} onKeyDown={onKey} onKeyUp={onKey}>
            <RibbonIcon name="open" />
          </Button>
        )}
      </Tip>
      <Tip info={{ name: t("recent.title") }}>
        {(describedBy) => (
          <Button
            type="button"
            className="rb split-arrow"
            aria-haspopup="menu"
            aria-expanded={menu.open}
            aria-label={t("recent.title")}
            aria-describedby={describedBy}
            onClick={toggle}
            onKeyDown={onKey}
            onKeyUp={onKey}
          >
            <RibbonIcon name="chevron" />
          </Button>
        )}
      </Tip>
      {menu.open && (
        <div className="menu" role="menu" aria-label={t("recent.title")} style={at}>
          {list.length === 0 && <div className="menu-note">{t("recent.none")}</div>}
          {list.map((r) => (
            <button
              key={r.path}
              type="button"
              role="menuitem"
              className="recent"
              disabled={r.missing}
              title={r.path}
              onClick={() => {
                menu.setOpen(false);
                onOpenRecent(r.path);
              }}
            >
              <span>
                <bdi>{r.name}</bdi>
                {r.missing && ` ${t("recent.missing")}`}
              </span>
              <span className="muted">
                <bdi>{r.folder}</bdi>
              </span>
            </button>
          ))}
          {list.length > 0 && (
            <>
              <hr />
              <button
                type="button"
                role="menuitem"
                onClick={async () => {
                  await onClear().catch(() => {});
                  setList([]);
                  menu.setOpen(false);
                }}
              >
                {t("recent.clear")}
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
