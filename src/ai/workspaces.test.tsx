// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { WorkspaceEvent, WorkspaceSnapshot } from "../api";
import { setWorkspace } from "../workspaceIpc";
import { useAssistant } from "./useAssistant";
const core = vi.hoisted(() => ({
  snapshots: new Map<number, unknown>(),
  listeners: new Set<(event: { payload: unknown }) => void>(),
  chats: [] as { id: string; drawing: string; updated: number }[],
  paths: new Map<number, string>(),
  calls: [] as { command: string; args?: Record<string, unknown> }[],
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
  core.calls.push({ command, args });
  if (command === "workspace_attach") return core.snapshots.get(Number(args?.workspace ?? 0));
  if (command === "drawing_info") return { path: core.paths.get(Number(args?.workspace)) };
  if (command === "ai_conversations") return core.chats;
  if (command === "ai_resume") {
    const transcript = [{ type: "user", text: String(args?.id), pointed: [], images: [] }];
    const workspace = Number(args?.workspace);
    const previous = core.snapshots.get(workspace) as WorkspaceSnapshot;
    if (previous) core.snapshots.set(workspace, { ...previous, transcript, status: "done" });
    return { transcript, drawing: core.paths.get(workspace) ?? "second.dxf" };
  }
  if (command === "ai_status") return { busy: true, provider: "mock", model: "mock", has_key: true, message_count: 1 };
  return null;
}) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, callback: (e: { payload: unknown }) => void) => {
  if (name === "workspace:event") core.listeners.add(callback);
  return () => { core.listeners.delete(callback); };
}) }));
const callbacks = { onMutated: vi.fn(), onUi: vi.fn(), onNeedSettings: vi.fn() };
function snapshot(workspace: number, text: string, status = "working"): WorkspaceSnapshot {
  return { workspace, cursor: 2, status, error: null, away: null, memory_limited: false,
    choice: { provider: workspace === 1 ? "anthropic" : "openai_compatible", model: `model-${workspace}`, effort: "high" },
    transcript: [{ type: "user", text, pointed: [], images: [] }, { type: "event", event: {
      type: "tool_call", id: "slow", name: "qiyas_execute", input: {}, mutating: true,
    } }],
  };
}
function emit(event: WorkspaceEvent) { core.listeners.forEach(callback => callback({ payload: event })); }
beforeEach(() => {
  core.chats = []; core.paths.clear(); core.snapshots.clear(); core.calls = []; core.listeners.clear(); setWorkspace(null); vi.clearAllMocks();
  core.snapshots.set(1, snapshot(1, "First drawing"));
  core.snapshots.set(2, snapshot(2, "Second drawing", "queued"));
});
afterEach(cleanup);

test("reload reattaches to a running card and does not send, clear or cancel the turn", async () => {
  const { result, unmount } = renderHook(() => useAssistant({ workspace: 1, lang: "en", ...callbacks }));
  await waitFor(() => expect(result.current.transcript.items).toHaveLength(2));
  expect(result.current.busy).toBe(true);
  expect(result.current.transcript.items[1]).toMatchObject({ kind: "tool", status: "running" });
  unmount();
  const reloaded = renderHook(() => useAssistant({ workspace: 1, lang: "en", ...callbacks }));
  await waitFor(() => expect(reloaded.result.current.transcript.items).toHaveLength(2));
  expect(core.calls.some(call => ["ai_cancel", "ai_clear", "ai_send"].includes(call.command))).toBe(false);
});

test("switching keeps independent providers, queue status and late events in their drawing", async () => {
  const { result, rerender } = renderHook(({ workspace }) => useAssistant({ workspace, lang: "en", ...callbacks }), { initialProps: { workspace: 1 } });
  await waitFor(() => expect(result.current.choice?.model).toBe("model-1"));
  rerender({ workspace: 2 });
  await waitFor(() => expect(result.current.choice?.model).toBe("model-2"));
  expect(result.current.phase).toBe("queued");
  act(() => emit({ workspace: 1, cursor: 3, event: { type: "text", text: "Hidden answer" } }));
  expect(JSON.stringify(result.current.transcript)).not.toContain("Hidden answer");
  act(() => emit({ workspace: 2, cursor: 3, event: { type: "working" } }));
  expect(result.current.phase).toBe("working");
  act(() => emit({ workspace: 2, cursor: 4, event: { type: "text", text: "Visible answer" } }));
  act(() => emit({ workspace: 2, cursor: 4, event: { type: "text", text: "Visible answer" } }));
  expect(JSON.stringify(result.current.transcript).match(/Visible answer/g)).toHaveLength(1);
  expect(core.calls.some(call => call.command === "ai_cancel")).toBe(false);
});

test("a provider choice addresses only its workspace", async () => {
  const { result } = renderHook(() => useAssistant({ workspace: 2, lang: "en", ...callbacks }));
  await waitFor(() => expect(result.current.choice?.model).toBe("model-2"));
  await act(() => result.current.choose({ provider: "claude_code", model: "sonnet", effort: "low" }));
  expect(core.calls.filter(call => call.command === "workspace_provider")).toEqual([
    { command: "workspace_provider", args: { workspace: 2, provider: "claude_code", model: "sonnet", effort: "low" } },
  ]);
  expect(core.calls.some(call => call.command === "set_settings")).toBe(false);
});

test("history resumes in the drawing it opened before React receives the new tab", async () => {
  const { result } = renderHook(() => useAssistant({ workspace: 1, lang: "en", ...callbacks }));
  await waitFor(() => expect(result.current.choice?.model).toBe("model-1"));
  setWorkspace(2);
  await act(() => result.current.resume("second-chat"));
  expect(core.calls.filter(call => call.command === "ai_resume")).toEqual([
    { command: "ai_resume", args: { id: "second-chat", workspace: 2 } },
  ]);
  expect(JSON.stringify(result.current.transcript)).toContain("First drawing");
});


test.each(["/jobs/named.dwg", "/autosaves/Untitled-1.dwg"])("opening %s restores its latest saved chat without another click", async (path) => {
  core.paths.set(1, path);
  core.chats = [{ id: "older", drawing: path, updated: 1 }, { id: "latest", drawing: path, updated: 3 },
    { id: "unrelated", drawing: "/jobs/other.dwg", updated: 9 }];
  core.snapshots.set(1, { ...snapshot(1, "", "idle"), transcript: [] });
  const { result } = renderHook(() => useAssistant({ workspace: 1, lang: "en", ...callbacks }));
  await waitFor(() => expect(result.current.transcript.items).toEqual([{ kind: "user", text: "latest", pointed: [], images: [] }]));
  expect(core.calls.filter(call => call.command === "ai_resume")).toEqual([
    { command: "ai_resume", args: { id: "latest", workspace: 1 } },
  ]);
});

test("returning to an intentionally empty chat does not restore its saved predecessor", async () => {
  core.paths.set(1, "/jobs/first.dwg");
  const hook = renderHook(({ workspace }) => useAssistant({ workspace, lang: "en", ...callbacks }), { initialProps: { workspace: 1 } });
  await waitFor(() => expect(hook.result.current.choice?.model).toBe("model-1"));
  act(() => hook.result.current.clear());
  core.snapshots.set(1, { ...snapshot(1, "", "idle"), transcript: [] });
  core.chats = [{ id: "previous", drawing: "/jobs/first.dwg", updated: 1 }];
  hook.rerender({ workspace: 2 });
  await waitFor(() => expect(hook.result.current.choice?.model).toBe("model-2"));
  hook.rerender({ workspace: 1 });
  await waitFor(() => expect(hook.result.current.transcript.items).toEqual([]));
  expect(core.calls.some(call => call.command === "ai_resume")).toBe(false);
});
