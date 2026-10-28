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

test("Claude is read once by itself and never signs in or polls", async () => {
  render(<SubscriptionSettings provider="claude_code" executable="" onExecutable={() => {}} />);
  expect(screen.getByText("Open a terminal and run claude, then type /login.")).toBeTruthy();
  expect(api.claudeCodeStatus).not.toHaveBeenCalled();
  expect(api.chatgptSignIn).not.toHaveBeenCalled();
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

test("ChatGPT chooses the first account model when no model was saved", async () => {
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true });
  const onModel = vi.fn();
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} model="" onModel={onModel} />);
  await screen.findByRole("option", { name: "Mock model" });
  await waitFor(() => expect(onModel).toHaveBeenCalledWith("mock-slug"));
  expect((screen.getByRole("combobox", { name: "Model available with your ChatGPT plan" }) as HTMLSelectElement).value).toBe("mock-slug");
});

test.each(["en", "ar"])("Claude shows only help-listed model and effort choices in %s", async (language) => {
  localStorage.setItem("qiyas.lang", language);
  api.claudeCodeStatus.mockResolvedValue({ installed: true, compatible: true, signed_in: true, version: "2.1.309", code: null,
    supports_model: true, supports_effort: true, model_aliases: ["fable", "opus", "sonnet"], effort_levels: ["low", "medium", "high", "xhigh", "max"] });
  const onModel = vi.fn();
  render(<LanguageProvider><SubscriptionSettings provider="claude_code" executable="" onExecutable={() => {}} model="" onModel={onModel} /></LanguageProvider>);
  const modelLabel = language === "en" ? "Model" : "النموذج";
  expect(screen.queryByRole("combobox", { name: modelLabel })).toBeNull();
  const model = await screen.findByRole("combobox", { name: modelLabel }) as HTMLSelectElement;
  expect(Array.from(model.options, (o) => o.value)).toEqual(["", "fable", "opus", "sonnet", "\u0000other"]);
  expect(Array.from((screen.getByRole("combobox", { name: language === "en" ? "Effort" : "مستوى الجهد" }) as HTMLSelectElement).options, (o) => o.value))
    .toEqual(["", "low", "medium", "high", "xhigh", "max"]);
  fireEvent.change(model, { target: { value: "sonnet" } });
  expect(onModel).toHaveBeenLastCalledWith("sonnet");
});

test.each(["en", "ar"])("ChatGPT effort follows the chosen model in %s", async (language) => {
  localStorage.setItem("qiyas.lang", language);
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true });
  api.chatgptModels.mockResolvedValue([
    { id: "reasoning", label: "Reasoning model", reasoning_efforts: ["low", "high"] },
    { id: "plain", label: "Plain model" },
  ]);
  const onEffort = vi.fn();
  const props = { provider: "chatgpt_plan" as const, executable: "", onExecutable: () => {}, model: "reasoning", effort: "high", onEffort };
  const { rerender } = render(<LanguageProvider><SubscriptionSettings {...props} /></LanguageProvider>);
  const label = language === "en" ? "Effort" : "مستوى الجهد";
  const select = await screen.findByRole("combobox", { name: label }) as HTMLSelectElement;
  expect(Array.from(select.options, (o) => o.value)).toEqual(["", "low", "high"]);
  expect(select.value).toBe("high");
  fireEvent.change(select, { target: { value: "low" } });
  expect(onEffort).toHaveBeenLastCalledWith("low");
  fireEvent.change(select, { target: { value: "" } });
  expect(onEffort).toHaveBeenLastCalledWith(null);
  fireEvent.change(screen.getByRole("option", { name: "Plain model" }).parentElement!, { target: { value: "plain" } });
  expect(onEffort).toHaveBeenLastCalledWith(null);
  rerender(<LanguageProvider><SubscriptionSettings {...props} model="plain" effort={null} /></LanguageProvider>);
  expect(screen.queryByRole("combobox", { name: label })).toBeNull();
});

test("Claude hides unsupported flags and forgets capabilities when the program changes", async () => {
  api.claudeCodeStatus.mockResolvedValueOnce({ installed: true, compatible: true, signed_in: true, code: null,
    supports_model: false, supports_effort: false, model_aliases: [], effort_levels: [] })
    .mockResolvedValueOnce({ installed: true, compatible: true, signed_in: true, code: null,
      supports_model: true, supports_effort: true, model_aliases: ["fable"], effort_levels: ["high"] });
  const { rerender } = render(<SubscriptionSettings provider="claude_code" executable="" onExecutable={() => {}} />);
  await screen.findByText("Claude Code is ready.");
  expect(screen.queryByLabelText("Model")).toBeNull();
  expect(screen.queryByLabelText("Effort")).toBeNull();
  fireEvent.change(screen.getByLabelText("Claude Code program (optional)"), { target: { value: "/test/claude" } });
  rerender(<SubscriptionSettings provider="claude_code" executable="/test/claude" onExecutable={() => {}} />);
  expect(screen.queryByLabelText("Model")).toBeNull();
  expect(screen.queryByLabelText("Effort")).toBeNull();
  await screen.findByLabelText("Effort");
  expect(api.claudeCodeStatus).toHaveBeenCalledTimes(2);
  expect(api.claudeCodeStatus).toHaveBeenLastCalledWith("/test/claude");
});


test("checking an older Claude program clears choices its help no longer supports", async () => {
  api.claudeCodeStatus.mockResolvedValue({ installed: true, compatible: true, signed_in: true,
    supports_model: false, supports_effort: false, model_aliases: [], effort_levels: [] });
  const onModel = vi.fn();
  const onEffort = vi.fn();
  render(<SubscriptionSettings provider="claude_code" executable="" onExecutable={() => {}} model="sonnet" effort="high" onModel={onModel} onEffort={onEffort} />);
  await waitFor(() => expect(onModel).toHaveBeenCalledWith(""));
  expect(onEffort).toHaveBeenCalledWith(null);
  expect(screen.queryByLabelText("Model")).toBeNull();
  expect(screen.queryByLabelText("Effort")).toBeNull();
});


test("a late Claude status cannot change the newly selected provider", async () => {
  let resolve!: (value: unknown) => void;
  api.claudeCodeStatus.mockReturnValue(new Promise((done) => { resolve = done; }));
  const onModel = vi.fn();
  const props = { executable: "", onExecutable: () => {}, model: "sonnet", onModel };
  const { rerender } = render(<SubscriptionSettings {...props} provider="claude_code" />);
  await waitFor(() => expect(api.claudeCodeStatus).toHaveBeenCalled());
  rerender(<SubscriptionSettings {...props} provider="chatgpt_plan" />);
  resolve({ installed: true, compatible: true, supports_model: false, supports_effort: false });
  await screen.findByRole("button", { name: "Continue with ChatGPT" });
  await waitFor(() => expect(api.chatgptAccount).toHaveBeenCalled());
  expect(onModel).not.toHaveBeenCalled();
});
