import type { Lang } from "./index";

/** Window formatting keeps Western digits in both languages. */
export function locale(lang: Lang): string {
  return lang === "ar" ? "ar-EG-u-nu-latn" : "en";
}
