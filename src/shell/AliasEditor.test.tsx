// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Settings } from "../api";
import { AliasEditor } from "./AliasEditor";

const settings = {
  provider: "anthropic",
  providers: { anthropic: { model: "claude-opus-5-5" }, openai_compatible: { model: "", base_url: null } },
  effort: "high",
  mcp_enabled: false,
  mcp_port: 48123,
  mcp_token: "0123456789abcdef0123456789abcdef",
  language: "en",
  theme: "system",
  aliases: { LO: "LAYOFF A-WALL", di: "AREA" },
} satisfies Settings;

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

async function setup() {
  api.getSettings.mockResolvedValue(settings);
  api.setSettings.mockResolvedValue(undefined);
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(<AliasEditor onSaved={onSaved} onClose={onClose} />);
  await screen.findByRole("textbox", { name: "Alias 1" });
  return { onSaved, onClose };
}

const save = () => fireEvent.click(screen.getByRole("button", { name: "Save" }));
const alias = (n: number) => screen.getByRole("textbox", { name: `Alias ${n}` }) as HTMLInputElement;
const command = (n: number) => screen.getByRole("textbox", { name: `Command ${n}` }) as HTMLInputElement;

test("the user's aliases as a table: add one, remove one, save them", async () => {
  const { onSaved, onClose } = await setup();
  expect([alias(1).value, command(1).value, alias(2).value, command(2).value]).toEqual(["LO", "LAYOFF A-WALL", "di", "AREA"]);
  fireEvent.click(screen.getByRole("button", { name: "Remove alias 2" }));
  fireEvent.click(screen.getByRole("button", { name: "Add an alias" }));
  fireEvent.change(alias(2), { target: { value: " pu2 " } });
  fireEvent.change(command(2), { target: { value: "purge" } });
  save();
  await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
  const aliases = { LO: "LAYOFF A-WALL", PU2: "purge" };
  expect(api.setSettings).toHaveBeenCalledWith({ ...settings, aliases });
  expect(onSaved).toHaveBeenCalledWith(aliases);
});

test("two aliases that differ only in case are refused", async () => {
  await setup();
  fireEvent.change(alias(2), { target: { value: "lo" } });
  save();
  expect((await screen.findByRole("alert")).textContent).toBe("LO is there twice (aliases ignore case)");
  expect(api.setSettings).not.toHaveBeenCalled();
});

test("an alias that is a command's own name, or one word short of a command, is refused", async () => {
  await setup();
  fireEvent.change(alias(2), { target: { value: "Line" } });
  save();
  expect((await screen.findByRole("alert")).textContent).toBe("LINE is a command's own name: it can't be an alias");
  fireEvent.change(alias(2), { target: { value: "L 2" } });
  save();
  expect(screen.getByRole("alert").textContent).toBe("An alias is one word, with a command after it");
  fireEvent.change(alias(2), { target: { value: "X2" } });
  fireEvent.change(command(2), { target: { value: "NOSUCH 3" } });
  save();
  expect(screen.getByRole("alert").textContent).toBe("NOSUCH is not a command");
  expect(api.setSettings).not.toHaveBeenCalled();
});
