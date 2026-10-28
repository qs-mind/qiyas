import { expect, test } from "vitest";
import { providerFailure } from "./providerFailure";

test.each([
  "chatgpt_failed", "chatgpt_incomplete", "chatgpt_temporary", "chatgpt_limit", "chatgpt_empty",
  "service_error", "network", "timeout", "stream", "rate_limited", "claude_limit", "claude_failed", "claude_stream",
])("%s keeps work and offers Continue in both languages and error formats", (code) => {
  for (const lang of ["en", "ar"] as const) {
    for (const error of [code, `ai.error.${code}`, JSON.stringify({ code: `ai.error.${code}` })]) {
      const failure = providerFailure(error, lang);
      expect(failure?.canContinue).toBe(true);
      expect(failure?.text).toContain(lang === "en" ? "drawing work so far is kept" : "ما أُنجز في الرسم محفوظ");
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
    expect(providerFailure(JSON.stringify({ code: "chatgpt_limit", reset_at }), "en")?.text).toContain("after the limit resets");
  }
});

test("ordinary stop reasons keep their existing handling; unknown provider codes are never shown", () => {
  expect(providerFailure("cancelled", "en")).toBeNull();
  expect(providerFailure("stopped after 12 steps", "en")).toBeNull();
  expect(providerFailure("ai.error.future_provider_failure", "en")?.text).toContain("service stopped answering");
});
