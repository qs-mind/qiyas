import { expect, test } from "vitest";
import type { Activity, AgentEvent, TranscriptEntry } from "../api";
import { translate } from "../i18n";
import { activityLine } from "./ToolCard";
import { EMPTY, readRuns, reduce, startNewOffered, undoableCard, type Action, type Item, type ToolCardItem, type Transcript } from "./messages";

const run = (actions: Action[], from: Transcript = EMPTY) => actions.reduce(reduce, from);

test("cache percentage includes cold input, cache reads and cache writes", () => {
  const transcript = run([{ type: "done", usage: { input_tokens: 100, cache_read_input_tokens: 800, cache_creation_input_tokens: 100 } }]);
  expect(transcript.items).toEqual([{ kind: "cache_usage", cached: 800, total: 1000 }]);
  expect(run([{ type: "restore", entries: [{ type: "event", event: { type: "done", usage: { input_tokens: 200, cache_read_input_tokens: 800 } } }] }]).items)
    .toEqual([{ kind: "cache_usage", cached: 800, total: 1000 }]);
});

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


test("local agent status stays a plain notice in the transcript", () => {
  const state = reduce(EMPTY, { type: "provider_status", code: "ai.claudeConnected" });
  expect(state.items).toEqual([{ kind: "provider_status", code: "ai.claudeConnected" }]);
});

test("completion keeps open findings with the answer and restores them from History", () => {
  const findings: AgentEvent = { type: "open_findings", findings: [{ rule: "text_overlaps_text", message: "Two labels overlap", handles: [2, 3] }], revision: { document_revision: 7 } };
  const transcript = run([{ type: "user", text: "Finish", pointed: [], images: [] }, { type: "text", text: "Done." }, findings, { type: "done", usage: {} }]);
  expect(transcript.items.map(item => item.kind)).toEqual(["user", "assistant", "open_findings"]);
  expect(transcript.items[1]).toMatchObject({ text: "Done.", finished: true });
  const restored = run([{ type: "restore", entries: [{ type: "user", text: "Finish", pointed: [], images: [] }, { type: "event", event: { type: "text", text: "Done." } }, { type: "event", event: findings }, { type: "event", event: { type: "done", usage: {} } }] }]);
  expect(restored.items).toEqual(transcript.items);
});


test("stale plan advice marks the latest plan and a changed plan clears it", () => {
  const phases = [{ id: "rooms", title: "Rooms", status: "active" as const }];
  const stale = run([{ type: "user", text: "Draw" }, { type: "plan", phases }, { type: "plan_progress", mutations_since_update: 8, stale: true }]);
  expect(stale.items.find((item) => item.kind === "plan")).toMatchObject({ stale: true, phases });
  const fresh = run([{ type: "plan", phases: [{ ...phases[0], status: "done" }] }, { type: "plan_progress", mutations_since_update: 0, stale: false }], stale);
  expect(fresh.items.find((item) => item.kind === "plan")).toMatchObject({ stale: false });
});

test("cache totals survive a stopped turn and update without duplicate rows", () => {
  const entries: TranscriptEntry[] = [
    { type: "user", text: "Measure", pointed: [], images: [] },
    { type: "event", event: { type: "cache_usage", usage: { input_tokens: 20, cache_read_input_tokens: 80 } } },
    { type: "event", event: { type: "cache_usage", usage: { input_tokens: 30, cache_read_input_tokens: 170, cache_creation_input_tokens: 50 } } },
    { type: "event", event: { type: "stopped", reason: "cancelled" } },
  ];
  const restored = reduce(EMPTY, { type: "restore", entries });
  expect(restored.items.filter((item) => item.kind === "cache_usage")).toEqual([{ kind: "cache_usage", cached: 170, total: 250 }]);
  expect(restored.items.at(-1)).toEqual({ kind: "stopped", reason: "cancelled" });
  let state = reduce(restored, { type: "user", text: "Continue" });
  state = reduce(state, { type: "cache_usage", usage: { input_tokens: 10, cache_read_input_tokens: 90 } });
  state = reduce(state, { type: "done", usage: { input_tokens: 10, cache_read_input_tokens: 90 } });
  expect(state.items.filter((item) => item.kind === "cache_usage")).toHaveLength(2);
});


test("retry discards unfinished thinking across cache telemetry and keeps its usage", () => {
  const transcript = run([
    { type: "user", text: "Continue" },
    { type: "thinking", text: "unfinished thinking" },
    { type: "cache_usage", usage: { input_tokens: 100, cache_read_input_tokens: 900 } },
    { type: "retrying", seconds: 0, code: "ai.error.output_limit", discard_partial: true },
  ]);
  expect(transcript.items.map((item) => item.kind)).toEqual(["user", "cache_usage", "retrying"]);
  expect(transcript.items[1]).toEqual({ kind: "cache_usage", cached: 900, total: 1000 });
});

test("a question event keeps every question with its options, and the answer is the next message", () => {
  const questions = [
    { id: "q1", question: "Which country?", allow_other: true, options: [{ label: "Egypt", recommended: true }, { label: "UAE" }] },
    { id: "q2", question: "Finishes?", allow_other: true, options: [{ label: "Standard", description: "Ceramic floors" }, { label: "High-end" }] },
  ];
  const transcript = run([
    { type: "user", text: "Design a villa package" },
    { type: "text", text: "I need two answers." },
    { type: "question", questions },
  ]);
  expect(transcript.items).toEqual([
    { kind: "user", text: "Design a villa package" },
    { kind: "assistant", text: "I need two answers.", finished: true },
    { kind: "question", questions },
  ]);
  const answered = reduce(transcript, { type: "user", text: "1a, 2b" });
  expect(answered.items.at(-1)).toEqual({ kind: "user", text: "1a, 2b" });
  // A saved conversation shows the same list.
  const entries: TranscriptEntry[] = [{ type: "event", event: { type: "question", questions } }];
  expect(run([{ type: "restore", entries }]).items).toEqual([{ kind: "question", questions }]);
});

test("a finish or language retry hides the early answer, as any discarded stream", () => {
  let state = reduce(EMPTY, { type: "user", text: "Build the package" });
  state = reduce(state, { type: "text", text: "I stopped here to save cost." });
  state = reduce(state, { type: "retrying", seconds: 0, code: "ai.error.finish_gate", discard_partial: true });
  expect(state.items).toEqual([
    { kind: "user", text: "Build the package" },
    { kind: "retrying", seconds: 0, code: "ai.error.finish_gate" },
  ]);
});

const readCard = (id: string, over: Partial<ToolCardItem> = {}): ToolCardItem => ({
  kind: "tool", id, name: "qiyas_read", label: "records", activity: { category: "drawing", effect: "read" },
  input: {}, status: "done", summary: "", handles: [], ui: null, mutating: false, undone: false, stale: false, ...over,
});

test("a run of identical finished reads is one line with a count; nothing else joins it", () => {
  const items: Item[] = [
    { kind: "user", text: "Check it" },
    readCard("a"), readCard("b"), { kind: "thinking", text: "hm" }, readCard("c"),
    readCard("d", { activity: { category: "takeoff", effect: "read" } }),
    readCard("e", { mutating: true, activity: { category: "drawing", effect: "edit" } }),
    readCard("f"), readCard("g"),
    readCard("h", { handles: [7] }),
    readCard("i", { status: "error" }),
  ];
  const { counts, hidden } = readRuns(items);
  expect([...counts]).toEqual([[1, 3], [7, 2]]);
  expect([...hidden].sort()).toEqual([2, 4, 8]);
});

test("an assistant message ends a run, so reads before and after it stay two lines", () => {
  const items: Item[] = [readCard("a"), readCard("b"), { kind: "assistant", text: "Done so far", finished: true }, readCard("c"), readCard("d")];
  expect([...readRuns(items).counts]).toEqual([[0, 2], [3, 2]]);
});

const line = (detail: NonNullable<Activity["detail"]>, status: "running" | "done" = "done", lang: "en" | "ar" = "en") =>
  activityLine(detail, status, (key, params) => translate(lang, key, params));

test("draw batches name the kinds, counts and shared layer", () => {
  const detail = { key: "activity.line.batch", params: {}, parts: [
    { key: "activity.line.draw", params: { what: "line", n: 12, layer: "A-WALL" } },
    { key: "activity.line.draw", params: { what: "arc", n: 4, layer: "A-WALL" } },
  ] };
  expect(line(detail)).toBe("Drew 12 lines and 4 arcs on A-WALL");
  expect(line(detail, "running")).toBe("Drawing 12 lines and 4 arcs on A-WALL…");
  expect(line({ ...detail, params: { more: 5 } })).toBe("Drew 12 lines and 4 arcs on A-WALL and 5 more steps");
  expect(line({ ...detail, parts: [detail.parts[0], { ...detail.parts[1], params: { ...detail.parts[1].params, layer: "A-DOOR" } }] }))
    .toBe("Drew 12 lines on A-WALL and 4 arcs on A-DOOR");
});

test("sheet, records, lookup and export lines name their objects", () => {
  expect(line({ key: "activity.line.make", params: { what: "sheet", name: "A-101 Ground floor", paper: "A1", scale: "1:100" } }))
    .toBe('Made sheet "A-101 Ground floor" (A1, 1:100)');
  expect(line({ key: "activity.line.read", params: { what: "table", n: 6, layer: "A-TABLE", sheet: "Ground floor" } }))
    .toBe('Read 6 tables on A-TABLE on sheet "Ground floor"');
  // A command's own name is not for the engineer: plain names when the window has them, else a count.
  expect(line({ key: "activity.line.lookup", params: { what: "command", name: "make_sheet, table" } }))
    .toBe("Looked up how to use 2 commands");
  expect(line({ key: "activity.line.lookup", params: { what: "command", name: "rooms, free_space" } }))
    .toBe("Looked up how to use room search and available space");
  expect(line({ key: "activity.line.lookup", params: { what: "command", name: "drafting_knowledge" } }))
    .toBe("Looked up how to use 1 command");
  // Model space by its window name, not its block's.
  expect(line({ key: "activity.line.read", params: { what: "table", n: 2, sheet: "*Model_Space" } }))
    .toBe('Read 2 tables on sheet "Model"');
  expect(line({ key: "activity.line.export", params: { what: "file", file: "boq.xlsx" } }))
    .toBe("Exported boq.xlsx");
  expect(line({ key: "activity.line.add", params: { what: "table", name: "Door schedule", rows: 6 } }))
    .toBe('Added the table "Door schedule" (6 rows)');
  expect(line({ key: "activity.line.move", params: { what: "text", n: 1 } })).toBe("Moved 1 text");
  expect(line({ key: "activity.line.check", params: { what: "room_size", notes: 2 } })).toBe("Checked room sizes: 2 notes");
  expect(line({ key: "activity.line.capture", params: { what: "view", name: "the ground floor" } })).toBe("Took a picture of the ground floor");
});

test("Arabic activities use Arabic word order and keep names and scale intact", () => {
  const readable = (text: string) => text.replace(/[\u2068\u2069]/g, "");
  expect(readable(line({ key: "activity.line.draw", params: { what: "wall", n: 4, layer: "A-WALL" } }, "done", "ar")))
    .toBe("رسم جدران (العدد: 4) على A-WALL");
  expect(readable(line({ key: "activity.line.make", params: { what: "sheet", name: "A-101 Ground floor", paper: "A1", scale: "1:100" } }, "done", "ar")))
    .toBe("أنشأ اللوحة «A-101 Ground floor» (A1، 1:100)");
  expect(line({ key: "activity.line.read", params: { what: "layer" } }, "done", "ar")).toBe("قرأ طبقات الرسم");
});

test("result details replace running details and survive restoring a conversation", () => {
  const running: Activity = { category: "records", effect: "read", detail: { key: "activity.line.read", params: { what: "wall", layer: "A-WALL" } } };
  const finished: Activity = { ...running, detail: { key: "activity.line.read", params: { what: "wall", layer: "A-WALL", n: 37 } } };
  const started = { ...call("a", "records", false), activity: running };
  const ended = result("a", { activity: finished, count: 37, handles: [] });
  const transcript = run([started, ended]);
  expect(transcript.items[0]).toMatchObject({ activity: finished, status: "done", count: 37 });
  const entries: TranscriptEntry[] = [{ type: "event", event: started }, { type: "event", event: ended }];
  expect(run([{ type: "restore", entries }]).items[0]).toMatchObject({ activity: finished, status: "done" });
  expect(run([started, result("a")]).items[0]).toMatchObject({ activity: running });
  expect(run([call("old"), result("old")]).items[0]).toMatchObject({ status: "done", activity: undefined });
});

test("only identical detail lines merge, regardless of parameter insertion order", () => {
  const activity = (n: number, layer: string): Activity => ({ category: "drawing", effect: "read", detail: { key: "activity.line.read", params: { what: "wall", n, layer } } });
  const items = [
    readCard("a", { activity: activity(37, "A-WALL") }),
    readCard("b", { activity: { category: "drawing", effect: "read", detail: { key: "activity.line.read", params: { layer: "A-WALL", n: 37, what: "wall" } } } }),
    readCard("c", { activity: activity(38, "A-WALL") }),
    readCard("d", { activity: activity(38, "A-DOOR") }),
    readCard("e", { activity: { category: "drawing", effect: "read", detail: { key: "activity.line.lookup", params: { what: "command", name: "table" } } } }),
    readCard("f", { activity: { category: "drawing", effect: "read", detail: { key: "activity.line.lookup", params: { what: "command", name: "make_sheet" } } } }),
    readCard("legacy"),
  ];
  expect([...readRuns(items).counts]).toEqual([[0, 2]]);
  expect([...readRuns(items).hidden]).toEqual([1]);
});

test("batch progress updates only its running card and survives reattachment", () => {
  const started = call("batch", "batch");
  const progress: AgentEvent = { type: "tool_progress", id: "batch", done: 3, total: 10, label: "draw.line" };
  const live = run([started, call("other"), progress]);
  expect(live.items[0]).toMatchObject({ status: "running", progress: { done: 3, total: 10, label: "draw.line" } });
  expect(live.items[1]).not.toHaveProperty("progress", { done: 3, total: 10, label: "draw.line" });
  const entries: TranscriptEntry[] = [started, progress].map((event) => ({ type: "event", event }));
  expect(run([{ type: "reattach", entries }]).items[0]).toMatchObject({ progress: { done: 3, total: 10, label: "draw.line" } });
  const done = run([result("batch"), { ...progress, done: 4 }], live);
  expect(done.items[0]).toMatchObject({ status: "done", progress: { done: 3, total: 10, label: "draw.line" } });
  expect(run([{ ...progress, done: 11 }], live)).toEqual(live);
  expect(run([{ ...progress, total: 0 }], live)).toEqual(live);
});
