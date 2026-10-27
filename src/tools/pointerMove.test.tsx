// @vitest-environment jsdom
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Drafting } from "../api";
import { usePointer } from "./pointer";
import { useTool, type ToolApi } from "./useTool";
import type { Tool } from "../viewport/Viewport";

afterEach(cleanup);

const api: ToolApi = {
  pick: async () => null,
  pickWindow: async () => [],
  snap: async () => null,
  entityInfo: vi.fn(),
  run: () => {},
  zoomTo: () => {},
  boundary: vi.fn(),
  hatchScale: () => 1,
  undo: () => {},
  onError: () => {},
};
const drafting = { osnap: false, snap_modes: {}, ortho: false, polar: false, polar_step_deg: 45, grid: false, grid_spacing: 10, grid_snap: false, precision: 3 } as unknown as Drafting;

/** Renders `useTool` and the status bar's reader, and counts how often each renders. */
function mount(tool: Tool, osnap = false) {
  const counts = { root: 0, bar: 0 };
  let current!: ReturnType<typeof useTool>;
  let cursor: unknown = null;
  function Bar({ pointer }: { pointer: ReturnType<typeof useTool>["pointer"] }) {
    counts.bar += 1;
    cursor = usePointer(pointer)?.cursor;
    return null;
  }
  function Root() {
    counts.root += 1;
    current = useTool(tool, api, { ...drafting, osnap });
    return <Bar pointer={current.pointer} />;
  }
  render(<Root />);
  return { counts, tool: () => current, cursor: () => cursor };
}

test("a pointer move renders only the status bar's reader while no tool needs the cursor", () => {
  const m = mount("select");
  const before = { ...m.counts };
  for (let i = 0; i < 20; i++) act(() => m.tool().onMove([i, i * 2], 0.01));
  expect(m.counts.root).toBe(before.root);
  expect(m.counts.bar).toBe(before.bar + 20);
  expect(m.cursor()).toEqual([19, 38]);
});

test("a tool in progress still follows the cursor with its preview, as before", () => {
  const m = mount("distance", true);
  const before = m.counts.root;
  act(() => m.tool().onMove([1, 1], 0.01));
  act(() => m.tool().onMove([2, 2], 0.01));
  expect(m.counts.root).toBeGreaterThan(before);
  expect(m.tool().cursor).toEqual([2, 2]);
  expect(m.cursor()).toEqual([2, 2]);
});
