import { expect, test } from "vitest";
import { providerFailure } from "./providerFailure";

test.each([
  "chatgpt_failed", "chatgpt_incomplete", "chatgpt_temporary", "chatgpt_limit", "chatgpt_empty",
  "chatgpt_offline", "chatgpt_timeout", "service_error", "network", "timeout", "stream", "rate_limited", "claude_limit", "claude_failed", "claude_stream",
])("%s keeps work and offers Continue in both languages and error formats", (code) => {
  for (const lang of ["en", "ar"] as const) {
    for (const error of [code, `ai.error.${code}`, JSON.stringify({ code: `ai.error.${code}` })]) {
      const failure = providerFailure(error, lang);
      expect(failure?.canContinue).toBe(true);
      expect(failure?.text).toContain(lang === "en" ? "drawing work so far is kept" : "العمل المنجز في الرسم محفوظ");
      expect(failure?.text).not.toContain(code);
    }
  }
});

test.each(["chatgpt_access", "chatgpt_setup", "chatgpt_ineligible", "chatgpt_model", "auth", "billing", "no_model", "no_api_key", "model_not_found"])(
  "%s requires Settings", (code) => expect(providerFailure(code, "en")?.canContinue).toBe(false),
);

test("an explicit limit reset is shown; missing or invalid reset uses the reset advice", () => {
  const reset = "2026-10-29T12:00:00Z";
  const failure = providerFailure(JSON.stringify({ code: "ai.error.chatgpt_limit", resets_at: reset }), "en");
  expect(failure?.text).toContain(new Date(reset).toLocaleString("en"));
  expect(failure?.canContinue).toBe(true);
  for (const reset_at of [undefined, "private diagnostic"]) {
    expect(providerFailure(JSON.stringify({ code: "chatgpt_limit", reset_at }), "en")?.text).toContain("until the limit resets");
  }
});

test("ordinary stop reasons keep their existing handling; unknown provider codes are never shown", () => {
  expect(providerFailure("cancelled", "en")).toBeNull();
  expect(providerFailure("stopped after 12 steps", "en")).toBeNull();
  expect(providerFailure("ai.error.future_provider_failure", "en")?.text).toContain("service stopped answering");
});

test("structured ChatGPT limits explain the credit permission without losing reset advice", () => {
  const failure = providerFailure(JSON.stringify({ code: "ai.error.chatgpt_limit", kind: "plan_limit", provider: "chatgpt_plan", resets_at: 1793620800, actions: [{ kind: "retry" }, { kind: "open_provider_page", page: "chatgpt_usage" }] }), "en");
  expect(failure?.text).toContain('Allow other apps to use credits after reaching your usage limit');
  expect(failure?.text).toContain('Settings → Usage & billing');
  expect(failure?.text).toContain('if you have credits');
  expect(failure?.text).toContain(new Date(1793620800 * 1000).toLocaleString('en'));
  expect(failure).toMatchObject({ kind: "plan_limit", provider: "chatgpt_plan", actions: [{ kind: "retry" }, { kind: "open_provider_page", page: "chatgpt_usage" }] });
});

test("Arabic reset times keep Western digits and provider diagnostics stay private", () => {
  const failure = providerFailure(JSON.stringify({ code: "ai.error.chatgpt_limit", kind: "plan_limit", provider: "chatgpt_plan", resets_at: 1793620800, message: "PRIVATE", actions: [{ kind: "open_provider_page", page: "https://untrusted.example" }, { kind: "retry" }] }), "ar");
  expect(failure?.text).not.toMatch(/[٠-٩]/);
  expect(failure?.text).not.toContain("PRIVATE");
  expect(failure?.text).toContain('Allow other apps to use credits after reaching your usage limit');
  expect(failure).toMatchObject({ actions: [{ kind: "retry" }] });
});

test("spend permission and exhausted credit are separate from temporary rate limits", () => {
  for (const [kind, expected] of [["spend_blocked", "permission"], ["out_of_credit", "credit"], ["rate_limited", "requests"]]) {
    expect(providerFailure(JSON.stringify({ code: "ai.error.rate_limited", kind, provider: "chatgpt_plan" }), "en")?.text).toContain(expected);
  }
});


test("saved codes acquire safe recovery kinds and actions", () => {
  expect(providerFailure("chatgpt_limit", "en")).toMatchObject({ kind: "plan_limit", provider: "chatgpt_plan", actions: [{ kind: "retry" }, { kind: "open_provider_page", page: "chatgpt_usage" }, { kind: "switch_provider" }] });
  expect(providerFailure("chatgpt_offline", "en")).toMatchObject({ kind: "offline", provider: "chatgpt_plan" });
  expect(providerFailure(JSON.stringify({ code: "future_code", kind: "temporary" }), "en")?.text).toContain("service stopped answering");
  expect(providerFailure(JSON.stringify({ code: "ai.error.timeout", kind: "other" }), "en")?.text).toContain("took too long");
});


test("Claude limit recovery keeps extra usage conditional and reports an explicit wait", () => {
  const failure = providerFailure(JSON.stringify({ code: "ai.error.claude_limit", kind: "plan_limit", provider: "claude_code" }), "en");
  expect(failure?.text).toContain("If extra usage is available for your account and you want to use it");
  expect(failure?.text).toContain("Wait until the limit resets");
  expect(providerFailure(JSON.stringify({ code: "ai.error.chatgpt_timeout", kind: "other" }), "en")?.text).toContain("took too long");
  expect(providerFailure(JSON.stringify({ code: "ai.error.rate_limited", kind: "rate_limited", retry_after: 15 }), "ar")?.text).toContain("15");
});


test.each(["chatgpt_timeout", "chatgpt_failed", "claude_failed", "claude_unavailable"])("legacy %s keeps the backend's conservative classification", (code) => {
  expect(providerFailure(code, "en")).toMatchObject({ kind: "other", actions: [{ kind: "retry" }, { kind: "open_settings" }, { kind: "switch_provider" }] });
});

test("legacy auth and billing include explicit retry while retaining canContinue compatibility", () => {
  for (const code of ["auth", "billing"]) {
    expect(providerFailure(code, "en")).toMatchObject({ canContinue: false, actions: [{ kind: "retry" }, { kind: "open_settings" }, { kind: "switch_provider" }] });
  }
  expect(providerFailure("chatgpt_rate_limited", "en")?.kind).toBe("rate_limited");
});

test("API billing links are reserved for credit and spend failures", () => {
  const plan = providerFailure(JSON.stringify({ code: "ai.error.plan_limit", provider: "openai" }), "en");
  expect(plan?.actions).toEqual([{ kind: "retry" }, { kind: "switch_provider" }]);
  const credit = providerFailure(JSON.stringify({ code: "ai.error.billing", provider: "openai" }), "en");
  expect(credit?.actions).toContainEqual({ kind: "open_provider_page", page: "openai_billing" });
});

test.each(["en", "ar"] as const)("%s puts a known ChatGPT reset before credit permission advice", (lang) => {
  const timestamp = 1793620800;
  const failure = providerFailure(JSON.stringify({ code: "ai.error.chatgpt_limit", kind: "plan_limit", provider: "chatgpt_plan", resets_at: timestamp }), lang)!;
  const formatted = new Date(timestamp * 1000).toLocaleString(lang === "ar" ? "ar-EG-u-nu-latn" : "en");
  expect(failure.text.indexOf(formatted)).toBeGreaterThan(-1);
  expect(failure.text.indexOf(formatted)).toBeLessThan(failure.text.indexOf("Allow other apps to use credits after reaching your usage limit"));
  expect(failure.text.split(formatted)).toHaveLength(2);
  expect(failure.text).toContain(lang === "en" ? "allowance is used up until" : "حتى");
});

test("incomplete failures retain the bounded reason in both languages", () => {
  const error = JSON.stringify({ code: "ai.error.content_filter", kind: "policy", provider: "chatgpt_plan", details: { reason: "content_filter" }, message: "PRIVATE" });
  for (const lang of ["en", "ar"] as const) {
    const failure = providerFailure(error, lang);
    expect(failure?.text).toContain("content_filter");
    expect(failure?.text).not.toContain("PRIVATE");
    expect(failure?.details).toEqual({ reason: "content_filter" });
  }
  expect(providerFailure(JSON.stringify({ code: "ai.error.response_incomplete", details: { reason: "PRIVATE\nsecret" } }), "en")?.text).not.toContain("PRIVATE");
  expect(providerFailure("ai.error.stream_parse", "en")).toMatchObject({ kind: "other" });
});
