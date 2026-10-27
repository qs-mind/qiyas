/** An output in the right column (design B): it takes the conversation's place, with a way back.
 *  Its head has the back arrow, its name (a menu to the other outputs), what it holds, an action
 *  and a close button. */
import { Menu } from "@base-ui/react/menu";
import type { ReactNode } from "react";
import { useT } from "../i18n";
import { Glyph } from "./glyphs";
import type { OutputId, OutputInfo } from "./outputs";

type Props = {
  id: OutputId;
  outputs: OutputInfo[];
  /** What it holds: the drawing and the count, as the panel says it. */
  meta?: string;
  /** One button at the end of the head (Export); disabled when there is nothing to do. */
  action?: { label: string; onClick(): void; disabled?: boolean };
  onSelect(id: OutputId): void;
  onBack(): void;
  children: ReactNode;
};

export function ArtifactFrame({ id, outputs, meta, action, onSelect, onBack, children }: Props) {
  const t = useT();
  return (
    <section className="art" aria-label={t(`outputs.${id}`)}>
      <div className="art-head">
        <button type="button" className="ib" aria-label={t("outputs.back")} title={t("outputs.back")} onClick={onBack}>
          <Glyph name="next" mirror />
        </button>
        <Menu.Root modal={false}>
          <Menu.Trigger className="art-title heading" aria-label={t("outputs.switch")}>
            {t(`outputs.${id}`)}
            <Glyph name="down" size={14} />
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Positioner className="rb-flyout-pos" side="bottom" align="start" sideOffset={4} collisionPadding={8}>
              <Menu.Popup className="rb-flyout outputs-menu" aria-label={t("outputs.title")} onKeyDown={(e) => e.key === "Escape" && e.stopPropagation()}>
                <Menu.RadioGroup value={id} onValueChange={(next) => onSelect(next as OutputId)}>
                  {outputs.map((o) => (
                    <Menu.RadioItem key={o.id} value={o.id} closeOnClick className={o.id === id ? "rb-item on" : "rb-item"}>
                      <span className="grow">{t(`outputs.${o.id}`)}</span>
                      <span className="muted">{o.count && o.count > 0 ? o.count : t("outputs.empty")}</span>
                    </Menu.RadioItem>
                  ))}
                </Menu.RadioGroup>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>
        {meta && <span className="art-meta"><bdi>{meta}</bdi></span>}
        <span className="grow" />
        {action && (
          <button type="button" className="exp" disabled={action.disabled} onClick={action.onClick}>{action.label}</button>
        )}
        <button type="button" className="ib" aria-label={t("outputs.close")} title={t("outputs.close")} onClick={onBack}>
          <Glyph name="close" />
        </button>
      </div>
      <div className="art-body">{children}</div>
    </section>
  );
}
