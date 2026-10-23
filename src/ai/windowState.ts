import type { Target } from "../viewport/batch";

/** Read only the currently rendered controls. Hidden panels are identified as hidden. */
export function readPanelState(selection: readonly Target[]) {
  const middle = document.querySelector<HTMLElement>(".middle");
  const layers = document.querySelector<HTMLElement>("[data-layer-state]");
  const leftVisible = !!middle && !middle.classList.contains("left-off");
  const rightVisible = !!middle && !middle.classList.contains("right-off");
  return {
    window: true,
    open_panel: rightVisible ? middle?.dataset.openPanel ?? null : null,
    left_visible: leftVisible,
    right_visible: rightVisible,
    layers: leftVisible && layers?.dataset.layerState ? JSON.parse(layers.dataset.layerState) as unknown : null,
    selection,
    dialogs: Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"]'))
      .filter(el => !el.hidden && el.getAttribute("aria-hidden") !== "true")
      .map(el => el.getAttribute("aria-label") ?? document.getElementById(el.getAttribute("aria-labelledby") ?? "")?.textContent ?? ""),
  };
}
