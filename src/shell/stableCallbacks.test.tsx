// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { memo } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { useStableCallbacks } from "./stableCallbacks";

afterEach(cleanup);

type P = { label: string; onSave?: (n: number) => number; onOpen?: () => void };

test("a memo below it renders for a changed value, not for a new closure, and calls the newest closure", () => {
  let renders = 0;
  const seen: P[] = [];
  const Body = memo(function Body(props: P) {
    renders += 1;
    seen.push(props);
    return null;
  });
  function Above(props: P) {
    return <Body {...useStableCallbacks(props)} />;
  }
  const first = vi.fn((n: number) => n + 1);
  const second = vi.fn((n: number) => n + 100);
  const view = render(<Above label="a" onSave={first} onOpen={() => {}} />);
  view.rerender(<Above label="a" onSave={second} onOpen={() => {}} />);
  expect(renders).toBe(1);
  // The one it was given first is the one it holds, and it runs the newest.
  expect(seen[0].onSave!(1)).toBe(101);
  expect(first).not.toHaveBeenCalled();
  view.rerender(<Above label="b" onSave={second} onOpen={() => {}} />);
  expect(renders).toBe(2);
  expect(seen[1].onSave).toBe(seen[0].onSave);
});

test("a handler that goes missing stays missing: a button the app disabled is not enabled", () => {
  const seen: P[] = [];
  function Above(props: P) {
    seen.push(useStableCallbacks(props));
    return null;
  }
  const view = render(<Above label="a" onSave={() => 1} />);
  expect(typeof seen[0].onSave).toBe("function");
  view.rerender(<Above label="a" />);
  expect(seen[1].onSave).toBeUndefined();
  view.rerender(<Above label="a" onSave={() => 2} />);
  expect(seen[2].onSave!(0)).toBe(2);
});
