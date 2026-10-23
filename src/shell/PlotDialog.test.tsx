// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import type { Paper, PlotStyle } from "../api";
import {
  defaultSetup,
  jobFor,
  pageLayout,
  PlotDialog,
  type PageSetup,
  type PreviewBatch,
} from "./PlotDialog";

const PAPERS: Paper[] = [
  { name: "ISO A3", w_mm: 297, h_mm: 420 },
  { name: "ISO A4", w_mm: 210, h_mm: 297 },
  { name: "ANSI B", w_mm: 279.4, h_mm: 431.8 },
];

const STYLE: PlotStyle = {
  kind: "ctb",
  description: "",
  scale_factor: 1,
  apply_factor: false,
  display_units: 0,
  styles: [],
  lineweights: [0, 0.25],
  extra: [],
};

const MODEL = { name: "Model", block: "*Model_Space", is_model: true };
const LAYOUT = { name: "Layout1", block: "*Paper_Space", is_model: false };

/** What the canvases were asked to draw; `null` contexts draw nothing. */
const drawn = { moveTo: 0, drawImage: 0 };
let fakeCanvas = false;

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  HTMLCanvasElement.prototype.getContext = function () {
    if (!fakeCanvas) return null;
    const noop = () => {};
    return {
      clearRect: noop,
      fillRect: noop,
      strokeRect: noop,
      beginPath: noop,
      lineTo: noop,
      stroke: noop,
      save: noop,
      restore: noop,
      rect: noop,
      clip: noop,
      setLineDash: noop,
      moveTo: () => drawn.moveTo++,
      drawImage: () => drawn.drawImage++,
    };
  } as never;
});

beforeEach(() => {
  drawn.moveTo = 0;
  drawn.drawImage = 0;
  fakeCanvas = false;
});

afterEach(cleanup);

const SETUP: PageSetup = {
  area: "extents",
  window: null,
  paper: "ISO A3",
  orientation: "landscape",
  scale: [1, 100],
  centre: true,
  lineweights: true,
};

function setup(over: Partial<Parameters<typeof PlotDialog>[0]> = {}) {
  const props = {
    sheet: MODEL,
    drawingPath: "/jobs/plan.dwg",
    units: "millimetres" as const,
    batch: null as PreviewBatch | null,
    setup: SETUP,
    stylePath: null as string | null,
    picked: null,
    picking: false,
    papers: vi.fn(async () => PAPERS),
    printers: vi.fn(async () => ({ printers: ["Office", "Plotter"], default: "Plotter" })),
    chooseStyle: vi.fn(async () => "/styles/mono.ctb"),
    loadStyle: vi.fn(async () => STYLE),
    choosePdf: vi.fn(async () => "/jobs/out/plan.pdf"),
    onPickWindow: vi.fn(),
    onKeep: vi.fn(),
    onPlotPdf: vi.fn(async () => {}),
    onPrint: vi.fn(async () => {}),
    onClose: vi.fn(),
    ...over,
  };
  const view = render(<PlotDialog {...props} />);
  return { props, view };
}

const combo = (name: string) => screen.getByRole("combobox", { name }) as HTMLSelectElement;
const plotButton = () => screen.getByRole("button", { name: "Plot to PDF…" }) as HTMLButtonElement;
/** The papers and printers have come in. */
const loaded = () => waitFor(() => expect(plotButton().disabled).toBe(false));

test("it starts from the sheet's saved setup, the catalog's papers and the default printer", async () => {
  const { props } = setup();
  await loaded();
  expect(screen.getByRole("heading", { name: "Plot" })).toBeTruthy();
  expect((screen.getByLabelText("Sheet") as HTMLInputElement).value).toBe("Model");
  expect(combo("What to plot").value).toBe("extents");
  // Model space has no layout area to offer.
  expect([...combo("What to plot").options].map((o) => o.value)).toEqual(["extents", "window"]);
  expect([...combo("Paper").options].map((o) => o.text)).toEqual([
    "ISO A3 (297 × 420 mm)",
    "ISO A4 (210 × 297 mm)",
    "ANSI B (279.4 × 431.8 mm)",
  ]);
  expect(combo("Paper").value).toBe("ISO A3");
  expect(combo("Orientation").value).toBe("landscape");
  expect(combo("Scale").value).toBe("1:100");
  expect((screen.getByLabelText("Centre on the paper") as HTMLInputElement).checked).toBe(true);
  expect((screen.getByLabelText("Plot lineweights") as HTMLInputElement).checked).toBe(true);
  expect(combo("Plot style").value).toBe("");
  expect([...combo("Printer").options].map((o) => o.value)).toEqual(["Office", "Plotter"]);
  expect(combo("Printer").value).toBe("Plotter");
  expect(props.loadStyle).not.toHaveBeenCalled();
});

test("a layout offers its own area, and starts from it", async () => {
  setup({ sheet: LAYOUT, setup: defaultSetup(false) });
  await loaded();
  expect([...combo("What to plot").options].map((o) => o.value)).toEqual(["extents", "window", "layout"]);
  expect(combo("What to plot").value).toBe("layout");
  expect(combo("Scale").value).toBe("fit");
  expect(combo("Paper").value).toBe("ISO A4");
});

test("Plot to PDF offers <drawing>.pdf beside the drawing, plots there, keeps the setup and closes", async () => {
  const { props } = setup();
  await loaded();
  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onClose).toHaveBeenCalled());
  expect(props.choosePdf).toHaveBeenCalledWith("/jobs/plan.pdf");
  expect(props.onPlotPdf).toHaveBeenCalledWith(
    {
      sheet: "*Model_Space",
      area: "extents",
      paper: PAPERS[0],
      orientation: "landscape",
      scale: { ratio: [1, 100] },
      centre: true,
      style: null,
      lineweights: true,
      output: null,
    },
    "/jobs/out/plan.pdf",
  );
  expect(props.onKeep).toHaveBeenCalledWith(SETUP, null);
});

test("a Windows drawing's PDF is offered beside it too; cancelling the Save dialog plots nothing", async () => {
  const { props } = setup({ drawingPath: "C:\\jobs\\villa.v2.dxf", choosePdf: vi.fn(async () => null) });
  await loaded();
  fireEvent.click(plotButton());
  await waitFor(() => expect(props.choosePdf).toHaveBeenCalledWith("C:\\jobs\\villa.v2.pdf"));
  expect(props.onPlotPdf).not.toHaveBeenCalled();
  expect(props.onClose).not.toHaveBeenCalled();
});

test("scale: fit, the presets, and a custom pair (which must be two numbers above 0)", async () => {
  const { props } = setup();
  await loaded();
  expect([...combo("Scale").options].map((o) => o.value)).toEqual([
    "fit",
    "1:1",
    "1:20",
    "1:50",
    "1:100",
    "1:200",
    "1:500",
    "custom",
  ]);
  fireEvent.change(combo("Scale"), { target: { value: "custom" } });
  // The custom pair starts from the scale chosen before.
  expect((screen.getByLabelText("Paper mm") as HTMLInputElement).value).toBe("1");
  expect((screen.getByLabelText("Drawing mm") as HTMLInputElement).value).toBe("100");
  fireEvent.change(screen.getByLabelText("Paper mm"), { target: { value: "2" } });
  fireEvent.change(screen.getByLabelText("Drawing mm"), { target: { value: "0" } });
  expect(plotButton().disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("Drawing mm"), { target: { value: "75" } });
  fireEvent.change(combo("Orientation"), { target: { value: "portrait" } });
  fireEvent.change(combo("Paper"), { target: { value: "ISO A4" } });
  fireEvent.click(screen.getByLabelText("Centre on the paper"));
  fireEvent.click(screen.getByLabelText("Plot lineweights"));
  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onPlotPdf).toHaveBeenCalled());
  expect(props.onPlotPdf).toHaveBeenCalledWith(
    expect.objectContaining({
      paper: PAPERS[1],
      orientation: "portrait",
      scale: { ratio: [2, 75] },
      centre: false,
      lineweights: false,
    }),
    "/jobs/out/plan.pdf",
  );
  expect(props.onKeep).toHaveBeenCalledWith(
    { ...SETUP, paper: "ISO A4", orientation: "portrait", scale: [2, 75], centre: false, lineweights: false },
    null,
  );
});

test("fit plots as large as the paper takes", async () => {
  const { props } = setup();
  await loaded();
  fireEvent.change(combo("Scale"), { target: { value: "fit" } });
  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onPlotPdf).toHaveBeenCalledWith(expect.objectContaining({ scale: "fit" }), "/jobs/out/plan.pdf"));
});

test("a window: the dialog steps aside for two clicks in the drawing, then plots that window", async () => {
  const { props, view } = setup();
  await loaded();
  fireEvent.change(combo("What to plot"), { target: { value: "window" } });
  // No window yet: nothing to plot.
  expect(plotButton().disabled).toBe(true);
  expect(screen.getByText("No window yet")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Pick window" }));
  expect(props.onPickWindow).toHaveBeenCalled();
  view.rerender(<PlotDialog {...props} picking />);
  expect(props.onClose).not.toHaveBeenCalled();
  view.rerender(<PlotDialog {...props} picked={[[30, 40], [10.25, 5]]} />);
  expect(screen.getByText("10.25, 5 – 30, 40")).toBeTruthy();

  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onPlotPdf).toHaveBeenCalled());
  expect(props.onPlotPdf).toHaveBeenCalledWith(
    expect.objectContaining({ area: { window: [[10.25, 5], [30, 40]] } }),
    "/jobs/out/plan.pdf",
  );
  expect(props.onKeep).toHaveBeenCalledWith(
    { ...SETUP, area: "window", window: [[10.25, 5], [30, 40]] },
    null,
  );
});

test("a saved window comes back with the setup", async () => {
  const { props } = setup({ setup: { ...SETUP, area: "window", window: [[0, 0], [8, 6]] } });
  await loaded();
  expect(screen.getByText("0, 0 – 8, 6")).toBeTruthy();
  fireEvent.click(plotButton());
  await waitFor(() =>
    expect(props.onPlotPdf).toHaveBeenCalledWith(expect.objectContaining({ area: { window: [[0, 0], [8, 6]] } }), "/jobs/out/plan.pdf"),
  );
});

test("a plot style file is chosen, read and plotted with; None plots the colours as drawn", async () => {
  const { props } = setup();
  await loaded();
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  await waitFor(() => expect(combo("Plot style").value).toBe("/styles/mono.ctb"));
  expect(props.loadStyle).toHaveBeenCalledWith("/styles/mono.ctb");
  expect(combo("Plot style").selectedOptions[0].text).toBe("mono.ctb");

  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onPlotPdf).toHaveBeenCalledTimes(1));
  expect(props.onPlotPdf).toHaveBeenLastCalledWith(expect.objectContaining({ style: STYLE }), "/jobs/out/plan.pdf");
  expect(props.onKeep).toHaveBeenLastCalledWith(SETUP, "/styles/mono.ctb");
});

test("choosing None drops the style", async () => {
  const { props } = setup({ stylePath: "/styles/mono.ctb" });
  await loaded();
  expect(props.loadStyle).toHaveBeenCalledWith("/styles/mono.ctb");
  expect(combo("Plot style").value).toBe("/styles/mono.ctb");
  fireEvent.change(combo("Plot style"), { target: { value: "" } });
  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onPlotPdf).toHaveBeenCalledWith(expect.objectContaining({ style: null }), "/jobs/out/plan.pdf"));
  expect(props.onKeep).toHaveBeenLastCalledWith(SETUP, null);
});

test("a remembered plot style that can't be read says why, and plots without it", async () => {
  const { props } = setup({
    stylePath: "/styles/gone.ctb",
    loadStyle: vi.fn(async () => {
      throw "Can't read /styles/gone.ctb: No such file";
    }),
  });
  await loaded();
  expect(screen.getByRole("alert").textContent).toBe(
    "The plot style gone.ctb can't be read: Can't read /styles/gone.ctb: No such file",
  );
  expect(combo("Plot style").value).toBe("");
  fireEvent.click(plotButton());
  await waitFor(() => expect(props.onPlotPdf).toHaveBeenCalledWith(expect.objectContaining({ style: null }), "/jobs/out/plan.pdf"));
});

test("Print sends the job to the printer chosen", async () => {
  const { props } = setup();
  await loaded();
  fireEvent.change(combo("Printer"), { target: { value: "Office" } });
  fireEvent.click(screen.getByRole("button", { name: "Print" }));
  await waitFor(() => expect(props.onClose).toHaveBeenCalled());
  expect(props.onPrint).toHaveBeenCalledWith(expect.objectContaining({ sheet: "*Model_Space", output: null }), "Office");
  expect(props.choosePdf).not.toHaveBeenCalled();
  expect(props.onKeep).toHaveBeenCalledWith(SETUP, null);
});

test("without printers, Print goes to the default one", async () => {
  const { props } = setup({ printers: vi.fn(async () => ({ printers: [], default: null })) });
  await loaded();
  expect([...combo("Printer").options].map((o) => o.text)).toEqual(["Default printer"]);
  fireEvent.click(screen.getByRole("button", { name: "Print" }));
  await waitFor(() => expect(props.onPrint).toHaveBeenCalledWith(expect.anything(), null));
});

test("a plot that fails says why and stays open", async () => {
  let fail = (_reason: string) => {};
  const { props } = setup({
    onPlotPdf: vi.fn(() => new Promise<void>((_, reject) => (fail = reject))),
  });
  await loaded();
  fireEvent.click(plotButton());
  // Plotting runs off the drawing: the dialog says so while it waits.
  expect((await screen.findByText("Plotting…")).querySelector(".dot-loader")).toBeTruthy();
  fail("Nothing to plot: the area is empty");
  expect((await screen.findByRole("alert")).textContent).toBe("Nothing to plot: the area is empty");
  expect(screen.queryByText("Plotting…")).toBeNull();
  expect(props.onClose).not.toHaveBeenCalled();
  expect(plotButton().disabled).toBe(false);
});

test("Cancel closes without keeping anything", async () => {
  const { props } = setup();
  await loaded();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(props.onClose).toHaveBeenCalled();
  expect(props.onKeep).not.toHaveBeenCalled();
});

test("the page layout follows the plot: fit inside 5 mm and centred, a ratio at its size", () => {
  const a4 = PAPERS[1];
  // 100 x 50 units fitted on A4 portrait: 2 mm a unit (200 mm across), centred.
  expect(pageLayout(a4, "portrait", "fit", true, [[0, 0], [100, 50]], 1)).toEqual({
    page: [210, 297],
    rect: [[5, 98.5], [205, 198.5]],
    fits: true,
  });
  // Not centred: the lower left at the margin (2 mm a unit: 287 / 143.5).
  expect(pageLayout(a4, "landscape", "fit", false, [[10, 10], [153.5, 60]], 1)?.rect).toEqual([
    [5, 5],
    [292, 105],
  ]);
  // 1:100 of a metre drawing: 10 mm a unit; 30 x 10 m is 300 x 100 mm, wider than A4 portrait.
  const big = pageLayout(a4, "portrait", { ratio: [1, 100] }, false, [[0, 0], [30, 10]], 1000);
  expect(big?.rect).toEqual([[0, 0], [300, 100]]);
  expect(big?.fits).toBe(false);
  // Turned, it fits.
  expect(pageLayout(a4, "landscape", { ratio: [1, 100] }, true, [[0, 0], [28, 10]], 1000)?.fits).toBe(true);
  // An empty area has no layout.
  expect(pageLayout(a4, "portrait", "fit", true, [[3, 3], [3, 3]], 1)).toBeNull();
});

test("the preview says when part of the area falls off the paper", async () => {
  // 28 x 10 m at 1:100 is 280 mm wide: off an A4 portrait page, on a landscape one.
  const batch: PreviewBatch = {
    segments: new Float32Array([0, 0, 28, 10]),
    colors: new Uint32Array([0xffffff]),
    origin: [0, 0],
    bounds: [[0, 0], [28, 10]],
  };
  setup({ units: "metres", batch, setup: { ...SETUP, paper: "ISO A4", orientation: "portrait" } });
  await loaded();
  expect(screen.getByText("Part of the area falls off the paper at this scale")).toBeTruthy();
  fireEvent.change(combo("Orientation"), { target: { value: "landscape" } });
  expect(screen.queryByText("Part of the area falls off the paper at this scale")).toBeNull();
});

test("the preview draws the area's lines once per area while the dialog is open", async () => {
  fakeCanvas = true;
  const batch: PreviewBatch = {
    // Two segments; only the first is inside the window picked later.
    segments: new Float32Array([0, 0, 1, 1, 50, 50, 60, 60]),
    colors: new Uint32Array([0xff0000, 0xffffff]),
    origin: [0, 0],
    bounds: [[0, 0], [60, 60]],
  };
  const { props, view } = setup({ batch });
  await loaded();
  // Loading the paper catalog can finish before the portal's canvas effect runs.
  await waitFor(() => {
    expect(drawn.moveTo).toBe(2);
    expect(drawn.drawImage).toBeGreaterThan(0);
  });
  const blits = drawn.drawImage;

  // Another paper and orientation reuse the picture of the area.
  fireEvent.change(combo("Paper"), { target: { value: "ISO A4" } });
  fireEvent.change(combo("Orientation"), { target: { value: "portrait" } });
  await waitFor(() => expect(drawn.drawImage).toBeGreaterThan(blits));
  expect(drawn.moveTo).toBe(2);

  // A window is another area: drawn once, then reused.
  fireEvent.change(combo("What to plot"), { target: { value: "window" } });
  view.rerender(<PlotDialog {...props} picked={[[-1, -1], [2, 2]]} />);
  await waitFor(() => expect(drawn.moveTo).toBe(3));
  const windowBlits = drawn.drawImage;
  fireEvent.change(combo("Scale"), { target: { value: "1:50" } });
  await waitFor(() => expect(drawn.drawImage).toBeGreaterThan(windowBlits));
  expect(drawn.moveTo).toBe(3);
});

test("the setup without a saved one: A4 landscape, fitted, centred, with lineweights", () => {
  expect(defaultSetup(true)).toEqual({
    area: "extents",
    window: null,
    paper: "ISO A4",
    orientation: "landscape",
    scale: "fit",
    centre: true,
    lineweights: true,
  });
  expect(defaultSetup(false).area).toBe("layout");
});

test("a job from a setup: an unknown paper falls back to A4, a window without corners to the extents", () => {
  const job = jobFor("*Model_Space", { ...SETUP, paper: "Gone", area: "window", window: null }, PAPERS, null, null);
  expect(job.paper).toEqual(PAPERS[1]);
  expect(job.area).toBe("extents");
});

test("a kept setup whose paper the catalog no longer has starts from the defaults", async () => {
  const { props } = setup({ setup: { ...SETUP, paper: "ISO B9", scale: [1, 50], centre: false } });
  await loaded();
  expect(combo("Paper").value).toBe("ISO A4");
  expect(combo("Scale").value).toBe("fit");
  expect((screen.getByLabelText("Centre on the paper") as HTMLInputElement).checked).toBe(true);
  fireEvent.click(plotButton());
  await waitFor(() =>
    expect(props.onPlotPdf).toHaveBeenCalledWith(
      expect.objectContaining({ paper: PAPERS[1], scale: "fit", centre: true }),
      "/jobs/out/plan.pdf",
    ),
  );
});

test("while plotting, a second click starts nothing and Escape doesn't close the dialog", async () => {
  let finish = () => {};
  const { props } = setup({
    onPlotPdf: vi.fn(() => new Promise<void>((done) => (finish = done))),
  });
  await loaded();
  fireEvent.click(plotButton());
  fireEvent.click(plotButton());
  await screen.findByText("Plotting…");
  expect(props.choosePdf).toHaveBeenCalledTimes(1);
  // Escape is ignored while it plots: the dialog is still there and was not asked to close.
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(props.onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeTruthy();
  finish();
  await waitFor(() => expect(props.onClose).toHaveBeenCalled());
  expect(props.onPlotPdf).toHaveBeenCalledTimes(1);
});

test("the layout area's preview says the layout plots on its own paper", async () => {
  setup({ sheet: LAYOUT, setup: defaultSetup(false) });
  await loaded();
  expect(screen.getByText("The preview shows what is drawn; the layout plots on its own paper")).toBeTruthy();
});
