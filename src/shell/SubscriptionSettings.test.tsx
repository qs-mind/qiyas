// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { SubscriptionSettings } from "./SubscriptionSettings";

const api = vi.hoisted(() => ({
  chatgptAccount: vi.fn(), chatgptSignIn: vi.fn(), chatgptSignOut: vi.fn(),
  cancelChatgptSignIn: vi.fn(), claudeCodeStatus: vi.fn(), subscriptionLink: vi.fn(),
}));
vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));
beforeEach(() => {
  api.chatgptAccount.mockResolvedValue({ signed_in: false, display_name: null, plan_granted: false });
  api.subscriptionLink.mockResolvedValue(undefined);
  api.claudeCodeStatus.mockResolvedValue({ installed: false, compatible: false, signed_in: null, version: null, code: "claude_missing" });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear(); });

test("Claude instructions and explicit checks never sign in or poll", async () => {
  render(<SubscriptionSettings provider="claude_code" executable="" onExecutable={() => {}} />);
  expect(screen.getByText("Open a terminal and run claude, then type /login.")).toBeTruthy();
  expect(api.claudeCodeStatus).not.toHaveBeenCalled();
  expect(api.chatgptSignIn).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Check Claude Code" }));
  await screen.findByText(/Claude Code is not installed/);
  expect(api.claudeCodeStatus).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Install Claude Code" }));
  await waitFor(() => expect(api.subscriptionLink).toHaveBeenCalledWith("claude_install"));
});

test("ChatGPT connects on click and displays account, plan usage and sign out", async () => {
  const account = { signed_in: true, display_name: "Test Surveyor", plan_granted: true };
  api.chatgptSignIn.mockResolvedValue(account);
  api.chatgptSignOut.mockResolvedValue(undefined);
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  expect(api.chatgptSignIn).not.toHaveBeenCalled();
  fireEvent.click(await screen.findByRole("button", { name: "Sign in with ChatGPT" }));
  await screen.findByText("Test Surveyor");
  expect(screen.getByText(/Usage counts against your ChatGPT plan/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
  await screen.findByRole("button", { name: "Sign in with ChatGPT" });
  expect(api.chatgptSignOut).toHaveBeenCalledTimes(1);
  expect(api.claudeCodeStatus).not.toHaveBeenCalled();
});

test("Arabic keeps the same account controls and explains missing plan permission", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  api.chatgptAccount.mockResolvedValue({ signed_in: true, display_name: "Test Surveyor", plan_granted: false });
  render(<LanguageProvider><SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} /></LanguageProvider>);
  await screen.findByRole("button", { name: "تسجيل الخروج" });
  expect(screen.getByRole("alert").textContent).toContain("لم تُمنح صلاحية");
  expect(screen.getByText("Test Surveyor").tagName).toBe("BDI");
});
