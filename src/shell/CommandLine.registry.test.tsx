// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { CmdlineStep, CommandWord } from "../api";
import { mergeAliases } from "../tools/aliases";
import type { ParseState } from "../tools/commandParser";
import { CommandLine } from "./CommandLine";

afterEach(cleanup);

const words: CommandWord[] = [
  { word: "WIDGET", command: "widget", summary: "Add a widget." },
  { word: "WG", command: "widget", summary: "Add a widget." },
];
const state: ParseState = { expect: "command", last: null, aliases: mergeAliases() };
const completed: CmdlineStep = { type: "command", command: { type: "count_block", block_name: "Widget", layer: null }, last: null };

function enter(line: string) {
  const input = screen.getByRole("combobox");
  fireEvent.change(input, { target: { value: line } });
  fireEvent.keyDown(input, { key: "Enter" });
}

test("a registry addition appears with its alias and summary, then reaches the core unchanged", async () => {
  const onAction = vi.fn();
  const onTyped = vi.fn().mockResolvedValue(completed);
  render(<CommandLine state={state} prompt="Command:" words={words} onAction={onAction} onTyped={onTyped} />);
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "wid" } });
  const row = screen.getByRole("option");
  expect(row.textContent).toBe("WIDGETWG");
  expect(row.getAttribute("title")).toBe("Add a widget.");
  enter("wg 12");
  await waitFor(() => expect(onTyped).toHaveBeenCalledWith("WG 12"));
  expect(onAction).not.toHaveBeenCalled();
});

test("a user alias expands before the core, and known window commands keep their existing behavior", async () => {
  const onAction = vi.fn();
  const onTyped = vi.fn().mockResolvedValue(completed);
  render(<CommandLine state={{ ...state, aliases: mergeAliases({ CUSTOM: "WG 3" }) }} prompt="Command:"
    words={words} onAction={onAction} onTyped={onTyped} />);
  enter("LINE");
  expect(onAction).toHaveBeenLastCalledWith({ type: "tool", tool: "line" });
  expect(onTyped).not.toHaveBeenCalled();
  enter("CUSTOM");
  await waitFor(() => expect(onTyped).toHaveBeenCalledWith("WG 3"));
});

test("answers go to the core until completion, and blank Enter repeats the whole command", async () => {
  const lines: string[] = [];
  function Harness() {
    const [active, setActive] = useState(false);
    const onTyped = async (line: string): Promise<CmdlineStep> => {
      lines.push(line);
      const asking = lines.length === 1;
      setActive(asking);
      return asking ? { type: "prompt", prompt: "Name:", last: null } : completed;
    };
    return <CommandLine state={state} prompt={active ? "Name:" : "Command:"} words={words}
      coreActive={active} onAction={() => {}} onTyped={onTyped} />;
  }
  render(<Harness />);
  enter("WIDGET");
  await screen.findByRole("combobox", { name: "Name:" });
  enter("Door");
  await screen.findByRole("combobox", { name: "Command:" });
  await waitFor(() => expect(lines).toEqual(["WIDGET", "Door"]));
  enter("");
  await waitFor(() => expect(lines).toEqual(["WIDGET", "Door", "WIDGET Door"]));
});

test("a failed answer keeps the prompt usable and is not included in the repeated command", async () => {
  const lines: string[] = [];
  function Harness() {
    const [active, setActive] = useState(false);
    const onTyped = async (line: string): Promise<CmdlineStep> => {
      lines.push(line);
      if (line === "bad") throw new Error("Enter a number");
      const asking = lines.length === 1;
      setActive(asking);
      return asking ? { type: "prompt", prompt: "Number:", last: null } : completed;
    };
    return <CommandLine state={state} prompt={active ? "Number:" : "Command:"} words={words}
      coreActive={active} onAction={() => {}} onTyped={onTyped} />;
  }
  render(<Harness />);
  enter("WIDGET");
  await screen.findByRole("combobox", { name: "Number:" });
  enter("bad");
  await screen.findByText("Error: Enter a number");
  enter("2");
  await screen.findByRole("combobox", { name: "Command:" });
  enter("");
  await waitFor(() => expect(lines).toEqual(["WIDGET", "bad", "2", "WIDGET 2"]));
});

test("cancelling an in-flight command before a ribbon prompt never repeats that prompt's answer alone", async () => {
  const lines: string[] = [];
  let release!: (step: CmdlineStep) => void;
  function Harness() {
    const [active, setActive] = useState(false);
    const onTyped = async (line: string): Promise<CmdlineStep> => {
      lines.push(line);
      if (lines.length === 1) return new Promise((resolve) => { release = resolve; });
      setActive(false);
      return completed;
    };
    return <>
      <button onClick={() => { setActive(true); release({ type: "cancelled" }); }}>Start from ribbon</button>
      <CommandLine state={state} prompt={active ? "Name:" : "Command:"} words={words}
        coreActive={active} onAction={() => {}} onTyped={onTyped} />
    </>;
  }
  render(<Harness />);
  enter("WIDGET");
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start from ribbon" })); });
  enter("Door");
  await screen.findByRole("combobox", { name: "Command:" });
  enter("");
  expect(lines).toEqual(["WIDGET", "Door"]);
});

test("unknown registry fallback keeps the window's translated error and one echo", async () => {
  const onTyped = vi.fn().mockRejectedValue("unknown command XYZ");
  const view = render(<CommandLine state={state} prompt="Command:" words={words} onAction={() => {}} onTyped={onTyped} />);
  enter("XYZ");
  await screen.findByText("Unknown command: XYZ");
  expect([...view.container.querySelectorAll(".command-log li")].map((line) => line.textContent))
    .toEqual(["Command: XYZ", "Unknown command: XYZ"]);
});
