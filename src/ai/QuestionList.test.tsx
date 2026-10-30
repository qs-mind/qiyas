// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Question } from "../api";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import { QuestionList, answerText } from "./QuestionList";

afterEach(cleanup);

const one: Question[] = [{
  id: "q1", question: "Which country’s codes govern this villa?", allow_other: true,
  options: [{ label: "Egypt", description: "Matches the Arabic text on the drawing", recommended: true }, { label: "Saudi Arabia" }, { label: "UAE" }],
}];
const two: Question[] = [...one, { id: "q2", question: "Which finish?", allow_other: false, options: [{ label: "Standard" }, { label: "High-end" }] }];

test("one question is a numbered list, the recommended option in the accent with its label, no boxes", () => {
  const onPick = vi.fn();
  render(<LanguageProvider><QuestionList questions={one} onPick={onPick} /></LanguageProvider>);
  const rows = screen.getAllByRole("listitem").map((li) => li.textContent);
  expect(rows[0]).toContain("1");
  expect(rows[0]).toContain(`· ${en["ai.recommended"]}`);
  expect(rows[0]).toContain("Matches the Arabic text on the drawing");
  expect(rows).toHaveLength(3);
  expect(screen.getByRole("textbox")).toBeTruthy();
  expect(document.querySelector(".qopt.hot")?.textContent).toContain("Egypt");
  // No cards: nothing here draws its own box.
  expect(document.querySelectorAll(".qcard, .card")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: /Saudi Arabia/ }));
  expect(onPick).toHaveBeenCalledWith("q1", "Saudi Arabia");
});

test("several questions are numbered and their options lettered; the chosen one is marked", () => {
  const onPick = vi.fn();
  render(<LanguageProvider><QuestionList questions={two} picked={{ q1: "Egypt", q2: "High-end" }} onPick={onPick} /></LanguageProvider>);
  expect(screen.queryByText(/Which finish/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(screen.getByRole("button", { name: /High-end/ }).className).toContain("picked");
  expect(screen.getByRole("button", { name: /High-end/ }).getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: /Standard/ }));
  expect(onPick).toHaveBeenCalledWith("q2", "Standard");
  expect(screen.getAllByRole("listitem")[0].textContent).toContain("a");
});

test("once answered it is only a record: the options do nothing and the 'Other' line is gone", () => {
  render(<LanguageProvider><QuestionList questions={one} /></LanguageProvider>);
  expect((screen.getByRole("button", { name: /Egypt/ }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByText(en["ai.questionOther"])).toBeNull();
});

test("Arabic questions read right to left in Arabic, whatever the window's language, and keep their words as written", () => {
  const ar: Question[] = [
    { id: "q1", question: "ما أبعاد الأرض؟", allow_other: true,
      options: [{ label: "أرسل الأبعاد", recommended: true }, { label: "افترض مسطحًا مستطيلاً ومدخلاً" }] },
    { id: "q2", question: "هل أفترض التوزيع؟", allow_other: false, options: [{ label: "نعم" }, { label: "لا" }] },
  ];
  render(<LanguageProvider><QuestionList questions={ar} onPick={() => {}} /></LanguageProvider>);
  const list = document.querySelector(".question-list")!;
  expect(list.getAttribute("dir")).toBe("rtl");
  expect(list.getAttribute("lang")).toBe("ar");
  const markers = [...document.querySelectorAll(".qo .k")].map((k) => k.textContent);
  expect(markers.slice(0, 3)).toEqual(["أ", "ب"]);
  expect(list.textContent).toContain("· موصى به");
  // The words stay as written; the complete Arabic face (theme.css) keeps them on the line.
  expect(list.textContent).toContain("مستطيلاً ومدخلاً");
});

test("the answer message: chosen options numbered, typed words for the one question left open, else after them", () => {
  expect(answerText(one, { q1: "Egypt" }, "")).toBe("Egypt");
  expect(answerText(one, {}, "Oman")).toBe("Oman");
  expect(answerText(two, { q1: "Egypt", q2: "Standard" }, "")).toBe("1. Egypt\n2. Standard");
  expect(answerText(two, { q1: "Egypt" }, "Marble floors")).toBe("1. Egypt\n2. Marble floors");
  const three = [...two, { id: "q3", question: "Storeys?", allow_other: true, options: [{ label: "One" }, { label: "Two" }] }];
  expect(answerText(three, { q2: "Standard" }, "Ask me later")).toBe("2. Standard\nAsk me later");
});

test("wizard keeps typed answers through Back and rerenders, sends once, then becomes read-only", async () => {
  const send = vi.fn();
  const props = { questions: two, picked: {}, onPick: vi.fn(), onAnswer: send };
  const view = render(<LanguageProvider><QuestionList {...props} /></LanguageProvider>);
  expect((screen.getByRole("button", { name: "Next" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Oman" } });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  expect(screen.getByText("2 of 2")).toBeTruthy();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Marble" } });
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  view.rerender(<LanguageProvider><QuestionList {...props} /></LanguageProvider>);
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("Oman");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("Marble");
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  expect(send).toHaveBeenCalledExactlyOnceWith("1. Oman\n2. Marble");
  await act(async () => {});
  view.rerender(<LanguageProvider><QuestionList {...props} active={false} /></LanguageProvider>);
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(document.querySelector(".qsummary")?.textContent).toBe("1. Oman\n2. Marble");
});

test.each(["or type another answer in the box below", "أو اكتب إجابة أخرى في المربع أدناه"])("input instruction %s is never a lettered option", async hint => {
  const send = vi.fn();
  render(<LanguageProvider><QuestionList questions={[{ ...one[0], options: [{ label: hint }] }]} onPick={vi.fn()} onAnswer={send} /></LanguageProvider>);
  expect(screen.queryByRole("listitem")).toBeNull();
  expect(document.querySelector(".k")).toBeNull();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "My answer" } });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  expect(send).toHaveBeenCalledExactlyOnceWith("My answer");
  await act(async () => {});
});

test("arrows choose options and a question with no options still accepts text", () => {
  const pick = vi.fn();
  const view = render(<LanguageProvider><QuestionList questions={one} onPick={pick} /></LanguageProvider>);
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "ArrowDown" });
  expect(pick).toHaveBeenCalledWith("q1", "Egypt");
  view.rerender(<LanguageProvider><QuestionList questions={[{ ...one[0], options: [] }]} onPick={pick} /></LanguageProvider>);
  expect(screen.queryByRole("listitem")).toBeNull();
  expect(screen.getByRole("textbox")).toBeTruthy();
});

test("Send waits for the result, prevents duplicate replies and keeps text on refusal", async () => {
  let finish!: (sent: boolean) => void;
  const send = vi.fn(() => new Promise<boolean>(resolve => { finish = resolve; }));
  render(<LanguageProvider><QuestionList questions={one} onPick={vi.fn()} onAnswer={send} /></LanguageProvider>);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Oman" } });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  expect(send).toHaveBeenCalledOnce();
  expect((screen.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finish(false));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("Oman");
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await act(async () => finish(true));
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(document.querySelector(".qsummary")?.textContent).toBe("Oman");
});

test("restored answers appear as a read-only summary", () => {
  render(<LanguageProvider><QuestionList questions={two} active={false} answeredText={"1. Oman\n2. Standard"} /></LanguageProvider>);
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
  expect(document.querySelector(".qsummary")?.textContent).toBe("1. Oman\n2. Standard");
});
