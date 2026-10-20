// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { CmdlineStep } from "../api";
import { useCommandLine } from "./useCommandLine";

const api = vi.hoisted(() => ({ cmdlineStep: vi.fn() }));
vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

const prompt: CmdlineStep = { type: "prompt", prompt: "Name:", last: null };
const cancelled: CmdlineStep = { type: "cancelled" };
const complete: CmdlineStep = {
  type: "command",
  command: { type: "draw", kind: "line", a: [0, 0], b: [10, 0] },
  last: [10, 0],
};

function setup() {
  const switchGen = { current: 0 };
  const apply = vi.fn().mockResolvedValue(undefined);
  const cancel = vi.fn();
  const lastPoint = { current: [1, 1] };
  // Only the state read by the hook and the handlers these tests exercise are needed here.
  const deps = {
    t: (key: string) => key,
    switchGen,
    info: { path: "/drawing.dwg", sheet: "model", problems: [], layers: [], units: "millimetres" },
    measure: { points: [], lastDrawPoint: [7, 8], selection: [], skipped: 0, hint: "", cancel },
    assistant: {},
    shell: { viewport: { current: null }, commandAliases: {}, setNotice: vi.fn(), setError: vi.fn() },
    drafting: {},
    toolChoice: { tool: "select", lastPoint, preset: { current: null } },
    commands: { apply },
    sheets: { batch: null, renderProgress: null, cancelReveal: vi.fn() },
    layers: {},
    attach: { pending: null, setPending: vi.fn() },
    blocks: {},
    annotate: {},
    drawings: { scriptLine: null, switching: { current: 0 } },
    takeoffState: {},
    takeoff: {},
    plot: { cancelPick: vi.fn() },
    options: { shortcuts: {} },
    parametric: {},
  } as unknown as Parameters<typeof useCommandLine>[0];
  const view = renderHook(() => useCommandLine(deps));
  return { ...view, apply, cancel, switchGen, lastPoint };
}

test("a ribbon command cancels an outstanding line before starting its own prompts", async () => {
  let release!: (step: CmdlineStep) => void;
  api.cmdlineStep
    .mockImplementationOnce(() => new Promise<CmdlineStep>((resolve) => { release = resolve; }))
    .mockResolvedValueOnce(cancelled)
    .mockResolvedValueOnce(prompt);
  const { result, apply, cancel } = setup();
  let first!: Promise<CmdlineStep>;
  act(() => { first = result.current.typeCommand("NEWLAYER"); });
  await waitFor(() => expect(api.cmdlineStep).toHaveBeenCalledTimes(1));
  let next!: Promise<void>;
  act(() => { next = result.current.startCommand("CIRCLE"); });
  await act(async () => {
    release(complete);
    await first;
    await next;
  });
  expect(api.cmdlineStep.mock.calls.map(([line]) => line)).toEqual(["NEWLAYER", "ESC", "CIRCLE"]);
  expect(apply).not.toHaveBeenCalled();
  expect(cancel).toHaveBeenCalledTimes(2);
  expect(result.current.coreActive).toBe(true);
});

test("switching drawings while the machine replies never applies its command to the new drawing", async () => {
  let release!: (step: CmdlineStep) => void;
  api.cmdlineStep.mockImplementationOnce(() => new Promise<CmdlineStep>((resolve) => { release = resolve; }));
  const { result, apply, switchGen, lastPoint } = setup();
  let pending!: Promise<CmdlineStep>;
  act(() => { pending = result.current.typeCommand("LINE 0,0 10,0"); });
  await waitFor(() => expect(api.cmdlineStep).toHaveBeenCalled());
  switchGen.current++;
  await act(async () => {
    release(complete);
    expect(await pending).toEqual(cancelled);
  });
  expect(apply).not.toHaveBeenCalled();
  expect(lastPoint.current).toEqual([1, 1]);
});

test("starting a registry command uses the parser's current last point and refreshes any completed command", async () => {
  api.cmdlineStep.mockResolvedValueOnce(complete);
  const { result, apply, cancel, lastPoint } = setup();
  await act(async () => { await result.current.typeCommand("LINE @1,0 @0,1"); });
  expect(api.cmdlineStep).toHaveBeenCalledWith("LINE @1,0 @0,1", [7, 8]);
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(apply).toHaveBeenCalledWith(complete.command, true);
  expect(lastPoint.current).toEqual([10, 0]);
});

test("an initial error leaves no active command, while a bad answer keeps the existing prompt", async () => {
  api.cmdlineStep
    .mockRejectedValueOnce("Unknown command")
    .mockResolvedValueOnce(prompt)
    .mockRejectedValueOnce("Enter a name")
    .mockResolvedValueOnce(complete);
  const { result, cancel } = setup();
  await act(async () => { await expect(result.current.typeCommand("UNKNOWN")).rejects.toBe("Unknown command"); });
  await act(async () => { await result.current.typeCommand("NEWLAYER"); });
  expect(cancel).toHaveBeenCalledTimes(2);
  await act(async () => { await expect(result.current.typeCommand("bad")).rejects.toBe("Enter a name"); });
  expect(result.current.coreActive).toBe(true);
  await act(async () => { await result.current.typeCommand("Doors"); });
  expect(result.current.coreActive).toBe(false);
  expect(cancel).toHaveBeenCalledTimes(2);
});
