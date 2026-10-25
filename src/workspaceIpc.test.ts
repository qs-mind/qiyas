import { afterEach, expect, test, vi } from "vitest";
import { invoke, setWorkspace } from "./workspaceIpc";
const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);
afterEach(() => { vi.useRealTimers(); setWorkspace(null); vi.resetAllMocks(); });

test("a sixty second drawing operation leaves the other drawing's requests interactive", async () => {
  vi.useFakeTimers();
  core.invoke.mockImplementation(async (command, args) => {
    if (command === "run_command" && args.workspace === 1) {
      await new Promise(resolve => setTimeout(resolve, 60_000));
      return { workspace: 1, completed: true };
    }
    return { workspace: args.workspace, completed: true };
  });
  setWorkspace(1);
  let done = false;
  const slow = invoke("run_command", { cmd: { type: "batch", commands: [] } }).then(value => { done = true; return value; });
  for (let second = 0; second < 60; second++) {
    setWorkspace(2);
    expect(await invoke("drawing_info")).toEqual({ workspace: 2, completed: true });
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(1000);
  }
  expect(await slow).toEqual({ workspace: 1, completed: true });
  expect(core.invoke.mock.calls[0][1].workspace).toBe(1);
});

test("busy picks queue on their original drawing and committed reads remain available", async () => {
  vi.useFakeTimers();
  setWorkspace(1);
  core.invoke.mockResolvedValueOnce({ path: "first.dxf" });
  expect(await invoke("drawing_info")).toEqual({ path: "first.dxf" });
  core.invoke.mockRejectedValueOnce("workspace_busy");
  expect(await invoke("drawing_info")).toEqual({ path: "first.dxf" });
  core.invoke.mockRejectedValueOnce("workspace_busy").mockResolvedValueOnce({ handle: 42 });
  const pick = invoke("pick", { x: 1, y: 2, tol: 0.1 });
  await vi.advanceTimersByTimeAsync(50);
  expect(await pick).toEqual({ handle: 42 });
  expect(core.invoke.mock.calls.at(-1)?.[1].workspace).toBe(1);
});
