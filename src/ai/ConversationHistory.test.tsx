// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ConversationSummary } from "../api";
import { AssistantPanel } from "./AssistantPanel";
import { ConversationHistory, useConversationHistory } from "./ConversationHistory";
import { EMPTY } from "./messages";

const api = vi.hoisted(() => ({
  chats: [] as ConversationSummary[],
  list: vi.fn(), remove: vi.fn(), removeAll: vi.fn(),
}));
vi.mock("../api", async (original) => ({
  ...(await original<object>()),
  aiConversations: () => { api.list(); return Promise.resolve([...api.chats]); },
  aiDeleteConversation: (id: string) => { api.remove(id); api.chats = api.chats.filter((chat) => chat.id !== id); return Promise.resolve(); },
  aiDeleteConversations: () => { api.removeAll(); api.chats = []; return Promise.resolve(); },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const saved = (id: string, drawing: string): ConversationSummary => ({
  id, title: `Chat ${id}`, drawing, drawing_exists: true, updated: 1_792_598_400_000, turns: 2,
});
const first = saved("one", "/jobs/plan.dwg");
const other = saved("two", "/jobs/other.dwg");
const prepare = vi.fn(async () => true);
const resume = vi.fn(async () => {});

beforeEach(() => {
  api.chats = [first, other];
  vi.clearAllMocks();
});
afterEach(cleanup);

function History({ drawing = first.drawing }: { drawing?: string | null }) {
  const history = useConversationHistory({ drawing, busy: false, empty: true, prepare, resume });
  return <ConversationHistory history={history} disabled={false} />;
}

test("History defaults to this drawing, switches to all drawings, deletes a row and deletes all", async () => {
  render(<History />);
  await waitFor(() => expect(api.list).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "History" }));
  await waitFor(() => expect(screen.getByText("Chat one")).toBeTruthy());
  expect(screen.queryByText("Chat two")).toBeNull();
  expect(screen.getByText("plan.dwg")).toBeTruthy();
  expect(screen.getByRole("button", { name: "This drawing" }).getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "All drawings" }));
  expect(screen.getByText("Chat two")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Delete conversation: Chat one" }));
  await waitFor(() => expect(screen.queryByText("Chat one")).toBeNull());
  expect(api.remove).toHaveBeenCalledWith("one");
  fireEvent.click(screen.getByRole("button", { name: "Delete all conversations" }));
  expect(api.removeAll).not.toHaveBeenCalled();
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "Delete all conversations" }));
  await waitFor(() => expect(api.removeAll).toHaveBeenCalledOnce());
  await waitFor(() => expect((screen.getByRole("button", { name: "History" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "History" }));
  await screen.findByText("No saved conversations.");
  expect(api.removeAll).toHaveBeenCalledOnce();
});

test("the empty panel offers Continue only for its drawing and restores after drawing preparation", async () => {
  const assistant = { transcript: EMPTY, busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn() };
  function Panel({ drawing }: { drawing: string }) {
    const history = useConversationHistory({ drawing, busy: false, empty: true, prepare, resume });
    return <AssistantPanel assistant={assistant} history={history} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />;
  }
  const view = render(<Panel drawing={first.drawing!} />);
  const button = await screen.findByRole("button", { name: "Continue: Chat one" });
  fireEvent.click(button);
  await waitFor(() => expect(resume).toHaveBeenCalledWith("one"));
  expect(prepare).toHaveBeenCalledWith(first);
  view.rerender(<Panel drawing="/jobs/new.dwg" />);
  expect(screen.queryByRole("button", { name: /^Continue:/ })).toBeNull();
});

test("resume waits for the drawing decision and does not restore after cancellation", async () => {
  let decide!: (value: boolean) => void;
  const preparation = vi.fn(() => new Promise<boolean>((resolve) => { decide = resolve; }));
  const { result } = renderHook(() => useConversationHistory({
    drawing: first.drawing, busy: false, empty: true, prepare: preparation, resume,
  }));
  await act(async () => {});
  let pending!: Promise<void>;
  await act(async () => { pending = result.current.resume(first); });
  expect(result.current.pending).toBe(true);
  expect(resume).not.toHaveBeenCalled();
  await act(async () => { decide(false); await pending; });
  expect(resume).not.toHaveBeenCalled();
  expect(result.current.pending).toBe(false);
});
