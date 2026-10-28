import { expect, test } from "vitest";
import type { ToolCardItem, Transcript } from "../ai/messages";
import { transcriptFiles } from "./outputFiles";

const root = new URL("../../", import.meta.url).pathname.replace(/\/$/, "");
const file = (name: string) => `${root}/samples/${name}`;
const card = (result: unknown, more: Partial<ToolCardItem> = {}): ToolCardItem => ({
  kind: "tool", id: "write", name: "qiyas_execute", status: "done", input: {}, summary: JSON.stringify(result),
  handles: [], ui: null, mutating: false, undone: false, stale: false, ...more,
});

test("collects PDFs and workbooks from written results, including batches, once per full path", () => {
  const transcript: Transcript = { items: [
    card({ written: file("plan.pdf") }),
    card({ outcome: { steps: [{ written: file("bill.xlsx") }, { written: [file("evidence.pdf"), file("plan.pdf")] }] } }),
    card({ written: file("revision/plan.pdf") }, { restored: true }),
  ] };
  expect(transcriptFiles(transcript)).toEqual([
    { path: file("plan.pdf"), name: "plan.pdf" },
    { path: file("bill.xlsx"), name: "bill.xlsx" },
    { path: file("evidence.pdf"), name: "evidence.pdf" },
    { path: file("revision/plan.pdf"), name: "plan.pdf" },
  ]);
});

test("ignores requested paths, ordinary messages, failed or running writes and malformed results", () => {
  const transcript: Transcript = { items: [
    { kind: "assistant", text: JSON.stringify({ written: file("claimed.pdf") }), finished: true },
    card({}, { input: { written: file("requested.xlsx") } }),
    card({ written: file("failed.pdf") }, { status: "error" }),
    card({ written: file("running.pdf") }, { status: "running" }),
    card({ path: file("read.pdf") }),
    card({}, { summary: "Done" }),
    card({}, { summary: '{"written":' }),
    card({ written: [null, {}, "relative.pdf", "https://example.test/a.pdf", file("script.sh"), file("bad\nname.pdf")] }),
  ] };
  expect(transcriptFiles(transcript)).toEqual([]);
});

test("a new conversation has no files from the previous conversation", () => {
  expect(transcriptFiles({ items: [] })).toEqual([]);
});
