import { afterEach, expect, test, vi } from "vitest";
import { readFileSync } from "node:fs";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { aiProviderStatus, subscriptionLink } from "./api";
import { setWorkspace } from "./workspaceIpc";
afterEach(() => { setWorkspace(null); invoke.mockReset(); });

test("status follows the displayed workspace and preserves structured failure data", async () => {
  setWorkspace(42);
  const status = { provider: "chatgpt_plan", status: "plan_limit", verified: false,
    last_failure: { kind: "plan_limit", resets_at: 1800000000 }, actions: [] };
  invoke.mockResolvedValue(status);
  expect(await aiProviderStatus()).toEqual(status);
  expect(invoke).toHaveBeenCalledWith("ai_provider_status", { workspace: 42 });
  await aiProviderStatus(7);
  expect(invoke).toHaveBeenLastCalledWith("ai_provider_status", { workspace: 7 });
});

test.each(["chatgpt_usage", "claude_usage", "openai_billing", "anthropic_billing"] as const)("provider link %s sends only a fixed identifier", async link => {
  await subscriptionLink(link);
  expect(invoke).toHaveBeenCalledWith("subscription_link", { link });
});

test("status and sign-in reopening are registered under the desktop allowlist", () => {
  const root = new URL("../crates/qiyas-desktop/", import.meta.url);
  const build = readFileSync(new URL("build.rs", root), "utf8");
  const handlers = readFileSync(new URL("src/lib.rs", root), "utf8");
  const capability = JSON.parse(readFileSync(new URL("capabilities/default.json", root), "utf8"));
  for (const command of ["ai_provider_status", "reopen_chatgpt_sign_in"]) {
    expect(build).toContain(`"${command}"`);
    expect(handlers).toContain(`::${command},`);
    expect(capability.permissions).toContain(`allow-${command.replaceAll("_", "-")}`);
  }
});
