/** The drawing's drafting settings (snaps, ortho, polar, grid, precision) and their dialog. */
import { useState } from "react";
import { setDrafting, type Drafting } from "../api";
import type { DraftingToggle } from "../tools/shortcuts";
import { type T, TOGGLED } from "./shared";

type Deps = {
  run(step: () => Promise<void>): Promise<void>;
  autosave(): Promise<void>;
  /** Says what a key or a click on a switch did: "Ortho on". */
  setNotice(text: string | null): void;
  t: T;
};

export function useDrafting({ run, autosave, setNotice, t }: Deps) {
  /** The drawing's snaps, ortho, polar, grid and display precision; null with no drawing. */
  const [drafting, setDraftingState] = useState<Drafting | null>(null);
  const [draftingOpen, setDraftingOpen] = useState(false);

  /** Makes `next` the drawing's drafting settings and saves them with the project. Throws when the
   *  core refuses them. */
  async function saveDrafting(next: Drafting) {
    await setDrafting(next);
    setDraftingState(next);
    await autosave();
  }

  /** Turns one drafting switch on or off (status bar or F3, F7, F8, F10). Ortho and polar exclude
   *  each other, as in AutoCAD. */
  function toggleDrafting(toggle: DraftingToggle) {
    if (!drafting) return;
    const key = TOGGLED[toggle];
    const next: Drafting = { ...drafting, [key]: !drafting[key] };
    if (toggle === "ortho" && next.ortho) next.polar = false;
    if (toggle === "polar" && next.polar) next.ortho = false;
    run(async () => {
      await saveDrafting(next);
      // The status bar's switch changes too; the key says it in words as well.
      setNotice(t(next[key] ? "drafting.on" : "drafting.off", { name: t(`status.${toggle}`) }));
    });
  }

  return { drafting, setDraftingState, draftingOpen, setDraftingOpen, saveDrafting, toggleDrafting };
}

export type DraftingState = ReturnType<typeof useDrafting>;
