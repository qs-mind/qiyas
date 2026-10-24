// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AssistantPanel } from "./AssistantPanel";
import { Markdown } from "./Markdown";
import { ToolCard } from "./ToolCard";
import { ConversationHistory } from "./ConversationHistory";
import { LanguageProvider } from "../i18n";
import { chatText, contentLanguage } from "./content";
import { uiError, uiErrorLog } from "../i18n/errors";
import { recoveringCard, type ToolCardItem, type Transcript } from "./messages";
import en from "../i18n/en.json";
import ar from "../i18n/ar.json";

afterEach(() => { cleanup(); localStorage.clear(); document.documentElement.lang = "en"; document.documentElement.dir = "ltr"; });
const card: ToolCardItem = { kind: "tool", id: "1", name: "qiyas_execute", label: "new_layer", input: { schema: "RAW INPUT" }, status: "error", summary: 'RAW ERROR {"default":null}', handles: [], ui: null, mutating: true, undone: false, stale: false, activity: { category: "layers", effect: "edit" } };

test("recovering cards use plain sentences and diagnostics only inside closed details", () => {
  render(<ToolCard card={card} recovering showTechnicalDetails />);
  expect(screen.getByText("Fixing a problem…")).toBeTruthy();
  expect(document.querySelector(".tool-card")?.getAttribute("title")).toBeNull();
  const details = document.querySelector("details")!;
  expect(details.open).toBe(false);
  expect(details.textContent).toContain("RAW ERROR");
  expect(document.querySelector(".tool-name")?.textContent).not.toContain("new_layer");
});

test("every generic category and effect has English and Arabic words", () => {
  for (const [key, value] of Object.entries(en).filter(([key]) => key.startsWith("activity."))) {
    expect(value).not.toContain("_");
    expect(ar[key as keyof typeof ar]).toMatch(/[\u0600-\u06ff]/);
  }
  localStorage.setItem("qiyas.lang", "ar");
  render(<LanguageProvider><ToolCard card={{ ...card, status: "done", count: 3 }} /></LanguageProvider>);
  expect(document.querySelector(".tool-name")?.textContent).toBe("تم تحديث الطبقات.");
});

test("Arabic loose lists and bubbles keep RTL after a Latin layer name, in an English window", () => {
  const text = "- **A-WALL** مساحة الجدار 12.5 m²\n\n- **A-DOOR** عرض الباب 0.9 m";
  render(<Markdown text={text} />);
  const md = document.querySelector(".md")!;
  expect(md.getAttribute("dir")).toBe("rtl");
  expect(md.getAttribute("lang")).toBe("ar");
  expect(md.querySelector("ul")?.hasAttribute("dir")).toBe(false);
  expect(md.querySelector("p")?.hasAttribute("dir")).toBe(false);
  expect(md.querySelector("li")?.getAttribute("dir")).toBe("rtl");
  expect(md.querySelector("bdi")?.getAttribute("dir")).toBe("ltr");
  expect(contentLanguage("A-WALL كام متر؟")).toBe("ar");
});

test("each list item and table cell follows its own prose", () => {
  render(<Markdown text={"- A-WALL مساحة الجدار\n- Review the wall\n\n| Item | المساحة |\n| --- | --- |\n| Wall | الجدار |"} />);
  expect([...document.querySelectorAll("li")].map((item) => item.dir)).toEqual(["rtl", "ltr"]);
  expect([...document.querySelectorAll("td")].map((item) => item.dir)).toEqual(["ltr", "rtl"]);
});

test("approval follows the chat and old PDF paths and raw Thinking never appear", () => {
  const transcript: Transcript = { items: [
    { kind: "user", text: 'A-WALL كام متر؟\n[PDF attachment: "plan.pdf"; 3 pages; path: "/home/name/attachments/plan.pdf"]' },
    { kind: "thinking", text: "PRIVATE REASONING" },
    { kind: "plan", phases: [{ id: "a", title: "الحصر", status: "pending" }], awaitingApproval: true },
  ] };
  const send = vi.fn();
  render(<AssistantPanel assistant={{ transcript, busy: false, error: null, send, cancel() {}, clear() {} }} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} />);
  expect(document.querySelector(".msg.user")?.getAttribute("dir")).toBe("rtl");
  expect(document.body.textContent).not.toContain("/home/");
  expect(document.body.textContent).not.toContain("PRIVATE REASONING");
  fireEvent.click(screen.getByRole("button", { name: ar["ai.approve"] }));
  expect(send).toHaveBeenCalledWith(ar["ai.approvedMessage"]);
});

test("unknown errors never leak backend prose or paths in either language", () => {
  for (const lang of ["en", "ar"] as const) {
    expect(uiError('serde_json: /home/name/project.dwg {"field":null}', lang)).toBe(lang === "ar" ? ar["err.unknown"] : en["err.unknown"]);
  }
  expect(uiErrorLog()).toContain("serde_json");
  expect(uiError({ code: "invalid_value", message: "invalid field /private/drawing" }, "ar")).toBe(ar["err.input"]);
  expect(uiErrorLog()).toContain("invalid field /private/drawing");
});

test("Delete all waits for confirmation and Cancel preserves conversations", async () => {
  const remove = vi.fn();
  render(<ConversationHistory disabled={false} history={{ chats: [{ id: "1", title: "Walls", drawing: "plan.dwg", updated: 1, drawing_exists: true, turns: 1 }], drawing: "plan.dwg", latest: null, pending: false, error: null, resume: vi.fn(), remove }} />);
  fireEvent.click(screen.getByRole("button", { name: "History" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Delete all conversations" })).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "Delete all conversations" }));
  expect(remove).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.getByRole("dialog")).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(remove).not.toHaveBeenCalled();
});

test("later user turns do not relabel a stopped step as recovering", () => {
  expect(recoveringCard([card, { kind: "user", text: "Another job" }, { ...card, status: "done" }], 0, true)).toBe(false);
  expect(recoveringCard([card, { ...card, status: "done" }], 0, false)).toBe(true);
});

test("paths reduce to names while HTML text, URLs and layer names keep their meaning", () => {
  expect(chatText('Saved "/work/My Project/file.dxf" and .local/report.pdf')).toBe('Saved "file.dxf" and report.pdf');
  expect(chatText('two <b>x</b> https://example.com/file.pdf A-WALL / A-AREA')).toBe('two <b>x</b> https://example.com/file.pdf A-WALL / A-AREA');
});
