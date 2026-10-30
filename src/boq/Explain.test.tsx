// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Explain, EXPLAIN_EVIDENCE } from "./Explain";
import { explainItem, type Explanation } from "../api";
import { LanguageProvider } from "../i18n";
import { TRACE_EVENT } from "./trace";
vi.mock("../api", () => ({ explainItem: vi.fn() }));
afterEach(() => { cleanup(); localStorage.clear(); vi.clearAllMocks(); });
const evidence = [{ handle: 42, sheet: "Model", drawing: "/test.dxf" }];
const result: Explanation = { item: "generated:A", quantity: 12, unit: "m2", confidence: 0.75, confidence_band: "medium", dimension_lines: [{ label: "Area", operation: "add", timesing: 1, factors: [3,4], expression: "3 * 4", value: 12, unit: "m2", method: "Test", included: true, decision: null, citations: [{ source: "Test source", clause: "Q1" }], sources: [{handle:42,sheet:"Model",grid:null}] }], drawings: ["/test.dxf"], evidence, elements: [], recognition_rules: [], measurement_rules: [] };
test("a drawing's item is traced on the drawing with its working, without reading it here", () => {
  const traced = vi.fn(); window.addEventListener(TRACE_EVENT, traced);
  const rowClick = vi.fn();
  render(<div onClick={rowClick}><Explain item="generated:A" /></div>);
  fireEvent.click(screen.getByRole("button", { name: /How was this measured/ }));
  expect((traced.mock.calls[0][0] as CustomEvent).detail).toEqual({ item: "generated:A", panel: true, drawing: null });
  expect(explainItem).not.toHaveBeenCalled();
  expect(rowClick).not.toHaveBeenCalled();
  window.removeEventListener(TRACE_EVENT, traced);
});
test("Arabic project explanation shows drawings and highlights only when asked", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  vi.mocked(explainItem).mockResolvedValue({ ...result, contributions: [{explanation:result}] });
  const shown = vi.fn(); window.addEventListener(EXPLAIN_EVIDENCE, shown);
  render(<LanguageProvider><Explain item="project:0" project="Test.qss" /></LanguageProvider>);
  fireEvent.click(screen.getByRole("button", { name: /كيف تم قياس هذه الكمية/ }));
  await screen.findByText("Test source / Q1");
  expect(shown).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /إظهار على الرسم/ }));
  await waitFor(() => expect(shown).toHaveBeenCalledTimes(1));
  expect(explainItem).toHaveBeenCalledExactlyOnceWith("project:0", "Test.qss");
  window.removeEventListener(EXPLAIN_EVIDENCE, shown);
});
