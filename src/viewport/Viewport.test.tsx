// @vitest-environment jsdom
/** The canvas's mouse and key behaviour, which the shortcut audit (note 69) rests on: the wheel zooms
 *  at the cursor, a middle drag pans, a double middle click shows everything, and Space is the pan key
 *  but a tap of it is AutoCAD's "repeat the last command". The renderer needs WebGL: it is stubbed. */
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Batch } from "./batch";
import { Viewport, type ViewportHandle } from "./Viewport";

vi.mock("./gl", () => ({
  createRenderer: () => new Proxy({}, { get: (_t, name) => (name === "dispose" ? () => {} : () => undefined) }),
}));

const batch = { bounds: [[0, 0], [100, 50]], quads: [], imageTable: [], images: [], origin: [0, 0], pxPerUnit: 1, baseTol: 0, curveTol: 0 } as unknown as Batch;

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 800, height: 600, right: 800, bottom: 600, x: 0, y: 0, toJSON() {} });
  HTMLCanvasElement.prototype.setPointerCapture = vi.fn();
  HTMLCanvasElement.prototype.releasePointerCapture = vi.fn();
  vi.spyOn(performance, "now");
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function mount(extra: { onSpaceTap?(): void } = {}) {
  const ref = createRef<ViewportHandle>();
  const view = render(
    <div className="viewport" tabIndex={0}>
      <Viewport ref={ref} batch={batch} atlas={[]} highlight={[]} tool="select" theme="light" onClick={() => {}} onMove={() => {}} {...extra} />
    </div>,
  );
  const canvas = view.container.querySelector("canvas")!;
  act(() => ref.current!.fitExtents());
  return { ref, canvas, ...view };
}
const screenOf = (ref: React.RefObject<ViewportHandle | null>, p: [number, number]) => ref.current!.toScreen(p);

test("the wheel zooms about the cursor: in for a forward notch, out for a back one, the point under the cursor stays put", () => {
  const { ref, canvas } = mount();
  const before = ref.current!.pxPerUnit();
  const at = screenOf(ref, [30, 20]);
  fireEvent.wheel(canvas, { deltaY: -100, clientX: at[0], clientY: at[1] });
  expect(ref.current!.pxPerUnit()).toBeGreaterThan(before);
  const after = screenOf(ref, [30, 20]);
  expect(after[0]).toBeCloseTo(at[0], 5);
  expect(after[1]).toBeCloseTo(at[1], 5);
  const zoomed = ref.current!.pxPerUnit();
  fireEvent.wheel(canvas, { deltaY: 100, clientX: at[0], clientY: at[1] });
  expect(ref.current!.pxPerUnit()).toBeLessThan(zoomed);
});

test("a middle drag pans the drawing by the distance dragged", () => {
  const { ref, canvas } = mount();
  const start = screenOf(ref, [0, 0]);
  fireEvent.pointerDown(canvas, { button: 1, clientX: 400, clientY: 300, pointerId: 1 });
  fireEvent.pointerMove(canvas, { clientX: 460, clientY: 330, pointerId: 1 });
  fireEvent.pointerUp(canvas, { button: 1, clientX: 460, clientY: 330, pointerId: 1 });
  const moved = screenOf(ref, [0, 0]);
  expect(moved[0] - start[0]).toBeCloseTo(60, 5);
  expect(moved[1] - start[1]).toBeCloseTo(30, 5);
});

test("a double middle click shows the whole drawing again; two slow ones do not", () => {
  const { ref, canvas } = mount();
  const fitted = screenOf(ref, [0, 0]);
  const middle = () => {
    fireEvent.pointerDown(canvas, { button: 1, clientX: 400, clientY: 300, pointerId: 1 });
    fireEvent.pointerUp(canvas, { button: 1, clientX: 400, clientY: 300, pointerId: 1 });
  };
  const clock = vi.mocked(performance.now);
  // Lost the view: zoomed in with the wheel.
  fireEvent.wheel(canvas, { deltaY: -300, clientX: 100, clientY: 100 });
  expect(screenOf(ref, [0, 0])).not.toEqual(fitted);
  clock.mockReturnValue(1000);
  middle();
  clock.mockReturnValue(1100);
  middle();
  const back = screenOf(ref, [0, 0]);
  expect(back[0]).toBeCloseTo(fitted[0], 5);
  expect(back[1]).toBeCloseTo(fitted[1], 5);
  // Slow: no fit.
  fireEvent.wheel(canvas, { deltaY: -300, clientX: 100, clientY: 100 });
  const zoomedIn = screenOf(ref, [0, 0]);
  clock.mockReturnValue(5000);
  middle();
  clock.mockReturnValue(6000);
  middle();
  expect(screenOf(ref, [0, 0])).toEqual(zoomedIn);
});

test("Space is the pan key: held with a drag it pans and repeats nothing; tapped alone over the drawing it repeats the last command", () => {
  const onSpaceTap = vi.fn();
  const { ref, canvas, container } = mount({ onSpaceTap });
  const start = screenOf(ref, [0, 0]);
  fireEvent.keyDown(window, { code: "Space", key: " " });
  fireEvent.pointerDown(canvas, { button: 0, clientX: 400, clientY: 300, pointerId: 1 });
  fireEvent.pointerMove(canvas, { clientX: 440, clientY: 300, pointerId: 1 });
  fireEvent.pointerUp(canvas, { button: 0, clientX: 440, clientY: 300, pointerId: 1 });
  fireEvent.keyUp(window, { code: "Space", key: " " });
  expect(screenOf(ref, [0, 0])[0] - start[0]).toBeCloseTo(40, 5);
  expect(onSpaceTap).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { code: "Space", key: " " });
  fireEvent.keyUp(window, { code: "Space", key: " " });
  expect(onSpaceTap).toHaveBeenCalledOnce();
  // On a field or a button Space is theirs.
  const input = document.createElement("input");
  container.appendChild(input);
  fireEvent.keyDown(input, { code: "Space", key: " " });
  fireEvent.keyUp(input, { code: "Space", key: " " });
  const button = document.createElement("button");
  document.body.appendChild(button);
  fireEvent.keyDown(button, { code: "Space", key: " " });
  fireEvent.keyUp(button, { code: "Space", key: " " });
  expect(onSpaceTap).toHaveBeenCalledOnce();
});
