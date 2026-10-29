// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { ToolCardItem } from "./messages";
import { ToolCard } from "./ToolCard";

afterEach(cleanup);
const card = (input: unknown, more: Partial<ToolCardItem> = {}): ToolCardItem => ({
  kind: "tool", id: "batch", name: "qiyas_execute", input, status: "running", summary: "", handles: [],
  ui: null, mutating: true, undone: false, stale: false, ...more,
});
const text = () => document.querySelector(".tool-name")!.textContent!.replace(/[\u2068\u2069]/g, "");

test.each(["en", "ar"] as const)("measurement setup and export batches use engineering words in %s", (language) => {
  const view = render(<ToolCard language={language} card={card({ op: "run", args: JSON.stringify({ command: { type: "measurement", kind: "set_method" } }) }, {
    activity: { category: "takeoff", effect: "edit", detail: { key: "activity.line.run", params: { command: "measurement.set_method" } } },
  })} />);
  expect(text()).toBe(language === "en" ? "Setting the measurement method…" : "يضبط طريقة القياس…");
  view.rerender(<ToolCard language={language} card={card({ op: "batch", args: JSON.stringify({ commands: [
    { type: "deliverable", kind: "export", report: "boq" },
    { type: "deliverable.export", kind: "dimensions" },
  ] }) }, { activity: { category: "takeoff", effect: "read" } })} />);
  expect(text()).toContain(language === "en" ? "Exporting the bill of quantities" : "يصدّر");
  expect(text()).toContain(language === "en" ? "the dimension sheets" : "أوراق القياس");
  expect(text()).not.toMatch(/Checking|measurement\.|deliverable\.|activity\./);
});

test.each(["en", "ar"] as const)("every surface operation has a localized line and unknown metadata never leaks identifiers in %s", (language) => {
  const view = render(<ToolCard language={language} card={card({})} />);
  const ops = ["artifact", "explain", "digest", "takeoff_kit", "workflow", "takeoff_check", "brief_check", "document", "state", "capabilities", "validate", "commands", "records", "record_schema", "query", "measure", "run", "check", "history", "events", "drawings", "selection", "panel_state", "rooms", "plan", "ask", "finish", "type", "batch", "set_records", "create_entities", "undo_own", "redo_own", "select", "view", "new_drawing", "open_drawing", "switch_drawing", "save_drawing", "close_drawing", "import_boq", "compare", "export_takeoff", "export_conflicts", "export_evidence", "plot"];
  for (const op of ops) {
    view.rerender(<ToolCard language={language} card={card({ op, args: "{}" })} />);
    expect(text()).not.toMatch(/activity\.|qiyas_|\b[a-z]+_[a-z]+\b/);
    if (language === "ar") expect(text()).toMatch(/[\u0600-\u06ff]/);
  }
  view.rerender(<ToolCard language={language} card={card({}, {
    activity: { category: "unknown_category", effect: "edit", detail: {
      key: "activity.line.untranslated", params: { command: "new_type.new_kind", what: "unknown_type", collection: "unknown_collection" },
    } },
  })} />);
  expect(text()).not.toMatch(/new_type|unknown_|activity\./);
});

test.each(["en", "ar"] as const)("a running card shows reported progress in %s, with no invented count", (language) => {
  const input = { op: "batch", args: { commands: [] } };
  const view = render(<ToolCard language={language} card={card(input, { progress: { completed: 3, total: 10 } })} />);
  expect(screen.getByRole("status").textContent!.replace(/[\u2068\u2069]/g, "")).toBe(language === "en" ? "3 of 10" : "3 من 10");
  view.rerender(<ToolCard language={language} card={card(input)} />);
  expect(screen.queryByRole("status")).toBeNull();
  view.rerender(<ToolCard language={language} card={card(input, { status: "done", progress: { completed: 3, total: 10 } })} />);
  expect(screen.queryByRole("status")).toBeNull();
});
