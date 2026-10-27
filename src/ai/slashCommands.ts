/** Slash commands in Mizan's box (W4): type "/" to see them. They are words of the window's
 *  language (English commands in English, Arabic in Arabic); either language's word runs when it
 *  is typed whole. A command is a prompt sent to Mizan, or an action of the window. */
import type { Lang } from "../i18n";

export type SlashId = "takeoff" | "boq" | "compare" | "sheet" | "check" | "connect" | "new";

export type SlashCommand = {
  id: SlashId;
  en: string;
  ar: string;
  /** `prompt`: sent as the user's message (its text is `ai.slash.<id>.prompt`); `app`: the window acts. */
  kind: "prompt" | "app";
};

export const SLASH_COMMANDS: SlashCommand[] = [
  { id: "takeoff", en: "takeoff", ar: "حصر", kind: "prompt" },
  { id: "boq", en: "boq", ar: "مقايسة", kind: "prompt" },
  { id: "compare", en: "compare", ar: "مقارنة", kind: "app" },
  { id: "sheet", en: "sheet", ar: "لوحة", kind: "prompt" },
  { id: "check", en: "check", ar: "فحص", kind: "prompt" },
  { id: "connect", en: "connect", ar: "ربط", kind: "app" },
  { id: "new", en: "new", ar: "جديد", kind: "app" },
];

/** The word the window's language uses for a command. */
export const slashWord = (c: SlashCommand, lang: Lang) => (lang === "ar" ? c.ar : c.en);

/** The commands to offer while `draft` is a slash and the start of a word: all of them for a bare
 *  "/", else those whose word (in the window's language) starts with what was typed. Empty once the
 *  line is more than one word, so ordinary text with a slash in it never opens the list. */
export function slashMatches(draft: string, lang: Lang): SlashCommand[] {
  if (!draft.startsWith("/") || /\s/.test(draft)) return [];
  const typed = draft.slice(1).toLowerCase();
  return SLASH_COMMANDS.filter((c) => slashWord(c, lang).toLowerCase().startsWith(typed));
}

/** The command a whole line names, in either language ("/takeoff", "/حصر"), or null. */
export function slashCommand(line: string): SlashCommand | null {
  const text = line.trim();
  if (!text.startsWith("/") || /\s/.test(text)) return null;
  const word = text.slice(1).toLowerCase();
  return SLASH_COMMANDS.find((c) => c.en === word || c.ar === word) ?? null;
}
