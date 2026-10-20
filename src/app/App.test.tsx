// @vitest-environment jsdom
/** Wiring checks for the window's composition root, the kind the type checker can't make: the
 *  hooks reach each other in the right order with the right state. The core (every IPC call
 *  through `invoke`), the file dialogs and the canvas are faked; `calls` is what reached the
 *  core, in order. */
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import App from "./App";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";

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
const expectToasts = (texts: string[]) => waitFor(() => expect(toasts()).toEqual(texts));

const core = vi.hoisted(() => ({
  calls: [] as string[],
  /** The drawings with an autosave newer than them. */
  autosaved: new Set<string>(),
  /** What the Open dialog answers. */
  chosen: "/d/plan.dwg",
  /** The tab on screen; a script's OPEN puts tab 2 there. */
  active: 1,
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
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    core.calls.push(`${cmd} ${JSON.stringify(args ?? null)}`);
    switch (cmd) {
      case "aliases":
        return {};
      case "recents":
      case "takeoff":
      case "totals":
      case "conflicts":
      case "paper_views":
        return [];
      case "pick_polygon":
        return [{ top: 42, entity: 42, chain: [], xref: null, kind: "Line" }];
      case "get_settings":
        return {
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
        return info(args!.path as string);
      case "compare":
        if (core.compareHold) await core.compareHold;
        return [];
      case "boq":
        return core.hasBoq ? { source: "/d/bill.xlsx", imported: "", items: [], skipped_rows: 0 } : null;
      case "drawing_info":
        return info(PLAN);
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
        return 1;
      case "tabs":
        return { tabs: [{ id: 1, path: PLAN, title: "plan.dwg", dirty: false }], active: core.active };
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
        return new Uint8Array([0x51, 0x49, 0x4e, 0x4d, 0, 0, 0, 0]).buffer;
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
const events = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());
const emit = (name: string, payload: unknown) => act(async () => events.get(name)?.({ payload }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (e: { payload: unknown }) => void) => {
    events.set(name, handler);
    return () => {};
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => core.chosen),
  save: vi.fn(async () => null),
  ask: vi.fn(async () => true),
  message: vi.fn(async () => "No"),
}));
/** The canvas needs WebGL; the fake keeps its props so a test can click the drawing. */
const canvas = vi.hoisted(() => ({ props: null as null | { onClick(...a: unknown[]): void } }));
vi.mock("../viewport/Viewport", () => ({
  Viewport: (props: { onClick(...a: unknown[]): void }) => {
    canvas.props = props;
    return null;
  },
}));

function info(path: string) {
  return {
    path,
    sheets: [
      { name: "Model", block: "*Model_Space", is_model: true },
      { name: "Layout1", block: "*Paper_Space", is_model: false },
    ],
    layers: [{ name: "0", color: [255, 255, 255], off: false, frozen: false, locked: false }],
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
  core.calls.length = 0;
  core.autosaved.clear();
  core.chosen = PLAN;
  core.active = 1;
  core.scriptOpens = false;
  core.scriptHold = null;
  core.openHold = null;
  core.compareHold = null;
  core.settings = {};
  core.blockEdit = null;
  core.hasBoq = false;
});

afterEach(cleanup);

/** Lets the window's async work finish. */
const settle = async () => {
  for (let i = 0; i < 10; i++) await act(async () => {});
};

/** Mounts the window and forgets the calls it made on the way. */
async function mount() {
  render(
    <LanguageProvider>
      <App />
    </LanguageProvider>,
  );
  await settle();
  core.calls.length = 0;
}

const clickOpen = async () => {
  fireEvent.click(screen.getAllByRole("button", { name: en.open })[0]);
  await settle();
};

const typeLine = async (text: string) => {
  const input = document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!;
  fireEvent.change(input, { target: { value: text } });
  fireEvent.keyDown(input, { key: "Enter" });
  await settle();
};

/** What the drawing's open sends to the core after the core has it: its drafting settings, its
 *  take-off, BOQ and conflicts, then its batch. */
const SHOWN = [
  "get_drafting null",
  "takeoff null",
  "totals null",
  "boq null",
  "conflicts null",
  "batch_version null",
  "tabs null",
  "render_batch {}",
];

test("open → an autosave is asked about first → Discard deletes it, then the drawing opens and shows", async () => {
  await mount();
  core.autosaved.add(PLAN);

  await clickOpen();
  // Asked, not opened.
  expect(core.calls).toEqual([`recoverable_drawings {"paths":["${PLAN}"]}`]);
  expect(screen.getByRole("heading", { name: en["recovery.title"] })).toBeTruthy();

  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["recovery.discard"] }));
  await settle();
  // Deleted, then opened without asking again, then shown.
  expect(core.calls).toEqual([
    `discard_autosave {"path":"${PLAN}"}`,
    `open_drawing {"path":"${PLAN}"}`,
    ...SHOWN,
  ]);
  expect(screen.queryByRole("heading", { name: en["recovery.title"] })).toBeNull();
  // The status bar names the drawing on screen.
  expect(screen.getByRole("status").textContent).toBe("plan.dwg");
});

test("DIVIDE 5 answers the divide prompt once: one divide_along for the command, none for the next pick", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;

  await typeLine("DIVIDE 5");
  expect(core.calls).toEqual([]);

  // The entity is clicked: the number typed answers the prompt, and the tool's command runs once.
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await settle();
  const divides = () => core.calls.filter((c) => c.startsWith("run_command"));
  expect(divides()).toEqual([
    `run_command {"cmd":{"type":"divide_along","handle":42,"chain":[],"count":5}}`,
  ]);
  // Drawn and saved after it.
  expect(core.calls.slice(-1)).toEqual(["save_project null"]);

  // The next pick asks for its own number.
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await settle();
  expect(divides()).toHaveLength(1);
  expect(screen.getByRole("form", { name: en.divide })).toBeTruthy();
});

test("Enter on a menu item, grid row, tree row or option is theirs: it does not finish the measure", async () => {
  await mount();
  await clickOpen();
  await typeLine("AA");
  for (const p of [[0, 0], [10, 0], [10, 10]]) canvas.props!.onClick(p, p, 1, false);
  await settle();
  core.calls.length = 0;
  for (const role of ["menuitem", "menuitemcheckbox", "row", "treeitem", "option"]) {
    const el = document.createElement("div");
    el.setAttribute("role", role);
    el.tabIndex = 0;
    document.body.appendChild(el);
    fireEvent.keyDown(el, { key: "Enter", code: "Enter" });
    el.remove();
  }
  await settle();
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
  // Enter elsewhere still finishes it.
  fireEvent.keyDown(document.body, { key: "Enter", code: "Enter" });
  await settle();
  expect(core.calls.filter((c) => c.startsWith("run_command")).length).toBe(1);
});

test("hooks made before apply and onCommand reach them: TEXT's text, a tool palette's command line", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;

  // TEXT: the in-place editor (useAnnotate) places the text through apply (useCommands).
  await typeLine("TEXT");
  canvas.props!.onClick([20, 20], [200, 200], 1, false);
  await settle();
  const value = document.querySelector<HTMLInputElement>(".text-in-place .text-value")!;
  fireEvent.change(value, { target: { value: "Hi" } });
  fireEvent.submit(value.form!);
  await settle();
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([
    `run_command {"cmd":{"type":"text","at":[20,20],"height":2.5,"rotation":0,"value":"Hi","style":"Standard"}}`,
  ]);

  // A palette button (useBlocks) runs its command line through onCommand (useCommandLine).
  core.calls.length = 0;
  goTab(en.insert);
  fireEvent.click(screen.getByRole("button", { name: en["block.toolPalettes"] }));
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Divide in 3" }));
  await settle();
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await settle();
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([
    `run_command {"cmd":{"type":"divide_along","handle":42,"chain":[],"count":3}}`,
  ]);
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
  await settle();
  // The first corner clicked, the other typed: both reach the plot, not the tool.
  expect(status()).toBe(en["plot.pickFirst"]);
  canvas.props!.onClick([0, 0], [0, 0], 1, false);
  await settle();
  expect(status()).toBe(en["plot.pickSecond"]);
  await typeLine("10,5");
  expect(screen.getByText("0, 0 – 10, 5")).toBeTruthy();

  core.calls.length = 0;
  vi.mocked(saveDialog).mockResolvedValueOnce("/d/plan.pdf");
  fireEvent.click(screen.getByRole("button", { name: en["plot.toPdf"] }));
  await settle();
  expect(vi.mocked(saveDialog)).toHaveBeenLastCalledWith({
    defaultPath: "/d/plan.pdf",
    filters: [{ name: "PDF", extensions: ["pdf"] }],
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
  expect(core.calls).toEqual([`plot_pdf_to ${JSON.stringify({ job, path: "/d/plan.pdf" })}`]);
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
    },
  ]);
  await expectToasts(["Sent to Office"]);

  goTab(en.output);
  for (const key of ["plot.button", "plot.quickPrint", "plot.printAll", "plot.styles"] as const) {
    expect((screen.getByRole("button", { name: en[key] }) as HTMLButtonElement).disabled).toBe(false);
  }
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["plot.printAll"] }));
  await settle();
  expect(printed().map((p) => [p.job.sheet, p.job.area, p.printer])).toEqual([["*Paper_Space", "layout", "Office"]]);
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
  await settle();
  expect(prompt()).toBe(en["draw.prompt.nextPoint"]);

  // useSheets reaches the tool's cancel through the root's measureRef.
  await typeLine("SHEET Layout1");
  expect(core.calls).toContain(`set_sheet {"block":"*Paper_Space"}`);
  expect(prompt()).toBe(en["draw.prompt.firstPoint"]);
});

test("Run script (Manage tab, SCRIPT, SCR): a .scr runs on the drawing, its log shows, and the window catches up", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  core.chosen = "/d/count.scr";

  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["script.run"] }));
  await settle();
  expect(vi.mocked(openDialog)).toHaveBeenLastCalledWith({ filters: [{ name: en["script.file"], extensions: ["scr"] }] });
  // The same drawing on screen after it: its info, take-off and batch read again, the take-off saved.
  expect(core.calls).toEqual([
    "tabs null",
    `run_script {"path":"/d/count.scr"}`,
    "tabs null",
    "drawing_info null",
    "takeoff null",
    "totals null",
    "boq null",
    "conflicts null",
    "batch_version null",
    "tabs null",
    "render_batch {}",
    "save_project null",
  ]);
  const log = screen.getByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") });
  expect(log.textContent).toContain("DOOR count = 3 pcs");
  expect(log.textContent).toContain(en["script.stopped"].replace("{line}", "2"));
  fireEvent.click(screen.getByRole("button", { name: en["script.close"] }));
  await settle();
  expect(screen.queryByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") })).toBeNull();

  // Typed, with an OPEN in the script: the drawing it put on screen is shown as a tab switch is.
  core.calls.length = 0;
  core.scriptOpens = true;
  await typeLine("SCR");
  expect(core.calls).toEqual([
    "tabs null",
    `run_script {"path":"/d/count.scr"}`,
    "tabs null",
    "drawing_info null",
    ...SHOWN,
    "save_project null",
  ]);
  expect(screen.getByRole("dialog", { name: en["script.title"].replace("{name}", "count.scr") })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["script.close"] }));
  await settle();
  core.calls.length = 0;
  await typeLine("SCRIPT");
  expect(core.calls.filter((c) => c.startsWith("run_script"))).toEqual([`run_script {"path":"/d/count.scr"}`]);
});

test("while a script runs the ribbon and the command line wait, and Stop asks the core to stop it", async () => {
  await mount();
  await clickOpen();
  core.chosen = "/d/long.scr";
  let finish = () => {};
  core.scriptHold = new Promise<void>((done) => (finish = done));

  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["script.run"] }));
  await settle();
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
  await settle();
  expect(core.calls).toContain("stop_script null");

  finish();
  await settle();
  expect(header.hasAttribute("inert")).toBe(false);
  expect(screen.queryByText(en["script.running"].replace("{line}", "1"))).toBeNull();
  expect(screen.getByRole("dialog", { name: en["script.title"].replace("{name}", "long.scr") })).toBeTruthy();
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
    await settle();
    expect(screen.queryByRole("heading", { name: title })).toBeNull();
  }
  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["options.button"] }));
  await settle();
  // Options hands the autosave interval to Settings.
  fireEvent.click(screen.getByRole("button", { name: en["options.toSettings"] }));
  await settle();
  expect(screen.queryByRole("heading", { name: en["options.title"] })).toBeNull();
  expect(screen.getByRole("heading", { name: en.settings })).toBeTruthy();
});

test("PURGE (PU) and AUDIT: a dry run first, then one change that the window catches up with", async () => {
  await mount();
  await typeLine("PURGE");
  expect(screen.getByText(en["cmd.noDrawing"])).toBeTruthy();
  await clickOpen();
  core.calls.length = 0;
  await typeLine("PU");
  expect(core.calls).toEqual([
    `run_command {"cmd":{"type":"purge","kinds":["layers","blocks","styles","linetypes","dimstyles","groups"],"dry_run":true}}`,
  ]);
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["purge.run"] }));
  await settle();
  // Taken out, redrawn, the layers read again, autosaved.
  expect(core.calls[0]).toBe(`run_command {"cmd":{"type":"purge","kinds":["layers"],"dry_run":false}}`);
  expect(core.calls).toContain("render_batch {}");
  expect(core.calls).toContain("drawing_info null");
  expect(core.calls.at(-1)).toBe("save_project null");
  expect(screen.getByText(en["purge.done"].replace("{n}", "1"))).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["purge.close"] }));
  await settle();

  core.calls.length = 0;
  goTab(en.manage);
  fireEvent.click(screen.getByRole("button", { name: en["audit.button"] }));
  await settle();
  expect(core.calls).toEqual([`run_command {"cmd":{"type":"audit","fix":false}}`]);
  expect(screen.getByText(en["audit.clean"])).toBeTruthy();
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
  await settle();
  expect(core.calls).toContain(`pick {"x":10,"y":10,"tol":9}`);
  // The canvas draws with the background and highlight colours (the crosshair's waits).
  expect((canvas.props as unknown as { colours: unknown }).colours).toEqual({ background: "#102030", highlight: "#ff8800" });
  // A key of the user's is the window's only: F5 never reloads it.
  expect(fireEvent.keyDown(window, { code: "F5", key: "F5" })).toBe(false);
  await settle();
  expect(fireEvent.keyDown(window, { code: "KeyO", key: "O", ctrlKey: true, shiftKey: true })).toBe(false);
  await settle();
  expect(document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!.labels![0].textContent).toBe(
    en["draw.prompt.circleCentre"],
  );
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
  await settle();
  expect(screen.queryByRole("heading", { name: en["audit.title"] })).toBeNull();
  await expectToasts([en["manage.blockEdit"]]);
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);
});

test("PARAMETERS (PAR) opens the manager; an edit is set, redrawn and saved; the bar lists the sheet's constraints", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  await typeLine("PAR");
  await settle();
  expect(core.calls).toContain(`run_command {"cmd":{"type":"list_parameters"}}`);
  expect(screen.getByRole("heading", { name: en["param.manager"] })).toBeTruthy();
  // The bar shows once the manager (or a parametric tool) is used.
  expect(screen.getByRole("toolbar", { name: en["param.bar"] })).toBeTruthy();

  core.calls.length = 0;
  const input = screen.getByRole("textbox", { name: en["param.expressionOf"].replace("{name}", "d1") });
  fireEvent.change(input, { target: { value: "6" } });
  fireEvent.keyDown(input, { key: "Enter" });
  await settle();
  expect(core.calls[0]).toBe(`run_command {"cmd":{"type":"set_parameter","name":"d1","expression":"6"}}`);
  expect(core.calls).toContain("render_batch {}");
  expect(core.calls).toContain("save_project null");

  // The Parametric tab holds the tools; a glyph's click deletes its constraint.
  goTab(en.parametric);
  expect(screen.getByRole("button", { name: en["param.gcHorizontal"] })).toBeTruthy();
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("button", { name: en["param.glyph"].replace("{kind}", en["param.kind.horizontal"]) }));
  await settle();
  expect(core.calls[0]).toBe(`run_command {"cmd":{"type":"delete_constraint","id":1}}`);
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
  await settle();
  core.calls.length = 0;

  // In the command line Delete edits the text: nothing is erased.
  const input = document.querySelector<HTMLInputElement>("input[aria-autocomplete]")!;
  fireEvent.keyDown(input, { code: "Delete", key: "Delete" });
  await settle();
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([]);

  fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
  await settle();
  expect(core.calls.filter((c) => c.startsWith("run_command"))).toEqual([
    `run_command {"cmd":{"type":"delete_entities","targets":[{"handle":42,"chain":[]}]}}`,
  ]);
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
  await settle();
  expect(document.querySelector(".command-line")).toBeNull();
  expect(status()).toBe(prompt);
});

test("Delete with nothing selected starts ERASE; Delete again erases what was picked", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  const runs = () => core.calls.filter((c) => c.startsWith("run_command"));

  fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
  await settle();
  expect(runs()).toEqual([]);
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await settle();
  fireEvent.keyDown(window, { code: "Delete", key: "Delete" });
  await settle();
  expect(runs()).toEqual([
    `run_command {"cmd":{"type":"delete_entities","targets":[{"handle":42,"chain":[]}]}}`,
  ]);
});

test("with the command line on, a hint it does not show stays in the status bar (COUNT: not a block)", async () => {
  await mount();
  await clickOpen();
  await typeLine("COUNT");
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await settle();
  expect(screen.getByRole("status").textContent).toBe(en["measure.notABlock"]);
});

test("Delete in a dialog or a menu erases nothing in the drawing behind", async () => {
  await mount();
  await clickOpen();
  canvas.props!.onClick([10, 10], [100, 100], 1, false);
  await settle();
  core.calls.length = 0;
  for (const role of ["dialog", "menu"]) {
    const box = document.createElement("div");
    box.setAttribute("role", role);
    const button = document.createElement("button");
    box.append(button);
    document.body.append(box);
    fireEvent.keyDown(button, { code: "Delete", key: "Delete" });
    await settle();
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
  await settle();
  expect(bar().textContent).toBe("plan.dwg");
  expect(bar().querySelector(".dot-loader")).toBeNull();

  let compared = () => {};
  core.compareHold = new Promise<void>((done) => (compared = done));
  await typeLine("COMPARE");
  expect(bar().textContent).toBe(en["compare.running"]);
  expect(bar().querySelector(".dot-loader.dot-chase")).toBeTruthy();
  compared();
  await settle();
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
  await settle();
  const panel = within(screen.getByRole("tabpanel", { name: en.conflicts }));
  expect(panel.getByText(en["conflicts.notCompared"])).toBeTruthy();

  await emit("ai:event", {
    type: "tool_call", id: "compare", name: "qiyas_execute", label: "compare",
    input: { op: "compare", args: "{}" }, mutating: true,
  });
  await emit("ai:event", {
    type: "tool_result", id: "compare", is_error: false, summary: "No conflicts",
    handles: [], ui: null,
  });
  await settle();
  expect(panel.getByText(en["conflicts.none"])).toBeTruthy();
  expect(panel.queryByText(en["conflicts.notCompared"])).toBeNull();
});

test("an area marked with the area tool reaches Mizan's composer, made after the tool", async () => {
  await mount();
  await clickOpen();
  core.calls.length = 0;
  fireEvent.click(screen.getByRole("tab", { name: en.assistant }));
  await settle();
  fireEvent.click(screen.getByRole("button", { name: en["ai.markArea"] }));
  await settle();
  // The tool hands the box to the wiring through the root's markRegionRef.
  (canvas.props as unknown as { onBox(b: number[][], crossing: boolean, shift: boolean): void }).onBox(
    [
      [0, 0],
      [5000, 2000],
    ],
    false,
    false,
  );
  await settle();
  expect(core.calls).toContain(`pick_polygon {"points":[[0,0],[5000,0],[5000,2000],[0,2000]],"crossing":true}`);
  expect(screen.getByText("Area · 5.00 × 2.00 m")).toBeTruthy();
  expect(screen.getByText("1 entities")).toBeTruthy();
});

test("Mizan selects for the user, and the window says what the user points at", async () => {
  await mount();
  await clickOpen();
  await emit("ai:select", { handles: [42], zoom: false });
  await settle();
  expect(screen.getByText("Selection · 1")).toBeTruthy();
  core.calls.length = 0;
  await emit("ai:pointed", { id: 7 });
  await settle();
  expect(core.calls).toEqual([
    `ai_window_reply {"id":7,"value":{"selection":[{"handle":42,"chain":[]}],"region":null,"view":null},"error":null}`,
  ]);
  // The canvas is faked: no picture, said as an error.
  core.calls.length = 0;
  await emit("ai:capture", { id: 8, maxSide: 1600 });
  await settle();
  expect(core.calls).toEqual([`ai_window_reply {"id":8,"value":null,"error":"No drawing is on screen"}`]);
});
