import { expect, test } from "vitest";
import type { Item } from "./messages";
import { chatLanguage, openQuestions } from "./thread";

const question = (id: string): Item => ({ kind: "question", questions: [{ id, question: "?", options: [{ label: "a" }, { label: "b" }], allow_other: true }] });

test("questions are open until the user writes after them", () => {
  expect(openQuestions([])).toBeNull();
  expect(openQuestions([{ kind: "user", text: "hi" }, question("q1")])?.index).toBe(1);
  expect(openQuestions([question("q1"), { kind: "assistant", text: "…", finished: true }])?.questions[0].id).toBe("q1");
  expect(openQuestions([question("q1"), { kind: "user", text: "a" }])).toBeNull();
  // Only the last questions count.
  expect(openQuestions([question("q1"), { kind: "user", text: "a" }, question("q2")])?.questions[0].id).toBe("q2");
});

test("the conversation speaks the user's latest language, else the window's", () => {
  expect(chatLanguage([], "en")).toBe("en");
  expect(chatLanguage([{ kind: "user", text: "ارسم فيلا" }], "en")).toBe("ar");
  expect(chatLanguage([{ kind: "user", text: "ارسم فيلا" }, { kind: "user", text: "draw a villa" }], "ar")).toBe("en");
  expect(chatLanguage([{ kind: "assistant", text: "مرحبا", finished: true }], "en")).toBe("en");
});
