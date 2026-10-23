/** The right-click menu of the drawing, on Base UI's Context Menu: the commands the ribbon and the
 *  command line already have, where the hand is. Move, Copy and Erase start the modify tool on the
 *  selection (they ask for objects when nothing is picked, as from the ribbon); Properties shows
 *  the properties palette; Zoom extents fits the sheet; Ask Mizan sends the selection to Mizan's
 *  composer (only with something selected). Each row says its command line alias in a
 *  small chip. Without a drawing the modify rows are disabled. It wraps the canvas's box: the box
 *  keeps its own props through `render`. */
import { ContextMenu } from "@base-ui/react/context-menu";
import type { ReactElement, ReactNode } from "react";
import { useT } from "../i18n";
import { aliasesFor } from "../shell/ribbonInfo";
import type { Tool } from "./Viewport";

type Props = {
  /** The canvas's box, a `div` with its ref and tab stop. */
  box: ReactElement<Record<string, unknown>>;
  /** Whether a drawing is open: the modify rows need one. */
  hasDrawing: boolean;
  onTool(tool: Tool): void;
  onProperties(): void;
  onZoomExtents(): void;
  /** Opens Mizan with the selection; left out while nothing is selected. */
  onAskMizan?(): void;
  /** The command line's aliases, for the chips; the built-in ones when left out. */
  aliases?: Record<string, string>;
  children: ReactNode;
};

const MODIFY: { tool: Tool; label: string }[] = [
  { tool: "move", label: "modify.move" },
  { tool: "copy", label: "modify.copy" },
  { tool: "erase", label: "modify.erase" },
];

export function CanvasMenu({
  box,
  hasDrawing,
  onTool,
  onProperties,
  onZoomExtents,
  onAskMizan,
  aliases,
  children,
}: Props) {
  const t = useT();
  const chip = (command: string) => {
    const [alias] = aliasesFor(command, aliases);
    return alias ? <kbd dir="auto">{alias}</kbd> : null;
  };
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger render={box}>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Positioner className="rb-flyout-pos" collisionPadding={8}>
          <ContextMenu.Popup
            className="rb-flyout canvas-menu"
            aria-label={t("canvas.menu")}
            // Escape closes the menu only, not the tool behind it.
            onKeyDown={(e) => e.key === "Escape" && e.stopPropagation()}
          >
            {MODIFY.map(({ tool, label }) => (
              <ContextMenu.Item key={tool} className="rb-item" disabled={!hasDrawing} onClick={() => onTool(tool)}>
                <span>{t(label)}</span>
                {chip(tool)}
              </ContextMenu.Item>
            ))}
            <ContextMenu.Separator className="menu-rule" />
            <ContextMenu.Item className="rb-item" disabled={!hasDrawing} onClick={onProperties}>
              <span>{t("props.title")}</span>
            </ContextMenu.Item>
            <ContextMenu.Item className="rb-item" onClick={onZoomExtents}>
              <span>{t("zoomExtents")}</span>
              {chip("zoomExtents")}
            </ContextMenu.Item>
            <ContextMenu.Separator className="menu-rule" />
            <ContextMenu.Item className="rb-item" disabled={!onAskMizan} onClick={() => onAskMizan?.()}>
              <span>{t("canvas.askMizan")}</span>
            </ContextMenu.Item>
          </ContextMenu.Popup>
        </ContextMenu.Positioner>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
