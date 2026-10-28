/** An output in the right column (design B): it takes the conversation's place, with a way back.
 *  Its head has the back arrow, its name (a menu to the other outputs), what it holds, an action
 *  and a close button. */
import { Menu } from "@base-ui/react/menu";
import { useState, type ReactNode } from "react";
import { useT } from "../i18n";
import { Glyph } from "./glyphs";
import { hasOutput, type OutputId, type OutputInfo } from "./outputs";
import type { OutputFile } from "./outputFiles";
import { openOutputFile, revealOutputFile } from "./outputFileActions";

type Props = ({ id: OutputId; file?: never } | { id: "file"; file: OutputFile }) & {
  outputs: OutputInfo[];
  /** What it holds: the drawing and the count, as the panel says it. */
  meta?: string;
  /** The output's buttons below the title (Export, Import BOQ, Compare...), in order; each
   *  disabled when there is nothing to do. The panel under the head has no buttons of its own for
   *  these. */
  actions?: { id: string; label: string; onClick(): void; disabled?: boolean; primary?: boolean }[];
  onSelect(id: OutputId): void;
  onBack(): void;
  children: ReactNode;
};

export function ArtifactFrame({ id, file, outputs, meta, actions = [], onSelect, onBack, children }: Props) {
  const t = useT();
  const title = id === "file" ? file.name : t(`outputs.${id}`);
  const [fileError, setFileError] = useState(false);
  async function fileAction(reveal = false) {
    if (!file) return;
    setFileError(false);
    try { await (reveal ? revealOutputFile(file.path) : openOutputFile(file.path)); }
    catch { setFileError(true); }
  }
  return (
    <section className="art" aria-label={title}>
      <div className="art-head">
        <button type="button" className="ib" aria-label={t("outputs.back")} title={t("outputs.back")} onClick={onBack}>
          <Glyph name="next" mirror />
        </button>
        <h2 className="art-heading" aria-label={title}>
        {file ? <bdi className="art-title heading">{title}</bdi> : <Menu.Root modal={false}>
          <Menu.Trigger className="art-title heading" aria-label={t("outputs.switch")}>
            {t(`outputs.${id}`)}
            <Glyph name="down" size={14} />
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Positioner className="rb-flyout-pos" side="bottom" align="start" sideOffset={4} collisionPadding={8}>
              <Menu.Popup className="rb-flyout outputs-menu" aria-label={t("outputs.title")} onKeyDown={(e) => e.key === "Escape" && e.stopPropagation()}>
                <Menu.RadioGroup value={id} onValueChange={(next) => onSelect(next as OutputId)}>
                  {outputs.filter(hasOutput).map((o) => (
                    <Menu.RadioItem key={o.id} value={o.id} closeOnClick className={o.id === id ? "rb-item on" : "rb-item"}>
                      <span className="grow">{t(`outputs.${o.id}`)}</span>
                      <span className="muted">{o.count}</span>
                    </Menu.RadioItem>
                  ))}
                </Menu.RadioGroup>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>}
        </h2>
        <span className="grow" />
        <button type="button" className="ib" aria-label={t("outputs.close")} title={t("outputs.close")} onClick={onBack}>
          <Glyph name="close" />
        </button>
      </div>
      {(meta || file || actions.length > 0) && <div className="art-toolbar">
        {meta && <span className="art-meta"><bdi>{meta}</bdi></span>}
        <span className="grow" />
        {file && <>
          <button type="button" className="exp" onClick={() => void fileAction()}>{t("outputs.openFile")}</button>
          <button type="button" className="exp" onClick={() => void fileAction(true)}>{t("outputs.showInFolder")}</button>
        </>}
        {actions.map((a) => (
          <button key={a.id} type="button" className={a.primary ? "exp primary" : "exp"} disabled={a.disabled} onClick={a.onClick}>{a.label}</button>
        ))}
      </div>}
      {fileError && <p className="outputs-note" role="alert">{t("outputs.fileActionFailed")}</p>}
      <div className="art-body">{children}</div>
    </section>
  );
}
