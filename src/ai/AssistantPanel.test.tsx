import { SupportLog } from "../shell/SupportLog";
// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AssistantPanel } from "./AssistantPanel";
import { LanguageProvider } from "../i18n";
import ar from "../i18n/ar.json";

const aiLog = vi.fn();
const aiAttachPdf = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<object>()), aiLog: () => aiLog(), aiAttachPdf: (...args: unknown[]) => aiAttachPdf(...args) }));
import { EMPTY, type ToolCardItem, type Transcript } from "./messages";

afterEach(() => {
  cleanup();
  localStorage.removeItem("qiyas.planDock.expanded");
});

const card: ToolCardItem = {
  kind: "tool",
  id: "t1",
  name: "qiyas_execute",
  label: "measure_area",
  activity: { category: "takeoff", effect: "edit" },
  input: { op: "run", args: JSON.stringify({ command: { type: "measure_area", handle: 7 } }) },
  status: "done",
  summary: "12.000 m2: wall",
  handles: [7],
  ui: null,
  mutating: true,
  undone: false,
  stale: false,
};

function setup(transcript: Transcript = EMPTY, busy = false, error: string | null = null, showTechnicalDetails = false) {
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
  render(<AssistantPanel showTechnicalDetails={showTechnicalDetails} assistant={assistant} onShow={onShow} onUndo={onUndo} onSettings={() => {}} />);
  return { assistant, onShow, onUndo };
}

test("a lost connection keeps completed work and offers Continue", () => {
  const { assistant } = setup({ items: [card] }, false,
    JSON.stringify({ code: "ai.error.network", message: "Connection failed", message_ar: "انقطع الاتصال" }));
  expect(screen.getByText("The connection to the AI service was lost. Your drawing work so far is kept. Continue picks up where Mizan stopped.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(assistant.send).toHaveBeenCalledWith("Continue from the last completed step. Keep the finished work.");
  expect(assistant.clear).not.toHaveBeenCalled();
  expect(document.querySelectorAll(".tool-card")).toHaveLength(1);
});

test("empty state offers three examples; one fills the box", () => {
  setup();
  const examples = screen.getAllByRole("listitem");
  expect(examples).toHaveLength(3);
  const first = examples[0].textContent!;
  fireEvent.click(within(examples[0]).getByRole("button"));
  expect((screen.getByLabelText("Ask Mizan, or type / for commands") as HTMLTextAreaElement).value).toBe(first);
  // No "connect an AI app" line in the empty chat; connecting lives in the header and settings.
  expect(screen.queryByText(/connect an AI app/)).toBeNull();
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
        { id: "drawing-check", title: "Review drawing check errors", status: "active" as const },
        { id: "drawing-check-review", title: "Review drawing errors: rule:[7]", status: "done" as const },
        { id: "d", title: "D", status: "skipped" as const },
      ] }] },
      busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn(),
    };
    render(<LanguageProvider><AssistantPanel showTechnicalDetails assistant={assistant} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} /></LanguageProvider>);
    expect(screen.getByRole("region", { name: ar["ai.plan"] })).toBeTruthy();
    for (const key of ["ai.plan.pending", "ai.plan.active", "ai.plan.done", "ai.plan.skipped"] as const) expect(screen.getByText(ar[key])).toBeTruthy();
    expect(document.documentElement.dir).toBe("rtl");
    expect(within(screen.getByRole("region", { name: ar["ai.plan"] })).getAllByText(ar["ai.reviewDrawingErrors"])).toHaveLength(2);
    expect(document.querySelector(".mizan-plan details")?.hasAttribute("open")).toBe(false);
    expect(document.querySelector(".mizan-plan pre")?.textContent).toBe("Review drawing errors: rule:[7]");
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
  expect(screen.getByText("now").closest("strong")).toBeTruthy();
  expect(screen.getByText("ai").closest("code")).toBeTruthy();
  // Markup in the model's text stays text.
  expect(screen.getByText(/two/).closest("li")?.textContent).toBe("two <b>x</b>");
  expect(document.querySelector(".msg b")).toBeNull();
  expect(screen.getByText("Measured area")).toBeTruthy();
  expect(document.querySelector(".technical-details")).toBeNull();
  fireEvent.click(screen.getByText("Show"));
  expect(onShow).toHaveBeenCalledWith(card);
  fireEvent.click(screen.getByText("Undo"));
  expect(onUndo).toHaveBeenCalledWith(card);
});

test("tool cards name their objects and layers without exposing command labels", () => {
  setup({
    items: [
      { ...card, id: "line", label: "draw.line", activity: { category: "draw", effect: "edit", detail: { key: "activity.line.draw", params: { what: "line", n: 4, layer: "A-WALL" } } } },
      { ...card, id: "ellipse", label: "draw.ellipse", activity: { category: "draw", effect: "edit", detail: { key: "activity.line.draw", params: { what: "ellipse", n: 1 } } } },
      { ...card, id: "read", name: "qiyas_read", label: null, activity: { category: "drawing", effect: "read", detail: { key: "activity.line.read", params: { what: "layer" } } }, mutating: false },
    ],
  });
  expect(screen.getByText("Drew 4 lines on A-WALL")).toBeTruthy();
  expect(screen.getByText("Drew 1 ellipse")).toBeTruthy();
  expect(screen.getByText("Read the drawing's layers")).toBeTruthy();
  expect(document.querySelector(".tool-card")?.getAttribute("title")).toBeNull();
});

test("Undo is only offered on the newest change, and not while busy", () => {
  const older = { ...card, id: "t0" };
  setup({ items: [older, card] });
  expect(screen.getAllByText("Undo")).toHaveLength(1);
  cleanup();
  setup({ items: [card] }, true);
  expect(screen.queryByText("Undo")).toBeNull();
  expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy();
  // No "Working…" line in the thread (founder, 29 Oct): the head's mark, the step cards and Stop say it.
  expect(screen.queryByText("Working…")).toBeNull();
  expect(document.querySelector(".assistant-head .mizan-mark .mm-pen line")).toBeTruthy();
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
  expect(screen.getByRole("alert").textContent).toBe("Mizan crashed; start a new conversation.Settings");
  fireEvent.click(screen.getByText("Start a new conversation"));
  expect(assistant.clear).toHaveBeenCalled();
  expect(screen.getByRole("status").textContent).toBe("");
});

test("Enter sends and empties the box; Shift+Enter does not send", async () => {
  const { assistant } = setup();
  const box = screen.getByLabelText("Ask Mizan, or type / for commands") as HTMLTextAreaElement;
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

test("Log is outside the chat header and support details load on demand", async () => {
  aiLog.mockResolvedValue({ path: "/cfg/logs/agent.log", text: "12:00 ERROR: diagnostic" });
  setup(EMPTY, false, "The AI service answered 400: raw diagnostic");
  expect(screen.queryByRole("button", { name: "Log" })).toBeNull();
  expect(screen.getByRole("alert").textContent).not.toContain("raw diagnostic");
  render(<SupportLog />);
  const details = document.querySelector(".support-log") as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event("toggle"));
  await waitFor(() => expect(screen.getByLabelText("Log").textContent).toContain("diagnostic"));
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
  expect(md.querySelector(".md-link")?.getAttribute("title")).toBeNull();
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
  const box = screen.getByLabelText("Ask Mizan, or type / for commands");
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
  const box = screen.getByLabelText("Ask Mizan, or type / for commands");
  const files = [1, 2, 3, 4, 5].map((n) => new File(["x"], `p${n}.png`, { type: "image/png" }));
  fireEvent.paste(box, { clipboardData: { files } });
  await waitFor(() => expect(screen.getAllByAltText("Picture")).toHaveLength(4));
  expect(screen.getByRole("alert").textContent).toBe("At most 4 pictures in one message");
  expect((screen.getByRole("button", { name: "Attach pictures or PDFs" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a user's message shows what went with it", () => {
  setup({
    items: [{ kind: "user", text: "And this?", pointed: ["Selection · 2"], images: ["data:image/png;base64,QUJD"] }],
  });
  expect(screen.getByText("Selection · 2")).toBeTruthy();
  expect(screen.getByAltText("Picture").getAttribute("src")).toBe("data:image/png;base64,QUJD");
});

test("an awaiting plan offers Approve and Change only until the next message", () => {
  const plan = { kind: "plan" as const, phases: [{ id: "walls", title: "Walls", status: "pending" as const }], awaitingApproval: true };
  const { assistant } = setup({ items: [{ kind: "user", text: "Draw" }, plan] });
  fireEvent.click(screen.getByRole("button", { name: "Change" }));
  expect(document.activeElement).toBe(screen.getByRole("textbox"));
  fireEvent.click(screen.getByRole("button", { name: "Approve" }));
  expect(assistant.send).toHaveBeenCalledWith("Approved, continue.");
  cleanup();
  setup({ items: [plan, { kind: "user", text: "Approved, continue." }] });
  expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
  cleanup();
  setup({ items: [plan] }, true);
  expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
});

test("restored pictures are placeholders and old tool cards have no Undo action", () => {
  setup({ items: [
    { kind: "user", text: "This drawing", images: ["[picture removed]"] },
    { ...card, restored: true },
  ] });
  expect(screen.getByText("[picture removed]")).toBeTruthy();
  expect(screen.queryByRole("img", { name: "Picture" })).toBeNull();
  expect(screen.queryByText("Undo")).toBeNull();
});

test("a pending resume cannot send, approve, undo, or start another chat", () => {
  const value = assistant({ transcript: { items: [card] } });
  const history: import("./ConversationHistory").ConversationHistoryState = {
    chats: [], drawing: null, latest: null, pending: true, error: null,
    resume: vi.fn(), remove: vi.fn(),
  };
  render(<AssistantPanel assistant={value} history={history} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Continue" } });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  expect(value.send).not.toHaveBeenCalled();
  expect((screen.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "New chat" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});


test("PDF attachments are retained with page provenance and sent without a drawing mutation", async () => {
  aiAttachPdf.mockResolvedValue({ name: "schedule.pdf", path: "/local/attachments/schedule.pdf", pages: 3 });
  const { assistant } = setup();
  const file = new File(["%PDF-test"], "schedule.pdf", { type: "application/pdf" });
  Object.defineProperty(file, "arrayBuffer", { value: async () => new TextEncoder().encode("%PDF-test").buffer });
  const input = document.querySelector('input[type="file"]')!;
  expect(input.getAttribute("accept")).toContain("application/pdf");
  fireEvent.change(input, { target: { files: [file] } });
  await screen.findByText("schedule.pdf · 3 pages");
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalledWith('[PDF attachment: "schedule.pdf"; 3 pages; path: "/local/attachments/schedule.pdf"]'));
  expect(assistant.send.mock.calls[0][0]).toContain("/local/attachments/schedule.pdf");
  await waitFor(() => expect(screen.queryByText("schedule.pdf · 3 pages")).toBeNull());
});

 test("support details are hidden by default and available when explicitly enabled", () => {
  setup({ items: [card] });
  expect(document.querySelector("details.technical-details")).toBeNull();
  cleanup();
  setup({ items: [card] }, false, null, true);
  expect(document.querySelector("details.technical-details pre")?.textContent).toContain(card.summary);
  expect(document.querySelector("details.technical-details")?.hasAttribute("open")).toBe(false);
});

test("recovered failures give one calm line per turn; unresolved errors and support records remain", () => {
  const failed = {...card, status: "error" as const, summary: "First diagnostic"};
  const items: Transcript["items"] = [
    {kind: "user", text: "First job"}, failed, {...failed, id: "f2", summary: "Second diagnostic"}, card,
    {kind: "user", text: "Second job"}, {...failed, id: "f3"}, card,
    {kind: "user", text: "Last job"}, {...failed, id: "unresolved"},
  ];
  setup({items});
  expect(screen.getAllByText("A problem was resolved.")).toHaveLength(2);
  expect(document.querySelectorAll(".tool-card.error")).toHaveLength(1);
  expect(document.querySelector(".recovery-summary")?.textContent).not.toContain("diagnostic");
  cleanup();
  setup({items}, false, null, true);
  expect(document.querySelector(".recovery-summary pre")?.textContent).toContain("First diagnostic");
  expect(document.querySelector(".recovery-summary pre")?.textContent).toContain("Second diagnostic");
});

test("running recovery stays one calm line and becomes an unresolved card if the turn stops", () => {
  const items: Transcript["items"] = [{kind: "user", text: "Check"}, {...card, status: "error"}, {...card, id: "f2", status: "error"}];
  setup({items}, true);
  expect(screen.getAllByText("Fixing a problem…")).toHaveLength(1);
  expect(document.querySelectorAll(".tool-card.error")).toHaveLength(0);
  cleanup();
  setup({items});
  expect(document.querySelectorAll(".tool-card.error")).toHaveLength(2);
});


test("a later successful read does not hide an unresolved edit", () => {
  setup({items: [{kind: "user", text: "Measure the wall"}, {...card, status: "error"}, {...card, name: "qiyas_read", label: "query", mutating: false}]});
  expect(document.querySelectorAll(".tool-card.error")).toHaveLength(1);
  expect(document.querySelector(".recovery-summary")).toBeNull();
});

test("open findings remain visible beside a completed answer without technical details", () => {
  setup({ items: [
    { kind: "assistant", text: "The drawing is ready for review.", finished: true },
    { kind: "open_findings", findings: [{ rule: "text_overlaps_text", message: "Entities 2 and 3 overlap", handles: [2, 3] }], revision: { document_revision: 7 } },
  ] });
  expect(screen.getByText("The drawing is ready for review.")).toBeTruthy();
  expect(screen.getByText("Open drawing findings")).toBeTruthy();
  expect(screen.getByText("Text overlaps other text. (1)")).toBeTruthy();
  expect(screen.queryByText("Entities 2 and 3 overlap")).toBeNull();
});


test("composer drafts belong to their drawing", () => {
  const assistant = { transcript: EMPTY, busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn() };
  const panel = (workspace: number) => <AssistantPanel assistant={{ ...assistant, workspace }} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />;
  const { rerender } = render(panel(1));
  fireEvent.change(screen.getByLabelText("Ask Mizan, or type / for commands"), { target: { value: "First drawing draft" } });
  rerender(panel(2));
  expect((screen.getByLabelText("Ask Mizan, or type / for commands") as HTMLTextAreaElement).value).toBe("");
  fireEvent.change(screen.getByLabelText("Ask Mizan, or type / for commands"), { target: { value: "Second drawing draft" } });
  rerender(panel(1));
  expect((screen.getByLabelText("Ask Mizan, or type / for commands") as HTMLTextAreaElement).value).toBe("First drawing draft");
});

test("a run of identical reads shows as one line with its count (note 18)", () => {
  const read = (id: string): ToolCardItem => ({ ...card, id, label: "records", mutating: false, handles: [], activity: { category: "drawing", effect: "read", detail: { key: "activity.line.read", params: { what: "wall", n: 37, layer: "A-WALL" } } }, summary: "" });
  setup({ items: [{ kind: "user", text: "Look" }, read("a"), read("b"), read("c"), read("d")] });
  expect(document.querySelectorAll(".tool-card")).toHaveLength(1);
  expect(document.querySelector(".tool-name")?.textContent).toBe("Read 37 walls on A-WALL (4 times)");
});

test("running activities use the present and plan updates name the completed steps", () => {
  setup({ items: [
    { ...card, status: "running", activity: { category: "draw", effect: "edit", detail: { key: "activity.line.draw", params: { n: 4, what: "wall", layer: "A-WALL" } } } },
    { ...card, id: "plan", label: "plan", mutating: false, handles: [], activity: { category: "drawing", effect: "view", detail: { key: "activity.line.plan", params: { n: 3, total: 7 } } } },
  ] });
  expect(screen.getByText("Drawing 4 walls on A-WALL…")).toBeTruthy();
  expect(screen.getByText("Updated the plan: 3 of 7 steps done")).toBeTruthy();
});

test("an option is chosen on the question itself, not in the box, and one question's choice sends at once", async () => {
  const question = { kind: "question" as const, questions: [{ id: "q", question: "Which country?", allow_other: true, options: [{ label: "Egypt", recommended: true }, { label: "UAE" }] }] };
  const { assistant } = setup({ items: [{ kind: "user", text: "Build it" }, question] });
  fireEvent.click(screen.getByRole("button", { name: /UAE/ }));
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("");
  await waitFor(() => expect(assistant.send).toHaveBeenCalledWith("UAE"));
});

const three = { kind: "question" as const, questions: ["q1", "q2", "q3"].map((id, n) => ({
  id, question: `Question ${n + 1}?`, allow_other: true, options: [{ label: `Yes ${n + 1}` }, { label: `No ${n + 1}` }],
})) };

test("several questions: choices are marked and go by themselves once every question has one", async () => {
  const { assistant } = setup({ items: [{ kind: "user", text: "Build it" }, three] });
  fireEvent.click(screen.getByRole("button", { name: /Yes 1/ }));
  fireEvent.click(screen.getByRole("button", { name: /No 2/ }));
  expect(screen.getByRole("button", { name: /Yes 1/ }).getAttribute("aria-pressed")).toBe("true");
  expect(document.querySelectorAll(".qopt.picked")).toHaveLength(2);
  expect(assistant.send).not.toHaveBeenCalled();
  // The same option again takes it off; another one replaces it.
  fireEvent.click(screen.getByRole("button", { name: /No 2/ }));
  expect(document.querySelectorAll(".qopt.picked")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: /Yes 2/ }));
  fireEvent.click(screen.getByRole("button", { name: /Yes 3/ }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalledWith("1. Yes 1\n2. Yes 2\n3. Yes 3"));
});

test("several questions: what is typed in the box answers the one left open, sent with Enter", async () => {
  const { assistant } = setup({ items: [{ kind: "user", text: "Build it" }, three] });
  fireEvent.click(screen.getByRole("button", { name: /Yes 1/ }));
  fireEvent.click(screen.getByRole("button", { name: /No 2/ }));
  const box = screen.getByRole("textbox");
  fireEvent.change(box, { target: { value: "The street is on the north" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(assistant.send).toHaveBeenCalledWith("1. Yes 1\n2. No 2\n3. The street is on the north"));
});


test("an alef carrying tanween is written with the tanween first in the box, so the word stays on the line", () => {
  setup();
  const box = screen.getByRole("textbox") as HTMLTextAreaElement;
  fireEvent.change(box, { target: { value: "مستطيلاً ومدخلاً" } });
  expect(box.value).toBe("مستطيلًا ومدخلًا");
});

test("an Arabic conversation shows Mizan's steps and its waiting line in Arabic in an English window", () => {
  const user = { kind: "user" as const, text: "ارسم فيلا صغيرة" };
  const lookup: ToolCardItem = { ...card, id: "t2", label: "commands", mutating: false, handles: [],
    activity: { category: "drawing", effect: "read", detail: { key: "activity.line.lookup", params: { what: "command", name: "drafting_knowledge, space_check" } } } };
  setup({ items: [user, lookup] }, true);
  const line = document.querySelector(".tool-card")!;
  expect(line.getAttribute("lang")).toBe("ar");
  expect(line.getAttribute("dir")).toBe("rtl");
  expect(line.textContent).toContain("بحث عن طريقة استخدام");
  // Never the commands' own names.
  expect(line.textContent).not.toMatch(/drafting_knowledge|space_check/);
  expect(document.querySelector(".working")).toBeNull();
  // The window's own words stay in its language.
  expect(screen.getByRole("textbox").getAttribute("placeholder")).toBe("Ask Mizan, or type / for commands");
});

test("answers chosen on a question with tanween send the word as written", async () => {
  const asked = { kind: "question" as const, questions: [{ id: "q1", question: "ما شكل الأرض؟",
    options: [{ label: "افترض مسطحاً مستطيلاً" }, { label: "لا توجد بيانات" }], allow_other: true }] };
  const { assistant } = setup({ items: [{ kind: "user", text: "ارسم" }, asked] });
  fireEvent.click(screen.getByRole("button", { name: /افترض/ }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalledWith("افترض مسطحاً مستطيلاً"));
});

test("the latest airport plan docks outside the thread with the only live approval actions", () => {
  const oldPlan = { kind: "plan" as const, phases: [
    { id: "tower", title: "Tower structure", status: "done" as const },
    { id: "boq", title: "Tower BOQ", status: "done" as const },
  ] };
  const airport = { kind: "plan" as const, awaitingApproval: true, phases: [
    { id: "terminal", title: "Airport terminal quantities", status: "pending" as const },
    { id: "airside", title: "Airport airside infrastructure", status: "pending" as const },
  ] };
  const { assistant } = setup({ items: [oldPlan, { kind: "user", text: "Plan the airport take-off" }, airport] });
  const dock = document.querySelector<HTMLElement>(".plan-dock")!;
  const log = document.querySelector<HTMLElement>(".assistant-log")!;
  expect(log.contains(dock)).toBe(false);
  expect(dock.compareDocumentPosition(screen.getByRole("textbox")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(within(dock).getByRole("button", { name: "Plan 0/2" }).getAttribute("aria-expanded")).toBe("true");
  expect(within(dock).getByText("Airport terminal quantities")).toBeTruthy();
  expect(within(dock).queryByText("Tower structure")).toBeNull();
  expect(log.querySelectorAll(".mizan-plan")).toHaveLength(2);
  expect(within(log).queryByRole("button", { name: "Approve" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Change" }));
  expect(document.activeElement).toBe(screen.getByRole("textbox"));
  fireEvent.click(screen.getByRole("button", { name: "Approve" }));
  expect(assistant.send).toHaveBeenCalledExactlyOnceWith("Approved, continue.");
  const original = within(log).getByText("Airport terminal quantities").closest("section")!;
  const scroll = vi.spyOn(original, "scrollIntoView");
  fireEvent.click(screen.getByRole("button", { name: "Show in chat" }));
  expect(scroll).toHaveBeenCalledExactlyOnceWith({ block: "nearest", behavior: "instant" });
});

test("a completed hospital dock disappears on the next user message and keeps its history card", () => {
  const hospital = { kind: "plan" as const, phases: [
    { id: "structure", title: "Hospital structure", status: "done" as const },
    { id: "mep", title: "Hospital MEP alternatives", status: "skipped" as const },
  ] };
  const props = { onShow() {}, onUndo() {}, onSettings() {} };
  const view = render(<AssistantPanel assistant={assistant({ transcript: { items: [hospital] } })} {...props} />);
  expect(screen.getByRole("button", { name: "Plan done 2/2" })).toBeTruthy();
  view.rerender(<AssistantPanel assistant={assistant({ transcript: { items: [hospital, { kind: "user", text: "Review the airport" }] } })} {...props} />);
  expect(document.querySelector(".plan-dock")).toBeNull();
  expect(screen.getByRole("region", { name: "Plan" })).toBeTruthy();
  view.rerender(<AssistantPanel assistant={assistant()} {...props} />);
  expect(document.querySelector(".assistant-empty")).toBeTruthy();
  expect(document.querySelector(".plan-dock")).toBeNull();
});


test.each(["wheel", "ArrowUp", "PageUp", "Home", "ShiftSpace"])("%s stops following before the reader is 32 pixels away", (intent) => {
  const box = { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 };
  const first: Transcript = { items: [{ kind: "assistant", text: "Reading", finished: false }] };
  const view = render(<AssistantPanel assistant={assistant({ transcript: first })} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  const log = document.querySelector<HTMLElement>(".assistant-log")!;
  layout(log, box);
  fireEvent.scroll(log);
  if (intent === "wheel") fireEvent.wheel(log, { deltaY: -1 });
  else fireEvent.keyDown(log, { key: intent === "ShiftSpace" ? " " : intent, shiftKey: intent === "ShiftSpace" });
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeTruthy();
  box.scrollTop = 599;
  fireEvent.scroll(log);
  box.scrollHeight = 1100;
  view.rerender(<AssistantPanel assistant={assistant({ transcript: { items: [{ kind: "assistant", text: "Reading more", finished: false }] } })} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  expect(box.scrollTop).toBe(599);
  expect(log.scrollTo).not.toHaveBeenCalled();
});
