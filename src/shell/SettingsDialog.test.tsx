// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Settings } from "../api";
import { mcpSnippets, SettingsDialog } from "./SettingsDialog";

const TOKEN = "0123456789abcdef0123456789abcdef";

const settings: Settings = {
  provider: "anthropic",
  providers: { anthropic: { model: "claude-opus-5-5" }, openai_compatible: { model: "", base_url: null } },
  effort: "high",
  mcp_enabled: false,
  mcp_port: 48123,
  mcp_token: TOKEN,
  language: "en",
  theme: "system",
  aliases: {},
};

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  hasApiKey: vi.fn(),
  setApiKey: vi.fn(),
  deleteApiKey: vi.fn(),
  appDir: vi.fn(),
  mcpStatus: vi.fn(),
  aiModels: vi.fn(),
}));

vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));

beforeAll(() => {
  // jsdom may not implement modal dialogs.
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

function setup(hasKey = false) {
  api.getSettings.mockResolvedValue(settings);
  api.hasApiKey.mockResolvedValue(hasKey);
  api.appDir.mockResolvedValue("/opt/Qiyas");
  api.mcpStatus.mockResolvedValue({ enabled: false, listening: false, addr: null, error: null });
  api.setSettings.mockResolvedValue(undefined);
  api.setApiKey.mockResolvedValue(undefined);
  api.deleteApiKey.mockResolvedValue(undefined);
  api.aiModels.mockResolvedValue({
    models: [
      { id: "claude-opus-5-5", label: "Claude Opus 5.5" },
      { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5" },
    ],
    from_service: true,
    error: null,
  });
  render(<SettingsDialog onClose={() => {}} />);
}

test("settings dialog shows its fields; base URL only for OpenAI-compatible", async () => {
  setup();
  const provider = (await screen.findByLabelText("Provider")) as HTMLSelectElement;
  expect(provider.value).toBe("anthropic");
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("claude-opus-5-5");
  expect((screen.getByLabelText("Effort") as HTMLSelectElement).value).toBe("high");
  expect((screen.getByLabelText("API key") as HTMLInputElement).type).toBe("password");
  expect((screen.getByLabelText("Port") as HTMLInputElement).value).toBe("48123");
  expect((screen.getByLabelText("Token") as HTMLInputElement).value).toBe(TOKEN);
  expect(screen.queryByLabelText("Base URL")).toBeNull();

  fireEvent.change(provider, { target: { value: "openai_compatible" } });
  expect(screen.getByLabelText("Base URL")).toBeTruthy();
  expect(api.hasApiKey).toHaveBeenLastCalledWith("openai_compatible");
});

test("a saved key shows the badge and can be removed", async () => {
  setup(true);
  expect(await screen.findByText("Saved")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  expect(api.deleteApiKey).toHaveBeenCalledWith("anthropic");
  await vi.waitFor(() => expect(screen.queryByText("Saved")).toBeNull());
});

test("switching providers keeps each profile and never transfers a typed key", async () => {
  setup();
  const provider = await screen.findByLabelText("Provider");
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "claude-sonnet-5-5" } });
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "typed-for-anthropic" } });
  fireEvent.change(provider, { target: { value: "openai_compatible" } });
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("");
  expect((screen.getByLabelText("API key") as HTMLInputElement).value).toBe("");
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://example.test/v1" } });
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Model id"), { target: { value: "google:gemini@3.8-flash" } });
  fireEvent.change(provider, { target: { value: "anthropic" } });
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("claude-sonnet-5-5");
  fireEvent.change(provider, { target: { value: "openai_compatible" } });
  expect((screen.getByLabelText("Base URL") as HTMLInputElement).value).toBe("https://example.test/v1");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({
    ...settings,
    provider: "openai_compatible",
    providers: {
      anthropic: { model: "claude-sonnet-5-5" },
      openai_compatible: { model: "google:gemini@3.8-flash", base_url: "https://example.test/v1" },
    },
  }));
  expect(api.setApiKey).not.toHaveBeenCalled();
});

test("save sends the settings and a new key, never reading a key back", async () => {
  setup();
  fireEvent.change(await screen.findByLabelText("Effort"), { target: { value: "max" } });
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "sk-new" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setApiKey).toHaveBeenCalledWith("anthropic", "sk-new"));
  expect(api.setSettings).toHaveBeenCalledWith({ ...settings, effort: "max" });
});

test("mcp snippets point at the bridge next to the app", () => {
  const unix = mcpSnippets("/opt/Qiyas", TOKEN);
  expect(unix.claudeCode).toBe(
    `claude mcp add qiyas -e QIYAS_MCP_TOKEN=${TOKEN} -- /opt/Qiyas/qiyas-mcp`,
  );
  expect(JSON.parse(unix.claudeDesktop)).toEqual({
    mcpServers: { qiyas: { command: "/opt/Qiyas/qiyas-mcp", env: { QIYAS_MCP_TOKEN: TOKEN } } },
  });
  const win = mcpSnippets("C:\\Program Files\\Qiyas", TOKEN);
  expect(win.claudeCode).toContain('-- "C:\\Program Files\\Qiyas\\qiyas-mcp.exe"');
  expect(JSON.parse(win.claudeDesktop).mcpServers.qiyas.command).toBe(
    "C:\\Program Files\\Qiyas\\qiyas-mcp.exe",
  );
});

test("the MCP status line says whether the server listens", async () => {
  setup();
  expect(await screen.findByText("Not running")).toBeTruthy();
  cleanup();

  api.mcpStatus.mockResolvedValue({
    enabled: true,
    listening: true,
    addr: "127.0.0.1:48123",
    error: null,
  });
  render(<SettingsDialog onClose={() => {}} />);
  expect(await screen.findByText("Listening on 127.0.0.1:48123")).toBeTruthy();
  cleanup();

  api.mcpStatus.mockResolvedValue({
    enabled: true,
    listening: false,
    addr: null,
    error: "The MCP server could not start on 127.0.0.1:48123: Address in use",
  });
  render(<SettingsDialog onClose={() => {}} />);
  expect(await screen.findByRole("status")).toHaveProperty(
    "textContent",
    "The MCP server could not start on 127.0.0.1:48123: Address in use",
  );
});

test("autosave runs every 5 minutes unless set otherwise; 0 turns it off", async () => {
  setup();
  const minutes = (await screen.findByLabelText("Autosave every (minutes, 0 off)")) as HTMLInputElement;
  expect(minutes.value).toBe("5");
  fireEvent.change(minutes, { target: { value: "10" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, autosave_minutes: 10 }));
});

test("the autosave minutes take whole numbers only; empty leaves them as they were", async () => {
  setup();
  const minutes = (await screen.findByLabelText("Autosave every (minutes, 0 off)")) as HTMLInputElement;
  fireEvent.change(minutes, { target: { value: "2.5" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect((await screen.findByRole("alert")).textContent).toContain("whole number");
  expect(api.setSettings).not.toHaveBeenCalled();
  fireEvent.change(minutes, { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith(settings));
});
