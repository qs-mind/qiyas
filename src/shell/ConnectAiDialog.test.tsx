// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";

const api = vi.hoisted(() => ({
  settings: { mcp_enabled: false, mcp_port: 48123, mcp_token: "0123456789abcdef0123456789abcdef" } as Record<string, unknown>,
  listening: false,
  saved: [] as Record<string, unknown>[],
}));
vi.mock("../api", async (original) => ({
  ...(await original<object>()),
  getSettings: async () => ({ ...api.settings }),
  setSettings: async (next: Record<string, unknown>) => { api.saved.push(next); api.settings = next; api.listening = next.mcp_enabled === true; },
  mcpStatus: async () => ({ enabled: api.settings.mcp_enabled, listening: api.listening, addr: api.listening ? "127.0.0.1:48123" : null, error: null }),
  appDir: async () => "/opt/Qiyas",
}));
import { ConnectAiDialog } from "./ConnectAiDialog";

beforeEach(() => {
  api.settings = { mcp_enabled: false, mcp_port: 48123, mcp_token: "0123456789abcdef0123456789abcdef" };
  api.listening = false;
  api.saved = [];
});
afterEach(cleanup);

const mount = (onSettings = () => {}) => render(<LanguageProvider><ConnectAiDialog onClose={() => {}} onSettings={onSettings} /></LanguageProvider>);

test("it is off until someone turns it on; one click turns it on and shows the commands to copy", async () => {
  mount();
  expect(await screen.findByText(en["connect.off"])).toBeTruthy();
  expect(screen.queryByText(en["settings.claudeCode"])).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: en["connect.turnOn"] }));
  await waitFor(() => expect(screen.getByText(`Listening on 127.0.0.1:48123`)).toBeTruthy());
  // The same settings the Settings dialog saves: only the switch changed, the address and the token stay as they were.
  expect(api.saved).toHaveLength(1);
  expect(api.saved[0]).toMatchObject({ mcp_enabled: true, mcp_port: 48123, mcp_token: "0123456789abcdef0123456789abcdef" });
  const code = document.querySelector(".snippet pre")!.textContent!;
  expect(code).toContain("claude mcp add qiyas -e QIYAS_MCP_TOKEN=");
  expect(code).toContain("/opt/Qiyas/qiyas-mcp");
  expect(screen.getByText(en["connect.warn"])).toBeTruthy();
});

test("a missing token is made, never left empty, and turning off stops it", async () => {
  api.settings = { ...api.settings, mcp_token: "" };
  mount();
  fireEvent.click(await screen.findByRole("button", { name: en["connect.turnOn"] }));
  await waitFor(() => expect(api.saved).toHaveLength(1));
  expect(String(api.saved[0].mcp_token)).toMatch(/^[0-9a-f]{32}$/);
  fireEvent.click(await screen.findByRole("button", { name: en["connect.turnOff"] }));
  await waitFor(() => expect(api.saved).toHaveLength(2));
  expect(api.saved[1].mcp_enabled).toBe(false);
});

test("the Settings link opens Settings for the port and the token", async () => {
  const onSettings = vi.fn();
  mount(onSettings);
  fireEvent.click(await screen.findByRole("button", { name: en["connect.more"] }));
  expect(onSettings).toHaveBeenCalledOnce();
});
