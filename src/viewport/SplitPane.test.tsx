// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import * as api from "../api";
import { SplitPane } from "./SplitPane";

vi.mock("./Viewport", () => ({ Viewport: () => null }));
vi.mock("./batch", () => ({ parseBatch: (batch: unknown) => batch }));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test("a split view skips busy refreshes and reads again when the main drawing changes", async () => {
  const version = vi.spyOn(api, "sheetBatchVersion").mockRejectedValueOnce("workspace_busy").mockResolvedValue(1);
  vi.spyOn(api, "sheetBatch").mockResolvedValue({} as ArrayBuffer);
  vi.spyOn(api, "paperViews").mockResolvedValue([]);
  const onError = vi.fn();
  const syncAtlas = vi.fn();
  const props = {
    sheets: [{ name: "Model", block: "model" }, { name: "Layout", block: "layout" }],
    mainSheet: "model", mainBatch: null, mainInset: null, atlas: [], theme: "light", syncAtlas, onError, onClose() {},
  } as unknown as Parameters<typeof SplitPane>[0];
  const view = render(<SplitPane {...props} />);
  await act(async () => fireEvent.change(screen.getByRole("combobox"), { target: { value: "layout" } }));
  expect(onError).not.toHaveBeenCalled();
  expect(syncAtlas).not.toHaveBeenCalled();
  await act(async () => view.rerender(<SplitPane {...props} mainBatch={{} as NonNullable<typeof props.mainBatch>} />));
  expect(version).toHaveBeenCalledTimes(2);
  expect(syncAtlas).toHaveBeenCalledOnce();
});
