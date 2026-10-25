// @vitest-environment jsdom
/** Wiring checks for the window's composition root, the kind the type checker can't make: the
 *  hooks reach each other in the right order with the right state. The core (every IPC call
 *  through `invoke`), the file dialogs and the canvas are faked; `calls` is what reached the
 *  core, in order. */
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import App from "./App";
import type { Batch } from "../viewport/batch";
import type { ConversationSummary } from "../api";

// These integration tests mount the whole shell and drive several accessible dialogs each.
// A loaded build machine can spend over five seconds in one test's DOM queries alone.
// Keep the extra budget in this file; individual async transitions still have a condition.
vi.setConfig({ testTimeout: 30_000 });
const until = (condition: () => void) => waitFor(condition, { timeout: 5_000 });

/** Shows a ribbon tab; the ones after Annotate are under "More". */
function goTab(name: string) {
  const tab = screen.queryByRole("tab", { name });
  if (tab) fireEvent.click(tab);
  else {
    fireEvent.click(document.querySelector<HTMLElement>(".ribbon-more")!);
    fireEvent.click(screen.getByRole("menuitemradio", { name }));
  }
}

const PLAN = "/d/plan.dwg";

/** What the notices say now (the toasts), the newest first: what was done and any error are
 *  toasts, the status bar says what is happening. */
const toasts = () => [...document.querySelectorAll('[data-sonner-toast]:not([data-removed="true"]) [data-title]')].map((el) => el.textContent);
/** Sonner shows a toast, and takes a replaced one away, a frame or two later. */
const expectToasts = (texts: string[]) => until(() => expect(toasts()).toEqual(texts));

const core = vi.hoisted(() => ({
  calls: [] as string[],
  workspaceCalls: [] as { cmd: string; args?: Record<string, unknown> }[],
  cursor: 0,
  /** The drawings with an autosave newer than them. */
  autosaved: new Set<string>(),
  /** What the Open dialog answers. */
  chosen: "/d/plan.dwg",
  /** The tab on screen; a script's OPEN puts tab 2 there. */
  active: 1,
  opened: false,
  scriptOpens: false,
  /** While set, run_script waits for it: the script is running. */
  scriptHold: null as Promise<void> | null,
  /** While set, open_drawing and compare wait for it: the window is opening or comparing. */
  openHold: null as Promise<void> | null,
  compareHold: null as Promise<void> | null,
  /** More of settings.json than the autosave minutes and the tool palette. */
  settings: {} as Record<string, unknown>,
  /** The block edited in place, as drawing_info says. */
  blockEdit: null as string | null,
  hasBoq: false,
  batch: null as ArrayBuffer | null,
  batchVersion: 1,
  layers: ["0"],
  undoBatch: null as ArrayBuffer | null,
  otherBatch: null as ArrayBuffer | null,
  untitled: false,
  savedConversations: [] as ConversationSummary[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd.startsWith("workspace_") || cmd === "ai_status") {
      core.workspaceCalls.push({ cmd, args });
    } else core.calls.push(`${cmd} ${JSON.stringify(args ?? null)}`);
    switch (cmd) {
      case "workspace_attach":
        return { workspace: args?.workspace ?? core.active, cursor: core.cursor, transcript: [], status: "", error: null,
          choice: { provider: "anthropic", model: "claude-opus-5-5", effort: null }, context: {} };
      case "aliases":
        return {};
      case "recents":
      case "takeoff":
      case "totals":
      case "conflicts":
      case "paper_views":
      case "measurement_methods":
        return [];
      case "ai_conversations":
        return core.savedConversations;
      case "pick_polygon":
        return [{ top: 42, entity: 42, chain: [], xref: null, kind: "Line" }];
      case "get_settings":
        return {
          provider: "anthropic",
          providers: {
            anthropic: { model: "claude-opus-5-5" },
            openai_compatible: { model: "", base_url: null },
          },
          autosave_minutes: 5,
          tool_palette: [{ kind: "command", label: "Divide in 3", value: "DIVIDE 3" }],
          ...core.settings,
        };
      case "app_dir":
        return "/opt/Qiyas";
      case "get_default_ribbon":
        return { tabs: [{ id: "home", label: "home", groups: [{ id: "file", label: "ribbon.file", buttons: [{ kind: "action", command: "open", label: "open" }] }] }] };
      case "recoverable_drawings":
        return (args!.paths as string[])
          .filter((p) => core.autosaved.has(p))
          .map((drawing) => ({ drawing, autosave: `${drawing}.sv$`, saved_at: 1 }));
      case "open_drawing":
        if (core.openHold) await core.openHold;
        core.opened = true;
        return info(args!.path as string);
      case "compare":
        if (core.compareHold) await core.compareHold;
        return [];
      case "measurement_report":
        return { items: [], warnings: [] };
      case "boq":
        return core.hasBoq ? { source: "/d/bill.xlsx", imported: "", items: [], skipped_rows: 0 } : null;
      case "drawing_info":
        return info(core.active === 2 && core.otherBatch ? "/d/other.dwg" : PLAN);
      case "get_drafting":
        return {
          osnap: true,
          snap_modes: {},
          ortho: false,
          polar: false,
          polar_step_deg: 45,
          grid: false,
          grid_spacing: 10,
          grid_snap: false,
          precision: 3,
        };
      case "batch_version":
        return core.batchVersion;
      case "tabs":
        if (!core.opened) return { tabs: [], active: null };
        return {
          tabs: [
            { id: 1, path: PLAN, title: "plan.dwg", dirty: false },
            ...(core.otherBatch ? [{ id: 2, path: "/d/other.dwg", title: "other.dwg", dirty: false }] : []),
          ],
          active: core.active,
        };
      case "activate_tab":
        core.active = args!.id as number;
        core.batch = core.otherBatch;
        core.batchVersion += 1;
        return info("/d/other.dwg");
      case "undo":
        if (!core.undoBatch) return false;
        core.batch = core.undoBatch;
        core.batchVersion += 1;
        return true;
      case "run_script":
        if (core.scriptHold) await core.scriptHold;
        if (core.scriptOpens) core.active = 2;
        return {
          ran: true,
          lines: [
            { line_no: 1, text: "COUNT DOOR", ok: true, message: "DOOR count = 3 pcs" },
            { line_no: 2, text: "MEASURE FFFF", ok: false, message: "No entity FFFF" },
          ],
          changed: true,
          took_ms: 12,
        };
      case "render_batch":
        // "Not modified": the window keeps the batch it has.
        return core.batch ?? new Uint8Array([0x51, 0x49, 0x4e, 0x4d, 0, 0, 0, 0]).buffer;
      case "pick":
        return { top: 42, entity: 42, chain: [], xref: null, kind: "Line" };
      case "paper_catalog":
        return [
          { name: "ISO A3", w_mm: 297, h_mm: 420 },
          { name: "ISO A4", w_mm: 210, h_mm: 297 },
        ];
      case "list_printers":
        return ["Office"];
      case "default_printer":
        return "Office";
      case "plot_pdf_to":
        return { path: args!.path, skipped: ["Image a.png", "Image b.png"] };
      case "plot_and_print":
        return `Sent to ${args!.printer}`;
      case "run_command": {
        const cmd = args!.cmd as { type: string; dry_run?: boolean; fix?: boolean };
        if (cmd.type === "purge")
          return {
            type: "purged",
            report: {
              dry_run: cmd.dry_run,
              layers: ["OLD"],
              blocks: [],
              styles: [],
              linetypes: [],
              dimstyles: [],
              groups: [],
              group_members: 0,
              total: 1,
              limited_reason: null,
            },
          };
        if (cmd.type === "audit") return { type: "audited", report: { fix: cmd.fix, counts: {}, repaired: 0, items: [] } };
        if (cmd.type === "list_parameters")
          return {
            type: "parameters",
            parameters: [{ id: 3, name: "d1", kind: "linear", expression: "4", value: 4, dimension: 30 }],
          };
        if (cmd.type === "list_constraints")
          return {
            type: "constraints",
            constraints: [
              { id: 1, kind: "horizontal", handles: [7], at: [2, 0], name: null, expression: null, value: null, dimension: null },
            ],
          };
        if (cmd.type === "set_parameter") return { type: "changed" };
        return { type: "added", handles: [7, 8, 9, 10] };
      }
      default:
        return null;
    }
  }),
}));
/** The core's events, by name: a test sends one with `emit`. */
const events = vi.hoisted(() => new Map<string, Set<(e: { payload: unknown }) => void>>());
const emit = (name: string, payload: unknown) => act(async () => {
  for (const handler of events.get(name) ?? []) handler({ payload });
});
const agentEvent = (event: unknown) => emit("workspace:event", { workspace: core.active, cursor: ++core.cursor, event });
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (e: { payload: unknown }) => void) => {
    const handlers = events.get(name) ?? new Set();
    handlers.add(handler); events.set(name, handlers);
    return () => { handlers.delete(handler); };
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }),
}));
const { openDialog, saveDialog } = vi.hoisted(() => ({
  openDialog: vi.fn(),
  saveDialog: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: openDialog,
  save: saveDialog,
  ask: vi.fn(async () => true),
  message: vi.fn(async () => "No"),
}));
/** The canvas needs WebGL; the fake keeps its props so a test can click the drawing. */
type CanvasProps = {
  onClick(...a: unknown[]): void;
  onZoomHint(px: number): Promise<void>;
  batch: Batch | null;
  overlay: Float64Array | null;
};
const canvas = vi.hoisted(() => ({ props: null as CanvasProps | null }));
vi.mock("../viewport/Viewport", () => ({
  Viewport: (props: CanvasProps) => {
    canvas.props = props;
    return null;
  },
}));

function info(path: string) {
  return {
    path,
    untitled: core.untitled,
    sheets: [
      { name: "Model", block: "*Model_Space", is_model: true },
      { name: "Layout1", block: "*Paper_Space", is_model: false },
    ],
    layers: core.layers.map((name) => ({ name, color: [255, 255, 255], off: false, frozen: false, locked: false })),
    units: "millimetres",
    problems: [],
    arabic_reversed: false,
    xrefs: [],
    sheet: "*Model_Space",
    hidden_layers: [],
    current: { layer: "0", color: "ByLayer", linetype: "ByLayer", lineweight: -1 },
    text_styles: ["Standard"],
    text_style: "Standard",
    dim_styles: [],
    dim_style: "Standard",
    annotation_scale: "1:1",
    annotation_factor: 1,
    block_edit: core.blockEdit,
  };
}

beforeAll(() => {
  window.matchMedia ??= ((media: string) => ({ matches: false, media })) as unknown as typeof window.matchMedia;
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

beforeEach(() => {
  events.clear();
  core.cursor = 0; core.workspaceCalls = [];
  openDialog.mockImplementation(async () => core.chosen);
  saveDialog.mockResolvedValue(null);
  core.calls.length = 0;
  core.autosaved.clear();
  core.chosen = PLAN;
  core.active = 1; core.opened = false;
  core.scriptOpens = false;
  core.scriptHold = null;
  core.openHold = null;
  core.compareHold = null;
  core.settings = {};
  core.blockEdit = null;
  core.hasBoq = false;
  core.batch = null;
  core.batchVersion = 1;
  core.layers = ["0"];
  core.undoBatch = null;
  core.otherBatch = null;
  core.untitled = false;
  core.savedConversations = [];
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/** Mounts the window and forgets the calls it made on the way. */
async function mount() {
  await act(async () => {
    render(
      <LanguageProvider>
        <App />
      </LanguageProvider>,
    );
  });
  await until(() => expect(events.has("drawing:shown")).toBe(true));
  core.calls.length = 0;
}

const clickOpen = async () => {
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: en.open })[0]));
  await until(() => {
    if (core.autosaved.has(core.chosen)) {
      expect(screen.getByRole("heading", { name: en["recovery.title"] })).toBeTruthy();
    } else {
      expect(document.querySelector("footer.status .message")?.textContent).toBe(core.openHold ? en.opening : "plan.dwg");
    }
  });
};

const typeLine = async (text: string) => {
  const input = document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!;
  await act(async () => {
    fireEvent.change(input, { target: { value: text } });
    fireEvent.keyDown(input, { key: "Enter" });
  });
  await until(() => expect(input.value).toBe(""));
};

/** A small real render batch: one horizontal segment per top-level handle, no text or fills. */
function drawingBatch(handles: number[]): ArrayBuffer {
  const rangesAt = 112;
  const segmentsAt = rangesAt + 32 * handles.length;
  const colorsAt = segmentsAt + 16 * handles.length;
  const buf = new ArrayBuffer(Math.ceil((colorsAt + 4 * handles.length) / 8) * 8);
  const view = new DataView(buf);
  view.setUint32(0, 0x51495942, true);
  view.setUint32(4, 2, true);
  [0, 0, 100, 10, 0, 0].forEach((n, i) => view.setFloat64(8 + 8 * i, n, true));
  const sections = [[3, rangesAt, 32 * handles.length], [4, segmentsAt, 0],
    [5, segmentsAt, 16 * handles.length], [6, colorsAt, 4 * handles.length]];
  view.setUint32(56, sections.length, true);
  sections.forEach((section, i) => section.forEach((n, j) => view.setUint32(60 + 12 * i + 4 * j, n, true)));
  handles.forEach((handle, i) => {
    const at = rangesAt + 32 * i;
    view.setBigUint64(at, BigInt(handle), true);
    view.setBigUint64(at + 8, BigInt(handle), true);
    view.setUint32(at + 16, 0xffffffff, true);
    view.setUint32(at + 20, i, true);
    view.setUint32(at + 24, 1, true);
    [handle * 10, 0, handle * 10 + 10, 0].forEach((n, j) => view.setFloat32(segmentsAt + 16 * i + 4 * j, n, true));
    view.setUint32(colorsAt + 4 * i, 0xffffff, true);
  });
  return buf;
}

function setDrawing(handles: number[]) {
  core.batch = drawingBatch(handles);
  core.batchVersion += 1;
}

const shownHandles = () => {
  const ranges = canvas.props?.batch?.ranges;
  return ranges ? Array.from({ length: ranges.length }, (_, i) => Number(ranges.top(i))) : [];
};

/** Advances just the callbacks requested before this frame; a queued reveal starts next frame. */
function manualFrames() {
  let id = 0;
  let now = performance.now();
  const pending = new Map<number, FrameRequestCallback>();
  const request = vi.fn((callback: FrameRequestCallback) => {
    pending.set(++id, callback);
    return id;
  });
  vi.stubGlobal("requestAnimationFrame", request);
  vi.stubGlobal("cancelAnimationFrame", (key: number) => pending.delete(key));
  return {
    request,
    async advance(ms: number) {
      now += ms;
      const callbacks = [...pending.values()];
      pending.clear();
      await act(async () => callbacks.forEach((callback) => callback(now)));
    },
  };
}

async function reveal(id: string, handles: number[], sheet = "*Model_Space") {
  const infoReads = core.calls.filter((call) => call === "drawing_info {\"workspace\":1}").length;
  await agentEvent({
    type: "tool_call", id, name: "qiyas_execute", label: "draw",
    input: { op: "draw", args: "{}" }, mutating: true,
  });
  await agentEvent({
    type: "tool_result", id, is_error: false, summary: "Drawn", handles,
    ui: { action: "reveal", sheet, handles },
  });
  await until(() => expect(core.calls.filter((call) => call === "drawing_info {\"workspace\":1}").length).toBeGreaterThan(infoReads));
}

/** What the drawing's open sends to the core after the core has it: its drafting settings, its
 *  take-off, BOQ and conflicts, then its batch. */
const SHOWN = [
  "get_drafting {\"workspace\":1}",
  "takeoff {\"workspace\":1}",
  "totals {\"workspace\":1}",
  "boq {\"workspace\":1}",
  "conflicts {\"workspace\":1}",
  "measurement_report {\"workspace\":1}",
  "batch_version {\"workspace\":1}",
  "tabs null",
  "render_batch {\"workspace\":1}",
];

test("open → an autosave is asked about first → Discard deletes it, then the drawing opens and shows", async () => {
  await mount();
  core.autosaved.add(PLAN);

  await clickOpen();
  // Asked, not opened.
  expect(core.calls).toEqual(["drawing_info null", `recoverable_drawings {"paths":["${PLAN}"]}`]);
  expect(screen.getByRole("heading", { name: en["recovery.title"] })).toBeTruthy();

  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["recovery.discard"] }));
  await until(() => {
    // Deleted, then opened without asking again, then shown.
    expect(core.calls).toEqual([
      `discard_autosave {"path":"${PLAN}"}`,
      `open_drawing {"path":"${PLAN}"}`,
      "tabs null",
      "tabs null",
      ...SHOWN,
      'ai_set_language {"lang":"en","workspace":1}',
    ]);
    expect(screen.queryByRole("heading", { name: en["recovery.title"] })).toBeNull();
    // The status bar names the drawing on screen.
    expect(screen.getByRole("status").textContent).toBe("plan.dwg");
  });
});

test("DIVIDE 5 answers the divide prompt once: one divide_along for the command, none for the next pick", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;

  await typeLine("DIVIDE 5");
  expect(core.calls).toEqual([]);

  // The entity is clicked: the number typed answers the prompt, and the tool's command runs once.
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  const divides = () => core.calls.filter((c) => c.startsWith("run_command"));
  await until(() => {
    expect(divides()).toEqual([
      `run_command {"cmd":{"type":"divide_along","handle":42,"chain":[],"count":5},"workspace":1}`,
    ]);
    // Drawn and saved after it.
    expect(core.calls.slice(-1)).toEqual(["save_project {\"workspace\":1}"]);
  });

  // The next pick asks for its own number.
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => {
    expect(divides()).toHaveLength(1);
    expect(screen.getByRole("form", { name: en.divide })).toBeTruthy();
  });
});

test("Enter on a menu item, grid row, tree row or option is theirs: it does not finish the measure", async () => {
  await mount();
  await clickOpen();
  await typeLine("AA");
  await act(async () => {
    for (const p of [[0, 0], [10, 0], [10, 10]]) canvas.props!.onClick(p, p, 1, false);
  });
  core.calls.length = 0;
  for (const role of ["menuitem", "menuitemcheckbox", "row", "treeitem", "option"]) {
    const el = document.createElement("div");
    el.setAttribute("role", role);
    el.tabIndex = 0;
    document.body.appendChild(el);
    await act(async () => fireEvent.keyDown(el, { key: "Enter", code: "Enter" }));
    el.remove();
  }
  await until(() => {
    expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
  });
  // Enter elsewhere still finishes it.
  fireEvent.keyDown(document.body, { key: "Enter", code: "Enter" });
  await until(() => {
    expect(core.calls.filter((c) => c.startsWith("run_command")).length).toBe(1);
  });
});

test("hooks made before apply and onCommand reach them: TEXT's text, a tool palette's command line", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;

  // TEXT: the in-place editor (useAnnotate) places the text through apply (useCommands).
  await typeLine("TEXT");
  canvas.props!.onClick([20, 20], [200, 200], 1, false);
  await until(() => expect(document.querySelector(".text-in-place .text-value")).toBeTruthy());
  const value = document.querySelector<HTMLInputElement>(".text-in-place .text-value")!;
  fireEvent.change(value, { target: { value: "Hi" } });
  fireEvent.submit(value.form!);
  await until(() => {
    expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([
      `run_command {"cmd":{"type":"text","at":[20,20],"height":2.5,"rotation":0,"value":"Hi","style":"Standard"},"workspace":1}`,
    ]);
  });

  // A palette button (useBlocks) runs its command line through onCommand (useCommandLine).
  core.calls.length = 0;
  goTab(en.insert);
  fireEvent.click(screen.getByRole("button", { name: en["block.toolPalettes"] }));
  await until(() => expect(screen.getByRole("button", { name: "Divide in 3" })).toBeTruthy());
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "Divide in 3" })));
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => {
    expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([
      `run_command {"cmd":{"type":"divide_along","handle":42,"chain":[],"count":3},"workspace":1}`,
    ]);
  });
});

const A4 = { name: "ISO A4", w_mm: 210, h_mm: 297 };

test("PLOT: a window clicked in the drawing (usePlot through the command line) is plotted to the PDF chosen", async () => {
  await mount();
  await clickOpen();
  const status = () => screen.getByRole("status").textContent;

  await typeLine("PLOT");
  expect(screen.getByRole("heading", { name: en["plot.title"] })).toBeTruthy();
  fireEvent.change(screen.getByRole("combobox", { name: en["plot.area"] }), { target: { value: "window" } });
  fireEvent.click(screen.getByRole("button", { name: en["plot.pickWindow"] }));
  await until(() => {
    // The first corner clicked, the other typed: both reach the plot, not the tool.
    expect(status()).toBe(en["plot.pickFirst"]);
  });
  canvas.props!.onClick([0, 0], [0, 0], 1, false);
  await until(() => {
    expect(status()).toBe(en["plot.pickSecond"]);
  });
  await typeLine("10,5");
  expect(screen.getByText("0, 0 – 10, 5")).toBeTruthy();

  core.calls.length = 0;
  vi.mocked(saveDialog).mockResolvedValueOnce("/d/plan.pdf");
  fireEvent.click(screen.getByRole("button", { name: en["plot.toPdf"] }));
  await until(() => {
    expect(vi.mocked(saveDialog)).toHaveBeenLastCalledWith({
      defaultPath: "/d/plan.pdf",
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
  });
  const job = {
    sheet: "*Model_Space",
    area: { window: [[0, 0], [10, 5]] },
    paper: A4,
    orientation: "landscape",
    scale: "fit",
    centre: true,
    style: null,
    lineweights: true,
    output: null,
  };
  expect(core.calls).toEqual(["drawing_info {\"workspace\":1}", `plot_pdf_to ${JSON.stringify({ job, path: "/d/plan.pdf", workspace: 1 })}`]);
  await expectToasts(["Plotted to \u2068/d/plan.pdf\u2069 — pictures not plotted: 2"]);
  // The status bar goes back to the drawing's name.
  expect(status()).toBe("plan.dwg");
});

test("QUICKPRINT and the Output tab: quick print, print all layouts and the plot style editor", async () => {
  await mount();
  await clickOpen();
  const printed = () => core.calls.filter((c) => c.startsWith("plot_and_print")).map((c) => JSON.parse(c.slice(15)));
  core.calls.length = 0;

  // The canvas is faked (no view): the sheet's extents, fitted, on the default printer.
  await typeLine("QUICKPRINT");
  expect(printed()).toEqual([
    {
      job: {
        sheet: "*Model_Space",
        area: "extents",
        paper: A4,
        orientation: "landscape",
        scale: "fit",
        centre: true,
        style: null,
        lineweights: true,
        output: null,
      },
      printer: "Office",
      workspace: 1,
    },
  ]);
  await expectToasts(["Sent to Office"]);

  goTab(en.output);
  for (const key of ["plot.button", "plot.quickPrint", "plot.printAll", "plot.styles"] as const) {
    expect((screen.getByRole("button", { name: en[key] }) as HTMLButtonElement).disabled).toBe(false);
  }
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["plot.printAll"] }));
  await until(() => {
    expect(printed().map((p) => [p.job.sheet, p.job.area, p.printer])).toEqual([["*Paper_Space", "layout", "Office"]]);
  });
  await expectToasts(["Layouts sent: 1, one print job each: Sent to Office"]);

  fireEvent.click(screen.getByRole("button", { name: en["plot.styles"] }));
  expect(screen.getByRole("heading", { name: en["plotStyle.title"] })).toBeTruthy();
});

test("another sheet cancels the tool in use, which is made after the sheets", async () => {
  await mount();
  await clickOpen();
  const prompt = () => document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!.labels![0].textContent;

  await typeLine("L");
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => {
    expect(prompt()).toBe(en["draw.prompt.nextPoint"]);
  });

  // useSheets reaches the tool's cancel through the root's measureRef.
  await typeLine("SHEET Layout1");
  expect(core.calls).toContain(`set_sheet {"block":"*Paper_Space","workspace":1}`);
  expect(prompt()).toBe(en["draw.prompt.firstPoint"]);
});

test("Run script (Manage tab, SCRIPT, SCR): a .scr runs on the drawing, its log shows, and the window catches up", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  core.chosen = "/d/count.scr";

  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["script.run"] }));
  await until(() => {
    expect(vi.mocked(openDialog)).toHaveBeenLastCalledWith({ defaultPath: "/d/", filters: [{ name: en["script.file"], extensions: ["scr"] }] });
    // The same drawing on screen after it: its info, take-off and batch read again, the take-off saved.
    expect(core.calls).toEqual([
      "drawing_info {\"workspace\":1}",
      "tabs null",
      `run_script {"path":"/d/count.scr","workspace":1}`,
      "tabs null",
      "drawing_info {\"workspace\":1}",
      "takeoff {\"workspace\":1}",
      "totals {\"workspace\":1}",
      "boq {\"workspace\":1}",
      "conflicts {\"workspace\":1}",
      "measurement_report {\"workspace\":1}",
      "batch_version {\"workspace\":1}",
      "tabs null",
      "render_batch {\"workspace\":1}",
      "save_project {\"workspace\":1}",
    ]);
  });
  const log = screen.getByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") });
  expect(log.textContent).toContain("DOOR count = 3 pcs");
  expect(log.textContent).toContain(en["script.stopped"].replace("{line}", "2"));
  fireEvent.click(screen.getByRole("button", { name: en["script.close"] }));
  await until(() => {
    expect(screen.queryByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") })).toBeNull();
  });

  // Typed, with an OPEN in the script: the drawing it put on screen is shown as a tab switch is.
  core.calls.length = 0;
  core.scriptOpens = true;
  await typeLine("SCR");
  expect(core.calls).toEqual([
    "drawing_info {\"workspace\":1}",
    "tabs null",
    `run_script {"path":"/d/count.scr","workspace":1}`,
    "tabs null",
    'drawing_info {"workspace":2}',
    "tabs null",
    ...SHOWN.map(call => call.replace('"workspace":1', '"workspace":2')),
    'save_project {"workspace":2}',
    'ai_set_language {"lang":"en","workspace":2}',
  ]);
  expect(screen.getByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["script.close"] }));
  await until(() => expect(screen.queryByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") })).toBeNull());
  core.calls.length = 0;
  await typeLine("SCRIPT");
  expect(core.calls.filter((c) => c.startsWith("run_script"))).toEqual([`run_script {"path":"/d/count.scr","workspace":2}`]);
});

test("while a script runs the ribbon and the command line wait, and Stop asks the core to stop it", async () => {
  await mount();
  await clickOpen();
  core.chosen = "/d/long.scr";
  let finish = () => {};
  core.scriptHold = new Promise<void>((done) => (finish = done));

  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["script.run"] }));
  await until(() => expect(screen.getByText(en["script.running"].replace("{line}", "1"))).toBeTruthy());
  const header = document.querySelector("header.top")!;
  expect(header.hasAttribute("inert")).toBe(true);
  expect(screen.getByText(en["script.running"].replace("{line}", "1"))).toBeTruthy();
  // The chase runs beside the text, hidden from assistive technology.
  expect(document.querySelector(".script-running .dot-loader.dot-chase")?.getAttribute("aria-hidden")).toBe("true");

  // A command typed meanwhile is refused, not run between the script's lines.
  core.calls.length = 0;
  await typeLine("COUNT DOOR");
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
  await expectToasts([en["script.busy"]]);

  fireEvent.click(screen.getByRole("button", { name: en["script.stop"] }));
  await until(() => {
    expect(core.calls).toContain("stop_script null");
  });

  finish();
  await until(() => {
    expect(header.hasAttribute("inert")).toBe(false);
    expect(screen.queryByText(en["script.running"].replace("{line}", "1"))).toBeNull();
    expect(screen.getByRole("dialog", { name: en["script.title"].replace("{name}", "long.scr") })).toBeTruthy();
  });
});

test("the Manage tab and OPTIONS (OP), CUI and ALIASEDIT open their dialogs, with no drawing open", async () => {
  await mount();
  for (const [line, title] of [
    ["OPTIONS", en["options.title"]],
    ["OP", en["options.title"]],
    ["CUI", en["cui.title"]],
    ["ALIASEDIT", en["aliasEdit.title"]],
  ]) {
    await typeLine(line);
    expect(screen.getByRole("heading", { name: title })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: en["settings.cancel"] }));
    await until(() => {
      expect(screen.queryByRole("heading", { name: title })).toBeNull();
    });
  }
  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["options.button"] }));
  await until(() => expect(screen.getByRole("button", { name: en["options.toSettings"] })).toBeTruthy());
  // Options hands the autosave interval to Settings.
  fireEvent.click(screen.getByRole("button", { name: en["options.toSettings"] }));
  await until(() => {
    expect(screen.queryByRole("heading", { name: en["options.title"] })).toBeNull();
    expect(screen.getByRole("heading", { name: en.settings })).toBeTruthy();
  });
});

test("PURGE (PU) and AUDIT: a dry run first, then one change that the window catches up with", async () => {
  await mount();
  await typeLine("PURGE");
  expect(screen.getByText(en["cmd.noDrawing"])).toBeTruthy();
  await clickOpen();
  core.calls.length = 0;
  await typeLine("PU");
  expect(core.calls).toEqual([
    `run_command {"cmd":{"type":"purge","kinds":["layers","blocks","styles","linetypes","dimstyles","groups"],"dry_run":true},"workspace":1}`,
  ]);
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["purge.run"] }));
  await until(() => {
    // Taken out, redrawn, the layers read again, autosaved.
    expect(core.calls[0]).toBe(`run_command {"cmd":{"type":"purge","kinds":["layers"],"dry_run":false},"workspace":1}`);
    expect(core.calls).toContain("render_batch {\"workspace\":1}");
    expect(core.calls).toContain("drawing_info {\"workspace\":1}");
    expect(core.calls.at(-1)).toBe("save_project {\"workspace\":1}");
    expect(screen.getByText(en["purge.done"].replace("{n}", "1"))).toBeTruthy();
  });
  fireEvent.click(screen.getByRole("button", { name: en["purge.close"] }));
  await until(() => expect(screen.queryByRole("heading", { name: en["purge.title"] })).toBeNull());

  core.calls.length = 0;
  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["audit.button"] }));
  await until(() => {
    expect(core.calls).toEqual([`run_command {"cmd":{"type":"audit","fix":false},"workspace":1}`]);
    expect(screen.getByText(en["audit.clean"])).toBeTruthy();
  });
});

test("the options reach the window: the pick box, the ribbon layout and a shortcut", async () => {
  core.settings = {
    pick_box_px: 9,
    background_color: "#102030",
    highlight_color: "#ff8800",
    crosshair_color: "#00ff00",
    shortcuts: { circle: "Ctrl+Shift+O", line: "F5" },
    ribbon: {
      tabs: [{ id: "home", label: "home", groups: [{ id: "file", label: "ribbon.file", buttons: [{ kind: "action", command: "open", label: "open" }] }] }],
    },
  };
  await mount();
  const ribbonTabs = within(document.querySelector<HTMLElement>(".ribbon-tabs")!).getAllByRole("tab");
  expect(ribbonTabs.map((t) => t.textContent)).toEqual([en.home]);
  await clickOpen();
  core.calls.length = 0;
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => {
    expect(core.calls).toContain(`pick {"x":10,"y":10,"tol":9,"workspace":1}`);
    // The canvas draws with the background and highlight colours (the crosshair's waits).
    expect((canvas.props as unknown as { colours: unknown }).colours).toEqual({ background: "#102030", highlight: "#ff8800" });
  });
  // A key of the user's is the window's only: F5 never reloads it.
  expect(fireEvent.keyDown(window, { code: "F5", key: "F5" })).toBe(false);
  await until(() => expect(document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!.labels![0].textContent).toBe(en["draw.prompt.firstPoint"]));
  expect(fireEvent.keyDown(window, { code: "KeyO", key: "O", ctrlKey: true, shiftKey: true })).toBe(false);
  await until(() => {
    expect(document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!.labels![0].textContent).toBe(
      en["draw.prompt.circleCentre"],
    );
  });
});

test("PURGE and AUDIT wait while a block is edited in place: the window says so, no dialog, no command", async () => {
  core.blockEdit = "DOOR";
  await mount();
  await clickOpen();
  core.calls.length = 0;
  await typeLine("PURGE");
  await expectToasts([en["manage.blockEdit"]]);
  expect(screen.queryByRole("heading", { name: en["purge.title"] })).toBeNull();
  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["audit.button"] }));
  await until(() => {
    expect(screen.queryByRole("heading", { name: en["audit.title"] })).toBeNull();
  });
  await expectToasts([en["manage.blockEdit"]]);
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
});

test("PARAMETERS (PAR) opens the manager; an edit is set, redrawn and saved; the bar lists the sheet's constraints", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  await typeLine("PAR");
  await until(() => {
    expect(core.calls).toContain(`run_command {"cmd":{"type":"list_parameters"},"workspace":1}`);
    expect(screen.getByRole("heading", { name: en["param.manager"] })).toBeTruthy();
    // The bar shows once the manager (or a parametric tool) is used.
    expect(screen.getByRole("toolbar", { name: en["param.bar"] })).toBeTruthy();
  });

  core.calls.length = 0;
  const input = screen.getByRole("textbox", { name: en["param.expressionOf"].replace("{name}", "d1") });
  fireEvent.change(input, { target: { value: "6" } });
  fireEvent.keyDown(input, { key: "Enter" });
  await until(() => {
    expect(core.calls[0]).toBe(`run_command {"cmd":{"type":"set_parameter","name":"d1","expression":"6"},"workspace":1}`);
    expect(core.calls).toContain("render_batch {\"workspace\":1}");
    expect(core.calls).toContain("save_project {\"workspace\":1}");
  });

  // The Parametric tab holds the tools; a glyph's click deletes its constraint.
  goTab(en.parametric);
  expect(screen.getByRole("button", { name: en["param.gcHorizontal"] })).toBeTruthy();
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["param.glyph"].replace("{kind}", en["param.kind.horizontal"]) }));
  await until(() => {
    expect(core.calls[0]).toBe(`run_command {"cmd":{"type":"delete_constraint","id":1},"workspace":1}`);
  });
});

test("the side panels fold away behind their handles, resize, and the choice comes back on the next start", async () => {
  localStorage.removeItem("qiyas.panels.hidden");
  await mount();
  const middle = document.querySelector(".middle")!;
  expect(middle.className).toBe("middle");
  fireEvent.click(screen.getByRole("button", { name: en["panels.hide.left"] }));
  fireEvent.click(screen.getByRole("button", { name: en["panels.hide.right"] }));
  expect(middle.classList.contains("left-off")).toBe(true);
  expect(middle.classList.contains("right-off")).toBe(true);
  // Folded: the rails stay, and now offer to show the panels again.
  expect(screen.getByRole("button", { name: en["panels.show.left"] })).toBeTruthy();
  expect(screen.getByRole("button", { name: en["panels.show.right"] })).toBeTruthy();

  cleanup();
  await mount();
  const again = document.querySelector(".middle")!;
  expect(again.classList.contains("left-off")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: en["panels.show.left"] }));
  expect(again.classList.contains("left-off")).toBe(false);
  expect(again.classList.contains("right-off")).toBe(true);
  // The handle resizes the panel it stands beside.
  expect((again as HTMLElement).style.getPropertyValue("--left-w")).toBe("220px");
  fireEvent.keyDown(screen.getByRole("separator", { name: en["panels.resize.left"] }), { key: "ArrowRight" });
  expect((again as HTMLElement).style.getPropertyValue("--left-w")).toBe("236px");
  // The BOQ tab keeps its own, wider width.
  expect((again as HTMLElement).style.getPropertyValue("--side-w")).toBe("340px");
  fireEvent.click(screen.getByRole("tab", { name: en.boq }));
  expect((again as HTMLElement).style.getPropertyValue("--side-w")).toBe("620px");
  localStorage.removeItem("qiyas.panels.hidden");
  localStorage.removeItem("qiyas.panels.width");
});

test("Delete erases what is selected, and types in a text field as ever", async () => {
  await mount();
  await clickOpen();
  // The select tool picks the entity under the click.
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => expect(screen.getByText("Selection · 1")).toBeTruthy());
  core.calls.length = 0;

  // In the command line Delete edits the text: nothing is erased.
  const input = document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!;
  await act(async () => fireEvent.keyDown(input, { code: "Delete", key: "Delete" }));
  await until(() => {
    expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
  });

  fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
  await until(() => {
    expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([
      `run_command {"cmd":{"type":"delete_entities","targets":[{"handle":42,"chain":[]}]},"workspace":1}`,
    ]);
  });
});

test("the tool's prompt is said once: on the command line, or in the status bar when that is hidden", async () => {
  await mount();
  await clickOpen();
  const status = () => screen.getByRole("status").textContent;
  const prompt = en["modify.prompt.select"].replace("{n}", "0");

  await typeLine("MOVE");
  // The command line asks; the status bar keeps the file name.
  expect(document.querySelector(".command-line label:not([cmdk-label])")?.textContent).toBe(prompt);
  expect(status()).toBe("plan.dwg");

  // Ctrl+9 hides the command line: the status bar asks instead.
  fireEvent.keyDown(window, { code: "Digit9", key: "9", ctrlKey: true });
  await until(() => {
    expect(document.querySelector(".command-line")).toBeNull();
    expect(status()).toBe(prompt);
  });
});

test("Delete with nothing selected starts ERASE; Delete again erases what was picked", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  const runs = () => core.calls.filter((c) => c.startsWith("run_command"));

  fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
  await until(() => {
    expect(runs()).toEqual([]);
  });
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => expect(document.querySelector(".command-line label:not([cmdk-label])")?.textContent).toBe(en["modify.prompt.select"].replace("{n}", "1")));
  fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
  await until(() => {
    expect(runs()).toEqual([
      `run_command {"cmd":{"type":"delete_entities","targets":[{"handle":42,"chain":[]}]},"workspace":1}`,
    ]);
  });
});

test("with the command line on, a hint it does not show stays in the status bar (COUNT: not a block)", async () => {
  await mount();
  await clickOpen();
  await typeLine("COUNT");
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => {
    expect(screen.getByRole("status").textContent).toBe(en["measure.notABlock"]);
  });
});

test("Delete in a dialog or a menu erases nothing in the drawing behind", async () => {
  await mount();
  await clickOpen();
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await until(() => expect(screen.getByText("Selection · 1")).toBeTruthy());
  core.calls.length = 0;
  for (const role of ["dialog", "menu"]) {
    const box = document.createElement("div");
    box.setAttribute("role", role);
    const button = document.createElement("button");
    box.append(button);
    document.body.append(box);
    await act(async () => fireEvent.keyDown(button, { code: "Delete", key: "Delete" }));
    box.remove();
  }
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
});

test("opening a drawing and comparing show dots beside the status bar's text, and only while they wait", async () => {
  await mount();
  const bar = () => document.querySelector("footer.status .message") as HTMLElement;
  expect(bar().querySelector(".dot-loader")).toBeNull();

  let opened = () => {};
  core.openHold = new Promise<void>((done) => (opened = done));
  await clickOpen();
  expect(bar().textContent).toBe(en.opening);
  expect(bar().querySelector(".dot-loader.dot-scan")).toBeTruthy();
  opened();
  await until(() => {
    expect(bar().textContent).toBe("plan.dwg");
    expect(bar().querySelector(".dot-loader")).toBeNull();
  });

  let compared = () => {};
  core.compareHold = new Promise<void>((done) => (compared = done));
  await typeLine("COMPARE");
  expect(bar().textContent).toBe(en["compare.running"]);
  expect(bar().querySelector(".dot-loader.dot-chase")).toBeTruthy();
  compared();
  // What the comparison found is a notice; the status bar is back to the drawing.
  await expectToasts([en["conflicts.none"]]);
  expect(bar().textContent).toBe("plan.dwg");
  expect(bar().querySelector(".dot-loader")).toBeNull();
});

test("Mizan's compare label marks an empty comparison as complete", async () => {
  core.hasBoq = true;
  await mount();
  await clickOpen();
  fireEvent.click(screen.getByRole("tab", { name: en.conflicts }));
  await until(() => expect(screen.getByRole("tabpanel", { name: en.conflicts })).toBeTruthy());
  const panel = within(screen.getByRole("tabpanel", { name: en.conflicts }));
  expect(panel.getByText(en["conflicts.notCompared"])).toBeTruthy();

  await agentEvent({
    type: "tool_call", id: "compare", name: "qiyas_execute", label: "compare",
    input: { op: "compare", args: "{}" }, mutating: true,
  });
  await agentEvent({
    type: "tool_result", id: "compare", is_error: false, summary: "No conflicts",
    handles: [], ui: null,
  });
  await until(() => {
    expect(panel.getByText(en["conflicts.none"])).toBeTruthy();
    expect(panel.queryByText(en["conflicts.notCompared"])).toBeNull();
  });
});

test("a reveal is queued and layers sync while the previous drawing stays on screen", async () => {
  setDrawing([1]);
  await mount();
  await clickOpen();
  const frames = manualFrames();
  expect(shownHandles()).toEqual([1]);

  core.layers.push("AI-WALLS");
  setDrawing([1, 7]);
  await reveal("first", [7]);
  expect(screen.getAllByText("AI-WALLS").length).toBeGreaterThan(0);
  expect(shownHandles()).toEqual([1]);
  await frames.advance(0);
  await frames.advance(200);
  expect(Array.from(canvas.props!.overlay!)).toEqual([70, 0, 75, 0]);

  setDrawing([1, 7, 8]);
  await reveal("second", [8]);
  // A zoom asks for new glyphs, but it must keep the new entities out of the base drawing.
  await act(async () => canvas.props!.onZoomHint(2));
  await until(() => {
    expect(shownHandles()).toEqual([1]);
    expect(Array.from(canvas.props!.overlay!)).toEqual([70, 0, 75, 0]);
    expect(core.calls).toContain("takeoff {\"workspace\":1}");
    expect(core.calls).toContain("drawing_info {\"workspace\":1}");
  });

  await frames.advance(3000);
  expect(shownHandles()).toEqual([1, 7]);
  await frames.advance(0);
  await frames.advance(200);
  expect(Array.from(canvas.props!.overlay!)).toEqual([80, 0, 85, 0]);
  await frames.advance(3000);
  expect(shownHandles()).toEqual([1, 7, 8]);
  expect(canvas.props!.overlay).toBeNull();
});

test("a fetched batch containing a later reveal keeps those entities hidden until their turn", async () => {
  setDrawing([1]);
  await mount();
  await clickOpen();
  const frames = manualFrames();
  // The core already finished both steps before the first render batch was fetched.
  setDrawing([1, 7, 8]);
  await reveal("first", [7]);
  await reveal("second", [8]);
  await frames.advance(0);
  await frames.advance(200);
  expect(shownHandles()).toEqual([1]);
  expect(Array.from(canvas.props!.overlay!)).toEqual([70, 0, 75, 0]);

  await frames.advance(3000);
  expect(shownHandles()).toEqual([1]);
  expect(Array.from(canvas.props!.overlay!)).toEqual([70, 0, 80, 0]);
  await frames.advance(0);
  await frames.advance(200);
  expect(shownHandles()).toEqual([1]);
  expect(Array.from(canvas.props!.overlay!)).toEqual([70, 0, 80, 0, 80, 0, 85, 0]);

  await frames.advance(3000);
  expect(shownHandles()).toEqual([1, 7, 8]);
  expect(canvas.props!.overlay).toBeNull();
});

test("reduced motion reveals nothing and shows the final drawing at once", async () => {
  const matchMedia = window.matchMedia;
  vi.stubGlobal("matchMedia", (query: string) => ({
    ...matchMedia(query),
    matches: query === "(prefers-reduced-motion: reduce)",
  }));
  setDrawing([1]);
  await mount();
  await clickOpen();
  const frames = manualFrames();

  setDrawing([1, 7]);
  await reveal("still", [7]);
  expect(shownHandles()).toEqual([1, 7]);
  expect(canvas.props!.overlay).toBeNull();
  expect(frames.request).not.toHaveBeenCalled();
});

test.each([
  { reason: "another sheet", sheet: "*Paper_Space", handles: [7] },
  { reason: "no drawable handles", sheet: "*Model_Space", handles: [] },
])("a reveal with $reason shows the current drawing without animation", async ({ sheet, handles }) => {
  setDrawing([1]);
  await mount();
  await clickOpen();
  const frames = manualFrames();
  setDrawing([1, 7]);

  await reveal("immediate", handles, sheet);
  expect(shownHandles()).toEqual([1, 7]);
  expect(canvas.props!.overlay).toBeNull();
  expect(frames.request).not.toHaveBeenCalled();
});

test("a user command cancels the active and queued reveals and shows the final drawing", async () => {
  setDrawing([1]);
  await mount();
  await clickOpen();
  const frames = manualFrames();
  setDrawing([1, 7]);
  await reveal("first", [7]);
  await frames.advance(0);
  await frames.advance(200);
  setDrawing([1, 7, 8]);
  await reveal("second", [8]);
  expect(shownHandles()).toEqual([1]);
  expect(canvas.props!.overlay!.length).toBeGreaterThan(0);

  await typeLine("COUNT DOOR");
  expect(shownHandles()).toEqual([1, 7, 8]);
  expect(canvas.props!.overlay).toBeNull();
  // Finishing the old timeline cannot bring either snapshot or overlay back.
  await frames.advance(3000);
  await frames.advance(3000);
  expect(shownHandles()).toEqual([1, 7, 8]);
  expect(canvas.props!.overlay).toBeNull();
});

test("undo cancels the active and queued reveals and keeps the undone drawing", async () => {
  setDrawing([1]);
  await mount();
  await clickOpen();
  const frames = manualFrames();
  setDrawing([1, 7]);
  await reveal("first", [7]);
  await frames.advance(0);
  await frames.advance(200);
  setDrawing([1, 7, 8]);
  await reveal("second", [8]);
  core.undoBatch = drawingBatch([1, 7]);

  await typeLine("UNDO");
  expect(core.calls).toContain("undo {\"workspace\":1}");
  expect(shownHandles()).toEqual([1, 7]);
  expect(canvas.props!.overlay).toBeNull();
  await frames.advance(3000);
  await frames.advance(3000);
  expect(shownHandles()).toEqual([1, 7]);
  expect(canvas.props!.overlay).toBeNull();
});

test("switching drawings cancels the active and queued reveals from the drawing left behind", async () => {
  setDrawing([1]);
  core.otherBatch = drawingBatch([9]);
  await mount();
  await clickOpen();
  const frames = manualFrames();
  setDrawing([1, 7]);
  await reveal("first", [7]);
  await frames.advance(0);
  await frames.advance(200);
  setDrawing([1, 7, 8]);
  await reveal("second", [8]);

  fireEvent.click(screen.getByRole("tab", { name: "other.dwg" }));
  await until(() => {
    expect(core.calls).toContain('activate_tab {"id":2,"keepConversation":false}');
    expect(shownHandles()).toEqual([9]);
    expect(canvas.props!.overlay).toBeNull();
  });
  await frames.advance(3000);
  await frames.advance(3000);
  expect(shownHandles()).toEqual([9]);
  expect(canvas.props!.overlay).toBeNull();
});

test("an area marked with the area tool reaches Mizan's composer, made after the tool", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("tab", { name: en.assistant }));
  await until(() => expect(screen.getByRole("button", { name: en["ai.markArea"] })).toBeTruthy());
  await act(async () => fireEvent.click(screen.getByRole("button", { name: en["ai.markArea"] })));
  // The tool hands the box to the wiring through the root's markRegionRef.
  (canvas.props as unknown as { onBox(b: number[][], crossing: boolean, shift: boolean): void }).onBox(
    [
      [0, 0],
      [5000, 2000],
    ],
    false,
    false,
  );
  await until(() => {
    expect(core.calls).toContain(`pick_polygon {"points":[[0,0],[5000,0],[5000,2000],[0,2000]],"crossing":true,"workspace":1}`);
    expect(screen.getByText("Area · 5.00 × 2.00 m")).toBeTruthy();
    expect(screen.getByText("1 entities")).toBeTruthy();
  });
});

test("Mizan selects for the user, and the window says what the user points at", async () => {
  await mount();
  await clickOpen();
  await emit("ai:select", { workspace: 1, handles: [42], zoom: false });
  await until(() => {
    expect(screen.getByText("Selection · 1")).toBeTruthy();
  });
  core.calls.length = 0;
  await emit("ai:pointed", { workspace: 1, id: 7 });
  await until(() => {
    expect(core.calls).toEqual([
      `ai_window_reply {"workspace":1,"id":7,"value":{"selection":[{"handle":42,"chain":[]}],"region":null,"view":null},"error":null}`,
    ]);
  });
  // The canvas is faked: no picture, said as an error.
  core.calls.length = 0;
  await emit("ai:capture", { workspace: 1, id: 8, maxSide: 1600 });
  await until(() => {
    expect(core.calls).toEqual([`ai_window_reply {"workspace":1,"id":8,"value":null,"error":"No drawing is on screen"}`]);
  });
});

test("an untitled drawing keeps its own Continue offer through its placeholder path", async () => {
  core.untitled = true; core.opened = true;
  core.savedConversations = [{
    id: "untitled-chat", title: "Draw the new plan", drawing: PLAN,
    drawing_exists: false, updated: 1_792_598_400_000, turns: 1,
  }];
  await mount();
  // An existing untitled tab is shown directly; its placeholder is not a file to open.
  await emit("drawing:shown", null);
  await until(() => expect(core.calls).toContain("render_batch {\"workspace\":1}"));
  fireEvent.click(screen.getByRole("tab", { name: en.assistant }));
  await until(() => {
    expect(screen.getByRole("button", { name: "Continue: Draw the new plan" })).toBeTruthy();
  });
});
