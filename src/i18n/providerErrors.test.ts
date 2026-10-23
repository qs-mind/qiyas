import { expect, test } from "vitest";
import { translate } from ".";
import { uiError } from "./errors";
import en from "./en.json";
import ar from "./ar.json";

test("every provider error has plain English and Arabic and raw service text never wins", () => {
  for (const kind of ["billing", "auth", "rate_limited", "model_not_found", "context_too_long", "output_limit", "timeout", "network", "stream", "service_error"]) {
    const code = `ai.error.${kind}`;
    expect(code in en && code in ar).toBe(true);
    for (const lang of ["en", "ar"] as const) {
      const raw = JSON.stringify({ code, message: "raw provider message with internal details" });
      expect(uiError(raw, lang)).toBe(translate(lang, code));
      expect(uiError(JSON.parse(raw), lang)).toBe(translate(lang, code));
      expect(uiError(raw, lang)).not.toContain("raw provider");
    }
  }
});
