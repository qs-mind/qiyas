// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Question } from "../api";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import { QuestionList } from "./QuestionList";

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
  expect(rows[3]).toContain(`4${en["ai.questionOther"]}`);
  expect(document.querySelector(".qopt.hot")?.textContent).toContain("Egypt");
  // No cards: nothing here draws its own box.
  expect(document.querySelectorAll(".qcard, .card")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: /Saudi Arabia/ }));
  expect(onPick).toHaveBeenCalledWith("Saudi Arabia");
});

test("several questions are numbered and their options lettered; an option answers with its question's number", () => {
  const onPick = vi.fn();
  render(<LanguageProvider><QuestionList questions={two} onPick={onPick} /></LanguageProvider>);
  expect(screen.getByText(/^2\. /).textContent).toContain("Which finish?");
  fireEvent.click(screen.getByRole("button", { name: /High-end/ }));
  expect(onPick).toHaveBeenCalledWith("2. High-end");
  expect(screen.getAllByRole("listitem")[0].textContent).toContain("a");
});

test("once answered it is only a record: the options do nothing and the 'Other' line is gone", () => {
  render(<LanguageProvider><QuestionList questions={one} /></LanguageProvider>);
  expect((screen.getByRole("button", { name: /Egypt/ }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByText(en["ai.questionOther"])).toBeNull();
});
