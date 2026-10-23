// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { SubscriptionSettings } from "./SubscriptionSettings";

const api = vi.hoisted(() => ({
  chatgptAccount: vi.fn(), chatgptSignIn: vi.fn(), chatgptSignOut: vi.fn(),
  chatgptModels: vi.fn(), chatgptSelectAccount: vi.fn(), chatgptResumePlan: vi.fn(),
  cancelChatgptSignIn: vi.fn(), claudeCodeStatus: vi.fn(), subscriptionLink: vi.fn(),
}));
vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));
beforeEach(() => {
  api.chatgptAccount.mockResolvedValue({ signed_in: false, display_name: null, plan_granted: false });
  api.chatgptModels.mockResolvedValue([{ id: "mock-slug", label: "Mock model" }]);
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
  api.chatgptSignOut.mockResolvedValue({ signed_in: false, display_name: null, plan_granted: false });
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  expect(api.chatgptSignIn).not.toHaveBeenCalled();
  await waitFor(() => expect((screen.getByRole("button", { name: "Continue with ChatGPT" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  await screen.findByText("Test Surveyor");
  expect(screen.getByText(/Usage counts against your ChatGPT plan/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
  await screen.findByRole("button", { name: "Continue with ChatGPT" });
  expect(api.chatgptSignOut).toHaveBeenCalledTimes(1);
  expect(api.claudeCodeStatus).not.toHaveBeenCalled();
});

test("Arabic keeps the same account controls and explains missing plan permission", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  api.chatgptAccount.mockResolvedValue({ signed_in: true, display_name: "Test Surveyor", plan_granted: false });
  render(<LanguageProvider><SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} /></LanguageProvider>);
  await screen.findByRole("button", { name: "تسجيل الخروج" });
  expect(screen.getByRole("alert").textContent).toContain("غير مفعّل");
  expect(screen.getByText("Test Surveyor").tagName).toBe("BDI");
});


test("account changes clear the old model list and load visible models for the active account", async () => {
  const choices = [{ id: "first", display_name: "First surveyor", signed_in: true }, { id: "second", display_name: "Second surveyor", signed_in: true }];
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true, active_id: "first", display_name: "First surveyor", accounts: choices });
  api.chatgptSelectAccount.mockResolvedValue({ signed_in: true, plan_granted: true, active_id: "second", display_name: "Second surveyor", accounts: choices });
  api.chatgptModels.mockResolvedValueOnce([{ id: "first-slug", label: "First model" }]).mockResolvedValueOnce([{ id: "second-slug", label: "Second model" }]);
  const onModel = vi.fn();
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} model="first-slug" onModel={onModel} />);
  await screen.findByRole("option", { name: "First model" });
  fireEvent.change(screen.getByRole("combobox", { name: "ChatGPT account" }), { target: { value: "second" } });
  await screen.findByRole("option", { name: "Second model" });
  expect(screen.queryByRole("option", { name: "First model" })).toBeNull();
  expect(api.chatgptSelectAccount).toHaveBeenCalledWith("second");
  expect(onModel).toHaveBeenCalledWith("");
  fireEvent.change(screen.getByRole("combobox", { name: "Model available with your ChatGPT plan" }), { target: { value: "second-slug" } });
  expect(onModel).toHaveBeenCalledWith("second-slug");
});

test("scope refusal keeps sign-in and model discovery is not called", async () => {
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: false, display_name: "Test Surveyor" });
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "Continue with ChatGPT" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
  expect(api.chatgptModels).not.toHaveBeenCalled();
});

test("revocation failure still signs out locally and explains the remaining action", async () => {
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true, display_name: "Test Surveyor" });
  api.chatgptSignOut.mockResolvedValue({ signed_in: false, plan_granted: false, display_name: "Test Surveyor", notice: "chatgpt_revocation_unconfirmed" });
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: "Sign out" }));
  await screen.findByText(/Signed out on this device/);
  expect(screen.queryByRole("button", { name: "Sign out" })).toBeNull();
});

test("plan limits require an explicit resume and do not poll or discover models while paused", async () => {
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true, display_name: "Test Surveyor", paused: true });
  api.chatgptResumePlan.mockResolvedValue({ signed_in: true, plan_granted: true, display_name: "Test Surveyor", paused: false });
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  await screen.findByText(/Requests are paused/);
  expect(api.chatgptModels).not.toHaveBeenCalled();
  expect(api.chatgptResumePlan).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Try plan requests again" }));
  await screen.findByRole("option", { name: "Mock model" });
  expect(api.chatgptResumePlan).toHaveBeenCalledTimes(1);
});

test("Arabic uses the reviewed Continue with ChatGPT label", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  render(<LanguageProvider><SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} /></LanguageProvider>);
  await waitFor(() => expect((screen.getByRole("button", { name: "المتابعة باستخدام ChatGPT" }) as HTMLButtonElement).disabled).toBe(false));
  expect(api.chatgptSignIn).not.toHaveBeenCalled();
});
