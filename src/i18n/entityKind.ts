import en from "./en.json";
import { translate } from ".";

const kinds = new Map(Object.keys(en).filter((key) => key.startsWith("entityKind.")).map((key) => [key.slice(11).toLowerCase(), key]));

/** Wire kind names never reach a user-facing label, including future unknown kinds. */
export function entityKind(kind: string, t: (key: string) => string = (key) => translate(typeof document !== "undefined" && document.documentElement.lang === "ar" ? "ar" : "en", key)): string {
  const key = kinds.get(kind.toLowerCase()) ?? "entityKind.unknown";
  const label = t(key);
  return label === key ? t("entityKind.unknown") : label;
}
