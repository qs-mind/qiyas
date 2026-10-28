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
