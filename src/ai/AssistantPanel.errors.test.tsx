// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import ar from "../i18n/ar.json";
import { AssistantPanel } from "./AssistantPanel";
import { EMPTY, type Transcript } from "./messages";

afterEach(() => {
  cleanup();
  localStorage.removeItem("qiyas.lang");
  document.documentElement.lang = "en";
  document.documentElement.dir = "ltr";
});

function setup(error: string | null, transcript: Transcript = EMPTY, busy = false) {
  const assistant = { transcript, busy, error, send: vi.fn().mockResolvedValue(true), cancel: vi.fn(), clear: vi.fn() };
  const onSettings = vi.fn();
  render(<LanguageProvider><AssistantPanel assistant={assistant} onSettings={onSettings} onShow={() => {}} onUndo={() => {}} /></LanguageProvider>);
  return { assistant, onSettings };
}

test.each(["chatgpt_failed", "ai.error.chatgpt_failed", JSON.stringify({ code: "ai.error.chatgpt_failed", message: "private diagnostic" })])(
  "a failed ChatGPT response explains what happened and continues the kept work: %s", (error) => {
    const { assistant } = setup(error);
    expect(screen.getByRole("alert").textContent).toContain("ChatGPT stopped answering. Your drawing work so far is kept.");
    expect(screen.getByRole("alert").textContent).not.toContain("chatgpt_failed");
    expect(screen.getByRole("alert").textContent).not.toContain("private diagnostic");
    fireEvent.click(screen.getByRole("button", { name: en["ai.continueWorkButton"] }));
    expect(assistant.send).toHaveBeenCalledWith(en["ai.continueWork"]);
    expect(assistant.clear).not.toHaveBeenCalled();
  },
);

test("ChatGPT access errors keep the Settings link without Continue", () => {
  const { onSettings } = setup(JSON.stringify({ code: "ai.error.chatgpt_access" }));
  expect(screen.getByRole("alert").textContent).toContain(en["ai.error.chatgpt_access"]);
  expect(screen.queryByRole("button", { name: en["ai.continueWorkButton"] })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: en.settings }));
  expect(onSettings).toHaveBeenCalledOnce();
});

test("a stopped item with chatgpt_incomplete offers the same sentence and Continue", () => {
  const { assistant } = setup(null, { items: [{ kind: "stopped", reason: "chatgpt_incomplete" }] });
  expect(screen.getByRole("note").textContent).toContain(en["ai.error.chatgpt_incomplete"]);
  fireEvent.click(screen.getByRole("button", { name: en["ai.continueWorkButton"] }));
  expect(assistant.send).toHaveBeenCalledWith(en["ai.continueWork"]);
});

test("Arabic stopped errors explain the kept work and send the Arabic continuation", () => {
  localStorage.setItem("qiyas.lang", "ar");
  const { assistant } = setup(null, { items: [{ kind: "stopped", reason: "ai.error.chatgpt_incomplete" }] });
  expect(screen.getByRole("note").textContent).toContain(ar["ai.error.chatgpt_incomplete"]);
  fireEvent.click(screen.getByRole("button", { name: ar["ai.continueWorkButton"] }));
  expect(assistant.send).toHaveBeenCalledWith(ar["ai.continueWork"]);
});

test("Continue waits while another run is busy", () => {
  setup("chatgpt_failed", EMPTY, true);
  expect((screen.getByRole("button", { name: en["ai.continueWorkButton"] }) as HTMLButtonElement).disabled).toBe(true);
});

vi.mock("../api", async (importOriginal) => ({ ...await importOriginal<typeof import("../api")>(), subscriptionLink: vi.fn().mockResolvedValue(undefined) }));

test.each(["en", "ar"] as const)("%s shows one actionable limit card and retries in the conversation language", (lang) => {
  localStorage.setItem("qiyas.lang", lang);
  const strings = lang === "ar" ? ar : en;
  const error = JSON.stringify({ code: "ai.error.chatgpt_limit", kind: "plan_limit", provider: "chatgpt_plan", actions: [{ kind: "retry" }, { kind: "open_provider_page", page: "chatgpt_usage" }, { kind: "open_settings" }, { kind: "switch_provider" }] });
  const { assistant, onSettings } = setup(null, { items: [{ kind: "user", text: "Continue the hospital tower drawing." }, { kind: "stopped", reason: error }] });
  const card = screen.getByRole("note");
  expect(card.getAttribute("dir")).toBe(lang === "ar" ? "rtl" : "ltr");
  expect(card.textContent).toContain("Allow other apps to use credits after reaching your usage limit");
  fireEvent.click(screen.getByRole("button", { name: strings["ai.failure.retry"] }));
  expect(assistant.send).toHaveBeenCalledWith(en["ai.continueWork"]);
  fireEvent.click(screen.getByRole("button", { name: strings["ai.failure.settings"] }));
  fireEvent.click(screen.getByRole("button", { name: strings["ai.failure.switchProvider"] }));
  expect(onSettings).toHaveBeenCalledTimes(2);
});

test.each(["en", "ar"] as const)("%s live spend-blocked card offers the allowlisted provider page", async (lang) => {
  localStorage.setItem("qiyas.lang", lang);
  const strings = lang === "ar" ? ar : en;
  const { subscriptionLink } = await import("../api");
  setup(JSON.stringify({ code: "ai.error.chatgpt_spend_blocked", kind: "spend_blocked", provider: "chatgpt_plan", actions: [{ kind: "retry" }, { kind: "open_provider_page", page: "chatgpt_usage" }] }));
  expect(screen.getByRole("alert").textContent).toContain(lang === "en" ? "permission" : "إذن");
  expect(screen.getByRole("alert").getAttribute("dir")).toBe(lang === "ar" ? "rtl" : "ltr");
  expect(screen.getByRole("alert").textContent).toContain("Allow other apps to use credits after reaching your usage limit");
  fireEvent.click(screen.getByRole("button", { name: strings["ai.failure.usage"] }));
  expect(subscriptionLink).toHaveBeenCalledWith("chatgpt_usage");
});

test("a failure present in both the live state and transcript appears once", () => {
  const error = JSON.stringify({ code: "ai.error.chatgpt_limit", kind: "plan_limit", provider: "chatgpt_plan", actions: [{ kind: "retry" }] });
  setup(error, { items: [{ kind: "stopped", reason: error }] });
  expect(screen.getAllByRole("button", { name: en["ai.failure.retry"] })).toHaveLength(1);
});

test("Try again stays disabled during a running turn", () => {
  setup(JSON.stringify({ code: "ai.error.rate_limited", kind: "rate_limited", provider: "openai", actions: [{ kind: "retry" }] }), EMPTY, true);
  expect((screen.getByRole("button", { name: en["ai.failure.retry"] }) as HTMLButtonElement).disabled).toBe(true);
});
