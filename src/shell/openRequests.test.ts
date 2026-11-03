// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useOpenRequests } from "./openRequests";

const core = vi.hoisted(() => ({
  waiting: [] as string[],
  listeners: new Map<string, () => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd !== "take_open_requests") throw new Error(cmd);
    const taken = core.waiting;
    core.waiting = [];
    return taken;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: () => void) => {
    core.listeners.set(name, handler);
    return () => core.listeners.delete(name);
  }),
}));

afterEach(() => {
  cleanup();
  core.waiting = [];
  core.listeners.clear();
});

/** Lets the hook's promises run. */
const settle = () => act(async () => {});

test("files given at launch open in turn, then files sent later", async () => {
  core.waiting = ["/d/plan.dwg", "/d/site.dxf"];
  const opened: string[] = [];
  let finish: () => void = () => {};
  const openPath = vi.fn(async (path: string) => {
    opened.push(`start ${path}`);
    if (path === "/d/plan.dwg") await new Promise<void>((done) => (finish = done));
    opened.push(`end ${path}`);
  });
  renderHook(() => useOpenRequests(openPath));
  await settle();
  // The second waits until `openPath` of the first returns (the real one returns once the drawing
  // shows, or as soon as it queues its autosave question: then the second may show first).
  expect(opened).toEqual(["start /d/plan.dwg"]);
  await act(async () => finish());
  expect(opened).toEqual(["start /d/plan.dwg", "end /d/plan.dwg", "start /d/site.dxf", "end /d/site.dxf"]);

  core.waiting = ["/d/terminal.dwg"];
  await act(async () => core.listeners.get("open:requested")!());
  expect(openPath).toHaveBeenLastCalledWith("/d/terminal.dwg");
  expect(openPath).toHaveBeenCalledTimes(3);
});

test("nothing waiting opens nothing; a closed window stops listening", async () => {
  const openPath = vi.fn(async () => {});
  const { unmount } = renderHook(() => useOpenRequests(openPath));
  await settle();
  expect(openPath).not.toHaveBeenCalled();
  expect(core.listeners.has("open:requested")).toBe(true);
  unmount();
  expect(core.listeners.has("open:requested")).toBe(false);
});
