/** Registry command words supplement the window's click-driven tools and dialog commands. */
import type { CommandWord } from "../api";
import { COMMANDS } from "./aliases";
import type { Suggestion } from "./commandParser";

export function registryAliases(words: CommandWord[]): Record<string, string> {
  const first = new Map<string, string>();
  const aliases: Record<string, string> = {};
  for (const item of words) {
    const canonical = first.get(item.command);
    if (!canonical) first.set(item.command, item.word);
    else if (!(COMMANDS as readonly string[]).includes(item.word)) aliases[item.word] = canonical;
  }
  return aliases;
}

export function registryNames(words: CommandWord[]): string[] {
  const first = new Map<string, string>();
  for (const item of words) if (!first.has(item.command)) first.set(item.command, item.word);
  return [...first.values()];
}

/** The existing names keep their window behavior; registry additions need no window release. */
export function suggestWords(prefix: string, aliases: Record<string, string>, words: CommandWord[]): Suggestion[] {
  const p = prefix.trim().toUpperCase();
  if (!p || /\s/.test(prefix.trimStart())) return [];
  const names = new Set<string>([...COMMANDS, ...registryNames(words)]);
  const found: Suggestion[] = [
    ...[...names].filter((n) => n.startsWith(p)).sort().map((name) => ({ name })),
    ...Object.keys(aliases)
      .filter((n) => n.startsWith(p) && !names.has(n))
      .sort()
      .map((name) => ({ name, target: aliases[name] })),
  ];
  const exact = found.findIndex((item) => item.name === p);
  if (exact > 0) found.unshift(...found.splice(exact, 1));
  return found;
}
