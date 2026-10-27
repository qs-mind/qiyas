/** The top bar's "Outputs" button (design B): a menu of what Mizan produced (take-off, BOQ,
 *  conflicts, take-off report). Choosing one shows it in the right column in place of the chat. */
import { Menu } from "@base-ui/react/menu";
import { useT } from "../i18n";
import { Glyph } from "./glyphs";
import { filledOutputs, type OutputId, type OutputInfo } from "./outputs";

type Props = {
  outputs: OutputInfo[];
  /** The output on screen, if one is. */
  shown: OutputId | null;
  onOpen(id: OutputId): void;
  onExportEvidence?(): void;
};

export function OutputsMenu({ outputs, shown, onOpen, onExportEvidence }: Props) {
  const t = useT();
  const filled = filledOutputs(outputs);
  return (
    <Menu.Root modal={false}>
      <Menu.Trigger className={shown ? "obtn on" : "obtn"} aria-label={t("outputs.title")}>
        <Glyph name="outputs" />
        <span className="obtn-label">{t("outputs.title")}</span>
        {filled > 0 && <span className="obtn-count" aria-label={t("outputs.filled", { n: filled })}>{filled}</span>}
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner className="rb-flyout-pos" side="bottom" align="end" sideOffset={6} collisionPadding={8}>
          <Menu.Popup className="rb-flyout outputs-menu" aria-label={t("outputs.title")} onKeyDown={(e) => e.key === "Escape" && e.stopPropagation()}>
            {outputs.map((o) => (
              <Menu.Item key={o.id} className={o.id === shown ? "rb-item on" : "rb-item"} onClick={() => onOpen(o.id)}>
                <span className="grow">{t(`outputs.${o.id}`)}</span>
                <span className="muted">{o.count && o.count > 0 ? o.count : t("outputs.empty")}</span>
              </Menu.Item>
            ))}
            <Menu.Item className="rb-item" onClick={onExportEvidence} disabled={!onExportEvidence}>
              <span className="grow">{t("exportEvidence")}</span>
            </Menu.Item>
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
