// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Explain, EXPLAIN_EVIDENCE } from "./Explain";
import { explainItem, type Explanation } from "../api";
import { LanguageProvider } from "../i18n";
vi.mock("../api", () => ({ explainItem: vi.fn() }));
afterEach(() => { cleanup(); localStorage.clear(); vi.clearAllMocks(); });
const evidence = [{ handle: 42, sheet: "Model", drawing: "/test.dxf" }];
const result: Explanation = { item: "generated:A", quantity: 12, unit: "m2", confidence: 0.75, confidence_band: "medium", dimension_lines: [{ label: "Area", operation: "add", timesing: 1, factors: [3,4], expression: "3 * 4", value: 12, unit: "m2", method: "Test", included: true, decision: null, citations: [{ source: "Test source", clause: "Q1" }], sources: [{handle:42,sheet:"Model",grid:null}] }], drawings: ["/test.dxf"], evidence, elements: [], recognition_rules: [], measurement_rules: [] };
test("one read exposes citations and highlights the source", async () => {
  vi.mocked(explainItem).mockResolvedValue(result);
  const shown = vi.fn(); window.addEventListener(EXPLAIN_EVIDENCE, shown);
  const rowClick = vi.fn();
  render(<div onClick={rowClick}><Explain item="generated:A" /></div>);
  fireEvent.click(screen.getByRole("button", { name: /How did you get this number/ }));
  await screen.findByText("Test source / Q1");
  expect(explainItem).toHaveBeenCalledExactlyOnceWith("generated:A", undefined);
  expect(screen.getByText(/75%/)).toBeTruthy();
  expect((shown.mock.calls[0][0] as CustomEvent).detail).toEqual(evidence);
  fireEvent.click(screen.getByRole("button", { name: /Show evidence/ }));
  expect(rowClick).not.toHaveBeenCalled();
  window.removeEventListener(EXPLAIN_EVIDENCE, shown);
});
test("Arabic project explanation shows drawings and highlights only when asked", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  vi.mocked(explainItem).mockResolvedValue({ ...result, contributions: [{explanation:result}] });
  const shown = vi.fn(); window.addEventListener(EXPLAIN_EVIDENCE, shown);
  render(<LanguageProvider><Explain item="project:0" project="Test.qss" /></LanguageProvider>);
  fireEvent.click(screen.getByRole("button", { name: /كيف حسبت/ }));
  await screen.findByText("Test source / Q1");
  expect(shown).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /إظهار الأدلة/ }));
  await waitFor(() => expect(shown).toHaveBeenCalledTimes(1));
  expect(explainItem).toHaveBeenCalledExactlyOnceWith("project:0", "Test.qss");
  window.removeEventListener(EXPLAIN_EVIDENCE, shown);
});
