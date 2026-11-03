// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { GeneratedBoq } from "./GeneratedBoq";
import { LanguageProvider } from "../i18n";
import glossary from "../i18n/ar-glossary.json";
import type { MeasurementReport } from "../api";
afterEach(() => { cleanup(); localStorage.clear(); });
test("shows rule decisions and source counts and follows refreshed generated quantities", () => {
  const report: MeasurementReport = { items: [{ code: "A", description_en: "Test surface", description_ar: "سطح", section: "Test", trade: "Test", unit: "m2", quantity: 12,
    lines: [{ label: "Opening", operation: "deduct", timesing: 1, factors: [0.4], expression: "-(1) * (0.4)", value: -0.4, unit: "m2", method: "Test method", included: false, decision: "Below threshold", citations: [{ source: "Test source", clause: "D1" }], sources: [{ handle: 42, sheet: "Model", grid: "A/1" }] }] }], warnings: [] };
  const onShow = vi.fn();
  const view = render(<GeneratedBoq report={report} onShow={onShow} />);
  fireEvent.click(screen.getByText(/A · Test surface/));
  expect(screen.getByText(/Excluded by rule/)).toBeTruthy();
  expect(screen.getByText(/Test source \/ D1/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Show on drawing (1) · Model · A/1" }));
  expect(onShow).toHaveBeenCalledWith([42], "Model");
  view.rerender(<GeneratedBoq report={{ ...report, items: [{ ...report.items[0], quantity: 15 }] }} onShow={onShow} />);
  expect(screen.getByText(/A · Test surface · 15 m²/)).toBeTruthy();
});


test.each(["en", "ar"] as const)("wall area bases remain distinct in the %s report", (lang) => {
  localStorage.setItem("qiyas.lang", lang);
  const keys = ["net_of_openings", "net_of_openings_and_wall_junctions"] as const;
  const report: MeasurementReport = { warnings: [], items: [{ code: "W", description_en: "Wall area", description_ar: "مساحة الحوائط", section: "Walls", trade: "wall", unit: "m2", quantity: 5,
    lines: keys.map((key) => ({ label: "Wall area", wall_area_basis: { label_en: glossary.concepts[key].en, label_ar: glossary.concepts[key].preferred },
      operation: "add", timesing: 1, factors: [5], expression: "5", value: 5, unit: "m2", method: "Drawing", included: true, decision: null, citations: [], sources: [] })) }] };
  render(<LanguageProvider><GeneratedBoq report={report} /></LanguageProvider>);
  for (const key of keys) {
    const label = glossary.concepts[key][lang === "ar" ? "preferred" : "en"];
    expect(screen.getByText((text) => text.startsWith(`Wall area (${label})`))).toBeTruthy();
  }
});
