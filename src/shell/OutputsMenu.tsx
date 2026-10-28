/** Artifacts, an icon in Mizan's head: drawing sheets, files written in this conversation, and
 *  populated take-off outputs. */
import { Menu } from "@base-ui/react/menu";
import { useState } from "react";
import type { Sheet } from "../api";
import { useT } from "../i18n";
import { Glyph } from "./glyphs";
import { outputGroups, type OutputId, type OutputInfo } from "./outputs";
import type { OutputFile } from "./outputFiles";
import { revealOutputFile } from "./outputFileActions";

type Props = {
  outputs: OutputInfo[];
  /** The output on screen, if one is. */
  shown: OutputId | "file" | null;
  onOpen(id: OutputId): void;
  onOpenFile?(file: OutputFile): void;
  onExportEvidence?(): void;
  sheets?: readonly Sheet[];
  sheet?: string;
  onSheet?(block: string): void;
  files?: readonly OutputFile[];
};

export function OutputsMenu({ outputs, shown, onOpen, onOpenFile, onExportEvidence, sheets = [], sheet, onSheet, files = [] }: Props) {
  const t = useT();
  const groups = outputGroups(outputs, sheets, files);
  const [fileError, setFileError] = useState(false);
  async function fileAction(path: string) {
    setFileError(false);
    try { await revealOutputFile(path); }
    catch { setFileError(true); }
  }
  return (
    <Menu.Root modal={false} onOpenChange={() => setFileError(false)}>
      <Menu.Trigger className={shown ? "ib artifacts-btn on" : "ib artifacts-btn"} aria-label={t("outputs.countLabel", { n: groups.count })}
        title={t("outputs.title")}>
        <Glyph name="artifacts" size={17} />
        {groups.count > 0 && <span className="artifacts-count" aria-hidden="true">{groups.count > 99 ? "99+" : groups.count}</span>}
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner className="rb-flyout-pos" side="bottom" align="end" sideOffset={6} collisionPadding={8}>
          <Menu.Popup className="rb-flyout outputs-menu" aria-label={t("outputs.title")} onKeyDown={(e) => e.key === "Escape" && e.stopPropagation()}>
            {groups.sheets.length > 0 && <Menu.Group>
              <Menu.GroupLabel className="outputs-label">{t("outputs.sheets")}</Menu.GroupLabel>
              {groups.sheets.map((s) => <Menu.Item key={s.block} className={s.block === sheet ? "rb-item on" : "rb-item"}
                onClick={() => onSheet?.(s.block)} disabled={!onSheet}>
                <bdi className="grow output-name">{s.name}</bdi>
              </Menu.Item>)}
            </Menu.Group>}
            {groups.files.length > 0 && <Menu.Group>
              <Menu.GroupLabel className="outputs-label">{t("outputs.files")}</Menu.GroupLabel>
              {groups.files.map((file) => <div className="output-file-row" key={file.path}>
                <Menu.Item className="rb-item output-name" title={file.path} onClick={() => onOpenFile?.(file)} disabled={!onOpenFile}>
                  <bdi>{file.name}</bdi>
                </Menu.Item>
                <Menu.Item className="rb-item output-reveal" closeOnClick={false} aria-label={t("outputs.revealFile", { name: file.name })}
                  onClick={() => void fileAction(file.path)}>{t("outputs.showInFolder")}</Menu.Item>
              </div>)}
            </Menu.Group>}
            {(groups.takeoff.length > 0 || onExportEvidence) && <Menu.Group>
              <Menu.GroupLabel className="outputs-label">{t("outputs.takeoffSection")}</Menu.GroupLabel>
              {groups.takeoff.map((o) => (
                <Menu.Item key={o.id} className={o.id === shown ? "rb-item on" : "rb-item"} onClick={() => onOpen(o.id)}>
                  <span className="grow">{t(`outputs.${o.id}`)}</span>
                  <span className="muted">{o.count}</span>
                </Menu.Item>
              ))}
              {onExportEvidence && <Menu.Item className="rb-item" onClick={onExportEvidence}>
                <span className="grow">{t("exportEvidence")}</span>
              </Menu.Item>}
            </Menu.Group>}
            {groups.count === 0 && <p className="outputs-note">{t("outputs.none")}</p>}
            {fileError && <p className="outputs-note" role="alert">{t("outputs.fileActionFailed")}</p>}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
