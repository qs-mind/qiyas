// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RecognitionPanel } from "./RecognitionPanel";
import { LanguageProvider } from "../i18n";
import type { RecognitionCandidate } from "../api";
const read = vi.fn();
vi.mock("../api", () => ({ runCommand: (command: unknown) => read(command) }));
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear(); });
const row: RecognitionCandidate = { handle: 42, rule: "r", confidence: 0.6, state: "pending",
  placement: { type: "Run", props: { height: 3 } }, evidence: [{ path: "plan.pdf", page: 2, detail: "note" }],
  issues: [], unresolved: [], object: null };
test("accept, fix with a saved rule, and reject use one review command", async () => {
  read.mockResolvedValue({ type: "recognition", result: { candidates: [row] } });
  const run = vi.fn().mockResolvedValue(undefined);
  render(<RecognitionPanel active drawing="drawing.dxf" revision={1} run={run} />);
  fireEvent.click(await screen.findByText("Accept"));
  await waitFor(() => expect(run).toHaveBeenCalledWith({ type: "recognition", kind: "review", handle: 42, decision: "accepted" }));
  await waitFor(() => expect((screen.getByText("Fix properties") as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByText("Fix properties"));
  fireEvent.change(screen.getByLabelText("height"), { target: { value: "4" } });
  fireEvent.click(screen.getByLabelText("Save correction as a rule"));
  fireEvent.click(screen.getByText("Save and accept"));
  await waitFor(() => expect(run).toHaveBeenCalledWith(expect.objectContaining({ props: { height: 4 }, save_as: "review-2a" })));
  await waitFor(() => expect((screen.getByText("Reject") as HTMLButtonElement).disabled).toBe(false));
  const saveRejection = screen.getByLabelText("Save correction as a rule") as HTMLInputElement;
  if (!saveRejection.checked) fireEvent.click(saveRejection);
  fireEvent.click(screen.getByText("Reject"));
  await waitFor(() => expect(run).toHaveBeenCalledWith(expect.objectContaining({ decision: "rejected", save_as: "review-2a" })));
});
test("Arabic review labels and unresolved properties remain actionable", async () => {
  read.mockResolvedValue({ type: "recognition", result: { candidates: [{ ...row, unresolved: ["width", "tag"] }] } });
  localStorage.setItem("qiyas.lang", "ar");
  const run = vi.fn().mockResolvedValue(undefined);
  render(<LanguageProvider><RecognitionPanel active drawing="drawing.dxf" revision={1} run={run} /></LanguageProvider>);
  const accept = await screen.findByText("قبول");
  expect((accept as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByText("تصحيح الخصائص"));
  fireEvent.change(screen.getByLabelText("width"), { target: { value: "1.2" } });
  fireEvent.change(screen.getByLabelText("tag"), { target: { value: "W02" } });
  fireEvent.click(screen.getByText("حفظ وقبول"));
  await waitFor(() => expect(run).toHaveBeenCalledWith(expect.objectContaining({ props: { height: 3, width: 1.2, tag: "W02" } })));
});
test("a late response cannot fill the queue of a different drawing", async () => {
  let finish!: (value: unknown) => void;
  read.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; })).mockResolvedValue({ type: "recognition", result: { candidates: [] } });
  const view = render(<RecognitionPanel active drawing="first.dxf" revision={1} run={vi.fn()} />);
  view.rerender(<RecognitionPanel active drawing="second.dxf" revision={2} run={vi.fn()} />);
  finish({ type: "recognition", result: { candidates: [row] } });
  await waitFor(() => expect(screen.getByText("No elements awaiting review.")).toBeTruthy());
  expect(screen.queryByText("Run")).toBeNull();
});
