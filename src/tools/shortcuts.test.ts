import { expect, test } from "vitest";
import { keyOf, normalizeKey, shortcutClash, shortcutFor, type Keys } from "./shortcuts";

const keys = (code: string, more: Partial<Keys> = {}): Keys => ({
  code,
  key: "",
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...more,
});

test("tool letters, Escape and Enter", () => {
  expect(shortcutFor(keys("KeyD"))).toBe("distance");
  expect(shortcutFor(keys("KeyA"))).toBe("area");
  expect(shortcutFor(keys("KeyC"))).toBe("count");
  expect(shortcutFor(keys("KeyS"))).toBe("select");
  expect(shortcutFor(keys("Escape"))).toBe("escape");
  expect(shortcutFor(keys("Enter"))).toBe("finish");
  expect(shortcutFor(keys("NumpadEnter"))).toBe("finish");
  expect(shortcutFor(keys("KeyD", { shiftKey: true }))).toBeNull();
  expect(shortcutFor(keys("KeyQ"))).toBeNull();
});

test("Delete erases; Shift, Ctrl or Alt with it does not", () => {
  expect(shortcutFor(keys("Delete"))).toBe("erase");
  expect(shortcutFor(keys("Delete", { shiftKey: true }))).toBeNull();
  expect(shortcutFor(keys("Delete", { ctrlKey: true }))).toBeNull();
  expect(shortcutFor(keys("Delete", { altKey: true }))).toBeNull();
  // A user who gives ERASE another key frees Delete.
  expect(shortcutFor(keys("Delete"), { erase: "E" })).toBeNull();
  expect(shortcutFor(keys("KeyE"), { erase: "E" })).toBe("erase");
});

test("undo, redo and save with Ctrl", () => {
  expect(shortcutFor(keys("KeyZ", { ctrlKey: true }))).toBe("undo");
  expect(shortcutFor(keys("KeyZ", { ctrlKey: true, shiftKey: true }))).toBe("redo");
  expect(shortcutFor(keys("KeyY", { ctrlKey: true }))).toBe("redo");
  expect(shortcutFor(keys("KeyS", { ctrlKey: true }))).toBe("save");
  expect(shortcutFor(keys("KeyN", { ctrlKey: true }))).toBe("new");
  expect(shortcutFor(keys("KeyN", { ctrlKey: true, shiftKey: true }))).toBeNull();
  expect(shortcutFor(keys("Digit9", { ctrlKey: true }))).toBe("commandLine");
  expect(shortcutFor(keys("Digit9"))).toBeNull();
  expect(shortcutFor(keys("KeyS", { ctrlKey: true, shiftKey: true }))).toBeNull();
  expect(shortcutFor(keys("KeyD", { ctrlKey: true }))).toBeNull();
});

test("an Arabic keyboard layout still finds the shortcut", () => {
  expect(shortcutFor(keys("KeyZ", { key: "ئ", ctrlKey: true }))).toBe("undo");
  expect(shortcutFor(keys("KeyD", { key: "ي" }))).toBe("distance");
});

test("Alt and Meta leave the key alone", () => {
  expect(shortcutFor(keys("KeyD", { altKey: true }))).toBeNull();
  expect(shortcutFor(keys("KeyZ", { ctrlKey: true, metaKey: true }))).toBeNull();
});

test("F3, F7, F8 and F10 switch snaps, grid, ortho and polar", () => {
  expect(shortcutFor(keys("F3"))).toBe("osnap");
  expect(shortcutFor(keys("F7"))).toBe("grid");
  expect(shortcutFor(keys("F8"))).toBe("ortho");
  expect(shortcutFor(keys("F10"))).toBe("polar");
  expect(shortcutFor(keys("F8", { ctrlKey: true }))).toBeNull();
  expect(shortcutFor(keys("F9"))).toBeNull();
});

test("Ctrl+Tab, Ctrl+Shift+Tab and Ctrl+W go through and close the file tabs", () => {
  expect(shortcutFor(keys("Tab", { ctrlKey: true }))).toBe("nextTab");
  expect(shortcutFor(keys("Tab", { ctrlKey: true, shiftKey: true }))).toBe("prevTab");
  expect(shortcutFor(keys("KeyW", { ctrlKey: true }))).toBe("closeTab");
  // By the physical key, so an Arabic layout closes too.
  expect(shortcutFor(keys("KeyW", { key: "ص", ctrlKey: true }))).toBe("closeTab");
  expect(shortcutFor(keys("Tab"))).toBeNull();
  expect(shortcutFor(keys("KeyW"))).toBeNull();
  expect(shortcutFor(keys("KeyW", { ctrlKey: true, shiftKey: true }))).toBeNull();
});

test("Ctrl+C copies to the clipboard and Ctrl+V pastes, Ctrl+Shift+V as a block", () => {
  expect(shortcutFor(keys("KeyC", { ctrlKey: true }))).toBe("copyclip");
  expect(shortcutFor(keys("KeyV", { ctrlKey: true }))).toBe("pasteclip");
  expect(shortcutFor(keys("KeyV", { ctrlKey: true, shiftKey: true }))).toBe("pasteblock");
  expect(shortcutFor(keys("KeyC", { ctrlKey: true, shiftKey: true }))).toBe("copybase");
});

test("a shortcut key is spelt one way: modifiers Ctrl, Alt, Shift, Meta, then the key", () => {
  expect(normalizeKey("shift+ctrl+c")).toBe("Ctrl+Shift+C");
  expect(normalizeKey(" f3 ")).toBe("F3");
  expect(normalizeKey("alt+pageup")).toBe("Alt+PageUp");
  expect(normalizeKey("d")).toBe("D");
  for (const bad of ["", "Ctrl+", "Ctrl+Ctrl+C", "F25", "F03", "Hyper+C", "CC", "ق"]) expect(normalizeKey(bad)).toBeNull();
});

test("a key pressed is read by the physical key, whatever the keyboard's language", () => {
  expect(keyOf(keys("KeyC", { key: "ؤ", ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+C");
  expect(keyOf(keys("Digit9", { ctrlKey: true }))).toBe("Ctrl+9");
  expect(keyOf(keys("F3"))).toBe("F3");
  expect(keyOf(keys("ArrowLeft", { altKey: true }))).toBe("Alt+ArrowLeft");
  // A modifier alone is not a key yet.
  expect(keyOf(keys("ShiftLeft", { shiftKey: true }))).toBeNull();
});

test("two commands on one key clash, the built-in ones counted unless the user moved them", () => {
  expect(shortcutClash({ purge: "Ctrl+Shift+P", audit: "ctrl+shift+p" })).toEqual({
    key: "Ctrl+Shift+P",
    commands: ["purge", "audit"],
  });
  // Ctrl+Z is Undo's.
  expect(shortcutClash({ options: "Ctrl+Z" })).toEqual({ key: "Ctrl+Z", commands: ["undo", "options"] });
  // Undo moved to Ctrl+U: Ctrl+Z is free.
  expect(shortcutClash({ undo: "Ctrl+U", options: "Ctrl+Z" })).toBeNull();
  expect(shortcutClash({ options: "F12" })).toBeNull();
});

test("the user's shortcuts start their command, and a command moved leaves its built-in key", () => {
  const mine = { circle: "Ctrl+Shift+O", undo: "Ctrl+U", osnap: "F12" };
  expect(shortcutFor(keys("KeyO", { ctrlKey: true, shiftKey: true }), mine)).toBe("circle");
  expect(shortcutFor(keys("KeyU", { ctrlKey: true }), mine)).toBe("undo");
  expect(shortcutFor(keys("F12"), mine)).toBe("osnap");
  // Ctrl+Z and F3 are free now.
  expect(shortcutFor(keys("KeyZ", { ctrlKey: true }), mine)).toBeNull();
  expect(shortcutFor(keys("F3"), mine)).toBeNull();
  // The rest as built in.
  expect(shortcutFor(keys("KeyY", { ctrlKey: true }), mine)).toBe("redo");
});
