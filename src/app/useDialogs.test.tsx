// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { TableInfo } from "../api";
import { DialogsProvider, useDialogOpen, useDialogs, useDialogState } from "./useDialogs";

afterEach(cleanup);

test("a field dialog stacks over the table editor and keeps its payload", () => {
  const table = { handle: 42, table: { cells: [] } as unknown as TableInfo, cell: [2, 3] as [number, number] };
  const { result } = renderHook(() => ({
    dialogs: useDialogs(), table: useDialogState("tableEditing"), field: useDialogState("field"),
  }), { wrapper: DialogsProvider });
  expect(result.current.dialogs.openDialog).toBeNull();
  act(() => result.current.table[1](table));
  act(() => result.current.field[1]({ mode: "cell", cell: [2, 3] }));
  expect(result.current.dialogs.openDialog).toBe("field");
  expect(result.current.dialogs.payload).toEqual({ mode: "cell", cell: [2, 3] });
  expect(result.current.table[0]).toBe(table);
  expect(result.current.dialogs.stacked).toEqual([{ openDialog: "tableEditing", payload: table }]);
  act(() => result.current.field[1](null));
  expect(result.current.dialogs.openDialog).toBe("tableEditing");
  expect(result.current.dialogs.payload).toBe(table);
  expect(result.current.dialogs.stacked).toEqual([]);
});

test("modeless dialogs coexist, updating one does not remount or close another", () => {
  const { result } = renderHook(() => ({
    dialogs: useDialogs(), palette: useDialogOpen("blockPalette"), styles: useDialogState("styles"),
  }), { wrapper: DialogsProvider });
  const setPalette = result.current.palette[1];
  act(() => setPalette(true));
  act(() => result.current.styles[1]({ initialPath: null }));
  act(() => setPalette(true));
  expect(result.current.dialogs.openDialog).toBe("styles");
  expect(result.current.palette[0]).toBe(true);
  expect(result.current.palette[1]).toBe(setPalette);
  act(() => result.current.styles[1]((was) => ({ initialPath: `${was?.initialPath ?? "/styles"}/mono.ctb` })));
  expect(result.current.styles[0]?.initialPath).toBe("/styles/mono.ctb");
  act(() => setPalette((open) => !open));
  expect(result.current.dialogs.openDialog).toBe("styles");
  expect(result.current.dialogs.stacked).toEqual([]);
  act(() => result.current.styles[1](null));
  expect(result.current.dialogs.openDialog).toBeNull();
  expect(result.current.dialogs.payload).toBeNull();
});

test("a handoff closes its own dialog without closing the newly opened one", () => {
  const { result } = renderHook(() => ({
    dialogs: useDialogs(), options: useDialogOpen("options"), settings: useDialogOpen("settings"),
  }), { wrapper: DialogsProvider });
  act(() => result.current.options[1](true));
  // A native dialog can report its close after the destination has opened.
  act(() => result.current.settings[1](true));
  act(() => result.current.options[1](false));
  expect(result.current.dialogs.openDialog).toBe("settings");
  expect(result.current.dialogs.isOpen("options")).toBe(false);
  expect(result.current.dialogs.stacked).toEqual([]);
});

test("separate app instances do not share open dialogs", () => {
  const first = renderHook(() => useDialogs(), { wrapper: DialogsProvider });
  act(() => first.result.current.update("settings", true));
  const second = renderHook(() => useDialogs(), { wrapper: DialogsProvider });
  expect(second.result.current.openDialog).toBeNull();
  act(() => second.result.current.update("about", true));
  expect(first.result.current.isOpen("about")).toBe(false);
  act(() => first.result.current.close("settings"));
  expect(first.result.current.openDialog).toBeNull();
  expect(second.result.current.openDialog).toBe("about");
});
