// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Settings } from "../api";
import { OptionsDialog } from "./OptionsDialog";

const settings: Settings = {
  provider: "anthropic",
  providers: { anthropic: { model: "claude-opus-5-5" }, openai_compatible: { model: "", base_url: null } },
  effort: "high",
  mcp_enabled: false,
  mcp_port: 48123,
  mcp_token: "0123456789abcdef0123456789abcdef",
  language: "en",
  theme: "system",
  aliases: {},
  autosave_minutes: 7,
  display_precision: 3,
  snap_aperture_px: 10,
  pick_box_px: 6,
  background_color: null,
  crosshair_color: null,
  highlight_color: "#6e4a9e",
  shortcuts: { line: "Ctrl+L" },
  ribbon: null,
};

const api = vi.hoisted(() => ({ getSettings: vi.fn(), setSettings: vi.fn() }));
vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function setup(more: Partial<Settings> = {}) {
  api.getSettings.mockResolvedValue({ ...settings, ...more });
  api.setSettings.mockResolvedValue(undefined);
  const onSaved = vi.fn();
  const onSettings = vi.fn();
  const onClose = vi.fn();
  render(<OptionsDialog onSaved={onSaved} onSettings={onSettings} onClose={onClose} />);
  await screen.findByRole("combobox", { name: "Decimals with no drawing open" });
  return { onSaved, onSettings, onClose };
}

const save = () => fireEvent.click(screen.getByRole("button", { name: "Save" }));

test("options round trip: precision, snap aperture, pick box and colours are saved with the rest", async () => {
  const { onSaved, onClose } = await setup();
  // The crosshair colour is kept but not drawn yet, and the dialog says so.
  expect(screen.getByText("The crosshair colour is kept for a later version: the canvas has no crosshair yet.")).toBeTruthy();
  // The ruling, said where the choice is made.
  expect(screen.getByText(/drafting settings.*while it is open/i)).toBeTruthy();
  fireEvent.change(screen.getByRole("combobox", { name: "Decimals with no drawing open" }), { target: { value: "2" } });
  fireEvent.change(screen.getByRole("spinbutton", { name: "Snap aperture (pixels)" }), { target: { value: "14" } });
  fireEvent.change(screen.getByRole("spinbutton", { name: "Pick box (pixels)" }), { target: { value: "4" } });
  // Background: from the theme to a colour of its own; highlight back to the theme's.
  fireEvent.click(screen.getByRole("checkbox", { name: "Background: theme default" }));
  fireEvent.change(screen.getByLabelText("Background colour"), { target: { value: "#102030" } });
  fireEvent.click(screen.getByRole("checkbox", { name: "Highlight: theme default" }));
  save();
  await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
  const saved = {
    ...settings,
    display_precision: 2,
    snap_aperture_px: 14,
    pick_box_px: 4,
    background_color: "#102030",
    highlight_color: null,
  };
  expect(api.setSettings).toHaveBeenCalledWith(saved);
  expect(onSaved).toHaveBeenCalledWith(saved);
});

test("a pixel size outside 1 to 50 is refused before anything is saved", async () => {
  await setup();
  fireEvent.change(screen.getByRole("spinbutton", { name: "Pick box (pixels)" }), { target: { value: "60" } });
  save();
  expect((await screen.findByRole("alert")).textContent).toBe("The snap aperture and the pick box take 1 to 50 pixels");
  expect(api.setSettings).not.toHaveBeenCalled();
});

test("the autosave interval is the Settings dialog's: shown here, changed there", async () => {
  const { onSettings } = await setup();
  expect(screen.getByText("Autosave every 7 minutes")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Change in Settings…" }));
  expect(onSettings).toHaveBeenCalled();
});

test("shortcuts: a key pressed in the table is kept, a clash is refused with a message", async () => {
  await setup();
  // The built-in keys and the user's are listed.
  expect((screen.getByRole("textbox", { name: "Key for Undo" }) as HTMLInputElement).value).toBe("Ctrl+Z");
  expect((screen.getByRole("textbox", { name: "Key for Line" }) as HTMLInputElement).value).toBe("Ctrl+L");
  // Ctrl+Z is Undo's: refused, Line keeps its key.
  const line = screen.getByRole("textbox", { name: "Key for Line" });
  fireEvent.keyDown(line, { code: "KeyZ", key: "z", ctrlKey: true });
  expect(screen.getByRole("alert").textContent).toBe("Ctrl+Z is already the key for Undo");
  expect((line as HTMLInputElement).value).toBe("Ctrl+L");
  // A free key, pressed with an Arabic layout (the physical key counts).
  fireEvent.keyDown(line, { code: "KeyK", key: "ن", ctrlKey: true, shiftKey: true });
  expect((line as HTMLInputElement).value).toBe("Ctrl+Shift+K");
  // A new one: Circle on F12.
  fireEvent.change(screen.getByRole("combobox", { name: "Command" }), { target: { value: "circle" } });
  fireEvent.keyDown(screen.getByRole("textbox", { name: "New key" }), { code: "F12", key: "F12" });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  // Undo moved to Ctrl+U.
  fireEvent.keyDown(screen.getByRole("textbox", { name: "Key for Undo" }), { code: "KeyU", key: "u", ctrlKey: true });
  save();
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  expect(api.setSettings.mock.calls[0][0].shortcuts).toEqual({ line: "Ctrl+Shift+K", circle: "F12", undo: "Ctrl+U" });
});

test("a shortcut of the user's goes back to its built-in key, or away", async () => {
  await setup({ shortcuts: { undo: "Ctrl+U", line: "Ctrl+L" } });
  fireEvent.click(screen.getByRole("button", { name: "Built-in key for Undo" }));
  expect((screen.getByRole("textbox", { name: "Key for Undo" }) as HTMLInputElement).value).toBe("Ctrl+Z");
  fireEvent.click(screen.getByRole("button", { name: "Remove the key for Line" }));
  expect(screen.queryByRole("textbox", { name: "Key for Line" })).toBeNull();
  save();
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  expect(api.setSettings.mock.calls[0][0].shortcuts).toEqual({});
});

test("the core's refusal is shown and the dialog stays open", async () => {
  const { onClose } = await setup();
  api.setSettings.mockRejectedValue("The background colour \"#12\" must be written #rrggbb");
  save();
  expect((await screen.findByRole("alert")).textContent).toBe("The background colour \"#12\" must be written #rrggbb");
  expect(onClose).not.toHaveBeenCalled();
});
