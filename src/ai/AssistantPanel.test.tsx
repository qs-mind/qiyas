// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AssistantPanel } from "./AssistantPanel";
import { LanguageProvider } from "../i18n";
import ar from "../i18n/ar.json";

const aiLog = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<object>()), aiLog: () => aiLog() }));
import { EMPTY, type ToolCardItem, type Transcript } from "./messages";

afterEach(cleanup);

const card: ToolCardItem = {
  kind: "tool",
  id: "t1",
  name: "qiyas_execute",
  label: "measure_area",
  input: { op: "run", args: JSON.stringify({ command: { type: "measure_area", handle: 7 } }) },
  status: "done",
  summary: "12.000 m2: wall",
  handles: [7],
  ui: null,
  mutating: true,
  undone: false,
  stale: false,
};

function setup(transcript: Transcript = EMPTY, busy = false, error: string | null = null) {
  const assistant = {
    transcript,
    busy,
    error,
    status: { provider: "mock", model: "mock", has_key: true, busy, message_count: 0 },
    send: vi.fn().mockResolvedValue(true),
    cancel: vi.fn(),
    clear: vi.fn(),
  };
  const onShow = vi.fn();
  const onUndo = vi.fn();
  render(<AssistantPanel assistant={assistant} onShow={onShow} onUndo={onUndo} onSettings={() => {}} />);
  return { assistant, onShow, onUndo };
}

test("empty state offers three examples; one fills the box", () => {
  setup();
  const examples = screen.getAllByRole("listitem");
  expect(examples).toHaveLength(3);
  fireEvent.click(screen.getByText("Count the doors on this sheet"));
  expect((screen.getByLabelText("Type your request…") as HTMLTextAreaElement).value).toBe(
    "Count the doors on this sheet",
  );
  expect((screen.getByRole("button", { name: "New chat" }) as HTMLButtonElement).disabled).toBe(true);
});

test("the phase checklist appears above the running answer and has no Undo action", () => {
  setup({ items: [
    { kind: "user", text: "Draw the plan" },
    { kind: "plan", phases: [
      { id: "sizes", title: "Check sizes", status: "done" },
      { id: "walls", title: "Draw walls", status: "active" },
      { id: "labels", title: "Room labels", status: "pending" },
      { id: "sheet", title: "Sheet", status: "skipped" },
    ] },
    { ...card, id: "plan", label: "plan", mutating: false, summary: "Done" },
    { kind: "assistant", text: "Drawing the walls.", finished: false },
  ] }, true);
  const plan = screen.getByRole("region", { name: "Plan" });
  expect(plan.querySelectorAll("li")).toHaveLength(4);
  expect(plan.querySelector('[aria-current="step"]')?.textContent).toContain("Draw walls");
  for (const status of ["Done", "Active", "Pending", "Skipped"]) expect(screen.getByText(status)).toBeTruthy();
  const answer = screen.getByText("Drawing the walls.");
  expect(plan.compareDocumentPosition(answer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.queryByText("Undo")).toBeNull();
  expect(document.querySelector(".tool-card")).toBeNull();
});

test("the checklist translates all states into Arabic and follows the page direction", () => {
  localStorage.setItem("qiyas.lang", "ar");
  try {
    const assistant = {
      transcript: { items: [{ kind: "plan" as const, phases: [
        { id: "a", title: "A", status: "pending" as const },
        { id: "b", title: "B", status: "active" as const },
        { id: "c", title: "C", status: "done" as const },
        { id: "d", title: "D", status: "skipped" as const },
      ] }] },
      busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn(),
    };
    render(<LanguageProvider><AssistantPanel assistant={assistant} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} /></LanguageProvider>);
    expect(screen.getByRole("region", { name: ar["ai.plan"] })).toBeTruthy();
    for (const key of ["ai.plan.pending", "ai.plan.active", "ai.plan.done", "ai.plan.skipped"] as const) expect(screen.getByText(ar[key])).toBeTruthy();
    expect(document.documentElement.dir).toBe("rtl");
  } finally {
    localStorage.removeItem("qiyas.lang");
    document.documentElement.dir = "ltr";
    document.documentElement.lang = "en";
  }
});

test("a missing provider model explains how to choose another", () => {
  setup(EMPTY, false, "Anthropic has no model google:gemini@3.8-flash: choose one in Settings");
  expect(screen.getByText("Anthropic has no model google:gemini@3.8-flash: choose one in Settings")).toBeTruthy();
});

test("New chat in the header starts a new conversation; not while Mizan works", () => {
  const { assistant } = setup({ items: [{ kind: "user", text: "Measure the wall" }] });
  fireEvent.click(screen.getByRole("button", { name: "New chat" }));
  expect(assistant.clear).toHaveBeenCalledTimes(1);
  cleanup();
  setup({ items: [{ kind: "user", text: "Measure the wall" }] }, true);
  expect((screen.getByRole("button", { name: "New chat" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a transcript shows messages, Markdown and a tool card with Show and Undo", () => {
  const { onShow, onUndo } = setup({
    items: [
      { kind: "user", text: "Measure the wall" },
      { kind: "assistant", text: "Measuring **now**.\n\n- one `ai`\n- two <b>x</b>", finished: true },
      card,
    ],
  });
  expect(screen.getByText("Measure the wall")).toBeTruthy();
  expect(screen.getByText("now").tagName).toBe("STRONG");
  expect(screen.getByText("ai").tagName).toBe("CODE");
  // Markup in the model's text stays text.
  expect(screen.getByText(/two/).textContent).toBe("two <b>x</b>");
  expect(document.querySelector(".msg b")).toBeNull();
  expect(screen.getByText("Measure area")).toBeTruthy();
  expect(screen.getByText("12.000 m2: wall")).toBeTruthy();
  fireEvent.click(screen.getByText("Show"));
  expect(onShow).toHaveBeenCalledWith(card);
  fireEvent.click(screen.getByText("Undo"));
  expect(onUndo).toHaveBeenCalledWith(card);
});

test("tool cards translate command labels and make unknown labels readable", () => {
  setup({
    items: [
      { ...card, id: "line", label: "draw.line" },
      { ...card, id: "ellipse", label: "draw.ellipse" },
      { ...card, id: "read", name: "qiyas_read", label: null, mutating: false },
    ],
  });
  expect(screen.getByText("Draw line")).toBeTruthy();
  expect(screen.getByText("Draw · ellipse")).toBeTruthy();
  expect(screen.getByText("Read drawing")).toBeTruthy();
});

test("Undo is only offered on the newest change, and not while busy", () => {
  const older = { ...card, id: "t0" };
  setup({ items: [older, card] });
  expect(screen.getAllByText("Undo")).toHaveLength(1);
  cleanup();
  setup({ items: [card] }, true);
  expect(screen.queryByText("Undo")).toBeNull();
  expect(screen.getByText("Stop")).toBeTruthy();
  expect(screen.getByText("Working…")).toBeTruthy();
  // The Qiyas mark, alive, beside the text (not dots, not a ring), hidden from assistive technology.
  const mark = screen.getByText("Working…").querySelector(".mizan-mark.mizan-mark-md");
  expect(mark?.getAttribute("aria-hidden")).toBe("true");
  expect(mark?.querySelector(".mm-pen line")).toBeTruthy();
  expect(screen.getByText("Working…").querySelector(".dot-loader")).toBeNull();
  expect(document.querySelector(".spinner")).toBeNull();
});

test("after an undo the card before it can be undone; a stale card says why", () => {
  const older = { ...card, id: "t0" };
  const { onUndo } = setup({ items: [older, { ...card, undone: true }] });
  expect(screen.getByText("Undone")).toBeTruthy();
  fireEvent.click(screen.getByText("Undo"));
  expect(onUndo).toHaveBeenCalledWith(older);
  cleanup();
  setup({ items: [{ ...card, stale: true }] });
  expect((screen.getByText("Undo") as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText("Can't undo: the take-off changed since")).toBeTruthy();
});

test("a crash says so and offers a new conversation; the status line announces the end", () => {
  const { assistant } = setup({ items: [{ kind: "user", text: "hi" }] }, false, "crashed");
  expect(screen.getByRole("alert").textContent).toBe("Mizan crashed; start a new conversation.Show log");
  fireEvent.click(screen.getByText("Start a new conversation"));
  expect(assistant.clear).toHaveBeenCalled();
  expect(screen.getByRole("status").textContent).toBe("");
});

test("Enter sends and empties the box; Shift+Enter does not send", async () => {
  const { assistant } = setup();
  const box = screen.getByLabelText("Type your request…") as HTMLTextAreaElement;
  fireEvent.change(box, { target: { value: "Count the doors" } });
  fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
  expect(assistant.send).not.toHaveBeenCalled();
  fireEvent.keyDown(box, { key: "Enter" });
  expect(assistant.send).toHaveBeenCalledWith("Count the doors");
  await waitFor(() => expect(box.value).toBe(""));
});

test("a refusal shows its card and a button for a new conversation", () => {
  const { assistant } = setup({
    items: [{ kind: "refused", category: null, text: "Not this one." }],
  });
  expect(screen.getByText("Request declined")).toBeTruthy();
  fireEvent.click(screen.getByText("Start a new conversation"));
  expect(assistant.clear).toHaveBeenCalled();
});

test("Log shows the agent log in place of the conversation; an error offers it", async () => {
  aiLog.mockResolvedValue({ path: "/cfg/logs/agent.log", text: "12:00  ERROR\n    The AI service answered 400" });
  setup(EMPTY, false, "The AI service answered 400: The request was rejected");
  fireEvent.click(screen.getByText("Show log"));
  await waitFor(() => expect(screen.getByText(/ERROR/)).toBeTruthy());
  expect(screen.getByText("/cfg/logs/agent.log")).toBeTruthy();
  expect(screen.queryByText("Show log")).toBeNull();
  expect(screen.getByText("Log").getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(screen.getByText("Log"));
  expect(screen.getAllByRole("listitem")).toHaveLength(3);
});

test("an empty log says so", async () => {
  aiLog.mockResolvedValue({ path: "/cfg/logs/agent.log", text: "" });
  setup();
  fireEvent.click(screen.getByText("Log"));
  await waitFor(() => expect(screen.getByText("Nothing in the log yet. Send Mizan a message.")).toBeTruthy());
});

/** The assistant the panel is given, with what a test changes. */
const assistant = (over: Partial<{ transcript: Transcript; send: () => Promise<boolean> }> = {}) => ({
  transcript: EMPTY,
  busy: false,
  error: null,
  status: { provider: "mock", model: "mock", has_key: true, busy: false, message_count: 0 },
  send: vi.fn().mockResolvedValue(true),
  cancel: vi.fn(),
  clear: vi.fn(),
  ...over,
});

/** The log's box has no layout in jsdom: give it a height and a scroll position to read and move. */
function layout(el: HTMLElement, box: { scrollHeight: number; clientHeight: number; scrollTop: number }) {
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => box.scrollHeight });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => box.clientHeight });
  Object.defineProperty(el, "scrollTop", { configurable: true, get: () => box.scrollTop, set: (v) => (box.scrollTop = v) });
  el.scrollTo = vi.fn((arg: ScrollToOptions | number) => {
    box.scrollTop = typeof arg === "number" ? arg : (arg.top ?? 0);
  }) as unknown as typeof el.scrollTo;
}

test("the thread follows the newest message while the reader is at the bottom; scrolled up it leaves them, and a button jumps to the latest", () => {
  const box = { scrollHeight: 1000, clientHeight: 400, scrollTop: 0 };
  const first = { items: [{ kind: "user" as const, text: "How much tiling?" }] };
  const { rerender } = render(<AssistantPanel assistant={assistant({ transcript: first })} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  const log = document.querySelector<HTMLElement>(".assistant-log")!;
  layout(log, box);

  // At the bottom: a message that arrives scrolls into view, and no button is offered.
  box.scrollTop = 600;
  fireEvent.scroll(log);
  box.scrollHeight = 1200;
  const second = { items: [...first.items, { kind: "assistant" as const, text: "About 75 m2.", finished: false }] };
  rerender(<AssistantPanel assistant={assistant({ transcript: second })} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  expect(box.scrollTop).toBe(1200);
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();

  // The reader scrolls up to re-read: a button offers the way back, and what arrives next does not move them.
  box.scrollTop = 100;
  fireEvent.scroll(log);
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeTruthy();
  box.scrollHeight = 1500;
  const third = { items: [...second.items, { kind: "user" as const, text: "And the BOQ?" }] };
  rerender(<AssistantPanel assistant={assistant({ transcript: third })} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  expect(box.scrollTop).toBe(100);
  fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
  expect(box.scrollTop).toBe(1500);
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();

  // Following again: the next one scrolls into view.
  box.scrollHeight = 1700;
  rerender(<AssistantPanel assistant={assistant({ transcript: { items: [...third.items, { kind: "assistant", text: "Done.", finished: true }] } })} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  expect(box.scrollTop).toBe(1700);
});

test("scrolling back to the bottom by hand takes the button away; sending a message follows it", () => {
  const box = { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 };
  const items = [{ kind: "user" as const, text: "Hi" }];
  const props = { onShow() {}, onUndo() {}, onSettings() {} };
  const send = vi.fn(async () => true);
  const { rerender } = render(<AssistantPanel assistant={assistant({ transcript: { items }, send })} {...props} />);
  const log = document.querySelector<HTMLElement>(".assistant-log")!;
  layout(log, box);
  box.scrollTop = 50;
  fireEvent.scroll(log);
  box.scrollHeight = 1300;
  rerender(<AssistantPanel assistant={assistant({ transcript: { items: [...items, { kind: "assistant", text: "Hello", finished: true }] }, send })} {...props} />);
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeTruthy();
  // Near enough to the bottom (within a line or two) is the bottom.
  box.scrollTop = 880;
  fireEvent.scroll(log);
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  // The reader scrolls up again and then sends: the thread goes to the bottom to show it.
  box.scrollTop = 0;
  fireEvent.scroll(log);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Thanks" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(box.scrollTop).toBe(1300);
});

test("Mizan's answers render headings, rules, nested lists and tables, and links never navigate", () => {
  const text = [
    "### Drawing analysis",
    "The plan is in **metres**.",
    "",
    "---",
    "",
    "- **Columns:**",
    "  - 6 columns",
    "",
    "| Type | Value |",
    "|:---|---:|",
    "| Linear | 12.00 m |",
    "",
    "See [the docs](https://example.com).",
  ].join("\n");
  setup({ items: [{ kind: "assistant", text, finished: true }] });
  const md = document.querySelector(".md")!;
  expect(md.querySelector("h4")?.textContent).toBe("Drawing analysis");
  expect(md.textContent).not.toContain("###");
  expect(md.querySelector("hr")).toBeTruthy();
  expect(md.querySelector("ul ul li")?.textContent).toBe("6 columns");
  const table = md.querySelector(".md-table table")!;
  expect([...table.querySelectorAll("th")].map((c) => c.textContent)).toEqual(["Type", "Value"]);
  expect(table.querySelector("td:last-child")?.textContent).toBe("12.00 m");
  expect(md.textContent).not.toContain("|");
  expect(md.querySelector("a")).toBeNull();
  expect(md.querySelector(".md-link")?.getAttribute("title")).toBe("https://example.com");
});

function pointingSetup(more: Partial<import("../app/useAssistantWiring").Pointing> = {}) {
  const send = vi.fn<import("../app/useAssistantWiring").Pointing["send"]>().mockResolvedValue(true);
  const pointing = {
    selection: { count: 3, kinds: "2 LwPolyline, 1 Insert" },
    region: { width: 12.3456, height: 8.1, count: 47 },
    detachSelection: vi.fn(),
    clearRegion: vi.fn(),
    markArea: vi.fn(),
    send,
    focusKey: 0,
    ask: vi.fn(),
    ...more,
  };
  const read = vi.fn(async (file: File) => ({
    media_type: file.type,
    data: "QUJD",
    url: `data:${file.type};base64,QUJD`,
  }));
  const assistant = {
    transcript: EMPTY,
    busy: false,
    error: null,
    status: null,
    send: vi.fn().mockResolvedValue(true),
    cancel: vi.fn(),
    clear: vi.fn(),
  };
  render(
    <AssistantPanel
      assistant={assistant}
      onShow={() => {}}
      onUndo={() => {}}
      onSettings={() => {}}
      pointing={pointing}
      read={read}
    />,
  );
  return { pointing, read, assistant, send };
}

test("what the user points at shows as chips above the box; × leaves it out", () => {
  const { pointing } = pointingSetup();
  expect(screen.getByText("Selection · 3")).toBeTruthy();
  expect(screen.getByText("2 LwPolyline, 1 Insert")).toBeTruthy();
  expect(screen.getByText("Area · 12.35 × 8.10 m")).toBeTruthy();
  expect(screen.getByText("47 entities")).toBeTruthy();
  const [selectionX, areaX] = screen.getAllByRole("button", { name: "Remove" });
  fireEvent.click(selectionX);
  expect(pointing.detachSelection).toHaveBeenCalledTimes(1);
  fireEvent.click(areaX);
  expect(pointing.clearRegion).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Mark an area for Mizan" }));
  expect(pointing.markArea).toHaveBeenCalledTimes(1);
});

test("a pasted picture shows as a thumbnail and goes with the message", async () => {
  const { send: sendPointed, read } = pointingSetup({ selection: null, region: null });
  const box = screen.getByLabelText("Type your request…");
  const photo = new File(["x"], "site.jpg", { type: "image/jpeg" });
  fireEvent.paste(box, { clipboardData: { files: [photo] } });
  await waitFor(() => expect(screen.getAllByAltText("Picture")).toHaveLength(1));
  expect(read).toHaveBeenCalledWith(photo);
  // A picture alone can be sent.
  const send = screen.getByRole("button", { name: "Send" }) as HTMLButtonElement;
  expect(send.disabled).toBe(false);
  fireEvent.change(box, { target: { value: "What is this?" } });
  fireEvent.click(send);
  await waitFor(() => expect(sendPointed).toHaveBeenCalledTimes(1));
  expect(sendPointed.mock.calls[0][0]).toBe("What is this?");
  expect(sendPointed.mock.calls[0][1]).toEqual([
    { media_type: "image/jpeg", data: "QUJD", url: "data:image/jpeg;base64,QUJD" },
  ]);
  await waitFor(() => expect(screen.queryAllByAltText("Picture")).toHaveLength(0));
});

test("at most four pictures go with one message", async () => {
  pointingSetup({ selection: null, region: null });
  const box = screen.getByLabelText("Type your request…");
  const files = [1, 2, 3, 4, 5].map((n) => new File(["x"], `p${n}.png`, { type: "image/png" }));
  fireEvent.paste(box, { clipboardData: { files } });
  await waitFor(() => expect(screen.getAllByAltText("Picture")).toHaveLength(4));
  expect(screen.getByRole("alert").textContent).toBe("At most 4 pictures in one message");
  expect((screen.getByRole("button", { name: "Attach pictures" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a user's message shows what went with it", () => {
  setup({
    items: [{ kind: "user", text: "And this?", pointed: ["Selection · 2"], images: ["data:image/png;base64,QUJD"] }],
  });
  expect(screen.getByText("Selection · 2")).toBeTruthy();
  expect(screen.getByAltText("Picture").getAttribute("src")).toBe("data:image/png;base64,QUJD");
});
