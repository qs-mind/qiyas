// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { WorkspacePicker } from "./WorkspacePicker";
const subscription = vi.hoisted(() => ({
  claude: vi.fn(async () => ({ installed: true, compatible: true, signed_in: true, supports_model: true, supports_effort: true, effort_levels: ["high"] })),
  models: vi.fn(async () => [{ id: "plan-model", label: "Plan model", reasoning_efforts: ["low", "high"] }]),
}));
vi.mock("../api", () => ({
  claudeCodeStatus: subscription.claude,
  chatgptModels: subscription.models,
  getSettings: async () => ({ providers: { anthropic: { model: "one", effort: "high" }, openai_compatible: { model: "two", effort: "low" } } }),
  aiModels: async () => ({ models: [{ id: "one", label: "One" }], from_service: true, error: null }),
}));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
test("the drawing picker uses that provider's saved model without saving global settings", async () => {
  const change = vi.fn(async () => {});
  const { rerender } = render(<LanguageProvider><WorkspacePicker choice={{ provider: "anthropic", model: "one", effort: "high" }} busy={false} onChange={change} /></LanguageProvider>);
  await waitFor(() => expect(screen.getByRole("option", { name: "One — one", hidden: true })).toBeTruthy());
  fireEvent.change(screen.getByLabelText("Provider"), { target: { value: "openai_compatible" } });
  expect(change).toHaveBeenCalledWith({ provider: "openai_compatible", model: "two", effort: "low" });
  rerender(<LanguageProvider><WorkspacePicker choice={{ provider: "anthropic", model: "one", effort: "high" }} busy onChange={change} /></LanguageProvider>);
  expect(screen.getByLabelText("Provider").closest("fieldset")?.disabled).toBe(true);
});


test("Claude choices require an explicit check and preserve Default effort when editing the model", async () => {
  const change = vi.fn(async () => {});
  render(<LanguageProvider><WorkspacePicker choice={{ provider: "claude_code", model: "", effort: null }} busy={false} onChange={change} /></LanguageProvider>);
  await waitFor(() => expect(screen.getByLabelText("Provider").closest("fieldset")?.disabled).toBe(false));
  expect(subscription.claude).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Check Claude Code", hidden: true }));
  await waitFor(() => expect(screen.getByRole("option", { name: "High", hidden: true })).toBeTruthy());
  expect(screen.queryByRole("option", { name: "Low", hidden: true })).toBeNull();
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "sonnet" } });
  expect(change).toHaveBeenCalledWith({ provider: "claude_code", model: "sonnet", effort: null });
});

test("ChatGPT model choices clear effort unsupported by the selected model", async () => {
  const change = vi.fn(async () => {});
  render(<LanguageProvider><WorkspacePicker choice={{ provider: "chatgpt_plan", model: "", effort: "max" }} busy={false} onChange={change} /></LanguageProvider>);
  await waitFor(() => expect(screen.getByLabelText("Provider").closest("fieldset")?.disabled).toBe(false));
  expect(subscription.models).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Read the list again", hidden: true }));
  await waitFor(() => expect(screen.getByRole("option", { name: "Plan model", hidden: true })).toBeTruthy());
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "plan-model" } });
  expect(change).toHaveBeenCalledWith({ provider: "chatgpt_plan", model: "plan-model", effort: null });
});
