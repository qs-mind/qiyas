// @vitest-environment jsdom
/** `useDrawings`' recovery flows with the core (`invoke`) and the file dialogs faked: what reaches
 *  the core, what Save As is asked with, and the status bar notice. */
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { useRef, useState, type SetStateAction } from "react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { setWorkspace } from "../workspaceIpc";
import type { DrawingInfo, Recoverable } from "../api";
import { LanguageProvider, useT } from "../i18n";
import { enqueue, laterOpens, localTime } from "../shell/recoveryQueue";
import { FileDialogs } from "./Dialogs";
import { useDrawings, type Drawings } from "./useDrawings";

const VILLA = "/jobs/villa.dwg";
const RECOVERED = "/jobs/villa (recovered).dwg";
const UNTITLED = "/home/qs/.config/com.qsmind.qiyas/untitled/Untitled-1.dwg";

const core = vi.hoisted(() => ({
  calls: [] as string[],
  busy: false,
  active: 2,
  keepBusy: false,
  /** What `discard_autosave` refuses with (a live lock), or null. */
  refuseDiscard: null as string | null,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    core.calls.push(`${cmd} ${JSON.stringify(args ?? null)}`);
    switch (cmd) {
      case "recents":
        return recoverable.recents.map((path) => ({ path }));
      case "untitled_autosaves":
        return recoverable.untitled;
      case "recoverable_drawings":
        return recoverable.found;
      case "take_open_requests":
        return [];
      case "get_settings":
        return { autosave_minutes: 0 };
      case "recover_autosave":
        return info(RECOVERED);
      case "open_drawing":
        return info(args!.path as string);
      case "new_drawing":
        return { ...info(UNTITLED), untitled: true };
      case "tabs":
        return { tabs: [{ id: 2, path: VILLA, title: "villa.dwg", dirty: true, busy: core.busy }], active: core.active };
      case "drawing_info":
        return info(VILLA);
      case "activate_tab":
        return info(VILLA);
      case "ai_cancel":
        if (!core.keepBusy) core.busy = false;
        return null;
      case "close_tab":
        return { tabs: [], active: null };
      case "discard_autosave":
        if (core.refuseDiscard) throw core.refuseDiscard;
        return null;
      default:
        return null;
    }
  }),
}));
const events = vi.hoisted(() => ({ handlers: new Map<string, (e: { payload: unknown }) => void>() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (e: { payload: unknown }) => void) => {
    events.handlers.set(name, handler);
    return () => {};
  }),
}));
vi.mock("@tauri-apps/api/path", () => ({ documentDir: async () => "/home/qs/Documents" }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }),
}));
const recoverable = vi.hoisted(() => ({ recents: [] as string[], found: [] as unknown[], untitled: [] as string[] }));
const dialogs = vi.hoisted(() => ({ save: vi.fn(async (_options: { defaultPath?: string }): Promise<string | null> => null) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
  save: dialogs.save,
  ask: vi.fn(async () => true),
  message: vi.fn(async () => "No"),
}));

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(() => {
  cleanup();
  core.calls.length = 0;
  localStorage.removeItem("qiyas.files.lastFolder");
  core.refuseDiscard = null;
  core.busy = false; core.active = 2; core.keepBusy = false; vi.useRealTimers();
  recoverable.recents = [];
  recoverable.found = [];
  recoverable.untitled = [];
  dialogs.save.mockClear();
});

function info(path: string) {
  return {
    path,
    sheets: [{ name: "Model", block: "*Model_Space", is_model: true }],
    layers: [],
    problems: [],
    arabic_reversed: false,
    xrefs: [],
    sheet: "*Model_Space",
    hidden_layers: [],
  } as unknown as DrawingInfo;
}

const item = (more: Partial<Recoverable> = {}): Recoverable => ({
  drawing: VILLA,
  autosave: `${VILLA}.sv$`,
  saved_at: 1_760_000_000,
  drawing_at: 1_759_999_000,
  crashed: true,
  locked_elsewhere: null,
  older_copies: [],
  ...more,
});

/** `useDrawings` with the window around it reduced to state and no-ops; nothing open at first. */
function useWindow(notices: string[], viewport?: Parameters<typeof useDrawings>[0]["viewport"]) {
  const [info, setInfo] = useState<DrawingInfo | null>(null);
  const switchGen = useRef(0);
  const autosaveOff = useRef(false);
  const preset = useRef(null);
  const lastPoint = useRef(null);
  const nothing = () => {};
  const none = async () => {};
  return useDrawings({
    t: useT(),
    switchGen,
    info,
    setInfo,
    setError: nothing,
    setNotice: (n: SetStateAction<string | null>) => {
      if (typeof n === "string") notices.push(n);
    },
    run: async (step: () => Promise<void>) => step(),
    settingsOpen: false,
    fileTabs: [],
    setFileTabs: nothing,
    setActiveTab: nothing,
    refreshTabs: none,
    autosaveOff,
    autosave: none,
    setDraftingState: nothing,
    sheets: { reset: nothing, clear: nothing },
    setArabicReversed: nothing,
    setSheetBlock: nothing,
    setHidden: nothing,
    redraw: none,
    takeoffState: { reset: nothing, clear: nothing },
    refreshTakeoffAndBoq: none,
    attach: { reset: nothing },
    preset,
    lastPoint,
    measure: { cancel: nothing },
    viewport,
  });
}

async function launch(notices: string[] = []) {
  const view = renderHook(() => useWindow(notices), { wrapper: LanguageProvider });
  // The launch effects (recents, settings, open requests) settle.
  await act(async () => {});
  return view;
}

test("at launch an untitled drawing's crash autosave is asked about with the recent drawings", async () => {
  recoverable.recents = [VILLA];
  recoverable.untitled = [UNTITLED];
  await launch();
  expect(core.calls).toContain(`recoverable_drawings {"paths":["${VILLA}","${UNTITLED}"]}`);
});

test("Mizan's requests: a drawing it put on screen is shown; closing unsaved work asks, keeping its conversation", async () => {
  const { result } = await launch();
  await act(async () => events.handlers.get("drawing:shown")!({ payload: null }));
  expect(core.calls).toContain('drawing_info {"workspace":2}');

  core.calls.length = 0;
  // Don't Save (the faked dialog answers No): the tab closes, the conversation goes on.
  await act(async () => events.handlers.get("drawing:close")!({ payload: 2 }));
  expect(core.calls).toContain(`activate_tab {"id":2,"keepConversation":true}`);
  expect(core.calls).toContain(`close_tab {"id":2,"force":true,"keepConversation":true}`);
  expect(result.current.newOpen).toBe(false);
});

test("New makes an untitled drawing in the units chosen; its Save starts in Documents offering a real name (the date when there is nothing to name it by), never saves over the placeholder", async () => {
  const { result } = await launch();
  await act(() => result.current.newDrawing("cm"));
  expect(core.calls).toContain(`new_drawing {"units":"cm"}`);
  expect(result.current.newOpen).toBe(false);

  await act(() => result.current.saveFile());
  expect(dialogs.save).toHaveBeenCalledTimes(1);
  expect(dialogs.save.mock.calls[0][0].defaultPath).toMatch(/^\/home\/qs\/Documents\/Qiyas drawing \d{4}-\d{2}-\d{2}\.dwg$/);
  expect(core.calls.some((c) => c.startsWith("save_drawing "))).toBe(false);

  dialogs.save.mockResolvedValueOnce("/jobs/new.dwg");
  await act(() => result.current.saveFile());
  expect(core.calls).toContain(`save_drawing_as {"path":"/jobs/new.dwg","workspace":2}`);
});

test("Recover at launch, nothing open: Save As is offered with the recovered drawing's name; cancelled, the notice says where it came from", async () => {
  const notices: string[] = [];
  const { result } = await launch(notices);
  act(() => result.current.setRecoveries((q) => enqueue(q, [item()], null, true)));

  await act(() => result.current.recoverFrom(result.current.recovery!, VILLA));

  expect(core.calls).toContain(`recover_autosave {"path":"${VILLA}"}`);
  expect(dialogs.save).toHaveBeenCalledTimes(1);
  expect(dialogs.save.mock.calls[0][0].defaultPath).toBe(RECOVERED);
  expect(notices.at(-1)).toBe(`Recovered from autosave of ⁨${localTime(1_760_000_000, "en")}⁩`);
  expect(result.current.recovery).toBeNull();
});

test("Recover then a real Save As: the notice says both where it came from and where it went", async () => {
  const notices: string[] = [];
  const { result } = await launch(notices);
  act(() => result.current.setRecoveries((q) => enqueue(q, [item()], null, true)));
  dialogs.save.mockResolvedValueOnce("/jobs/villa-fixed.dwg");

  await act(() => result.current.recoverFrom(result.current.recovery!, VILLA));

  expect(core.calls).toContain(`save_drawing_as {"path":"/jobs/villa-fixed.dwg","workspace":2}`);
  expect(notices.at(-1)).toBe(
    `Recovered from autosave of ⁨${localTime(1_760_000_000, "en")}⁩; saved to ⁨/jobs/villa-fixed.dwg⁩`,
  );
});

test("an older copy is recovered by its own path, and only it leaves the row", async () => {
  const notices: string[] = [];
  const { result } = await launch(notices);
  const older = { path: `${VILLA}.sv$.crash-20261010-090000`, saved_at: 1_760_080_000, kind: "crash" as const };
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ crashed: false, older_copies: [older] })], VILLA)));

  await act(() => result.current.recoverFrom(result.current.recovery!, VILLA, older.path));

  expect(core.calls).toContain(`recover_autosave {"path":"${older.path}"}`);
  expect(notices.at(-1)).toContain(localTime(1_760_080_000, "en"));
  expect(result.current.recovery?.items.map((i) => [i.drawing, i.older_copies.length])).toEqual([[VILLA, 0]]);
});

test("Recover acts on the question it was clicked in, even when the launch question came in ahead", async () => {
  const { result } = await launch();
  act(() => result.current.setRecoveries((q) => enqueue(q, [item()], VILLA)));
  const asked = result.current.recovery!;
  // The launch question arrives while the click is on its way.
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ drawing: "/jobs/tower.dwg" })], null, true)));

  await act(() => result.current.recoverFrom(asked, VILLA));

  expect(result.current.recovery?.items.map((i) => i.drawing)).toEqual(["/jobs/tower.dwg"]);
  act(() => result.current.setRecoveries((q) => q.slice(1)));
  expect(result.current.recovery).toBeNull();
});

test("Recover with another drawing open: Save As suggests the recovered drawing, not the one before", async () => {
  const { result } = await launch();
  await act(() => result.current.openPath("/jobs/tower.dwg", false));
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ crashed: false })], null, true)));

  await act(() => result.current.recoverFrom(result.current.recovery!, VILLA));

  expect(dialogs.save.mock.calls[0][0].defaultPath).toBe(RECOVERED);
});

test("Open anyway: a drawing open in another Qiyas opens here once, its autosave left alone", async () => {
  const { result } = await launch();
  const locked = item({ crashed: false, locked_elsewhere: { pid: 4242, host: "site-office-pc", started_at: 1 } });
  act(() => result.current.setRecoveries((q) => enqueue(q, [locked], VILLA)));
  core.calls.length = 0;

  await act(() => result.current.openAnyway(result.current.recovery!, VILLA));

  expect(core.calls.filter((c) => c.startsWith("open_drawing"))).toEqual([`open_drawing {"path":"${VILLA}"}`]);
  expect(core.calls.some((c) => c.startsWith("discard_autosave"))).toBe(false);
  expect(result.current.recovery).toBeNull();
});

test("a refused Discard reaches the dialog as its error: the question stays and nothing opens", async () => {
  const { result } = await launch();
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ crashed: false })], VILLA)));
  core.refuseDiscard = "/jobs/villa.dwg is open in another Qiyas (process 4242 on x): its autosave stays";
  core.calls.length = 0;

  await act(() => expect(result.current.discardFrom(result.current.recovery!, VILLA)).rejects.toBe(core.refuseDiscard));

  expect(core.calls).toEqual([`discard_autosave {"path":"${VILLA}"}`]);
  expect(result.current.recovery?.items.map((i) => i.drawing)).toEqual([VILLA]);
});

test("at launch a recent drawing open in another Qiyas, or one that crashed clean, is not asked about; a crash autosave is", async () => {
  recoverable.found = [
    item({ drawing: "/jobs/a.dwg" }),
    item({ drawing: "/jobs/clean.dwg", autosave: null, saved_at: null }),
    item({ drawing: "/jobs/b.dwg", autosave: null, saved_at: null, crashed: false, locked_elsewhere: { pid: 7, host: "x", started_at: 1 } }),
  ];
  recoverable.recents = ["/jobs/a.dwg", "/jobs/clean.dwg", "/jobs/b.dwg"];
  const { result } = await launch();
  expect(result.current.recovery?.items.map((i) => i.drawing)).toEqual(["/jobs/a.dwg"]);
});

/** The window's file dialogs over the hook, as `App` renders them; `grab` gets the hook's latest. */
function Window({ grab }: { grab(drawings: Drawings): void }) {
  const drawings = useWindow([]);
  grab(drawings);
  return <FileDialogs drawings={drawings} />;
}

async function openWithLater(found: Recoverable[]) {
  let latest: Drawings | null = null;
  render(
    <LanguageProvider>
      <Window grab={(d) => (latest = d)} />
    </LanguageProvider>,
  );
  await act(async () => {});
  recoverable.found = found;
  await act(() => latest!.openPath(VILLA));
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: "Later" }));
  await act(async () => {});
}

test("Later on an open's question opens the drawing itself and discards nothing", async () => {
  await openWithLater([item()]);
  expect(core.calls.filter((c) => /^(open_drawing|discard_autosave)/.test(c))).toEqual([
    `open_drawing {"path":"${VILLA}"}`,
  ]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("Later on a drawing open in another Qiyas opens nothing", async () => {
  await openWithLater([
    item({ crashed: false, autosave: null, saved_at: null, locked_elsewhere: { pid: 7, host: "x", started_at: 1 } }),
  ]);
  expect(core.calls.filter((c) => /^(open_drawing|discard_autosave)/.test(c))).toEqual([]);
});

const crashCopy = (stamp: string) => ({ path: `${VILLA}.sv$.crash-${stamp}`, saved_at: 1_760_080_000, kind: "crash" as const });
const opens = () => core.calls.filter((c) => c.startsWith("open_drawing"));

test("an open's copies-only question: discarding the last copy opens the drawing asked for", async () => {
  const { result } = await launch();
  const copies = [crashCopy("2"), crashCopy("1")];
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ autosave: null, saved_at: null, crashed: false, older_copies: copies })], VILLA)));
  core.calls.length = 0;

  await act(() => result.current.discardFrom(result.current.recovery!, VILLA, copies[0].path));
  expect(opens()).toEqual([]);
  await act(() => result.current.discardFrom(result.current.recovery!, VILLA, copies[1].path));

  expect(core.calls.filter((c) => c.startsWith("discard_autosave"))).toEqual(
    copies.map((c) => `discard_autosave {"path":"${c.path}"}`),
  );
  expect(opens()).toEqual([`open_drawing {"path":"${VILLA}"}`]);
  expect(result.current.recovery).toBeNull();
});

test("Discard of the main autosave keeps the older copies listed; the drawing opens when the last is dealt with", async () => {
  const { result } = await launch();
  const copies = [crashCopy("1")];
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ older_copies: copies })], VILLA)));
  core.calls.length = 0;

  await act(() => result.current.discardFrom(result.current.recovery!, VILLA));
  expect(result.current.recovery?.items.map((i) => [i.autosave, i.older_copies.length])).toEqual([[null, 1]]);
  expect(opens()).toEqual([]);

  await act(() => result.current.discardFrom(result.current.recovery!, VILLA, copies[0].path));
  expect(opens()).toEqual([`open_drawing {"path":"${VILLA}"}`]);
  expect(result.current.recovery).toBeNull();
});

test("Recover of the main autosave keeps the older copies listed, and Later no longer opens the original", async () => {
  const { result } = await launch();
  act(() => result.current.setRecoveries((q) => enqueue(q, [item({ older_copies: [crashCopy("1")] })], VILLA)));

  await act(() => result.current.recoverFrom(result.current.recovery!, VILLA));

  expect(result.current.recovery?.items.map((i) => [i.autosave, i.older_copies.length])).toEqual([[null, 1]]);
  expect(laterOpens(result.current.recovery!)).toBeNull();
});

test("continuing a chat waits for autosave recovery before restoring the conversation", async () => {
  const tower = "/jobs/tower.dwg";
  recoverable.found = [item({ drawing: tower, autosave: `${tower}.sv$` })];
  const { result } = await launch();
  let finished: boolean | undefined;
  let pending!: Promise<boolean>;
  await act(async () => {
    pending = result.current.prepareConversation({ id: "saved", title: "Tower", drawing: tower, drawing_exists: true, updated: 1, turns: 1 });
    void pending.then((value) => { finished = value; });
  });
  expect(result.current.recovery?.then).toBe(tower);
  expect(finished).toBeUndefined();
  expect(core.calls.some((call) => call.startsWith("open_drawing"))).toBe(false);
  await act(async () => { result.current.dismissRecovery(result.current.recovery!); await pending; });
  expect(core.calls).toContain(`open_drawing {"path":"${tower}"}`);
  expect(finished).toBe(true);
});

test("a missing chat drawing continues on screen with a notice and an open tab keeps the conversation", async () => {
  const notices: string[] = [];
  const { result } = await launch(notices);
  const chat = { id: "saved", title: "Villa", drawing: VILLA, drawing_exists: true, updated: 1, turns: 1 };
  await act(async () => { expect(await result.current.prepareConversation(chat)).toBe(true); });
  expect(core.calls.some((call) => call.startsWith("open_drawing"))).toBe(false);
  await act(async () => { expect(await result.current.prepareConversation({ ...chat, drawing: null, drawing_exists: false })).toBe(true); });
  expect(notices).toContain("The original drawing is unavailable. Continuing on the drawing on screen.");
});


test("closing a busy drawing asks first; waiting for its tool leaves tab switching usable", async () => {
  vi.useFakeTimers();
  core.busy = true; core.keepBusy = true;
  const { result } = await launch();
  const { ask } = await import("../fileDialogs");
  vi.mocked(ask).mockResolvedValueOnce(false);
  await act(async () => result.current.closeFileTab(2));
  expect(ask).toHaveBeenCalledWith("Mizan is working on this drawing: stop and close?", expect.anything());
  expect(core.calls.some(call => call.startsWith("ai_cancel"))).toBe(false);
  let close!: Promise<void>;
  act(() => { close = result.current.closeFileTab(2); });
  await act(async () => { await vi.advanceTimersByTimeAsync(50); });
  expect(core.calls).toContain('ai_cancel {"workspace":2}');
  for (let second = 0; second < 60; second++) {
    await act(async () => { expect(await result.current.switchTab(2)).toBe(true); });
    expect(core.calls.some(call => call.startsWith("close_tab"))).toBe(false);
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  }
  core.busy = false;
  await act(async () => { await vi.advanceTimersByTimeAsync(50); await close; });
  expect(core.calls.some(call => call.startsWith("close_tab"))).toBe(true);
});


test("Save As keeps its workspace when the visible tab changes during the file dialog", async () => {
  const { result } = await launch();
  await act(() => result.current.newDrawing("cm"));
  let choose!: (path: string) => void;
  dialogs.save.mockImplementationOnce(() => new Promise(resolve => { choose = resolve; }));
  let saving!: Promise<void>;
  await act(async () => { saving = result.current.saveFileAs(); });
  setWorkspace(3);
  await act(async () => { choose("/jobs/original.dwg"); await saving; });
  expect(core.calls).toContain('save_drawing_as {"path":"/jobs/original.dwg","workspace":2}');
  expect(core.calls).not.toContain('save_drawing_as {"path":"/jobs/original.dwg","workspace":3}');
  expect(core.calls).toContain('drawing_info {"workspace":2}');
});


test("a file tool's tab switch saves the outgoing canvas view to its original workspace", async () => {
  const viewport = { current: { visible: () => [[10, 20], [30, 40]] } } as unknown as Parameters<typeof useDrawings>[0]["viewport"];
  renderHook(() => useWindow([], viewport), { wrapper: LanguageProvider });
  await act(async () => {});
  core.calls.length = 0;
  core.active = 3; // The native file tool switches before notifying the webview.
  await act(async () => events.handlers.get("drawing:shown")!({ payload: null }));
  const views = core.calls.filter(call => call.startsWith("workspace_view_state ")).map(call => JSON.parse(call.slice("workspace_view_state ".length)));
  expect(views).toEqual([{ workspace: 2, context: { selection: [], region: null, view: [[10, 20], [30, 40]], picture: false } }]);
});
