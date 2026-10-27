// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";
import { AssistantPanel } from "./AssistantPanel";
import { EMPTY } from "./messages";

vi.mock("../api", async (original) => ({ ...(await original<object>()), aiLog: vi.fn(), aiAttachPdf: vi.fn() }));

afterEach(() => {
  cleanup();
  localStorage.removeItem("qiyas.lang");
});

function setup(lang: "en" | "ar" = "en") {
  localStorage.setItem("qiyas.lang", lang);
  const assistant = {
    transcript: EMPTY,
    busy: false,
    error: null,
    send: vi.fn().mockResolvedValue(true),
    cancel: vi.fn(),
    clear: vi.fn(),
  };
  const onCompare = vi.fn();
  render(
    <LanguageProvider>
      <AssistantPanel assistant={assistant} onShow={() => {}} onUndo={() => {}} onSettings={() => {}} onCompare={onCompare} />
    </LanguageProvider>,
  );
  const box = () => screen.getByLabelText(lang === "ar" ? ar["ai.placeholder"] : en["ai.placeholder"]);
  return { assistant, onCompare, box };
}

const type = (box: HTMLElement, value: string) => fireEvent.change(box, { target: { value } });
const enter = (box: HTMLElement) => fireEvent.keyDown(box, { key: "Enter" });

test("a slash opens the commands, in English in the English window, and narrows as it is typed", () => {
  const { box } = setup();
  type(box(), "/");
  const rows = screen.getAllByRole("option").map((row) => row.textContent);
  expect(rows).toEqual([
    `/takeoff${en["ai.slash.takeoff.desc"]}`,
    `/boq${en["ai.slash.boq.desc"]}`,
    `/compare${en["ai.slash.compare.desc"]}`,
    `/sheet${en["ai.slash.sheet.desc"]}`,
    `/check${en["ai.slash.check.desc"]}`,
    `/new${en["ai.slash.new.desc"]}`,
  ]);
  type(box(), "/ta");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  type(box(), "/zzz");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("Enter on a slash prompt command sends its prompt to Mizan; the arrows choose another", () => {
  const { assistant, box } = setup();
  type(box(), "/");
  enter(box());
  expect(assistant.send).toHaveBeenLastCalledWith(en["ai.slash.takeoff.prompt"]);
  type(box(), "/");
  fireEvent.keyDown(box(), { key: "ArrowDown" });
  enter(box());
  expect(assistant.send).toHaveBeenLastCalledWith(en["ai.slash.boq.prompt"]);
});

test("clicking a row runs it, and Escape closes the list but keeps the text", () => {
  const { assistant, box } = setup();
  type(box(), "/c");
  fireEvent.click(screen.getByRole("option", { name: /\/check/ }));
  expect(assistant.send).toHaveBeenCalledWith(en["ai.slash.check.prompt"]);
  type(box(), "/");
  fireEvent.keyDown(box(), { key: "Escape" });
  expect(screen.queryByRole("listbox")).toBeNull();
  expect((box() as HTMLTextAreaElement).value).toBe("/");
});

test("the window's own commands act without sending anything: /new starts a chat, /compare asks for a BOQ", () => {
  const { assistant, onCompare, box } = setup();
  type(box(), "/new");
  enter(box());
  expect(assistant.clear).toHaveBeenCalledOnce();
  type(box(), "/compare");
  enter(box());
  expect(onCompare).toHaveBeenCalledOnce();
  expect(assistant.send).not.toHaveBeenCalled();
});

test("ordinary text with a slash in it is sent as it is", () => {
  const { assistant, box } = setup();
  type(box(), "/takeoff the villa");
  expect(screen.queryByRole("listbox")).toBeNull();
  enter(box());
  expect(assistant.send).toHaveBeenCalledWith("/takeoff the villa");
});

test("in the Arabic window the commands are Arabic words, and an English word typed whole still runs", () => {
  const { assistant, box } = setup("ar");
  type(box(), "/");
  expect(screen.getAllByRole("option").map((row) => row.textContent?.split(ar["ai.slash.takeoff.desc"])[0])).toContain("/حصر");
  type(box(), "/حصر");
  enter(box());
  expect(assistant.send).toHaveBeenLastCalledWith(ar["ai.slash.takeoff.prompt"]);
  type(box(), "/boq");
  expect(screen.queryByRole("listbox")).toBeNull();
  enter(box());
  expect(assistant.send).toHaveBeenLastCalledWith(ar["ai.slash.boq.prompt"]);
});
