// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ModelList } from "../api";
import { LanguageProvider } from "../i18n";
import { ModelPicker } from "./ModelPicker";

afterEach(cleanup);

const settle = async () => {
  for (let i = 0; i < 5; i++) await act(async () => {});
};

function setup(list: ModelList, value = "claude-opus-5-5", provider: "anthropic" | "openai_compatible" = "anthropic") {
  const onChange = vi.fn();
  const load = vi.fn().mockResolvedValue(list);
  render(<ModelPicker provider={provider} baseUrl={null} value={value} onChange={onChange} load={load} />);
  return { onChange, load };
}

const claude: ModelList = {
  models: [
    { id: "claude-opus-5-5", label: "Claude Opus 5.5" },
    { id: "claude-fable-5-1", label: "Claude Fable 5.1" },
  ],
  from_service: true,
  error: null,
};

test("lists the provider's models and chooses one", async () => {
  const { onChange, load } = setup(claude);
  await settle();
  expect(load).toHaveBeenCalledWith("anthropic", null);
  const select = screen.getByLabelText("Model") as HTMLSelectElement;
  const options = [...select.options].map((o) => o.textContent);
  expect(options).toEqual(["Claude Opus 5.5 — claude-opus-5-5", "Claude Fable 5.1 — claude-fable-5-1", "Other…"]);
  fireEvent.change(select, { target: { value: "claude-fable-5-1" } });
  expect(onChange).toHaveBeenCalledWith("claude-fable-5-1");
});

test("keeps a saved model the service no longer lists, and Other takes a typed id", async () => {
  const { onChange } = setup(claude, "claude-old-1");
  await settle();
  const select = screen.getByLabelText("Model") as HTMLSelectElement;
  expect(select.value).toBe("claude-old-1");
  expect(select.options[0].textContent).toContain("(saved)");
  fireEvent.change(select, { target: { value: "\u0000other" } });
  const id = screen.getByLabelText("Model id") as HTMLInputElement;
  fireEvent.change(id, { target: { value: "my-model" } });
  expect(onChange).toHaveBeenLastCalledWith("my-model");
});

test("falls back to the built-in list and says why", async () => {
  setup({ models: claude.models, from_service: false, error: "The API key was refused" });
  await settle();
  expect(screen.getByRole("status").textContent).toContain("The API key was refused");
});

test("a long list gets a filter", async () => {
  const many: ModelList = {
    models: Array.from({ length: 20 }, (_, i) => ({ id: `vendor/model-${i}`, label: `vendor/model-${i}` })),
    from_service: true,
    error: null,
  };
  setup(many, "vendor/model-3", "openai_compatible");
  await act(async () => void (await new Promise((r) => setTimeout(r, 600))));
  await settle();
  fireEvent.change(screen.getByLabelText("Filter models"), { target: { value: "model-1" } });
  const select = screen.getByLabelText("Model") as HTMLSelectElement;
  const ids = [...select.options].map((o) => o.value).filter((v) => v.startsWith("vendor"));
  // The saved model stays; the rest match the filter.
  expect(ids).toContain("vendor/model-3");
  expect(ids.filter((v) => v !== "vendor/model-3").every((v) => v.includes("model-1"))).toBe(true);
});

test("works in Arabic", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  const load = vi.fn().mockResolvedValue(claude);
  render(
    <LanguageProvider>
      <ModelPicker provider="anthropic" baseUrl={null} value="claude-opus-5-5" onChange={() => {}} load={load} />
    </LanguageProvider>,
  );
  await settle();
  expect(screen.getByRole("option", { name: "آخر…" })).toBeTruthy();
  localStorage.removeItem("qiyas.lang");
});
