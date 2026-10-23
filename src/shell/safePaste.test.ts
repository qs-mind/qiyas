// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  clipboardReadText: vi.fn<() => Promise<string | null>>(),
  clipboardReadImage: vi.fn<() => Promise<ArrayBuffer>>(),
}));
vi.mock("../api", () => api);

import { editableField, installSafePaste, isPasteKey } from "./safePaste";

const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
const settle = () => new Promise((r) => setTimeout(r, 0));

describe("safe paste", () => {
  let uninstall: () => void;
  beforeEach(() => {
    uninstall = installSafePaste();
    api.clipboardReadText.mockResolvedValue("a villa in Riyadh");
    api.clipboardReadImage.mockResolvedValue(new ArrayBuffer(0));
  });
  afterEach(() => {
    uninstall();
    document.body.innerHTML = "";
    vi.clearAllMocks();
  });

  it("knows the paste keys", () => {
    expect(isPasteKey(key({ code: "KeyV", ctrlKey: true }))).toBe(true);
    expect(isPasteKey(key({ code: "KeyV", ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(isPasteKey(key({ code: "Insert", shiftKey: true }))).toBe(true);
    expect(isPasteKey(key({ code: "KeyV" }))).toBe(false);
    expect(isPasteKey(key({ code: "KeyV", metaKey: true }))).toBe(false);
  });

  it("only takes fields that can be typed into", () => {
    const box = document.createElement("textarea");
    const ro = Object.assign(document.createElement("input"), { readOnly: true });
    const check = Object.assign(document.createElement("input"), { type: "checkbox" });
    expect(editableField(box)).toBe(box);
    expect(editableField(ro)).toBeNull();
    expect(editableField(check)).toBeNull();
    expect(editableField(document.body)).toBeNull();
  });

  it("types the clipboard's text in at the caret, through Qiyas", async () => {
    const box = document.createElement("textarea");
    document.body.append(box);
    box.value = "Draw: ";
    box.focus();
    box.setSelectionRange(6, 6);
    const typed = vi.fn();
    box.addEventListener("input", typed);
    const e = key({ code: "KeyV", ctrlKey: true });
    box.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
    await settle();
    expect(box.value).toBe("Draw: a villa in Riyadh");
    expect(typed).toHaveBeenCalled();
  });

  it("leaves keys outside text fields to the drawing", async () => {
    const e = key({ code: "KeyV", ctrlKey: true });
    document.body.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
    await settle();
    expect(api.clipboardReadText).not.toHaveBeenCalled();
  });

  it("drops a paste when the user has moved to another field", async () => {
    const [a, b] = [document.createElement("input"), document.createElement("input")];
    document.body.append(a, b);
    a.focus();
    a.dispatchEvent(key({ code: "KeyV", ctrlKey: true }));
    b.focus();
    await settle();
    expect(a.value).toBe("");
    expect(b.value).toBe("");
  });

  it("does nothing when the clipboard is empty", async () => {
    api.clipboardReadText.mockResolvedValue(null);
    const box = document.createElement("input");
    document.body.append(box);
    box.focus();
    box.dispatchEvent(key({ code: "KeyV", ctrlKey: true }));
    await settle();
    expect(api.clipboardReadImage).toHaveBeenCalled();
    expect(box.value).toBe("");
  });
});
