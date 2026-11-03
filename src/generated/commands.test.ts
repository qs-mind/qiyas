import { expect, expectTypeOf, test } from "vitest";
import { COMMAND_WORDS, type Command, type CommandMap } from "./commands";
import { registryAliases, registryNames } from "../tools/commandWords";

test("wire inputs allow core defaults and retain discriminated payloads", () => {
  const inputs: Command[] = [
    { type: "draw", kind: "polyline", points: [[0, 0], [12, 0], [12, 8]] },
    { type: "text", at: [0, 0], value: "Terminal concourse" },
    { type: "dimension", kind: "linear", p1: [0, 0], p2: [12, 0], line_point: [0, 2], rotation: null },
    { type: "delete_entities", handles: [{ handle: 42 }] },
    { type: "set_layer", name: "A-WALL", color: 7 },
    { type: "recognition", kind: "list" },
    { type: "id_point", p: [0, 0], kind: null },
    { type: "batch", commands: [{ type: "draw", kind: "line", a: [0, 0], b: [12, 0] }] },
  ];
  const line: CommandMap["draw.line"] = { type: "draw", kind: "line", a: [0, 0], b: [12, 0] };
  inputs.push(line);
  for (const input of inputs) {
    if (input.type === "draw" && input.kind === "line") {
      expectTypeOf(input.a).toEqualTypeOf<[number, number]>();
      expect(input.b).toEqual([12, 0]);
    }
  }
  // @ts-expect-error A line needs its second endpoint.
  const missingEndpoint: Command = { type: "draw", kind: "line", a: [0, 0] };
  // @ts-expect-error A point is a pair of coordinates.
  const wrongPoint: Command = { type: "draw", kind: "point", p: [0, 0, 0] };
  // @ts-expect-error Unknown commands are not wire inputs.
  const unknownCommand: Command = { type: "not_a_command" };
  void [missingEndpoint, wrongPoint, unknownCommand];
});

test("generated words keep canonical names ahead of their aliases", () => {
  const words = [...COMMAND_WORDS];
  expect(registryNames(words)).toContain("LINE");
  expect(registryNames(words)).not.toContain("L");
  expect(registryAliases(words).L).toBe("LINE");
  expect(registryAliases(words).CP).toBe("COPY");
  expect(new Set(words.map((entry) => entry.word)).size).toBe(words.length);
});
