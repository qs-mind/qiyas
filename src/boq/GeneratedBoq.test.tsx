// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { GeneratedBoq } from "./GeneratedBoq";
import type { MeasurementReport } from "../api";
afterEach(cleanup);
test("shows rule decisions and source handles and follows refreshed generated quantities", () => {
  const report: MeasurementReport = { items: [{ code: "A", description_en: "Test surface", description_ar: "سطح", section: "Test", trade: "Test", unit: "m2", quantity: 12,
    lines: [{ label: "Opening", operation: "deduct", timesing: 1, factors: [0.4], expression: "-(1) * (0.4)", value: -0.4, unit: "m2", method: "Test method", included: false, decision: "Below threshold", citations: [{ source: "Test source", clause: "D1" }], sources: [{ handle: 42, sheet: "Model", grid: "A/1" }] }] }], warnings: [] };
  const onShow = vi.fn();
  const view = render(<GeneratedBoq report={report} onShow={onShow} />);
  fireEvent.click(screen.getByText(/A · Test surface/));
  expect(screen.getByText(/Excluded by rule/)).toBeTruthy();
  expect(screen.getByText(/Test source \/ D1/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "42 · Model · A/1" }));
  expect(onShow).toHaveBeenCalledWith([42], "Model");
  view.rerender(<GeneratedBoq report={{ ...report, items: [{ ...report.items[0], quantity: 15 }] }} onShow={onShow} />);
  expect(screen.getByText(/A · Test surface · 15 m2/)).toBeTruthy();
});
