// @vitest-environment jsdom
/** The plot hook over a faked core: `calls` is every IPC call, in order, with its arguments. */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { DrawingInfo, Paper, PlotStyle } from "../api";
import { defaultSetup, type PageSetup } from "../shell/PlotDialog";
import type { Bounds } from "../viewport/camera";
import { plotError, usePlot } from "./usePlot";

const PAPERS: Paper[] = [
  { name: "ISO A3", w_mm: 297, h_mm: 420 },
  { name: "ISO A4", w_mm: 210, h_mm: 297 },
];

const STYLE: PlotStyle = {
  kind: "ctb",
  description: "",
  scale_factor: 1,
  apply_factor: false,
  display_units: 0,
  styles: [],
  lineweights: [0],
  extra: [],
};

const core = vi.hoisted(() => ({
  calls: [] as string[],
  /** What each IPC command answers (a function for one that depends on its arguments). */
  answers: {} as Record<string, unknown>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    core.calls.push(`${cmd} ${JSON.stringify(args ?? null)}`);
    const answer = core.answers[cmd];
    return typeof answer === "function" ? answer(args) : answer;
  }),
}));

const dialogs = vi.hoisted(() => ({ open: null as string | null, save: null as string | null }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => dialogs.open),
  save: vi.fn(async () => dialogs.save),
}));

const PLAN = "/d/plan.dwg";

function info(path = PLAN, layouts = ["Layout1", "Layout2"]): DrawingInfo {
  return {
    path,
    units: "millimetres",
    sheets: [
      { name: "Model", block: "*Model_Space", is_model: true },
      ...layouts.map((name, i) => ({ name, block: i === 0 ? "*Paper_Space" : `*Paper_Space${i - 1}`, is_model: false })),
    ],
  } as DrawingInfo;
}

/** The translator: a key and its values, so the tests read what was said, not its wording. */
const t = (key: string, vars?: Record<string, string | number>) => (vars ? `${key} ${JSON.stringify(vars)}` : key);

function setup(over: { info?: DrawingInfo | null; sheet?: string; visible?: Bounds | null } = {}) {
  const deps = {
    t,
    switchGen: { current: 1 },
    info: over.info === undefined ? info() : over.info,
    sheet: over.sheet ?? "*Model_Space",
    viewport: { current: over.visible === null ? null : { visible: () => over.visible ?? [[0, 0], [40, 20]] } } as never,
    setError: vi.fn(),
    setNotice: vi.fn(),
    focusDrawing: vi.fn(),
    measure: { cancel: vi.fn() },
    run: async (step: () => Promise<void>) => {
      deps.setNotice(null);
      try {
        await step();
        deps.setError(null);
      } catch (e) {
        deps.setError(String(e));
      }
    },
  };
  const hook = renderHook((d: typeof deps) => usePlot(d), { initialProps: deps });
  return { deps, hook, result: hook.result };
}

beforeEach(() => {
  core.calls.length = 0;
  core.answers = {
    drawing_info: info(),
    paper_catalog: PAPERS,
    list_printers: ["Office", "Plotter"],
    default_printer: "Office",
    load_plot_style: STYLE,
    plot_and_print: (a: { printer: string | null }) => (a.printer ? `Sent to ${a.printer}` : "Opened in the PDF viewer: print from there"),
  };
  dialogs.open = null;
  dialogs.save = null;
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

const KEPT: PageSetup = {
  area: "extents",
  window: null,
  paper: "ISO A3",
  orientation: "portrait",
  scale: [1, 50],
  centre: false,
  lineweights: false,
};

test("Plot opens on the sheet on screen with its kept setup and the drawing's plot style", () => {
  const { result, deps, hook } = setup({ sheet: "*Paper_Space" });
  act(() => result.current.openPlot());
  expect(result.current.plotAsk).toEqual({
    sheet: { name: "Layout1", block: "*Paper_Space", is_model: false },
    setup: defaultSetup(false),
    stylePath: null,
  });

  act(() => result.current.keep({ ...KEPT, area: "layout" }, "/s/mono.ctb"));
  act(() => result.current.closePlot());
  expect(result.current.plotAsk).toBeNull();
  expect(deps.focusDrawing).toHaveBeenCalled();
  act(() => result.current.openPlot());
  expect(result.current.plotAsk?.setup).toEqual({ ...KEPT, area: "layout" });
  expect(result.current.plotAsk?.stylePath).toBe("/s/mono.ctb");

  // Model space has its own setup; the style is the drawing's.
  hook.rerender({ ...deps, sheet: "*Model_Space" });
  act(() => result.current.openPlot());
  expect(result.current.plotAsk?.setup).toEqual(defaultSetup(true));
  expect(result.current.plotAsk?.stylePath).toBe("/s/mono.ctb");

  // Another drawing closes the dialog, and keeps nothing of this one.
  hook.rerender({ ...deps, info: info("/d/other.dwg") });
  expect(result.current.plotAsk).toBeNull();
  act(() => result.current.openPlot());
  expect(result.current.plotAsk?.stylePath).toBeNull();
  expect(core.calls).toEqual([]);
});

test("without a drawing, plotting and printing say to open one", async () => {
  const { result, deps } = setup({ info: null });
  act(() => result.current.openPlot());
  await act(() => result.current.quickPrint());
  await act(() => result.current.printAll());
  expect(result.current.plotAsk).toBeNull();
  expect(deps.setError.mock.calls).toEqual([["cmd.noDrawing"], ["cmd.noDrawing"], ["cmd.noDrawing"]]);
  expect(core.calls).toEqual([]);
});

test("the plot window is two clicks in the drawing; Escape goes back without one", () => {
  const { result, deps } = setup();
  expect(result.current.pickPoint([1, 2])).toBe(false);

  act(() => result.current.startWindowPick());
  expect(deps.measure.cancel).toHaveBeenCalled();
  expect(result.current.windowPick).toEqual({ first: null });
  let taken = false;
  act(() => {
    taken = result.current.pickPoint([1, 2]);
  });
  expect(taken).toBe(true);
  expect(result.current.windowPick).toEqual({ first: [1, 2] });
  act(() => {
    result.current.pickPoint([5, 6]);
  });
  expect(result.current.windowPick).toBeNull();
  expect(result.current.pickedWindow).toEqual([[1, 2], [5, 6]]);

  act(() => result.current.startWindowPick());
  act(() => result.current.cancelPick());
  expect(result.current.windowPick).toBeNull();
  expect(result.current.pickedWindow).toEqual([[1, 2], [5, 6]]);
});

test("Plot to PDF goes where the Save dialog said, and says where and how many pictures were left out", async () => {
  core.answers.plot_pdf_to = { path: "/elsewhere/plan.pdf", skipped: ["Image a.png not found", "Image b.png not found"] };
  const { result, deps } = setup();
  const job = {
    sheet: "*Model_Space",
    area: "extents" as const,
    paper: PAPERS[0],
    orientation: "landscape" as const,
    scale: "fit" as const,
    centre: true,
    style: null,
    lineweights: true,
    output: null,
  };
  await act(() => result.current.plotToPdf(job, "/elsewhere/plan.pdf"));
  expect(core.calls).toEqual([`plot_pdf_to ${JSON.stringify({ job, path: "/elsewhere/plan.pdf" })}`]);
  // The path is isolated, so it reads whole inside an Arabic sentence.
  expect(deps.setNotice).toHaveBeenLastCalledWith('plot.doneSkipped {"path":"\u2068/elsewhere/plan.pdf\u2069","n":2}');

  core.answers.plot_pdf_to = { path: "/d/plan.pdf", skipped: [] };
  await act(() => result.current.plotToPdf(job, "/d/plan.pdf"));
  expect(deps.setNotice).toHaveBeenLastCalledWith('plot.done {"path":"\u2068/d/plan.pdf\u2069"}');

  // The core's known errors in the window's words, for the dialog.
  core.answers.plot_pdf_to = () => Promise.reject("Nothing to plot: the area is empty");
  await expect(result.current.plotToPdf(job, "/d/plan.pdf")).rejects.toBe("plot.err.nothing");
});

test("the core's plot errors in the window's words; others as the core said them", () => {
  expect(plotError(t, "The area does not fit the paper")).toBe("plot.err.noFit");
  expect(plotError(t, "Can't write the PDF: Permission denied")).toBe('plot.err.write {"why":"Permission denied"}');
  expect(plotError(t, "lp: printer is stopped")).toBe("lp: printer is stopped");
});

test("Print says where the job went in the window's own words", async () => {
  const { result, deps } = setup();
  const job = { sheet: "*Model_Space" } as never;
  await act(() => result.current.print(job, "Plotter"));
  expect(core.calls).toEqual([`plot_and_print ${JSON.stringify({ job, printer: "Plotter" })}`]);
  expect(deps.setNotice).toHaveBeenLastCalledWith('plot.sentTo {"printer":"Plotter"}');
  await act(() => result.current.print(job, null));
  expect(deps.setNotice).toHaveBeenLastCalledWith("plot.openedViewer");
  core.answers.plot_and_print = "Sent to the default printer";
  await act(() => result.current.print(job, null));
  expect(deps.setNotice).toHaveBeenLastCalledWith("plot.sentDefault");
  // Anything else is said as the core said it.
  core.answers.plot_and_print = "Queued";
  await act(() => result.current.print(job, "Office"));
  expect(deps.setNotice).toHaveBeenLastCalledWith("Queued");
});

test("the printers and the default one; none when they can't be listed", async () => {
  const { result } = setup();
  expect(await result.current.printers()).toEqual({ printers: ["Office", "Plotter"], default: "Office" });
  core.answers.list_printers = () => Promise.reject("lpstat: not found");
  core.answers.default_printer = () => Promise.reject("lpstat: not found");
  expect(await result.current.printers()).toEqual({ printers: [], default: null });
});

test("Quick print: the view on screen, fitted and turned to suit it, on the default printer", async () => {
  const { result, deps } = setup({ visible: [[0, 0], [20, 40]] });
  act(() => result.current.openPlot());
  act(() => result.current.keep(KEPT, "/s/mono.ctb"));
  act(() => result.current.closePlot());
  core.calls.length = 0;

  await act(() => result.current.quickPrint());
  const job = {
    sheet: "*Model_Space",
    area: { window: [[0, 0], [20, 40]] },
    paper: PAPERS[0],
    orientation: "portrait",
    scale: "fit",
    centre: false,
    style: STYLE,
    lineweights: false,
    output: null,
  };
  expect(core.calls).toContain(`plot_and_print ${JSON.stringify({ job, printer: "Office" })}`);
  expect(core.calls).toContain(`load_plot_style {"path":"/s/mono.ctb"}`);
  expect(deps.setNotice.mock.calls).toEqual([[null], ["plot.plotting", { waiting: true }], ['plot.sentTo {"printer":"Office"}']]);
  expect(deps.setError).toHaveBeenLastCalledWith(null);
});

test("Quick print without a kept setup: A4, landscape for a wide view, and no plot style", async () => {
  const { result } = setup();
  await act(() => result.current.quickPrint());
  const printed = core.calls.find((c) => c.startsWith("plot_and_print"))!;
  expect(JSON.parse(printed.slice("plot_and_print ".length))).toEqual({
    job: {
      sheet: "*Model_Space",
      area: { window: [[0, 0], [40, 20]] },
      paper: PAPERS[1],
      orientation: "landscape",
      scale: "fit",
      centre: true,
      style: null,
      lineweights: true,
      output: null,
    },
    printer: "Office",
  });
  expect(core.calls.some((c) => c.startsWith("load_plot_style"))).toBe(false);
});

test("Print all: one print job per layout, each with its kept setup, on the default printer", async () => {
  const { result, deps, hook } = setup({ sheet: "*Paper_Space0" });
  act(() => result.current.openPlot());
  act(() => result.current.keep({ ...KEPT, area: "window", window: [[1, 1], [9, 9]] }, null));
  // Kept by the layout's name: Layout2's.
  expect(JSON.parse(localStorage.getItem(`qiyas.plot:${PLAN}`)!).sheets).toHaveProperty("Layout2");
  hook.rerender({ ...deps, sheet: "*Model_Space" });
  core.calls.length = 0;

  await act(() => result.current.printAll());
  const printed = core.calls.filter((c) => c.startsWith("plot_and_print")).map((c) => JSON.parse(c.slice(15)));
  expect(printed.map((p) => [p.job.sheet, p.job.area, p.job.paper.name, p.printer])).toEqual([
    ["*Paper_Space", "layout", "ISO A4", "Office"],
    // A kept window can't be checked against this revision of the layout: the layout plots.
    ["*Paper_Space0", "layout", "ISO A3", "Office"],
  ]);
  expect(deps.setNotice).toHaveBeenLastCalledWith(
    'plot.printedAll {"n":2,"said":"plot.sentTo {\\"printer\\":\\"Office\\"}"}',
  );
});

test("Print all goes on past a layout that fails, then says which", async () => {
  core.answers.plot_and_print = (a: { job: { sheet: string } }) =>
    a.job.sheet === "*Paper_Space" ? Promise.reject("Nothing to plot: the area is empty") : "Sent to Office";
  const { result, deps } = setup();
  await act(() => result.current.printAll());
  expect(core.calls.filter((c) => c.startsWith("plot_and_print"))).toHaveLength(2);
  expect(deps.setError).toHaveBeenLastCalledWith(
    'plot.printAllFailed {"n":1,"total":2,"errors":"Layout1: plot.err.nothing"}',
  );
  // "Plotting…" goes with the failure.
  expect(deps.setNotice).toHaveBeenLastCalledWith(null);
});

test("Print all stops before its next layout when another drawing comes on screen", async () => {
  const { result, deps } = setup();
  core.answers.plot_and_print = () => {
    // The user switches file tab while the first layout prints.
    deps.switchGen.current++;
    return "Sent to Office";
  };
  await act(() => result.current.printAll());
  expect(core.calls.filter((c) => c.startsWith("plot_and_print"))).toHaveLength(1);
  expect(deps.setError).toHaveBeenLastCalledWith("plot.stoppedSwitch");
  expect(deps.setNotice).toHaveBeenLastCalledWith(null);
});

test("Quick print stops when another drawing comes on screen before it plots", async () => {
  const { result, deps } = setup();
  core.answers.default_printer = () => {
    deps.switchGen.current++;
    return "Office";
  };
  await act(() => result.current.quickPrint());
  expect(core.calls.some((c) => c.startsWith("plot_and_print"))).toBe(false);
  expect(deps.setError).toHaveBeenLastCalledWith("plot.stoppedSwitch");
});

test("a second press while printing starts nothing", async () => {
  const { result } = setup();
  await act(async () => {
    await Promise.all([result.current.printAll(), result.current.printAll(), result.current.quickPrint()]);
  });
  expect(core.calls.filter((c) => c.startsWith("plot_and_print"))).toHaveLength(2);
  // Once it has ended, another runs.
  await act(() => result.current.quickPrint());
  expect(core.calls.filter((c) => c.startsWith("plot_and_print"))).toHaveLength(3);
});

test("a kept setup whose paper the catalog no longer has is not used", async () => {
  const { result } = setup({ sheet: "*Paper_Space" });
  act(() => result.current.openPlot());
  act(() => result.current.keep({ ...KEPT, paper: "ISO B9", area: "layout" }, null));
  core.calls.length = 0;
  await act(() => result.current.printAll());
  const printed = core.calls.filter((c) => c.startsWith("plot_and_print")).map((c) => JSON.parse(c.slice(15)));
  expect(printed[0].job).toMatchObject({ sheet: "*Paper_Space", paper: PAPERS[1], scale: "fit", centre: true });
});

test("another sheet drops a plot window being clicked, and the dialog", () => {
  const { result, deps, hook } = setup();
  act(() => result.current.openPlot());
  act(() => result.current.startWindowPick());
  act(() => {
    result.current.pickPoint([1, 2]);
  });
  hook.rerender({ ...deps, sheet: "*Paper_Space" });
  expect(result.current.windowPick).toBeNull();
  expect(result.current.plotAsk).toBeNull();
});

test("Print all without layouts says so", async () => {
  const { result, deps } = setup({ info: info(PLAN, []) });
  await act(() => result.current.printAll());
  expect(deps.setError).toHaveBeenLastCalledWith("plot.noLayouts");
  expect(core.calls).toEqual([]);
});

test("the plot style editor opens on the drawing's plot style; its files get their extension", async () => {
  const { result, hook, deps } = setup();
  act(() => result.current.openStyles());
  expect(result.current.stylesAsk).toEqual({ initialPath: null });
  act(() => result.current.closeStyles());
  expect(result.current.stylesAsk).toBeNull();

  act(() => result.current.openPlot());
  act(() => result.current.keep(KEPT, "/s/mono.ctb"));
  act(() => result.current.openStyles());
  expect(result.current.stylesAsk).toEqual({ initialPath: "/s/mono.ctb" });
  // Without a drawing it opens empty.
  hook.rerender({ ...deps, info: null });
  act(() => result.current.openStyles());
  expect(result.current.stylesAsk).toEqual({ initialPath: null });

  dialogs.save = "/s/copy";
  expect(await result.current.chooseStyleSave("ctb", "/s/mono.ctb")).toBe("/s/copy.ctb");
  dialogs.save = "/s/copy.STB";
  expect(await result.current.chooseStyleSave("stb", "/s/x.stb")).toBe("/s/copy.STB");
  dialogs.save = "/d/out/plan";
  expect(await result.current.choosePdf("/d/plan.pdf")).toBe("/d/out/plan.pdf");
  dialogs.save = null;
  expect(await result.current.choosePdf("/d/plan.pdf")).toBeNull();
  dialogs.open = "/s/mono.ctb";
  expect(await result.current.chooseStyle()).toBe("/s/mono.ctb");
});
