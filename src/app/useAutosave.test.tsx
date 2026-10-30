// @vitest-environment jsdom
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useAutosave } from "./useDrawings";
import { translate } from "../i18n";
import * as api from "../api";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test.each(["workspace_busy", "mizan_working"])("autosave skips %s without disabling itself, then retries", async busy => {
  const save = vi.spyOn(api, "saveProject").mockRejectedValueOnce(busy).mockResolvedValue("plan.qiyas");
  const error = vi.fn();
  const { result } = renderHook(() => useAutosave((key, vars) => translate("en", key, vars), error));
  await result.current.autosave();
  expect(error).not.toHaveBeenCalled();
  expect(result.current.autosaveOff.current).toBe(false);
  await result.current.autosave();
  expect(save).toHaveBeenCalledTimes(2);
  expect(error).not.toHaveBeenCalled();
});

test("a real autosave failure still reports once and disables further automatic writes", async () => {
  const save = vi.spyOn(api, "saveProject").mockRejectedValue("disk full");
  const error = vi.fn();
  const { result } = renderHook(() => useAutosave((key, vars) => translate("en", key, vars), error));
  await result.current.autosave();
  await result.current.autosave();
  expect(error).toHaveBeenCalledOnce();
  expect(save).toHaveBeenCalledOnce();
  expect(result.current.autosaveOff.current).toBe(true);
});
