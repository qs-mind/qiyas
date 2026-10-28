// @vitest-environment jsdom
import { expect, test, vi } from "vitest";
import { TRACE_EVENT, trace } from "./trace";

test("trace preserves the selector, line and export drawing in the window event", () => {
  const listener = vi.fn();
  window.addEventListener(TRACE_EVENT, listener);
  try {
    trace({ item: "generated:W", line: "dimension-7", drawing: "/plans/source.dxf" });
    expect(TRACE_EVENT).toBe("qiyas:trace");
    expect(listener).toHaveBeenCalledTimes(1);
    expect((listener.mock.calls[0][0] as CustomEvent).detail).toEqual({
      item: "generated:W", line: "dimension-7", drawing: "/plans/source.dxf",
    });
  } finally { window.removeEventListener(TRACE_EVENT, listener); }
});

test.each(["generated:W", "boq:W", "boq-row:0", "takeoff:7", "project:0"])("item trace %s omits a line and normalizes an unknown drawing", item => {
  const listener = vi.fn();
  window.addEventListener(TRACE_EVENT, listener);
  try {
    trace({ item });
    expect((listener.mock.calls[0][0] as CustomEvent).detail).toEqual({ item, drawing: null });
    expect(Object.hasOwn((listener.mock.calls[0][0] as CustomEvent).detail, "line")).toBe(false);
  } finally { window.removeEventListener(TRACE_EVENT, listener); }
});
