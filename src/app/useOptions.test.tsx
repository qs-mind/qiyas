// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Settings } from "../api";
import { useOptions } from "./useOptions";
const getSettings = vi.fn();
vi.mock("../api", async (original) => ({...(await original<object>()), getSettings: () => getSettings()}));

test("Mizan support preference reads from settings and updates only after a saved settings change", async () => {
  getSettings.mockResolvedValue({show_technical_details: true});
  const {result} = renderHook(() => useOptions({t: key => key, info: null, setError: vi.fn()}));
  expect(result.current.showTechnicalDetails).toBe(false);
  await waitFor(() => expect(result.current.showTechnicalDetails).toBe(true));
  act(() => result.current.takeSettings({show_technical_details: false} as Settings));
  expect(result.current.showTechnicalDetails).toBe(false);
  act(() => result.current.takeSettings({} as Settings));
  expect(result.current.showTechnicalDetails).toBe(false);
});
