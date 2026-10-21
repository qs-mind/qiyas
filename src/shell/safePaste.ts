/** Pastes into text fields read the clipboard through Qiyas (`clipboard_read_text`,
 *  `clipboard_read_image`), never through the web view: WebKitGTK 2.54 on Wayland hangs and then
 *  crashes reading the clipboard itself. Ctrl+V, Ctrl+Shift+V and Shift+Insert in a text field
 *  give it a paste event (its own onPaste may take a picture and prevent the rest), then the text
 *  typed in as one edit, so the field's undo and React's onChange both see it. Keys outside text
 *  fields stay the drawing's (PASTECLIP). */
import { clipboardReadImage, clipboardReadText } from "../api";

type Field = HTMLInputElement | HTMLTextAreaElement | HTMLElement;

const TEXT_INPUTS = ["text", "search", "url", "tel", "email", "password", "number", ""];

/** Whether `e` is a paste key. */
export const isPasteKey = (e: KeyboardEvent) =>
  !e.altKey &&
  !e.metaKey &&
  ((e.ctrlKey && e.code === "KeyV") || (e.shiftKey && !e.ctrlKey && e.code === "Insert"));

/** The text field `target` is, when it can be typed into. */
export function editableField(target: EventTarget | null): Field | null {
  if (!(target instanceof HTMLElement)) return null;
  if (target instanceof HTMLTextAreaElement) return target.readOnly || target.disabled ? null : target;
  if (target instanceof HTMLInputElement) {
    if (!TEXT_INPUTS.includes(target.getAttribute("type")?.toLowerCase() ?? "")) return null;
    return target.readOnly || target.disabled ? null : target;
  }
  return target.isContentEditable ? target : null;
}

/** The clipboard's picture as a PNG file, or null. */
async function clipboardPicture(): Promise<File | null> {
  const buf = await clipboardReadImage();
  if (buf.byteLength < 8) return null;
  const view = new DataView(buf);
  const [w, h] = [view.getUint32(0, true), view.getUint32(4, true)];
  if (w === 0 || h === 0 || buf.byteLength < 8 + w * h * 4) return null;
  const canvas = document.createElement("canvas");
  [canvas.width, canvas.height] = [w, h];
  canvas.getContext("2d")?.putImageData(new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h), 0, 0);
  const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png"));
  return blob ? new File([blob], "pasted.png", { type: "image/png" }) : null;
}

/** Types `text` into `field` at its caret, replacing the selection. */
function insertText(field: Field, text: string) {
  if (document.execCommand?.("insertText", false, text)) return;
  if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) {
    const [start, end] = [field.selectionStart ?? field.value.length, field.selectionEnd ?? field.value.length];
    field.setRangeText(text, start, end, "end");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

/** Pastes the clipboard into `field`: text first, else a picture. */
export async function pasteInto(field: Field) {
  const text = await clipboardReadText().catch(() => null);
  const picture = text ? null : await clipboardPicture().catch(() => null);
  if (!text && !picture) return;
  // The user moved on while the clipboard was read: the paste is not for where they are now.
  if (document.activeElement !== field) return;
  let event: Event | null = null;
  try {
    const data = new DataTransfer();
    if (text) data.setData("text/plain", text);
    if (picture) data.items.add(picture);
    event = new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true });
  } catch {
    // No DataTransfer to hand over: the text is typed in all the same.
  }
  if (event && !field.dispatchEvent(event)) return;
  if (text) insertText(field, text);
}

/** Takes paste keys in text fields away from the web view, for the whole window. */
export function installSafePaste(target: Window = window) {
  const onKeyDown = (e: KeyboardEvent) => {
    if (!isPasteKey(e)) return;
    const field = editableField(e.target);
    if (!field) return;
    e.preventDefault();
    if (!e.repeat) void pasteInto(field);
  };
  target.addEventListener("keydown", onKeyDown, true);
  return () => target.removeEventListener("keydown", onKeyDown, true);
}
