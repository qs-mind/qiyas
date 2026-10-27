import type { Tool } from "../shell/Ribbon";

/** A drafting switch the function keys turn on and off, as in AutoCAD. */
export type DraftingToggle = "osnap" | "grid" | "ortho" | "polar";

/** What a key press asks for: a tool, a drafting switch, or one of the actions below. */
export type Shortcut =
  | Tool
  | DraftingToggle
  | "undo"
  | "redo"
  | "save"
  | "saveAs"
  | "open"
  | "plot"
  | "find"
  | "selectAll"
  | "cut"
  | "askMizan"
  | "cheatsheet"
  | "new"
  | "escape"
  | "finish"
  | "commandLine"
  | "nextTab"
  | "prevTab"
  | "closeTab";

export type Keys = Pick<KeyboardEvent, "code" | "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">;

const TOGGLE_CODES: Record<string, DraftingToggle> = {
  F3: "osnap",
  F7: "grid",
  F8: "ortho",
  F10: "polar",
};

const TOOL_CODES: Record<string, Tool> = {
  KeyD: "distance",
  KeyA: "area",
  KeyC: "count",
  KeyS: "select",
};

/** The shortcut for a key press, or null. Matched on `code` (the physical key), not `key`, so the
 *  shortcuts work the same with an Arabic keyboard layout. `user` (the Options dialog's, command
 *  to key) comes first; a command it gives a key no longer answers its built-in one. */
export function shortcutFor(e: Keys, user: Record<string, string> = {}): Shortcut | null {
  const mine = userShortcut(e, user);
  if (mine) return mine as Shortcut;
  const builtIn = builtInShortcut(e);
  return builtIn !== null && builtIn in user ? null : builtIn;
}

/** The command the user gave the key pressed, or null. */
export function userShortcut(e: Keys, user: Record<string, string>): string | null {
  const pressed = keyOf(e);
  if (!pressed) return null;
  return Object.entries(user).find(([, key]) => normalizeKey(key) === pressed)?.[0] ?? null;
}

function builtInShortcut(e: Keys): Shortcut | null {
  if (e.altKey || e.metaKey) return null;
  if (e.ctrlKey) {
    if (e.code === "KeyZ") return e.shiftKey ? "redo" : "undo";
    if (e.code === "KeyY") return "redo";
    // As in AutoCAD: Ctrl+S saves, Ctrl+Shift+S saves as, Ctrl+N and Ctrl+O make and open a drawing,
    // Ctrl+P plots, Ctrl+A selects all.
    if (e.code === "KeyS") return e.shiftKey ? "saveAs" : "save";
    if (e.code === "KeyN" && !e.shiftKey) return "new";
    if (e.code === "KeyO" && !e.shiftKey) return "open";
    if (e.code === "KeyP" && !e.shiftKey) return "plot";
    if (e.code === "KeyA" && !e.shiftKey) return "selectAll";
    // Ctrl+F is Find and replace here (AutoCAD's Ctrl+F is the running object snap, F3 here).
    if (e.code === "KeyF" && !e.shiftKey) return "find";
    // Ctrl+X cuts (CUTCLIP): copy, then erase.
    if (e.code === "KeyX" && !e.shiftKey) return "cut";
    // Mizan: Ctrl+K goes to his box; Ctrl+/ shows the shortcuts.
    if (e.code === "KeyK" && !e.shiftKey) return "askMizan";
    if (e.code === "Slash" && !e.shiftKey) return "cheatsheet";
    // Shows and hides the command line, as in AutoCAD.
    if (e.code === "Digit9" && !e.shiftKey) return "commandLine";
    // The file tabs, as in a browser.
    if (e.code === "Tab") return e.shiftKey ? "prevTab" : "nextTab";
    if (e.code === "KeyW" && !e.shiftKey) return "closeTab";
    // The clipboard, as in AutoCAD: Ctrl+Shift+C copies with a base point, Ctrl+Shift+V pastes
    // as a block.
    if (e.code === "KeyC") return e.shiftKey ? "copybase" : "copyclip";
    if (e.code === "KeyV") return e.shiftKey ? "pasteblock" : "pasteclip";
    return null;
  }
  if (e.code === "Escape") return "escape";
  // "/" goes to Mizan's box with a slash typed, which lists his commands.
  if (e.code === "Slash" && !e.shiftKey) return "askMizan";
  // Delete erases what is selected (ERASE takes the selection as it is), as in AutoCAD.
  if (e.code === "Delete" && !e.shiftKey) return "erase";
  if (!e.shiftKey && e.code in TOGGLE_CODES) return TOGGLE_CODES[e.code];
  if (e.code === "Enter" || e.code === "NumpadEnter") return "finish";
  if (e.shiftKey) return null;
  return TOOL_CODES[e.code] ?? null;
}

/** The keys above as the Options dialog lists them, command and key (`normalizeKey` spelling). */
export const BUILT_IN_SHORTCUTS: [string, string][] = [
  ["undo", "Ctrl+Z"],
  ["redo", "Ctrl+Shift+Z"],
  ["redo", "Ctrl+Y"],
  ["save", "Ctrl+S"],
  ["saveAs", "Ctrl+Shift+S"],
  ["new", "Ctrl+N"],
  ["open", "Ctrl+O"],
  ["plot", "Ctrl+P"],
  ["find", "Ctrl+F"],
  ["selectAll", "Ctrl+A"],
  ["cut", "Ctrl+X"],
  ["askMizan", "Slash"],
  ["askMizan", "Ctrl+K"],
  ["cheatsheet", "Ctrl+Slash"],
  ["commandLine", "Ctrl+9"],
  ["nextTab", "Ctrl+Tab"],
  ["prevTab", "Ctrl+Shift+Tab"],
  ["closeTab", "Ctrl+W"],
  ["copyclip", "Ctrl+C"],
  ["copybase", "Ctrl+Shift+C"],
  ["pasteclip", "Ctrl+V"],
  ["pasteblock", "Ctrl+Shift+V"],
  ["escape", "Escape"],
  ["erase", "Delete"],
  ["finish", "Enter"],
  ["osnap", "F3"],
  ["grid", "F7"],
  ["ortho", "F8"],
  ["polar", "F10"],
  ["distance", "D"],
  ["area", "A"],
  ["count", "C"],
  ["select", "S"],
];

/** The keys a shortcut may end in besides letters, digits and F1 to F24 (as the core's). */
const NAMED_KEYS = [
  "Slash",
  "Escape",
  "Enter",
  "Tab",
  "Space",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
];
const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"] as const;

/** A key spelt as the core keeps it (`settings::shortcut_key`): modifiers in the order Ctrl, Alt,
 *  Shift, Meta, each once, then a letter, a digit, F1 to F24 or a named key, in any case; null
 *  when it is not a key. */
export function normalizeKey(text: string): string | null {
  const parts = text.split("+").map((p) => p.trim());
  const key = parts.pop() ?? "";
  const held = [false, false, false, false];
  for (const m of parts) {
    const i = MODIFIERS.findIndex((n) => n.toLowerCase() === m.toLowerCase());
    if (i < 0 || held[i]) return null;
    held[i] = true;
  }
  const upper = key.toUpperCase();
  let name: string | undefined;
  if (/^[A-Za-z0-9]$/.test(key) || /^F([1-9]|1[0-9]|2[0-4])$/.test(upper)) name = upper;
  else name = NAMED_KEYS.find((n) => n.toLowerCase() === key.toLowerCase());
  if (!name) return null;
  return [...MODIFIERS.filter((_, i) => held[i]), name].join("+");
}

/** The key a key press makes, by the physical key (so it is the same with an Arabic keyboard
 *  layout); null for a modifier alone or a key a shortcut can't take. */
export function keyOf(e: Keys): string | null {
  const code = e.code === "NumpadEnter" ? "Enter" : e.code;
  const key = /^Key[A-Z]$/.test(code) ? code.slice(3) : /^Digit[0-9]$/.test(code) ? code.slice(5) : code;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Meta"].filter(Boolean);
  return normalizeKey([...mods, key].join("+"));
}

/** A key as people read it: "Slash" is "/". */
export const keyLabel = (key: string) => key.replace(/\bSlash$/, "/");

/** The first key two commands share once the user's shortcuts (command to key) are laid over the
 *  built-in ones (a command the user gave a key keeps only that one); null when none. */
export function shortcutClash(user: Record<string, string>): { key: string; commands: [string, string] } | null {
  const holder = new Map<string, string>();
  const pairs = [...BUILT_IN_SHORTCUTS.filter(([c]) => !(c in user)), ...Object.entries(user)];
  for (const [command, text] of pairs) {
    const key = normalizeKey(text);
    if (!key) continue;
    const other = holder.get(key);
    if (other !== undefined && other !== command) return { key, commands: [other, command] };
    holder.set(key, command);
  }
  return null;
}
