// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Settings } from "../api";
import { LanguageProvider } from "../i18n";
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
  measurementMethods: vi.fn(),
  chatgptAccount: vi.fn(),
  claudeCodeStatus: vi.fn(),
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
  api.chatgptAccount.mockResolvedValue({ signed_in: false, display_name: null, plan_granted: false });
  api.getSettings.mockResolvedValue(settings);
  api.hasApiKey.mockResolvedValue(hasKey);
  api.appDir.mockResolvedValue("/opt/Qiyas");
  api.mcpStatus.mockResolvedValue({ enabled: false, listening: false, addr: null, error: null });
  api.setSettings.mockResolvedValue(undefined);
  api.setApiKey.mockResolvedValue(undefined);
  api.deleteApiKey.mockResolvedValue(undefined);
  api.measurementMethods.mockResolvedValue([
    { name: "local-test", title: "Local test rules", based_on: "", source_note: "Test only", rules_loaded: false },
  ]);
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
  expect((screen.getByLabelText("Effort") as HTMLSelectElement).value).toBe("medium");
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
  // The select mounts with only the saved model while the provider list is loading.
  await screen.findByRole("option", { name: "Claude Sonnet 5.5 — claude-sonnet-5-5" });
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
  api.aiModels.mockResolvedValue({ models: [{ id: "google:gemini@3.8-flash", label: "Gemini" }], from_service: true, error: null });
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

test("approval and measurement settings keep the selected values on save", async () => {
  setup();
  const autonomy = await screen.findByLabelText("Plan approval");
  expect((autonomy as HTMLSelectElement).value).toBe("autonomous");
  await screen.findByRole("option", { name: "local-test" });
  expect((screen.getByLabelText("Method of measurement") as HTMLSelectElement).value).toBe("");
  fireEvent.change(autonomy, { target: { value: "per_phase" } });
  fireEvent.change(screen.getByLabelText("Method of measurement"), { target: { value: "local-test" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({
    ...settings, autonomy: "per_phase", measurement_method: "local-test",
  }));
});

test("save sends the settings and a new key, never reading a key back", async () => {
  setup();
  fireEvent.change(await screen.findByLabelText("Effort"), { target: { value: "max" } });
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "sk-new" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setApiKey).toHaveBeenCalledWith("anthropic", "sk-new"));
  expect(api.setSettings).toHaveBeenCalledWith({ ...settings, providers: { ...settings.providers, anthropic: { ...settings.providers.anthropic, effort: "max" } } });
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
    "That didn’t work. Technical details are available in Settings.",
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

test("unlisted typed models block Save when the service list is readable", async () => {
  setup();
  await screen.findByRole("option", { name: "Claude Sonnet 5.5 — claude-sonnet-5-5" });
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Model id"), { target: { value: "marketing-name" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect((await screen.findByRole("alert")).textContent).toContain("This model is not available");
  expect(api.setSettings).not.toHaveBeenCalled();
  expect(api.setApiKey).not.toHaveBeenCalled();
});

test("effort stays with its provider and None is available", async () => {
  setup();
  const provider = await screen.findByLabelText("Provider");
  fireEvent.change(screen.getByLabelText("Effort"), { target: { value: "max" } });
  fireEvent.change(provider, { target: { value: "openai_compatible" } });
  expect((screen.getByLabelText("Effort") as HTMLSelectElement).value).toBe("low");
  expect(screen.queryByRole("option", { name: "Max" })).toBeNull();
  fireEvent.change(screen.getByLabelText("Effort"), { target: { value: "none" } });
  fireEvent.change(provider, { target: { value: "anthropic" } });
  expect((screen.getByLabelText("Effort") as HTMLSelectElement).value).toBe("max");
  fireEvent.change(provider, { target: { value: "openai_compatible" } });
  expect((screen.getByLabelText("Effort") as HTMLSelectElement).value).toBe("none");
});

test("a typed key is cleared as soon as its address changes", async () => {
  setup(true);
  fireEvent.change(await screen.findByLabelText("Provider"), { target: { value: "openai_compatible" } });
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://one.example.test/v1" } });
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "synthetic-test-key" } });
  await vi.waitFor(() => expect(api.aiModels).toHaveBeenCalledWith("openai_compatible", "https://one.example.test/v1", "synthetic-test-key"));
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://two.example.test/v1" } });
  expect((screen.getByLabelText("API key") as HTMLInputElement).value).toBe("");
  expect(screen.queryByText("Saved")).toBeNull();
});

test("token rotation is only a draft until Save and survives settings readback", async () => {
  setup();
  await screen.findByLabelText("Token");
  fireEvent.click(screen.getByRole("button", { name: "Replace token on Save" }));
  const rotated = (screen.getByLabelText("Token") as HTMLInputElement).value;
  expect(rotated).toMatch(/^[0-9a-f]{32}$/);
  expect(rotated).not.toBe(TOKEN);
  expect(api.setSettings).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, mcp_token: rotated }));
  cleanup();
  api.getSettings.mockResolvedValue({ ...settings, mcp_token: rotated });
  render(<SettingsDialog onClose={() => {}} />);
  expect((await screen.findByLabelText("Token") as HTMLInputElement).value).toBe(rotated);
});


test("method choices contain names plus local files without requiring rules", async () => {
  setup();
  const select = await screen.findByLabelText("Method of measurement") as HTMLSelectElement;
  await screen.findByRole("option", { name: "local-test" });
  expect(Array.from(select.options).map((option) => option.text)).toEqual([
    "None (geometric quantities)", "POMI (RICS)", "NRM2 (RICS)", "SMM7", "CESMM4",
    "Local practice – Egypt", "Local practice – KSA", "Local practice – UAE", "local-test", "Other…",
  ]);
  fireEvent.change(select, { target: { value: "POMI (RICS)" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, measurement_method: "POMI (RICS)" }));
});

test("Other saves free text and restores it when Settings reopens", async () => {
  setup();
  fireEvent.change(await screen.findByLabelText("Method of measurement"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Method name"), { target: { value: "Client's method / ARM4" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  const saved = { ...settings, measurement_method: "Client's method / ARM4" };
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith(saved));
  cleanup();
  api.getSettings.mockResolvedValue(saved);
  render(<SettingsDialog onClose={() => {}} />);
  expect(await screen.findByLabelText("Method name")).toHaveProperty("value", saved.measurement_method);
  fireEvent.change(screen.getByLabelText("Method of measurement"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenLastCalledWith({ ...settings, measurement_method: null }));
});


test("named methods remain available without files and local choices have Arabic labels", async () => {
  setup();
  cleanup();
  api.measurementMethods.mockResolvedValue([]);
  localStorage.setItem("qiyas.lang", "ar");
  try {
    render(<LanguageProvider><SettingsDialog onClose={() => {}} /></LanguageProvider>);
    const select = await screen.findByLabelText("طريقة القياس") as HTMLSelectElement;
    expect(Array.from(select.options).map((option) => option.text)).toEqual([
      "بلا طريقة (كميات هندسية)", "POMI (RICS)", "NRM2 (RICS)", "SMM7", "CESMM4",
      "الممارسة المحلية – مصر", "الممارسة المحلية – السعودية", "الممارسة المحلية – الإمارات", "أخرى…",
    ]);
    fireEvent.change(select, { target: { value: "Local practice – Egypt" } });
    expect(select.value).toBe("Local practice – Egypt");
    fireEvent.change(select, { target: { value: "\u0000other" } });
    expect(screen.getByLabelText("اسم طريقة القياس")).toBeTruthy();
  } finally {
    localStorage.removeItem("qiyas.lang");
    document.documentElement.lang = "en";
    document.documentElement.dir = "ltr";
  }
});

test("subscription choices never ask for an API key or fetch API models", async () => {
  setup();
  const provider = await screen.findByLabelText("Provider");
  expect(Array.from((provider as HTMLSelectElement).options).map((option) => option.text)).toEqual([
    "Anthropic API key", "OpenAI-compatible API key", "ChatGPT (your plan)", "Claude Code (your subscription)",
  ]);
  api.aiModels.mockClear();
  api.hasApiKey.mockClear();
  fireEvent.change(provider, { target: { value: "claude_code" } });
  expect(screen.queryByLabelText("API key")).toBeNull();
  expect(screen.queryByLabelText("Effort")).toBeNull();
  expect(screen.getByText("Open a terminal and run claude, then type /login.")).toBeTruthy();
  expect(api.claudeCodeStatus).not.toHaveBeenCalled();
  expect(api.hasApiKey).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "sonnet" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  expect(api.aiModels).not.toHaveBeenCalled();
  expect(api.setApiKey).not.toHaveBeenCalled();
  expect(api.setSettings.mock.calls[0][0].providers.claude_code.model).toBe("sonnet");
});
