// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider, translate, type Lang } from "../i18n";
import { SubscriptionSettings } from "./SubscriptionSettings";

const api = vi.hoisted(() => ({
  aiProviderStatus: vi.fn(),
  chatgptAccount: vi.fn(), chatgptSignIn: vi.fn(), chatgptSignOut: vi.fn(),
  chatgptModels: vi.fn(), chatgptSelectAccount: vi.fn(), chatgptResumePlan: vi.fn(),
  cancelChatgptSignIn: vi.fn(), reopenChatgptSignIn: vi.fn(), claudeCodeStatus: vi.fn(), subscriptionLink: vi.fn(),
}));
vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));
beforeEach(() => {
  api.aiProviderStatus.mockResolvedValue({ provider: "chatgpt_plan", status: "connected", configured: true, verified: false, last_failure: null, actions: [] });
  api.chatgptAccount.mockResolvedValue({ signed_in: false, display_name: null, plan_granted: false });
  api.chatgptModels.mockResolvedValue([{ id: "mock-slug", label: "Mock model" }]);
  api.subscriptionLink.mockResolvedValue(undefined);
  api.cancelChatgptSignIn.mockResolvedValue(undefined);
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
  expect(screen.getByRole("alert").textContent).toContain("غير مفعل");
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
  fireEvent.click(screen.getByRole("button", { name: "Switch account" }));
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
  expect(within(screen.getByRole("radiogroup", { name: language === "en" ? "Effort" : "مستوى الجهد" })).getAllByRole("radio").map(radio => radio.textContent))
    .toEqual(["settings.subscriptionDefault", "effort.low", "effort.medium", "effort.high", "effort.xhigh", "effort.max"].map(key => translate(language as Lang, key)));
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
  const group = await screen.findByRole("radiogroup", { name: label });
  const choices = within(group).getAllByRole("radio");
  expect(choices.map(radio => radio.textContent)).toEqual(["settings.subscriptionDefault", "effort.low", "effort.high"].map(key => translate(language as Lang, key)));
  expect(choices[2].getAttribute("aria-checked")).toBe("true");
  fireEvent.click(choices[1]);
  expect(onEffort).toHaveBeenLastCalledWith("low");
  fireEvent.click(choices[0]);
  expect(onEffort).toHaveBeenLastCalledWith(null);
  fireEvent.change(screen.getByRole("option", { name: "Plain model" }).parentElement!, { target: { value: "plain" } });
  expect(onEffort).toHaveBeenLastCalledWith(null);
  rerender(<LanguageProvider><SubscriptionSettings {...props} model="plain" effort={null} /></LanguageProvider>);
  expect(screen.queryByRole("radiogroup", { name: label })).toBeNull();
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


test("waiting keeps cancel and reopen usable and timeout enables a fresh sign-in", async () => {
  let reject!: (reason: unknown) => void;
  api.chatgptSignIn.mockReturnValueOnce(new Promise((_resolve, fail) => { reject = fail; }));
  api.reopenChatgptSignIn.mockResolvedValue(undefined);
  api.cancelChatgptSignIn.mockResolvedValue(undefined);
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  const start = await screen.findByRole("button", { name: "Continue with ChatGPT" });
  await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(start);
  const reopen = await screen.findByRole("button", { name: "Open the page again" });
  const cancel = screen.getByRole("button", { name: "Cancel" });
  expect(screen.getByText("Finish signing in in your browser")).toBeTruthy();
  expect(reopen.closest(".settings-sign-in-wait")?.querySelector(".dot-loader")).toBeTruthy();
  expect((reopen as HTMLButtonElement).disabled).toBe(false);
  expect((cancel as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(reopen);
  await waitFor(() => expect(api.reopenChatgptSignIn).toHaveBeenCalledTimes(1));
  expect(api.chatgptSignIn).toHaveBeenCalledTimes(1);
  expect((start as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(cancel);
  expect(api.cancelChatgptSignIn).toHaveBeenCalledTimes(1);
  reject(JSON.stringify({ code: "chatgpt_timeout", message: "Sign-in timed out." }));
  await screen.findByRole("alert");
  await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
  expect(screen.queryByRole("button", { name: "Open the page again" })).toBeNull();
});


test.each(["complete", "fail"] as const)("switching provider cancels sign-in and ignores its late %s", async (outcome) => {
  let complete!: (value: unknown) => void;
  let fail!: (reason: unknown) => void;
  api.chatgptSignIn.mockReturnValueOnce(new Promise((resolve, reject) => { complete = resolve; fail = reject; }));
  api.claudeCodeStatus.mockResolvedValue({ installed: true, compatible: true, signed_in: true,
    supports_model: true, supports_effort: true, model_aliases: ["sonnet"], effort_levels: ["high"] });
  const onModel = vi.fn();
  const props = { executable: "", onExecutable: () => {}, onModel };
  const view = render(<SubscriptionSettings {...props} provider="chatgpt_plan" />);
  const start = await screen.findByRole("button", { name: "Continue with ChatGPT" });
  await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByRole("button", { name: "Open the page again" });
  onModel.mockClear();
  view.rerender(<SubscriptionSettings {...props} provider="claude_code" model="sonnet" />);
  expect(api.cancelChatgptSignIn).toHaveBeenCalledTimes(1);
  expect((screen.getByLabelText("Claude Code program (optional)") as HTMLInputElement).disabled).toBe(false);
  await screen.findByLabelText("Model");
  await act(async () => {
    if (outcome === "complete") complete({ signed_in: true, plan_granted: true, display_name: "Old ChatGPT account" });
    else fail(JSON.stringify({ code: "ai.error.chatgpt_timeout" }));
  });
  expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("sonnet");
  expect((screen.getByLabelText("Model") as HTMLSelectElement).disabled).toBe(false);
  expect(onModel).not.toHaveBeenCalled();
  expect(api.chatgptModels).not.toHaveBeenCalled();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("an old sign-in completion cannot release a new sign-in's waiting controls", async () => {
  let completeOld!: (value: unknown) => void;
  api.chatgptSignIn.mockReturnValueOnce(new Promise((resolve) => { completeOld = resolve; }))
    .mockReturnValueOnce(new Promise(() => {}));
  const props = { executable: "", onExecutable: () => {} };
  const view = render(<SubscriptionSettings {...props} provider="chatgpt_plan" />);
  const start = await screen.findByRole("button", { name: "Continue with ChatGPT" });
  await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(start);
  view.rerender(<SubscriptionSettings {...props} provider="claude_code" />);
  view.rerender(<SubscriptionSettings {...props} provider="chatgpt_plan" />);
  const nextStart = screen.getByRole("button", { name: "Continue with ChatGPT" });
  await waitFor(() => expect((nextStart as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(nextStart);
  await act(async () => { completeOld({ signed_in: true, plan_granted: true, display_name: "Old account" }); });
  expect((nextStart as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByRole("button", { name: "Open the page again" })).toBeTruthy();
  expect(screen.queryByText("Old account")).toBeNull();
  expect(api.chatgptModels).not.toHaveBeenCalled();
});

test("closing settings cancels a pending ChatGPT sign-in", async () => {
  api.chatgptSignIn.mockReturnValueOnce(new Promise(() => {}));
  const view = render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  const start = await screen.findByRole("button", { name: "Continue with ChatGPT" });
  await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(start);
  view.unmount();
  expect(api.cancelChatgptSignIn).toHaveBeenCalledTimes(1);
});

test.each([
  ["plan_limit", "Allowance used until", true],
  ["spend_blocked", "Needs the credits switch", true],
  ["ok", "Connected", false],
] as const)("the account row renders %s and its recovery actions from provider status", async (status, label, retry) => {
  const account = { signed_in: true, display_name: "Project surveyor", plan_granted: true, paused: retry };
  api.chatgptAccount.mockResolvedValue(account);
  api.aiProviderStatus.mockResolvedValue({
    provider: "chatgpt_plan", status, configured: true, verified: !retry, last_failure: null,
    resets_at: status === "plan_limit" ? 1793700000 : undefined,
    actions: retry ? [{ kind: "retry" }, { kind: "open_provider_page", page: "chatgpt_usage" }, { kind: "switch_provider" }] : [],
  });
  api.chatgptResumePlan.mockResolvedValue({ ...account, paused: false });
  render(<SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} />);
  const line = await screen.findByText(new RegExp(`^${label}`));
  expect(line.getAttribute("data-state")).toBe(status);
  expect(line.classList.contains("settings-status")).toBe(true);
  expect(screen.getByText("Project surveyor").tagName).toBe("BDI");
  expect(screen.getByRole("button", { name: "Switch account" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Open ChatGPT usage settings" }));
  await waitFor(() => expect(api.subscriptionLink).toHaveBeenCalledWith("chatgpt_usage"));
  if (retry) {
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(api.chatgptResumePlan).toHaveBeenCalledOnce());
  } else {
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  }
});

test("Arabic limit times use Western digits and an isolated date", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  api.chatgptAccount.mockResolvedValue({ signed_in: true, plan_granted: true, paused: true });
  api.aiProviderStatus.mockResolvedValue({ provider: "chatgpt_plan", status: "plan_limit", configured: true, verified: false,
    resets_at: 1793700000, last_failure: null, actions: [{ kind: "retry" }] });
  render(<LanguageProvider><SubscriptionSettings provider="chatgpt_plan" executable="" onExecutable={() => {}} /></LanguageProvider>);
  const line = await screen.findByText(/^تم استهلاك الحصة حتى/);
  expect(line.textContent).toMatch(/\u2068.*[0-9].*\u2069/);
  expect(line.textContent).not.toMatch(/[٠-٩]/);
});
