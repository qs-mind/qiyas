// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import {
  AttachImageDialog,
  defaultImageWidth,
  ImageDisplayPanel,
  NEUTRAL_DISPLAY,
} from "./AttachImageDialog";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

function setup(over: Partial<Parameters<typeof AttachImageDialog>[0]> = {}) {
  const props = {
    units: "metres" as const,
    defaultWidth: 12.5,
    picked: null,
    picking: false,
    chooseFile: vi.fn(async () => "/jobs/scan.png"),
    imageSize: vi.fn(async (): Promise<[number, number]> => [3000, 2000]),
    onPickPoint: vi.fn(),
    onAttach: vi.fn(async () => {}),
    onClose: vi.fn(),
    ...over,
  };
  const view = render(<AttachImageDialog {...props} />);
  return { props, view };
}

test("a new image is a tenth of the sheet wide, or 10 units on an empty sheet", () => {
  expect(defaultImageWidth([[0, 0], [250, 100]])).toBe(25);
  expect(defaultImageWidth([[-50, 0], [50, 0]])).toBe(10);
  expect(defaultImageWidth(null)).toBe(10);
  // 10 m on an empty sheet, in the drawing's units.
  expect(defaultImageWidth(null, 0.001)).toBe(10_000);
  expect(defaultImageWidth([[0, 0], [0, 0]], 0.01)).toBe(1000);
  expect(defaultImageWidth([[0, 0], [0, 0]])).toBe(10);
});

test("an image is chosen, sized and attached at a picked point", async () => {
  const { props, view } = setup();
  const attach = screen.getByRole("button", { name: "Attach" }) as HTMLButtonElement;
  expect(attach.disabled).toBe(true);
  expect((screen.getByLabelText("Width (metres)") as HTMLInputElement).value).toBe("12.5");

  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  expect(await screen.findByText("3000 × 2000 pixels")).toBeTruthy();
  expect(props.imageSize).toHaveBeenCalledWith("/jobs/scan.png");
  expect(attach.disabled).toBe(false);

  // Click in the drawing: the dialog steps aside without closing, and comes back with the point.
  fireEvent.click(screen.getByRole("button", { name: "Click in the drawing" }));
  expect(props.onPickPoint).toHaveBeenCalled();
  view.rerender(<AttachImageDialog {...props} picking />);
  expect(props.onClose).not.toHaveBeenCalled();
  view.rerender(<AttachImageDialog {...props} picked={[4, 7]} />);
  expect((screen.getByLabelText("X") as HTMLInputElement).value).toBe("4");
  expect((screen.getByLabelText("Y") as HTMLInputElement).value).toBe("7");

  fireEvent.change(screen.getByLabelText("Width (metres)"), { target: { value: "30" } });
  fireEvent.click(attach);
  await waitFor(() =>
    expect(props.onAttach).toHaveBeenCalledWith({ path: "/jobs/scan.png", at: [4, 7], width: 30 }),
  );
  await waitFor(() => expect(props.onClose).toHaveBeenCalled());
});

test("a width of 0 can't be attached, and an unreadable file says why", async () => {
  const { props } = setup({
    imageSize: vi.fn(async (): Promise<[number, number]> => {
      throw "This image can't be read: /jobs/scan.png: bad header";
    }),
  });
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  expect((await screen.findByRole("alert")).textContent).toContain("bad header");
  expect((screen.getByRole("button", { name: "Attach" }) as HTMLButtonElement).disabled).toBe(true);
  expect(props.onAttach).not.toHaveBeenCalled();
  cleanup();

  setup({ units: "unknown" });
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  await screen.findByText("3000 × 2000 pixels");
  // Unknown units are millimetres.
  fireEvent.change(screen.getByLabelText("Width (millimetres)"), { target: { value: "0" } });
  expect((screen.getByRole("button", { name: "Attach" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a slider sets the image when let go, once, and Reset goes back to neutral", async () => {
  const onCommit = vi.fn(async () => {});
  const value = { brightness: 50, contrast: 50, fade: 0 };
  render(<ImageDisplayPanel value={value} onCommit={onCommit} onClose={() => {}} />);
  const brightness = screen.getByRole("slider", { name: /Brightness/ });
  fireEvent.change(brightness, { target: { value: "62" } });
  fireEvent.change(brightness, { target: { value: "70" } });
  expect(onCommit).not.toHaveBeenCalled();
  fireEvent.pointerUp(brightness);
  await waitFor(() => expect(onCommit).toHaveBeenCalledWith({ brightness: 70, contrast: 50, fade: 0 }));
  // Leaving it unchanged sets nothing more.
  fireEvent.blur(brightness);
  expect(onCommit).toHaveBeenCalledTimes(1);

  fireEvent.change(screen.getByRole("slider", { name: /Fade/ }), { target: { value: "40" } });
  fireEvent.keyUp(screen.getByRole("slider", { name: /Fade/ }), { key: "ArrowRight" });
  await waitFor(() => expect(onCommit).toHaveBeenLastCalledWith({ brightness: 70, contrast: 50, fade: 40 }));

  fireEvent.click(screen.getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(onCommit).toHaveBeenLastCalledWith(NEUTRAL_DISPLAY));
  expect(onCommit).toHaveBeenCalledTimes(3);
});

test("a refused setting is shown and can be tried again", async () => {
  const onCommit = vi.fn(async () => {
    throw "Brightness, contrast and fade go from 0 to 100";
  });
  render(
    <ImageDisplayPanel
      value={{ brightness: 50, contrast: 50, fade: 0 }}
      onCommit={onCommit}
      onClose={() => {}}
    />,
  );
  const contrast = screen.getByRole("slider", { name: /Contrast/ });
  fireEvent.change(contrast, { target: { value: "80" } });
  fireEvent.pointerUp(contrast);
  expect((await screen.findByRole("alert")).textContent).toContain("0 to 100");
  fireEvent.blur(contrast);
  await waitFor(() => expect(onCommit).toHaveBeenCalledTimes(2));
});
