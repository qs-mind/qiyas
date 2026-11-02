// @vitest-environment node
import { expect, test } from "vitest";
import { locale } from "./locale";

test("window locales keep Western digits and the existing English locale", () => {
  expect(locale("en")).toBe("en");
  expect(locale("ar")).toBe("ar-EG-u-nu-latn");
  const number = new Intl.NumberFormat(locale("ar")).format(1234.5);
  expect(number).not.toMatch(/[\u0660-\u0669\u06f0-\u06f9]/);
  expect(number).toBe("1,234.5");
});
