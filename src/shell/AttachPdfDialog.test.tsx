// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { AttachPdfDialog, CalibrateDialog, scaleFor, sheetName } from "./AttachPdfDialog";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  // jsdom draws nothing; the thumbnails are not looked at.
  HTMLCanvasElement.prototype.getContext = (() => null) as never;
});

afterEach(cleanup);

const thumb = (page: number) => ({ page, width: 2, height: 1, data: new Uint8Array(8) });

function setup(over: Partial<Parameters<typeof AttachPdfDialog>[0]> = {}) {
  const props = {
    units: "metres" as const,
    picked: null,
    picking: false,
    progress: null,
    chooseFile: vi.fn(async () => "/jobs/plans.pdf"),
    pageCount: vi.fn(async () => 45),
    pageSize: vi.fn(async (): Promise<[number, number]> => [2384, 1684]),
    thumbnails: vi.fn(async (_: string, first: number, count: number) =>
      Array.from({ length: Math.min(count, 46 - first) }, (_, i) => thumb(first + i)),
    ),
    onPickPoint: vi.fn(),
    onAttach: vi.fn(async () => {}),
    onClose: vi.fn(),
    ...over,
  };
  const view = render(<AttachPdfDialog {...props} />);
  return { props, view };
}

test("scales and sheets follow the page and the drawing's units", () => {
  // 1:100 in a metre drawing: one point (0.3528 mm) is 35.28 mm, 0.03528 m.
  expect(scaleFor(100, "metres")).toBeCloseTo(0.0352778, 6);
  expect(scaleFor(50, "millimetres")).toBeCloseTo(17.6389, 3);
  expect(scaleFor(100, "unknown")).toBe(scaleFor(100, "millimetres"));
  expect(sheetName(2384, 1684)).toBe("A1");
  expect(sheetName(595, 842)).toBe("A4");
  expect(sheetName(200, 100)).toBe("71 × 35 mm");
});

test("a page of a long PDF is chosen 20 at a time and attached at a picked point", async () => {
  const { props, view } = setup();
  const attach = screen.getByRole("button", { name: "Attach" }) as HTMLButtonElement;
  expect(attach.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  expect(await screen.findByText("Pages 1–20 of 45")).toBeTruthy();
  await waitFor(() => expect(props.thumbnails).toHaveBeenCalledWith("/jobs/plans.pdf", 1, 20));
  expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(20);
  expect(await screen.findByText(/page A1/)).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Next pages" }));
  fireEvent.click(screen.getByRole("button", { name: "Next pages" }));
  expect(screen.getByText("Pages 41–45 of 45")).toBeTruthy();
  await waitFor(() => expect(props.thumbnails).toHaveBeenCalledWith("/jobs/plans.pdf", 41, 20));
  // Pages 21–40 were asked for on the way; going back to them asks nothing more.
  expect(props.thumbnails).toHaveBeenCalledTimes(3);
  fireEvent.click(screen.getByRole("button", { name: "Previous pages" }));
  expect(screen.getByText("Pages 21–40 of 45")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Next pages" }));
  await waitFor(() =>
    expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(5),
  );
  expect(props.thumbnails).toHaveBeenCalledTimes(3);
  fireEvent.click(screen.getByRole("option", { name: "Page 44" }));
  expect(screen.getByRole("option", { name: "Page 44" }).getAttribute("aria-selected")).toBe("true");

  // Click in the drawing: the dialog steps aside without closing, and comes back with the point.
  fireEvent.click(screen.getByRole("button", { name: "Click in the drawing" }));
  expect(props.onPickPoint).toHaveBeenCalled();
  view.rerender(<AttachPdfDialog {...props} picking />);
  expect((document.querySelector("dialog") as HTMLDialogElement).open).toBe(false);
  view.rerender(<AttachPdfDialog {...props} picked={[12.5, -3]} />);
  expect((document.querySelector("dialog") as HTMLDialogElement).open).toBe(true);
  expect(props.onClose).not.toHaveBeenCalled();
  expect((screen.getByRole("spinbutton", { name: "X" }) as HTMLInputElement).value).toBe("12.5");

  fireEvent.click(screen.getByRole("radio", { name: "Import: lines and text" }));
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "200" } });
  fireEvent.click(attach);
  await waitFor(() => expect(props.onClose).toHaveBeenCalled());
  expect(props.onAttach).toHaveBeenCalledWith({
    path: "/jobs/plans.pdf",
    page: 44,
    at: [12.5, -3],
    scale: scaleFor(200, "metres"),
    mode: "import",
  });
});

test("a custom scale is drawing units per point, and a failure stays in the dialog", async () => {
  const onAttach = vi.fn(async () => {
    throw "PDF support is not installed: run scripts/pdfium.sh";
  });
  const { props } = setup({ onAttach, progress: 40 });
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  await screen.findByText("Pages 1–20 of 45");
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "custom" } });
  fireEvent.change(screen.getByRole("spinbutton", { name: "Drawing units per PDF point" }), {
    target: { value: "0.5" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Attach" }));
  expect((await screen.findByRole("alert")).textContent).toBe(
    "PDF support is unavailable in this installation. Contact support.",
  );
  expect(onAttach).toHaveBeenCalledWith(expect.objectContaining({ scale: 0.5, mode: "underlay" }));
  expect(props.onClose).not.toHaveBeenCalled();
});

test("while the page is drawn a labelled bar shows how far it is; before there is a percentage the dots wait beside the words", async () => {
  let finish = () => {};
  const onAttach = vi.fn(() => new Promise<void>((done) => (finish = done)));
  const { view, props } = setup({ onAttach, progress: null });
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  await screen.findByText("Pages 1–20 of 45");
  fireEvent.click(screen.getByRole("button", { name: "Attach" }));
  // Reading the PDF: no percentage, so dots beside the words and no bar.
  const reading = await screen.findByText("Reading the PDF…");
  expect(reading.querySelector(".dot-loader.dot-pulse")?.getAttribute("aria-hidden")).toBe("true");
  expect(screen.queryByRole("progressbar")).toBeNull();
  // Drawing the page: a bar with its label and the percentage, no dots.
  view.rerender(<AttachPdfDialog {...props} progress={40} />);
  const bar = await screen.findByRole("progressbar", { name: "Drawing the page…" });
  expect(bar.getAttribute("aria-valuenow")).toBe("40");
  expect(bar.textContent).toContain("40 %");
  expect(screen.queryByText("Reading the PDF…")).toBeNull();
  finish();
});

test("a PDF that can't be read says why", async () => {
  setup({
    pageCount: vi.fn(async () => {
      throw "This PDF can't be opened: bad header";
    }),
  });
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  expect((await screen.findByRole("alert")).textContent).toBe("The file could not be read. Choose another file or a backup.");
});

test("calibration asks for the real distance in metres and gives drawing units", async () => {
  const onCalibrate = vi.fn(async () => {});
  const onClose = vi.fn();
  render(
    <CalibrateDialog
      points={[
        [0, 0],
        [300, 400],
      ]}
      units="millimetres"
      onCalibrate={onCalibrate}
      onClose={onClose}
    />,
  );
  expect(screen.getByText("On the page now: 0.500 m")).toBeTruthy();
  const apply = screen.getByRole("button", { name: "Calibrate" }) as HTMLButtonElement;
  expect(apply.disabled).toBe(true);
  fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "4" } });
  fireEvent.click(apply);
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(onCalibrate).toHaveBeenCalledWith(4000);
});
