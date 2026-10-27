/** The keyboard shortcuts cheat sheet's content (Ctrl+/). Keys are spelt as `tools/shortcuts.ts`
 *  spells them ("Slash" is shown as "/"); a key starting with "@" is a mouse gesture, an i18n key.
 *  A test keeps this list and the built-in keys in step. */

export type ShortcutRow = { id: string; keys: string[] };
export type ShortcutSection = { id: string; rows: ShortcutRow[] };

export const SHORTCUT_SECTIONS: ShortcutSection[] = [
  {
    id: "file",
    rows: [
      { id: "new", keys: ["Ctrl+N"] },
      { id: "open", keys: ["Ctrl+O"] },
      { id: "save", keys: ["Ctrl+S"] },
      { id: "saveAs", keys: ["Ctrl+Shift+S"] },
      { id: "plot", keys: ["Ctrl+P"] },
      { id: "closeTab", keys: ["Ctrl+W"] },
      { id: "tabs", keys: ["Ctrl+Tab", "Ctrl+Shift+Tab"] },
    ],
  },
  {
    id: "edit",
    rows: [
      { id: "undo", keys: ["Ctrl+Z"] },
      { id: "redo", keys: ["Ctrl+Y", "Ctrl+Shift+Z"] },
      { id: "copy", keys: ["Ctrl+C"] },
      { id: "copyBase", keys: ["Ctrl+Shift+C"] },
      { id: "cut", keys: ["Ctrl+X"] },
      { id: "paste", keys: ["Ctrl+V"] },
      { id: "pasteBlock", keys: ["Ctrl+Shift+V"] },
      { id: "selectAll", keys: ["Ctrl+A"] },
      { id: "erase", keys: ["Delete"] },
      { id: "find", keys: ["Ctrl+F"] },
      { id: "escape", keys: ["Escape"] },
      { id: "repeat", keys: ["Enter", "Space"] },
    ],
  },
  {
    id: "view",
    rows: [
      { id: "zoom", keys: ["@shortcuts.k.wheel"] },
      { id: "pan", keys: ["@shortcuts.k.middleDrag", "Space"] },
      { id: "extents", keys: ["@shortcuts.k.doubleMiddle"] },
      { id: "commandLine", keys: ["Ctrl+9"] },
      { id: "ribbon", keys: ["Ctrl+F1"] },
    ],
  },
  {
    id: "draft",
    rows: [
      { id: "osnap", keys: ["F3"] },
      { id: "grid", keys: ["F7"] },
      { id: "ortho", keys: ["F8"] },
      { id: "polar", keys: ["F10"] },
    ],
  },
  {
    id: "tools",
    rows: [
      { id: "distance", keys: ["D"] },
      { id: "area", keys: ["A"] },
      { id: "count", keys: ["C"] },
      { id: "select", keys: ["S"] },
    ],
  },
  {
    id: "mizan",
    rows: [
      { id: "commands", keys: ["Slash"] },
      { id: "ask", keys: ["Ctrl+K"] },
      { id: "cheatsheet", keys: ["Ctrl+Slash"] },
    ],
  },
];
