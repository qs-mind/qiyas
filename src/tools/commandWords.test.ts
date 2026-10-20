import { expect, test } from "vitest";
import type { CommandWord } from "../api";
import { aliasProblem } from "../shell/AliasEditor";
import { mergeAliases } from "./aliases";
import { registryAliases, registryNames, suggestWords } from "./commandWords";

const words: CommandWord[] = [
  { word: "WIDGET", command: "widget", summary: "A new core command." },
  { word: "WG", command: "widget", summary: "A new core command." },
  { word: "LAYOFF", command: "layer_hidden", summary: "Hide or show a layer." },
  { word: "LAYON", command: "layer_hidden", summary: "Hide or show a layer." },
];

test("registry aliases are suggestions and never rename a known window command", () => {
  const aliases = { ...registryAliases(words), ...mergeAliases({ WG: "WIDGET 2" }) };
  expect(registryAliases(words)).toEqual({ WG: "WIDGET" });
  expect(suggestWords("layo", aliases, words)).toEqual([{ name: "LAYOFF" }, { name: "LAYON" }]);
  expect(suggestWords("wg", aliases, words)).toEqual([{ name: "WG", target: "WIDGET 2" }]);
  expect(suggestWords("wid", aliases, words)).toEqual([{ name: "WIDGET" }]);
});

test("new registry names are valid alias targets while aliases can still be overridden", () => {
  const names = registryNames(words);
  expect(aliasProblem([{ alias: "DI", command: "WIDGET 2" }], names)).toBeNull();
  expect(aliasProblem([{ alias: "WIDGET", command: "LINE" }], names)?.key).toBe("aliasEdit.builtIn");
});
