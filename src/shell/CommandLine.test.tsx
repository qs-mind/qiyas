// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { mergeAliases } from "../tools/aliases";
import type { ParseState } from "../tools/commandParser";
import { CommandLine } from "./CommandLine";

afterEach(cleanup);

const state = (expect: ParseState["expect"] = "command"): ParseState => ({
  expect,
  last: [1, 1],
  aliases: mergeAliases(),
});

function setup(s = state(), prompt = "Command:") {
  const onAction = vi.fn();
  const view = render(<CommandLine state={s} prompt={prompt} onAction={onAction} />);
  const input = screen.getByRole("combobox", { name: prompt }) as HTMLInputElement;
  const type = (text: string) => fireEvent.change(input, { target: { value: text } });
  const key = (k: string) => fireEvent.keyDown(input, { key: k });
  const enter = (text: string) => {
    type(text);
    key("Enter");
  };
  const log = () => [...view.container.querySelectorAll(".command-log li")].map((l) => l.textContent);
  return { onAction, input, type, key, enter, log, view };
}

test("Enter runs the line, echoes it and an empty Enter repeats it", () => {
  const { onAction, input, enter, key, log } = setup();
  enter("di");
  expect(onAction).toHaveBeenLastCalledWith({ type: "tool", tool: "distance" });
  expect(input.value).toBe("");
  expect(log()).toEqual(["Command: di"]);
  key("Enter");
  expect(onAction).toHaveBeenCalledTimes(2);
  expect(onAction).toHaveBeenLastCalledWith({ type: "tool", tool: "distance" });
  expect(log()).toEqual(["Command: di", "Command: di"]);
});

test("errors show inline, in the language of the window, and run nothing", () => {
  const { onAction, enter, log } = setup();
  enter("XYZ");
  expect(onAction).not.toHaveBeenCalled();
  expect(log()).toEqual(["Command: XYZ", "Unknown command: XYZ"]);
});

test("↑ and ↓ go through the lines typed before", () => {
  const { input, enter, key } = setup();
  enter("LAYOFF A");
  enter("Z E");
  key("ArrowUp");
  expect(input.value).toBe("Z E");
  key("ArrowUp");
  expect(input.value).toBe("LAYOFF A");
  key("ArrowUp");
  expect(input.value).toBe("LAYOFF A");
  key("ArrowDown");
  expect(input.value).toBe("Z E");
  key("ArrowDown");
  expect(input.value).toBe("");
});

test("typing lists the matching commands; ↓ and Enter run one, Tab fills it in", () => {
  const { onAction, input, type, key } = setup();
  type("lay");
  const names = () => screen.getAllByRole("option").map((o) => o.firstChild?.textContent);
  expect(names()).toEqual(["LAYALL", "LAYER", "LAYISO", "LAYOFF", "LAYON", "LAYSTATE", "LAYTRANS", "LAYUNISO"]);
  expect(input.getAttribute("aria-expanded")).toBe("true");
  key("ArrowDown");
  key("ArrowDown");
  key("ArrowDown");
  expect(screen.getByRole("option", { selected: true }).textContent).toBe("LAYISO");
  key("Tab");
  expect(input.value).toBe("LAYISO ");
  expect(screen.queryByRole("listbox")).toBeNull();

  type("LAYA");
  key("ArrowDown");
  key("Enter");
  expect(onAction).toHaveBeenLastCalledWith({ type: "showAll" });

  // Escape closes the list first, then cancels.
  type("Z");
  key("Escape");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(onAction).toHaveBeenCalledTimes(1);
  key("Escape");
  expect(onAction).toHaveBeenLastCalledWith({ type: "cancel" });
});

test("at a point prompt, a typed point is an answer, not a command to repeat", () => {
  const { onAction, enter, key, log } = setup(state("point"), "Specify next point:");
  enter("@2,0");
  expect(onAction).toHaveBeenLastCalledWith({ type: "point", p: [3, 1] });
  key("Enter");
  expect(onAction).toHaveBeenLastCalledWith({ type: "finish" });
  expect(log()).toEqual(["Specify next point: @2,0", "Specify next point:"]);
});

test("Enter never repeats COUNT <block> or AI <text>; tools and UNDO still repeat", () => {
  const { onAction, enter, key } = setup();
  enter("COUNT DOOR");
  key("Enter");
  enter("AI how long is the wall?");
  key("Enter");
  expect(onAction.mock.calls.map(([a]) => a.type)).toEqual(["count", "ai"]);

  enter("U");
  key("Enter");
  enter("AA");
  key("Enter");
  expect(onAction.mock.calls.slice(2).map(([a]) => a)).toEqual([
    { type: "run", what: "undo" },
    { type: "run", what: "undo" },
    { type: "tool", tool: "area" },
    { type: "tool", tool: "area" },
  ]);
});

test("history keeps the last 50 lines", () => {
  const { input, enter, key } = setup();
  for (let i = 1; i <= 55; i++) enter(`LAYOFF L${i}`);
  key("ArrowUp");
  expect(input.value).toBe("LAYOFF L55");
  for (let i = 0; i < 60; i++) key("ArrowUp");
  expect(input.value).toBe("LAYOFF L6");
});

const options = () => screen.getAllByRole("option");
const chipOf = (name: string) =>
  options()
    .find((o) => o.querySelector("span")!.textContent === name)
    ?.querySelector("kbd")?.textContent;

test("each command shows its shortcut as a small key", () => {
  const { type } = setup();
  type("dis");
  expect(chipOf("DIST")).toBe("DI");
  type("tex");
  expect(chipOf("TEXT")).toBe("DT");
  // One with no shortcut has no key.
  type("layo");
  expect(chipOf("LAYOFF")).toBeUndefined();
});

test("a shortcut whose command does not start with what was typed is a row of its own; headings show only when there are both kinds", () => {
  const { view, type } = setup();
  const headings = () => [...view.container.querySelectorAll("[cmdk-group-heading]")].map((h) => h.textContent);
  type("t");
  // MTEXT is here only because T is typed for it.
  expect(headings()).toEqual(["Commands", "Shortcuts"]);
  const mtext = options().find((o) => o.querySelector("span")!.textContent === "MTEXT")!;
  expect(mtext.querySelector("kbd")!.textContent).toBe("T");
  expect(options().filter((o) => o.querySelector("span")!.textContent === "TEXT")).toHaveLength(1);
  // Only commands: no headings.
  type("lay");
  expect(headings()).toEqual([]);
});

test("nothing is chosen until the arrows choose it, so Enter runs what was typed, not the first row", () => {
  const { onAction, type, key, log } = setup();
  type("layo");
  expect(screen.queryByRole("option", { selected: true })).toBeNull();
  key("Enter");
  expect(onAction).not.toHaveBeenCalled();
  expect(log()).toEqual(["Command: layo", "Unknown command: layo"]);
});

test("↑ from nothing goes to the last row, ↓ past the last goes round to the first; the input points at the chosen row", () => {
  const { input, type, key } = setup();
  type("lay");
  const names = () => options().map((o) => o.querySelector("span")!.textContent);
  expect(input.getAttribute("aria-activedescendant")).toBeNull();
  key("ArrowUp");
  expect(screen.getByRole("option", { selected: true }).querySelector("span")!.textContent).toBe(names().at(-1));
  expect(input.getAttribute("aria-activedescendant")).toBe(screen.getByRole("option", { selected: true }).id);
  expect(input.getAttribute("aria-controls")).toBe(screen.getByRole("listbox").id);
  key("ArrowDown");
  expect(screen.getByRole("option", { selected: true }).querySelector("span")!.textContent).toBe(names()[0]);
});

test("a click on a row runs it, and the pointer resting on one does not choose it", () => {
  const { onAction, input, type } = setup();
  type("area");
  const row = options()[0];
  fireEvent.pointerMove(row);
  expect(screen.queryByRole("option", { selected: true })).toBeNull();
  // The mouse is pressed on the list without taking the focus off the input.
  const press = fireEvent.mouseDown(row);
  expect(press).toBe(false);
  fireEvent.click(row);
  expect(onAction).toHaveBeenLastCalledWith({ type: "tool", tool: "area" });
  expect(input.value).toBe("");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("the list closes when the input loses the focus, and while a point is asked for it is not offered", () => {
  const { input, type } = setup();
  type("lay");
  expect(screen.getByRole("listbox")).toBeTruthy();
  fireEvent.blur(input);
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(input.getAttribute("aria-expanded")).toBe("false");
  cleanup();
  const point = setup(state("point"), "Specify next point:");
  point.type("lay");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("Home and End are the caret's: they are not taken (prevented) by the list", () => {
  const { input, type } = setup();
  type("lay");
  for (const key of ["Home", "End"]) {
    // Not prevented, so the caret moves; and it does not reach cmdk's root, which would prevent it.
    expect(fireEvent.keyDown(input, { key })).toBe(true);
  }
});
