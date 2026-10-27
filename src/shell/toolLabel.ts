/** The name of a tool as its ribbon button says it, for a message ("Again: Distance"). */
import type { T } from "../app/shared";
import { DEFAULT_RIBBON, QUICK_ACCESS } from "./ribbonLayout";

const BUTTONS = [...QUICK_ACCESS, ...DEFAULT_RIBBON.tabs.flatMap((x) => x.groups.flatMap((g) => g.buttons))];

export function toolLabel(tool: string, t: T): string {
  const button = BUTTONS.find((b) => b.kind === "tool" && b.command === tool);
  return button ? t(button.label) : tool.toUpperCase();
}
