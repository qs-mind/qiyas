import { describe, expect, it } from "vitest";
import { slashCommand, slashMatches, slashWord, SLASH_COMMANDS } from "./slashCommands";

describe("slash commands", () => {
  it("offers every command for a bare slash, in the window's language", () => {
    expect(slashMatches("/", "en").map((c) => slashWord(c, "en"))).toEqual(["takeoff", "boq", "compare", "sheet", "check", "new"]);
    expect(slashMatches("/", "ar").map((c) => slashWord(c, "ar"))).toEqual(["حصر", "مقايسة", "مقارنة", "لوحة", "فحص", "جديد"]);
  });
  it("narrows as the word is typed", () => {
    expect(slashMatches("/ta", "en").map((c) => c.id)).toEqual(["takeoff"]);
    expect(slashMatches("/c", "en").map((c) => c.id)).toEqual(["compare", "check"]);
    expect(slashMatches("/م", "ar").map((c) => c.id)).toEqual(["boq", "compare"]);
    expect(slashMatches("/zzz", "en")).toEqual([]);
  });
  it("never opens for ordinary text", () => {
    expect(slashMatches("takeoff", "en")).toEqual([]);
    expect(slashMatches("/takeoff the villa", "en")).toEqual([]);
    expect(slashMatches("", "en")).toEqual([]);
  });
  it("runs a whole word of either language", () => {
    expect(slashCommand("/takeoff")?.id).toBe("takeoff");
    expect(slashCommand(" /حصر ")?.id).toBe("takeoff");
    expect(slashCommand("/مقارنة")?.id).toBe("compare");
    expect(slashCommand("/nothing")).toBeNull();
    expect(slashCommand("takeoff")).toBeNull();
  });
  it("gives every command a word in both languages", () => {
    for (const c of SLASH_COMMANDS) expect(c.en && c.ar).toBeTruthy();
    expect(new Set(SLASH_COMMANDS.map((c) => c.en)).size).toBe(SLASH_COMMANDS.length);
    expect(new Set(SLASH_COMMANDS.map((c) => c.ar)).size).toBe(SLASH_COMMANDS.length);
  });
});
