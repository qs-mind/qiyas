import { expect, test } from "vitest";
import type { AgentEvent } from "../api";
import { EMPTY, reduce, startNewOffered, undoableCard, type Action, type Transcript } from "./messages";

const run = (actions: Action[], from: Transcript = EMPTY) => actions.reduce(reduce, from);

const call = (id: string, label = "measure_area", mutating = true): AgentEvent => ({
  type: "tool_call",
  id,
  name: mutating ? "qiyas_execute" : "qiyas_read",
  label,
  input: mutating
    ? { op: "run", args: JSON.stringify({ command: { type: label, handle: 7 } }) }
    : { op: label, args: "{}" },
  mutating,
});

const result = (id: string, more: Partial<Extract<AgentEvent, { type: "tool_result" }>> = {}): AgentEvent => ({
  type: "tool_result",
  id,
  summary: "12.000 m2: wall",
  handles: [7],
  ui: null,
  is_error: false,
  ...more,
});

test("items keep the order they arrive in", () => {
  const t = run([
    { type: "user", text: "Measure the wall" },
    { type: "text", text: "Measuring." },
    call("a"),
    result("a"),
    { type: "text", text: "Done." },
    { type: "done", usage: {} },
  ]);
  expect(t.items.map((i) => i.kind)).toEqual(["user", "assistant", "tool", "assistant"]);
});

test("plan updates stay above the answer and earlier turns keep their checklist", () => {
  const pending = { id: "walls", title: "Draw walls", status: "pending" as const };
  const active = { ...pending, status: "active" as const };
  const done = { ...pending, status: "done" as const };
  const first = run([
    { type: "user", text: "Draw the plan" },
    { type: "text", text: "First I will check the sizes." },
    { type: "plan", phases: [pending] },
    { type: "plan", phases: [active] },
    { type: "text", text: " Then draw." },
    { type: "plan", phases: [done] },
    { type: "done", usage: {} },
  ]);
  expect(first.items).toEqual([
    { kind: "user", text: "Draw the plan" },
    { kind: "plan", phases: [done] },
    { kind: "assistant", text: "First I will check the sizes. Then draw.", finished: true },
  ]);
  const second = run([
    { type: "user", text: "Now add labels" },
    { type: "plan", phases: [{ id: "labels", title: "Room labels", status: "active" }] },
  ], first);
  expect(second.items.filter((item) => item.kind === "plan")).toHaveLength(2);
  expect(second.items[1]).toEqual(first.items[1]);
  expect(undoableCard(second)).toBeNull();
  expect(run([{ type: "clear" }], second)).toEqual(EMPTY);
});

test("text deltas join into one streamed message that finishes at the next item", () => {
  const t = run([{ type: "text", text: "Hel" }, { type: "text", text: "lo " }, { type: "text", text: "there" }]);
  expect(t.items).toEqual([{ kind: "assistant", text: "Hello there", finished: false }]);
  const after = run([call("a")], t);
  expect(after.items[0]).toEqual({ kind: "assistant", text: "Hello there", finished: true });
  const ended = run([{ type: "end" }], t);
  expect(ended.items[0]).toMatchObject({ finished: true });
});

test("a tool card runs, then is done with its summary, handles and ui", () => {
  const running = run([call("a")]);
  expect(running.items[0]).toMatchObject({
    kind: "tool", id: "a", name: "qiyas_execute", label: "measure_area", status: "running", mutating: true,
  });
  const ui = { action: "zoom", bounds: [[0, 0], [1, 1]] as [[number, number], [number, number]] };
  const done = run([result("a", { ui })], running);
  expect(done.items[0]).toMatchObject({
    kind: "tool",
    status: "done",
    summary: "12.000 m2: wall",
    handles: [7],
    ui,
  });
});

test("a failed tool shows as an error, and the right card of several is updated", () => {
  const t = run([
    call("a", "records", false),
    call("b"),
    result("b", { is_error: true, summary: "Not closed", handles: [] }),
    result("a", { summary: "3 found", handles: [] }),
  ]);
  expect(t.items).toMatchObject([
    { id: "a", status: "done", summary: "3 found", mutating: false },
    { id: "b", status: "error", summary: "Not closed" },
  ]);
});

test("a refusal ends the streamed text and offers a new conversation", () => {
  const t = run([
    { type: "user", text: "…" },
    { type: "text", text: "Let me" },
    { type: "refused", category: "cyber", text: "I can't help with that." },
  ]);
  expect(t.items.at(-2)).toMatchObject({ kind: "assistant", finished: true });
  expect(t.items.at(-1)).toEqual({ kind: "refused", category: "cyber", text: "I can't help with that." });
  expect(startNewOffered(t)).toBe(true);
});

test("a long conversation offers a new one; a cancel does not", () => {
  const long = run([{ type: "stopped", reason: "conversation is long, start a new one" }]);
  expect(long.items).toEqual([{ kind: "stopped", reason: "conversation is long, start a new one" }]);
  expect(startNewOffered(long)).toBe(true);
  expect(startNewOffered(run([{ type: "stopped", reason: "cancelled" }]))).toBe(false);
  expect(startNewOffered(EMPTY)).toBe(false);
});

test("empty thinking is dropped; thinking deltas join", () => {
  expect(run([{ type: "thinking", text: "" }]).items).toEqual([]);
  const t = run([
    { type: "thinking", text: "First " },
    { type: "thinking", text: "the walls." },
    { type: "text", text: "Ok" },
  ]);
  expect(t.items).toEqual([
    { kind: "thinking", text: "First the walls." },
    { kind: "assistant", text: "Ok", finished: false },
  ]);
});

test("undo marks the card; the end of a turn fails cards still running; clear empties", () => {
  const t = run([call("a"), result("a"), { type: "undone", id: "a" }, call("b"), { type: "end" }]);
  expect(t.items).toMatchObject([
    { id: "a", undone: true },
    { id: "b", status: "error" },
  ]);
  expect(run([{ type: "clear" }], t)).toEqual(EMPTY);
});

test("unsend takes back the last user message only", () => {
  const t = run([{ type: "user", text: "hi" }]);
  expect(run([{ type: "unsend" }], t)).toEqual(EMPTY);
  const answered = run([{ type: "text", text: "Hello" }], t);
  expect(run([{ type: "unsend" }], answered)).toEqual(answered);
});

test("the undoable card is the newest change not undone; a stale undo is kept on the card", () => {
  const t = run([call("a"), result("a"), call("b", "records", false), result("b"), call("c"), result("c")]);
  expect(undoableCard(t)?.id).toBe("c");
  const afterUndo = run([{ type: "undone", id: "c" }], t);
  expect(undoableCard(afterUndo)?.id).toBe("a");
  expect(undoableCard(run([{ type: "undone", id: "a" }], afterUndo))).toBeNull();
  const stale = run([{ type: "undo_stale", id: "c" }], t);
  expect(stale.items.at(-1)).toMatchObject({ id: "c", stale: true, undone: false });
  expect(undoableCard(run([call("d"), result("d", { is_error: true })], t))?.id).toBe("c");
});

test("saved events replay through the live reducer and leave old Undo steps finished", async () => {
  const { replay } = await import("./messages");
  const entries: import("../api").TranscriptEntry[] = [
    { type: "user", text: "Measure this", pointed: ["Selection · 1"], images: ["[picture removed]"] },
    ...[
      { type: "thinking", text: "Read first" },
      { type: "plan", phases: [{ id: "measure", title: "Measure", status: "active" }] },
      call("saved"), result("saved"),
      { type: "text", text: "Measured " }, { type: "text", text: "12 m2." },
      { type: "done", usage: {} },
    ].map((event) => ({ type: "event" as const, event: event as AgentEvent })),
  ];
  const live = run(entries.map((entry) => entry.type === "event" ? entry.event : entry));
  const saved = replay(entries);
  expect(saved.items).toEqual(live.items.map((item) => item.kind === "tool" ? { ...item, restored: true } : item));
  expect(undoableCard(live)?.id).toBe("saved");
  expect(undoableCard(saved)).toBeNull();
});

test("an approval event restores the current checklist and finishes streaming text", () => {
  const phases = [{ id: "check", title: "Check", status: "pending" as const }];
  const transcript = run([
    { type: "user", text: "Draw" },
    { type: "text", text: "Here is the plan." },
    { type: "awaiting_approval", plan: { phases } },
  ]);
  expect(transcript.items).toEqual([
    { kind: "user", text: "Draw" },
    { kind: "plan", phases, awaitingApproval: true },
    { kind: "assistant", text: "Here is the plan.", finished: true },
  ]);
});

test("a retried stream removes the incomplete answer and keeps the completed tools", () => {
  let state = reduce(EMPTY, { type: "user", text: "Continue" });
  state = reduce(state, { type: "text", text: "An incomplete answer" });
  state = reduce(state, { type: "retrying", seconds: 5, code: "ai.error.service_error", discard_partial: true });
  state = reduce(state, { type: "text", text: "The complete answer" });
  expect(state.items).toEqual([
    { kind: "user", text: "Continue" },
    { kind: "retrying", seconds: 5, code: "ai.error.service_error" },
    { kind: "assistant", text: "The complete answer", finished: false },
  ]);
});
