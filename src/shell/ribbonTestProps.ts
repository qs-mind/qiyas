import { isRibbonTool, type RibbonCommand } from "./ribbonLayout";

/** Small command tables for tests that enable only the actions they exercise. */
export function ribbonProps(
  handlers: Partial<Record<RibbonCommand, (command: RibbonCommand) => void>> = {},
  onAction: (command: RibbonCommand) => void = () => {},
  pressed: Partial<Record<RibbonCommand, boolean>> = {},
) {
  const state = { select: true, groupSelect: true, ucsIcon: true, ...pressed };
  return {
    onAction: (command: RibbonCommand) => (handlers[command] ?? onAction)(command),
    isEnabled: (command: RibbonCommand) => isRibbonTool(command) || command === "open" || command === "zoomExtents" || !!handlers[command],
    isPressed: (command: RibbonCommand) => state[command] === true,
  };
}
