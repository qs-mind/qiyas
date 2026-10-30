// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useStickToBottom } from "./useStickToBottom";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

function Thread({ version = 0 }: { version?: number }) {
  const thread = useStickToBottom([version]);
  return <><div data-testid="log" ref={thread.ref} onScroll={thread.onScroll} onWheel={thread.onWheel} onKeyDown={thread.onKeyDown}>
    <p>Content</p>
  </div>{thread.away && <button onClick={() => thread.jump()}>Latest</button>}</>;
}

test("resize and content changes recompute geometry after intent stopped following", () => {
  let resize: () => void = () => {};
  const observe = vi.fn();
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize = callback; }
    observe = observe;
    disconnect() {}
  });
  const view = render(<Thread />);
  const log = screen.getByTestId("log");
  const size = { height: 400, content: 400, top: 0 };
  Object.defineProperties(log, {
    clientHeight: { get: () => size.height },
    scrollHeight: { get: () => size.content },
    scrollTop: { get: () => size.top, set: value => { size.top = value; } },
  });
  fireEvent.keyDown(log, { key: "PageUp" });
  expect(screen.queryByRole("button")).toBeNull();
  size.content = 600;
  act(() => resize());
  expect(screen.getByRole("button")).toBeTruthy();
  expect(observe).toHaveBeenCalledWith(log.firstElementChild);
  size.height = 600;
  act(() => resize());
  expect(screen.queryByRole("button")).toBeNull();
  size.content = 800;
  view.rerender(<Thread version={1} />);
  expect(screen.getByRole("button")).toBeTruthy();
  size.top = 200;
  fireEvent.scroll(log);
  expect(screen.queryByRole("button")).toBeNull();
});
