import { expect, test } from "vitest";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";
import { COMMANDS, DEFAULT_ALIASES } from "../tools/aliases";
import { parse } from "../tools/commandParser";
import { aliasesFor, COMMAND_LINE, HEADLINE, isPrimaryGroup } from "./ribbonInfo";
import { DEFAULT_RIBBON, QUICK_ACCESS } from "./ribbonLayout";

const buttons = [...QUICK_ACCESS, ...DEFAULT_RIBBON.tabs.flatMap((t) => t.groups.flatMap((g) => g.buttons))];
const commands = [...new Set(buttons.map((b) => b.command))];
const strings = { en, ar } as Record<string, Record<string, string>>;

test("every command's line is a real command of the command line, and a tool's reaches that tool", () => {
  for (const [command, [name, option]] of Object.entries(COMMAND_LINE)) {
    expect(COMMANDS as readonly string[], command).toContain(name);
    const kind = buttons.find((b) => b.command === command)?.kind;
    if (kind === "tool") {
      const typed = [name, option].filter(Boolean).join(" ");
      const result = parse(typed, { expect: "command", last: null, aliases: DEFAULT_ALIASES });
      expect(result, typed).toEqual(expect.objectContaining({ type: "tool", tool: command }));
    }
  }
  // Only ribbon commands.
  for (const c of Object.keys(COMMAND_LINE)) expect(commands, c).toContain(c);
});

test("aliases come from the alias table: the shortest two, with the option; none when there are none", () => {
  expect(aliasesFor("line")).toEqual(["L"]);
  expect(aliasesFor("trim")).toEqual(["TR"]);
  expect(aliasesFor("area")).toEqual(["AA"]);
  expect(aliasesFor("distance")).toEqual(["DI", "DISTANCE"]);
  expect(aliasesFor("copy")).toEqual(["CO", "CP"]);
  expect(aliasesFor("gcParallel")).toEqual(["GC PAR"]);
  expect(aliasesFor("zoomExtents")).toEqual(["Z E"]);
  expect(aliasesFor("select")).toEqual([]);
  expect(aliasesFor("exportConflicts")).toEqual([]);
  expect(aliasesFor("count")).toEqual([]);
  // The user's own alias is found too; one with arguments is not an alias of the command alone.
  expect(aliasesFor("line", { ...DEFAULT_ALIASES, LN: "LINE", LX: "LINE 0,0" })).toEqual(["L", "LN"]);
});

test("every alias shown is in the alias table", () => {
  for (const c of commands) for (const a of aliasesFor(c)) expect(Object.keys(DEFAULT_ALIASES), c).toContain(a.split(" ")[0]);
});

test("every command has a description in English and Arabic, and the new ribbon words exist in both", () => {
  for (const lang of ["en", "ar"]) {
    for (const c of [...commands, "commentCloud"]) {
      expect(strings[lang][`tip.${c}`], `${lang} tip.${c}`).toBeTruthy();
    }
    for (const k of ["ribbon.more", "ribbon.alias", "ribbon.markup", "ribbon.edit", "ribbon.groupMore"]) expect(strings[lang][k], k).toBeTruthy();
  }
  // The revision cloud with a comment (its button comes with the other branch).
  expect(en["tip.commentCloud"]).toBe("Click round what you want to mark, Enter, then type the comment. Drawn on the COMMENTS layer.");
  expect(ar["tip.commentCloud"]).toBe("انقر حول موضع الملاحظة، ثم اضغط Enter واكتب التعليق. ينشئ قياس السحابة على طبقة COMMENTS.");
  // The same keys in both languages, and the same placeholders.
  expect(Object.keys(ar).sort()).toEqual(Object.keys(en).sort());
  expect(ar["ribbon.groupMore"]).toContain("{name}");
});

test("the headline tools are the Measure and Take-off groups, and only whole groups of them are drawn large", () => {
  const primary = DEFAULT_RIBBON.tabs.flatMap((t) => t.groups.filter(isPrimaryGroup).map((g) => `${t.id}/${g.id}`));
  expect(primary).toEqual(["measure/measure", "measure/takeoff"]);
  expect(HEADLINE.size).toBe(8);
});
