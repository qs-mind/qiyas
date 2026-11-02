// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within, act } from "@testing-library/react";
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
  chatgptModels: vi.fn(),
  claudeCodeStatus: vi.fn(),
  aiProviderStatus: vi.fn(),
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

function chooseProvider(provider: string) {
  const labels: Record<string, string> = { anthropic: "Anthropic API key", openai_compatible: "OpenAI-compatible API key", chatgpt_plan: "ChatGPT (your plan)", claude_code: "Claude Code (your subscription)" };
  fireEvent.click(screen.getByRole("radio", { name: labels[provider] }));
}

function setup(hasKey = false) {
  api.aiProviderStatus.mockResolvedValue({ provider: "chatgpt_plan", status: "needs_sign_in", configured: false, verified: false, last_failure: null, actions: [] });
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
  const provider = (await screen.findByRole("radio", { name: "Anthropic API key" })) as HTMLInputElement;
  expect(provider.value).toBe("anthropic");
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("claude-opus-5-5");
  expect(screen.getByRole("radio", { name: "Medium" }).getAttribute("aria-checked")).toBe("true");
  expect((screen.getByLabelText("API key") as HTMLInputElement).type).toBe("password");
  expect((screen.getByLabelText("Port") as HTMLInputElement).value).toBe("48123");
  expect((screen.getByLabelText("Token") as HTMLInputElement).value).toBe(TOKEN);
  expect(screen.queryByLabelText("Base URL")).toBeNull();

  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  expect(screen.getByLabelText("Base URL")).toBeTruthy();
  expect(api.hasApiKey).toHaveBeenLastCalledWith("openai_compatible");
});

test("a saved key shows the badge and can be removed", async () => {
  setup(true);
  expect(await screen.findByText("Key saved on this device")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  expect(api.deleteApiKey).toHaveBeenCalledWith("anthropic");
  await vi.waitFor(() => expect(screen.queryByText("Key saved on this device")).toBeNull());
});

test("switching providers keeps each profile and never transfers a typed key", async () => {
  setup();
  await screen.findByRole("radio", { name: "Anthropic API key" });
  // The select mounts with only the saved model while the provider list is loading.
  await screen.findByRole("option", { name: "Claude Sonnet 5.5 — claude-sonnet-5-5" });
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "claude-sonnet-5-5" } });
  await vi.waitFor(() => expect(api.setSettings.mock.calls.at(-1)?.[0].providers.anthropic.model).toBe("claude-sonnet-5-5"));
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "typed-for-anthropic" } });
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("");
  expect((screen.getByLabelText("API key") as HTMLInputElement).value).toBe("");
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://example.test/v1" } });
  fireEvent.blur(screen.getByLabelText("Base URL"));
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "\u0000other" } });
  api.aiModels.mockResolvedValue({ models: [{ id: "google:gemini@3.8-flash", label: "Gemini" }], from_service: true, error: null });
  fireEvent.change(screen.getByLabelText("Model id"), { target: { value: "google:gemini@3.8-flash" } });
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("anthropic");
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("claude-sonnet-5-5");
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  expect((screen.getByLabelText("Base URL") as HTMLInputElement).value).toBe("https://example.test/v1");
  api.aiModels.mockResolvedValue({ models: [{ id: "google:gemini@3.8-flash", label: "Gemini" }], from_service: true, error: null });
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({
    ...settings,
    provider: "openai_compatible",
    providers: {
      anthropic: { model: "claude-sonnet-5-5" },
      openai_compatible: { model: "google:gemini@3.8-flash", base_url: "https://example.test/v1", image_input: undefined },
    },
  }));
  expect(api.setApiKey).not.toHaveBeenCalled();
});

test("approval and measurement settings keep the selected values immediately", async () => {
  setup();
  const autonomy = await screen.findByRole("radio", { name: "Approve each phase" });
  expect((screen.getByRole("radio", { name: "Work autonomously" }) as HTMLInputElement).checked).toBe(true);
  fireEvent.click(screen.getByRole("tab", { name: "Measurement" }));
  await screen.findByRole("option", { name: "local-test" });
  expect((screen.getByLabelText("Method of measurement") as HTMLSelectElement).value).toBe("");
  fireEvent.click(autonomy);
  fireEvent.change(screen.getByLabelText("Method of measurement"), { target: { value: "local-test" } });
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({
    ...settings, autonomy: "per_phase", measurement_method: "local-test",
  }));
});

test("choices apply immediately and Connect stores a new key without reading it back", async () => {
  setup();
  fireEvent.click(await screen.findByRole("radio", { name: "Max" }));
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "sk-new" } });
  fireEvent.click(screen.getByRole("button", { name: "Connect key" }));
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

test("settings are in sections, one shown at a time", async () => {
  setup();
  expect(await screen.findByRole("tab", { name: "Mizan" })).toHaveProperty("ariaSelected", "true");
  for (const name of ["Measurement", "General", "Connected apps", "Support"]) expect(screen.getByRole("tab", { name })).toBeTruthy();
  expect(screen.queryByRole("checkbox", { name: "Let other AI apps use Qiyas" })).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "Connected apps" }));
  expect(await screen.findByRole("checkbox", { name: "Let other AI apps use Qiyas" })).toBeTruthy();
  expect(screen.queryByRole("combobox", { name: "Provider" })).toBeNull();
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
  fireEvent.click(await screen.findByRole("tab", { name: "Connected apps" }));
  expect(await screen.findByText("That didn’t work. Technical details are available in Settings.")).toHaveProperty(
    "textContent",
    "That didn’t work. Technical details are available in Settings.",
  );
});

test("autosave runs every 5 minutes unless set otherwise; 0 turns it off", async () => {
  setup();
  const minutes = (await screen.findByLabelText("Autosave every (minutes, 0 off)")) as HTMLInputElement;
  expect(minutes.value).toBe("5");
  fireEvent.change(minutes, { target: { value: "10" } });
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, autosave_minutes: 10 }));
});

test("the autosave minutes take whole numbers only; empty leaves them as they were", async () => {
  setup();
  const minutes = (await screen.findByLabelText("Autosave every (minutes, 0 off)")) as HTMLInputElement;
  fireEvent.click(screen.getByRole("tab", { name: "General" }));
  fireEvent.change(minutes, { target: { value: "2.5" } });
  expect((await screen.findByRole("alert")).textContent).toContain("whole number");
  expect(api.setSettings).not.toHaveBeenCalled();
  fireEvent.change(minutes, { target: { value: "" } });
  expect(api.setSettings).not.toHaveBeenCalled();
});

test("unlisted typed models are not saved when the service list is readable", async () => {
  setup();
  await screen.findByRole("option", { name: "Claude Sonnet 5.5 — claude-sonnet-5-5" });
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Model id"), { target: { value: "marketing-name" } });
  expect((await screen.findByRole("alert")).textContent).toContain("This model is not available");
  expect(api.setSettings).not.toHaveBeenCalled();
  expect(api.setApiKey).not.toHaveBeenCalled();
});

test("effort stays with its provider and None is available", async () => {
  setup();
  await screen.findByRole("radio", { name: "Anthropic API key" });
  fireEvent.click(screen.getByRole("radio", { name: "Max" }));
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  expect(screen.getByRole("radio", { name: "Low" }).getAttribute("aria-checked")).toBe("true");
  expect(screen.queryByRole("radio", { name: "Max" })).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: "None (service default)" }));
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("anthropic");
  expect(screen.getByRole("radio", { name: "Max" }).getAttribute("aria-checked")).toBe("true");
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  expect(screen.getByRole("radio", { name: "None (service default)" }).getAttribute("aria-checked")).toBe("true");
});

test("a typed key is cleared as soon as its address changes", async () => {
  setup(true);
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://one.example.test/v1" } });
  fireEvent.blur(screen.getByLabelText("Base URL"));
  fireEvent.change(screen.getByLabelText("API key"), { target: { value: "synthetic-test-key" } });
  await vi.waitFor(() => expect(api.aiModels).toHaveBeenCalledWith("openai_compatible", "https://one.example.test/v1", "synthetic-test-key"));
  fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://two.example.test/v1" } });
  fireEvent.blur(screen.getByLabelText("Base URL"));
  expect((screen.getByLabelText("API key") as HTMLInputElement).value).toBe("");
  expect(screen.queryByText("Key saved on this device")).toBeNull();
});

test("token rotation applies immediately and survives settings readback", async () => {
  setup();
  await screen.findByLabelText("Token");
  fireEvent.click(screen.getByRole("tab", { name: "Connected apps" }));
  fireEvent.click(await screen.findByRole("button", { name: "Replace token" }));
  const rotated = (screen.getByLabelText("Token") as HTMLInputElement).value;
  expect(rotated).toMatch(/^[0-9a-f]{32}$/);
  expect(rotated).not.toBe(TOKEN);
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, mcp_token: rotated }));
  cleanup();
  api.getSettings.mockResolvedValue({ ...settings, mcp_token: rotated });
  render(<SettingsDialog onClose={() => {}} />);
  expect((await screen.findByLabelText("Token") as HTMLInputElement).value).toBe(rotated);
});


test("method choices contain names plus local files without requiring rules", async () => {
  setup();
  const select = await screen.findByLabelText("Method of measurement") as HTMLSelectElement;
  fireEvent.click(screen.getByRole("tab", { name: "Measurement" }));
  await screen.findByRole("option", { name: "local-test" });
  expect(Array.from(select.options).map((option) => option.text)).toEqual([
    "None (geometric quantities)", "POMI", "NRM2", "SMM7", "CESMM4",
    "Local practice – Egypt", "Local practice – KSA", "Local practice – UAE", "local-test", "Other…",
  ]);
  fireEvent.change(select, { target: { value: "POMI" } });
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, measurement_method: "POMI" }));
});

test("Other saves free text and restores it when Settings reopens", async () => {
  setup();
  fireEvent.change(await screen.findByLabelText("Method of measurement"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Method name"), { target: { value: "Client's method / ARM4" } });
  const saved = { ...settings, measurement_method: "Client's method / ARM4" };
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith(saved));
  cleanup();
  api.getSettings.mockResolvedValue(saved);
  render(<SettingsDialog onClose={() => {}} />);
  expect(await screen.findByLabelText("Method name")).toHaveProperty("value", saved.measurement_method);
  fireEvent.change(screen.getByLabelText("Method of measurement"), { target: { value: "" } });
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
      "بلا طريقة (كميات هندسية)", "POMI", "NRM2", "SMM7", "CESMM4",
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
  await screen.findByRole("radio", { name: "Anthropic API key" });
  expect(screen.getAllByRole("radio").filter(radio => radio.getAttribute("name") === "provider").map(radio => radio.getAttribute("aria-label"))).toEqual([
    "ChatGPT (your plan)", "Claude Code (your subscription)", "Anthropic API key", "OpenAI-compatible API key",
  ]);
  // Let the opening key check for the saved API provider finish before counting calls.
  await vi.waitFor(() => expect(api.hasApiKey).toHaveBeenCalledWith("anthropic"));
  api.aiModels.mockClear();
  api.hasApiKey.mockClear();
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("claude_code");
  expect(screen.queryByLabelText("API key")).toBeNull();
  expect(screen.queryByLabelText("Effort")).toBeNull();
  expect(screen.getByText("Open a terminal and run claude, then type /login.")).toBeTruthy();
  expect(api.claudeCodeStatus).not.toHaveBeenCalled();
  expect(api.hasApiKey).not.toHaveBeenCalled();
  expect(screen.queryByLabelText("Model")).toBeNull();
  api.claudeCodeStatus.mockResolvedValue({ installed: true, compatible: true, signed_in: true, code: null,
    supports_model: true, model_aliases: ["sonnet"], supports_effort: true, effort_levels: ["low", "high"] });
  await screen.findByLabelText("Model");
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "sonnet" } });
  fireEvent.click(screen.getByRole("radio", { name: "High" }));
  await vi.waitFor(() => expect(api.setSettings.mock.calls.at(-1)?.[0].providers.claude_code?.effort).toBe("high"));
  expect(api.aiModels).not.toHaveBeenCalled();
  expect(api.setApiKey).not.toHaveBeenCalled();
  expect(api.setSettings.mock.calls.at(-1)![0].providers.claude_code.model).toBe("sonnet");
  expect(api.setSettings.mock.calls.at(-1)![0].providers.claude_code.effort).toBe("high");
});

test("ChatGPT automatic model and advertised effort are kept immediately", async () => {
  setup();
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true });
  api.chatgptModels.mockResolvedValue([{ id: "plan-first", label: "First account model", reasoning_efforts: ["low", "high"] }]);
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("chatgpt_plan");
  const model = await screen.findByRole("option", { name: "First account model" });
  expect((model.parentElement as HTMLSelectElement).value).toBe("plan-first");
  expect(screen.getByRole("radio", { name: "Default" }).getAttribute("aria-checked")).toBe("true");
  fireEvent.click(screen.getByRole("radio", { name: "High" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  await vi.waitFor(() => expect(api.setSettings.mock.calls.at(-1)![0].providers.chatgpt_plan).toEqual({ model: "plan-first", effort: "high" }));
});

test("Claude Other accepts a full model name and Default omits effort", async () => {
  setup();
  api.claudeCodeStatus.mockResolvedValue({ installed: true, compatible: true, signed_in: true, code: null,
    supports_model: true, model_aliases: ["opus"], supports_effort: true, effort_levels: ["max"] });
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("claude_code");
  fireEvent.change(await screen.findByLabelText("Model"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Model name"), { target: { value: "claude-full-1.2:test" } });
  fireEvent.click(screen.getByRole("radio", { name: "Max" }));
  fireEvent.click(screen.getByRole("radio", { name: "Default" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  await vi.waitFor(() => expect(api.setSettings.mock.calls.at(-1)![0].providers.claude_code).toEqual({ model: "claude-full-1.2:test", effort: null }));
});

test("technical details default off and save through the existing settings IPC", async () => {
  setup();
  fireEvent.click(await screen.findByRole("tab", { name: "Support" }));
  const toggle = await screen.findByRole("checkbox", { name: "Show technical details" });
  expect((toggle as HTMLInputElement).checked).toBe(false);
  fireEvent.click(toggle);
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({...settings, show_technical_details: true}));
  // Read the saved preference back on the next opening.
  const saved = api.setSettings.mock.calls.at(-1)![0];
  cleanup();
  setup();
  api.getSettings.mockResolvedValue(saved);
  cleanup();
  render(<SettingsDialog onClose={() => {}} />);
  fireEvent.click(await screen.findByRole("tab", { name: "Support" }));
  expect((await screen.findByRole("checkbox", { name: "Show technical details" }) as HTMLInputElement).checked).toBe(true);
  fireEvent.click(screen.getByRole("checkbox", { name: "Show technical details" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenLastCalledWith({...settings, show_technical_details: false}));
});

test("picture support is automatic with no manual choice", async () => {
  setup();
  await screen.findByRole("radio", { name: "Anthropic API key" });
  expect(screen.queryByLabelText("Image support")).toBeNull();
  expect(screen.queryByText("Image support is unknown. Mizan will rely on measured checks.")).toBeNull();
});

test("a choice applies immediately, confirms only after persistence, and has no Save or Cancel footer", async () => {
  setup();
  let finish!: () => void;
  api.setSettings.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
  fireEvent.click(await screen.findByRole("radio", { name: "Approve each phase" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, autonomy: "per_phase" }));
  expect(screen.queryByText("Saved")).toBeNull();
  await act(async () => finish());
  expect(await screen.findByText("Saved")).toBeTruthy();
  for (const tab of ["Mizan", "Measurement", "General", "Connected apps", "Support"]) {
    fireEvent.click(screen.getByRole("tab", { name: tab }));
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  }
  expect(screen.getByRole("button", { name: "Close settings" })).toBeTruthy();
});

test("queued changes preserve both values and notify the app with the persisted settings", async () => {
  setup();
  cleanup();
  const onSaved = vi.fn();
  const onClose = vi.fn();
  let finish!: () => void;
  api.setSettings.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
  render(<SettingsDialog onSaved={onSaved} onClose={onClose} />);
  fireEvent.click(await screen.findByRole("radio", { name: "Approve each phase" }));
  fireEvent.click(screen.getByRole("button", { name: "More drawings" }));
  fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledTimes(1));
  expect(onSaved).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => finish());
  await vi.waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(onSaved).toHaveBeenLastCalledWith({ ...settings, autonomy: "per_phase", max_concurrent_turns: 4 });
});

test("a rejected model cannot leak into an unrelated immediate save", async () => {
  setup();
  await screen.findByRole("option", { name: "Claude Sonnet 5.5 — claude-sonnet-5-5" });
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "\u0000other" } });
  fireEvent.change(screen.getByLabelText("Model id"), { target: { value: "unavailable-model" } });
  expect((await screen.findByRole("alert")).textContent).toContain("This model is not available");
  expect(screen.getByLabelText("Model id").getAttribute("aria-invalid")).toBe("true");
  fireEvent.click(screen.getByRole("radio", { name: "Approve each phase" }));
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenLastCalledWith({ ...settings, autonomy: "per_phase" }));
  expect(api.setApiKey).not.toHaveBeenCalled();
});

test("key visibility is explicit and the OpenAI shortcut applies its address", async () => {
  setup();
  await screen.findByRole("radio", { name: "Anthropic API key" });
  chooseProvider("openai_compatible");
  fireEvent.click(screen.getByRole("button", { name: "OpenAI" }));
  await vi.waitFor(() => expect(api.setSettings.mock.calls.at(-1)?.[0].providers.openai_compatible.base_url).toBe("https://api.openai.com/v1"));
  const key = screen.getByLabelText("API key") as HTMLInputElement;
  fireEvent.change(key, { target: { value: "synthetic-key" } });
  fireEvent.click(screen.getByRole("button", { name: "Show key" }));
  expect(key.type).toBe("text");
  expect(api.setApiKey).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Hide key" }));
  expect(key.type).toBe("password");
  fireEvent.click(screen.getByRole("button", { name: "Connect key" }));
  await vi.waitFor(() => expect(api.setApiKey).toHaveBeenCalledWith("openai_compatible", "synthetic-key"));
});

test("an incomplete port remains editable and only a valid port is persisted", async () => {
  setup();
  fireEvent.click(await screen.findByRole("tab", { name: "Connected apps" }));
  const port = await screen.findByLabelText("Port") as HTMLInputElement;
  fireEvent.change(port, { target: { value: "48" } });
  expect(port.value).toBe("48");
  expect(port.getAttribute("aria-invalid")).toBe("true");
  expect(api.setSettings).not.toHaveBeenCalled();
  fireEvent.change(port, { target: { value: "48125" } });
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ ...settings, mcp_port: 48125 }));
  expect(port.getAttribute("aria-invalid")).toBe("false");
});

test("Arabic sets RTL on the sheet and keeps Latin fields and Western digits readable", async () => {
  setup();
  cleanup();
  localStorage.setItem("qiyas.lang", "ar");
  try {
    render(<LanguageProvider><SettingsDialog onClose={() => {}} /></LanguageProvider>);
    const dialog = await screen.findByRole("dialog");
    const sheet = dialog.querySelector(".settings-sheet")!;
    expect(sheet.getAttribute("dir")).toBe("rtl");
    expect(sheet.getAttribute("lang")).toBe("ar");
    expect((await screen.findByLabelText("مفتاح الواجهة البرمجية")).getAttribute("dir")).toBe("ltr");
    const tabs = within(dialog).getAllByRole("tab");
    expect(tabs).toHaveLength(5);
    act(() => tabs[0].focus());
    fireEvent.keyDown(tabs[0], { key: "ArrowLeft" });
    await vi.waitFor(() => expect(document.activeElement).toBe(tabs[1]));
    fireEvent.click(tabs[2]);
    expect(tabs[2].getAttribute("aria-selected")).toBe("true");
    const minutes = screen.getByRole("textbox") as HTMLInputElement;
    expect(minutes.value).toBe("5");
    expect(minutes.dir).toBe("ltr");
  } finally {
    localStorage.removeItem("qiyas.lang");
    document.documentElement.lang = "en";
    document.documentElement.dir = "ltr";
  }
});

test("a failed immediate write stays open, reports its field error and can be retried", async () => {
  setup();
  cleanup();
  const onClose = vi.fn();
  api.setSettings.mockRejectedValueOnce("permission denied");
  render(<SettingsDialog onClose={onClose} />);
  fireEvent.click(await screen.findByRole("radio", { name: "Approve each phase" }));
  fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
  await screen.findByRole("alert");
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.queryByText("Saved")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByText("Saved");
  expect(api.setSettings).toHaveBeenLastCalledWith({ ...settings, autonomy: "per_phase" });
  expect(screen.queryByRole("alert")).toBeNull();
});
