// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { PackCheck } from "../api";
import { LanguageProvider, useLanguage } from "../i18n";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";
import { setWorkspace } from "../workspaceIpc";
import { AssistantPanel } from "./AssistantPanel";
import { PackCheckCard } from "./PackCheckCard";
import { EMPTY } from "./messages";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

afterEach(() => {
  cleanup();
  invoke.mockReset();
  setWorkspace(null);
  localStorage.clear();
  document.documentElement.lang = "en";
  document.documentElement.dir = "ltr";
});

const ready: PackCheck = {
  ready: true,
  lines: [
    { id: "units", passes: true, found_en: "The drawing's units are known.", found_ar: "وحدات الرسم معروفة.", fix_en: "", fix_ar: "" },
    { id: "annotation_scale", passes: true, found_en: "The annotation scale is 1:100.", found_ar: "مقياس الكتابة 1:100.", fix_en: "", fix_ar: "" },
    { id: "pdf_pages", passes: true, found_en: "Each page is vector or scanned: plan.pdf p1 vector; plan.pdf p2 scanned.", found_ar: "كل صفحة إما متجهية أو ممسوحة: plan.pdf p1 vector; plan.pdf p2 scanned.", fix_en: "", fix_ar: "" },
    { id: "calibration", passes: true, found_en: "Every scan is calibrated.", found_ar: "كل صورة ممسوحة مضبوطة المقياس.", fix_en: "", fix_ar: "" },
    { id: "orientation", passes: true, found_en: "Every page is stored upright.", found_ar: "كل الصفحات مخزّنة بالاتجاه المعتاد.", fix_en: "", fix_ar: "" },
    { id: "register", passes: true, found_en: "Sheet number and revision are read for 1 sheet(s).", found_ar: "قُرئ رقم اللوحة والمراجعة لعدد 1 من اللوحات.", fix_en: "", fix_ar: "" },
  ],
};
const needsFix: PackCheck = {
  ready: false,
  lines: ready.lines.map((line) => line.id === "units" ? {
    ...line, passes: false,
    found_en: "The drawing has no known units (INSUNITS 0), so lengths would be read as millimetres.",
    found_ar: "وحدات الرسم غير معروفة (INSUNITS 0)، فستُقرأ الأطوال بالمليمتر.",
    fix_en: "Ask who made the drawing which unit it uses, and set it in the drawing before measuring.",
    fix_ar: "اسأل من أعدّ الرسم عن الوحدة المستخدمة، واضبطها في الرسم قبل القياس.",
  } : line.id === "calibration" ? {
    ...line, passes: false,
    found_en: "Not calibrated: plan.pdf p2 on Model.", found_ar: "غير مضبوطة المقياس: plan.pdf p2 on Model.",
    fix_en: "Calibrate each scan: pick two points on a dimension you know and type its real length.",
    fix_ar: "اضبط مقياس كل صورة ممسوحة: اختر نقطتين على بُعد معلوم واكتب طوله الحقيقي.",
  } : line),
};
const reply = (result: PackCheck) => ({ type: "measurement", result });

test("a busy background pack read stays silent and retries after a revision change", async () => {
  invoke.mockRejectedValueOnce("workspace_busy").mockResolvedValue(reply(ready));
  const view = render(<PackCheckCard drawing="plan.dwg" revision={1} />);
  await act(async () => {});
  expect(screen.queryByRole("alert")).toBeNull();
  view.rerender(<PackCheckCard drawing="plan.dwg" revision={2} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  expect(invoke).toHaveBeenCalledTimes(2);
});

test("reads the current workspace through the command bus and names units and scan fixes before take-off", async () => {
  setWorkspace(7);
  invoke.mockResolvedValue(reply(needsFix));
  render(<PackCheckCard drawing="plan.dwg" revision={1} />);
  expect(screen.getByText(en["ai.packCheck.loading"])).toBeTruthy();
  await screen.findByText(en["ai.packCheck.fix"]);
  expect(invoke).toHaveBeenCalledWith("run_command", {
    cmd: { type: "measurement", kind: "pack_check" }, workspace: 7,
  });
  const card = screen.getByRole("region", { name: en["ai.packCheck.title"] });
  const rows = within(card).getAllByRole("listitem");
  expect(rows).toHaveLength(needsFix.lines.length);
  needsFix.lines.forEach((line, index) => {
    expect(rows[index].textContent).toContain(line.found_en);
    expect(rows[index].getAttribute("data-passes")).toBe(String(line.passes));
    expect(rows[index].textContent).toContain(line.passes ? en["ai.packCheck.passed"] : en["ai.packCheck.needsFix"]);
    if (!line.passes) expect(rows[index].textContent).toContain(line.fix_en);
  });
  expect(within(card).getAllByRole("button")).toHaveLength(1);
  expect(within(card).queryByRole("textbox")).toBeNull();
});

test("shows every passing check and refreshes after an external PDF change", async () => {
  invoke.mockResolvedValueOnce(reply(ready)).mockResolvedValue(reply(needsFix));
  render(<PackCheckCard drawing="plan.dwg" workspace={4} revision={1} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  expect(screen.getAllByText(en["ai.packCheck.passed"] + ":")).toHaveLength(6);
  fireEvent.click(screen.getByRole("button", { name: en["ai.packCheck.refresh"] }));
  await screen.findByText(en["ai.packCheck.fix"]);
  expect(screen.queryByText(en["ai.packCheck.ready"])).toBeNull();
  expect(invoke).toHaveBeenCalledTimes(2);
});

test("uses Arabic findings and fixes in RTL and changes language without another read", async () => {
  function SwitchLanguage() {
    const { setLang } = useLanguage();
    return <button onClick={() => setLang("ar")}>Arabic</button>;
  }
  invoke.mockResolvedValue(reply(needsFix));
  render(<LanguageProvider><SwitchLanguage /><PackCheckCard drawing="plan.dwg" revision={1} /></LanguageProvider>);
  await screen.findByText(en["ai.packCheck.fix"]);
  fireEvent.click(screen.getByRole("button", { name: "Arabic" }));
  const card = screen.getByRole("region", { name: ar["ai.packCheck.title"] });
  expect(card.getAttribute("dir")).toBe("rtl");
  expect(document.documentElement.dir).toBe("rtl");
  const rows = within(card).getAllByRole("listitem");
  needsFix.lines.forEach((line, index) => {
    expect(rows[index].textContent).toContain(line.found_ar);
    expect(rows[index].textContent).not.toContain(line.found_en);
    if (!line.passes) expect(rows[index].textContent).toContain(line.fix_ar);
  });
  expect(card.querySelector('bdi[dir="ltr"]')?.textContent).toContain("INSUNITS 0");
  expect(within(card).getByRole("button", { name: ar["ai.packCheck.refresh"] })).toBeTruthy();
  expect(invoke).toHaveBeenCalledTimes(1);
});

test("does not invent PDF or orientation checks when the backend supplies none", async () => {
  invoke.mockResolvedValue(reply({ ready: true, lines: ready.lines.filter((line) => ["units", "annotation_scale", "register"].includes(line.id)) }));
  render(<PackCheckCard drawing="cad-only.dwg" revision={1} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  expect(screen.getAllByRole("listitem")).toHaveLength(3);
  expect(screen.queryByText(en["ai.packCheck.pdf_pages"] + ":")).toBeNull();
});

test("a read failure stays visible, uses the UI error translation, and can be retried", async () => {
  localStorage.setItem("qiyas.lang", "ar");
  invoke.mockRejectedValueOnce("private diagnostic").mockResolvedValue(reply(ready));
  render(<LanguageProvider><PackCheckCard drawing="plan.dwg" revision={1} /></LanguageProvider>);
  expect((await screen.findByRole("alert")).textContent).toBe(ar["err.unknown"]);
  expect(screen.queryByText("private diagnostic")).toBeNull();
  expect(screen.queryByText(ar["ai.packCheck.ready"])).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: ar["ai.packCheck.refresh"] }));
  await screen.findByText(ar["ai.packCheck.ready"]);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a late reply from another drawing cannot replace the current workspace's checks", async () => {
  let finish!: (value: ReturnType<typeof reply>) => void;
  invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; })).mockResolvedValue(reply(ready));
  const view = render(<PackCheckCard drawing="first.dwg" workspace={1} revision={1} />);
  view.rerender(<PackCheckCard drawing="second.dwg" workspace={2} revision={1} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  await act(async () => { finish(reply(needsFix)); });
  expect(screen.queryByText(en["ai.packCheck.fix"])).toBeNull();
  expect(screen.getByText(en["ai.packCheck.ready"])).toBeTruthy();
  expect(invoke).toHaveBeenLastCalledWith("run_command", {
    cmd: { type: "measurement", kind: "pack_check" }, workspace: 2,
  });
});

test("a drawing revision clears the old ready result while its new check is pending", async () => {
  let finish!: (value: ReturnType<typeof reply>) => void;
  invoke.mockResolvedValueOnce(reply(ready)).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const view = render(<PackCheckCard drawing="plan.dwg" revision={1} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  view.rerender(<PackCheckCard drawing="plan.dwg" revision={2} />);
  expect(screen.queryByText(en["ai.packCheck.ready"])).toBeNull();
  expect(screen.getByText(en["ai.packCheck.loading"])).toBeTruthy();
  await act(async () => { finish(reply(needsFix)); });
  expect(screen.getByText(en["ai.packCheck.fix"])).toBeTruthy();
});

test("waits until Mizan finishes instead of reading on every edit during a turn", async () => {
  invoke.mockResolvedValue(reply(ready));
  const view = render(<PackCheckCard drawing="plan.dwg" revision={1} busy />);
  view.rerender(<PackCheckCard drawing="plan.dwg" revision={2} busy />);
  expect(screen.getByText(en["ai.packCheck.waiting"])).toBeTruthy();
  expect(invoke).not.toHaveBeenCalled();
  view.rerender(<PackCheckCard drawing="plan.dwg" revision={2} busy={false} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  expect(invoke).toHaveBeenCalledTimes(1);
});

test("Mizan shows the card before the first take-off request and removes it when the drawing closes", async () => {
  invoke.mockResolvedValue(reply(needsFix));
  const props = {
    assistant: { transcript: EMPTY, busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn() },
    onShow: vi.fn(), onUndo: vi.fn(), onSettings: vi.fn(),
  };
  const view = render(<AssistantPanel {...props} />);
  expect(screen.queryByRole("region", { name: en["ai.packCheck.title"] })).toBeNull();
  expect(invoke).not.toHaveBeenCalled();
  view.rerender(<AssistantPanel {...props} drawing={{ path: "plan.dwg", revision: 1, hasTakeoff: false }} />);
  await screen.findByText(en["ai.packCheck.fix"]);
  const card = screen.getByRole("region", { name: en["ai.packCheck.title"] });
  expect(card.compareDocumentPosition(screen.getByText(en["ai.emptyTitle"])) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(props.assistant.send).not.toHaveBeenCalled();
  view.rerender(<AssistantPanel {...props} />);
  expect(screen.queryByRole("region", { name: en["ai.packCheck.title"] })).toBeNull();
});

test("Mizan hides the pack check once a take-off exists and restores it when the take-off is removed", async () => {
  invoke.mockResolvedValue(reply(ready));
  const props = {
    assistant: { transcript: EMPTY, busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn() },
    onShow: vi.fn(), onUndo: vi.fn(), onSettings: vi.fn(),
  };
  const drawing = { path: "plan.dwg", revision: 1, hasTakeoff: true };
  const view = render(<AssistantPanel {...props} drawing={drawing} />);
  expect(screen.queryByRole("region", { name: en["ai.packCheck.title"] })).toBeNull();
  expect(invoke).not.toHaveBeenCalled();

  view.rerender(<AssistantPanel {...props} drawing={{ ...drawing, revision: 2, hasTakeoff: false }} />);
  await screen.findByText(en["ai.packCheck.ready"]);
  expect(invoke).toHaveBeenCalledTimes(1);

  view.rerender(<AssistantPanel {...props} drawing={{ ...drawing, revision: 3 }} />);
  expect(screen.queryByRole("region", { name: en["ai.packCheck.title"] })).toBeNull();
  expect(invoke).toHaveBeenCalledTimes(1);
});
