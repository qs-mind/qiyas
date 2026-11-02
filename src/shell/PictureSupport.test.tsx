// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { PictureSupport } from "./PictureSupport";

const check = vi.hoisted(() => vi.fn());
vi.mock("../api", async original => ({ ...(await original<object>()), aiPictureSupport: check }));
beforeEach(() => { vi.useFakeTimers(); check.mockReset(); });
afterEach(() => { cleanup(); vi.useRealTimers(); localStorage.removeItem("qiyas.lang"); });
const tick = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));

test("shows checking until background detection answers, including a false answer", async () => {
  check.mockResolvedValueOnce(null).mockResolvedValueOnce(false);
  render(<PictureSupport provider="anthropic" model="model" />);
  expect(screen.getByRole("note").textContent).toBe("Checking picture support…");
  expect(screen.getByRole("note").classList.contains("picture-support")).toBe(true);
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.queryByRole("combobox")).toBeNull();
  await tick(500);
  expect(check).toHaveBeenCalledTimes(1);
  await tick(2000);
  expect(screen.getByRole("note").textContent).toBe("Text only — Mizan uses exact drawing checks");
});

test("a runtime refusal overrides model metadata and an expired answer returns to checking", async () => {
  check.mockResolvedValueOnce(false).mockResolvedValueOnce(null);
  render(<PictureSupport provider="anthropic" model="model" metadata={true} />);
  expect(screen.getByRole("note").textContent).toBe("Reads pictures");
  await tick(500);
  expect(screen.getByRole("note").textContent).toContain("Text only");
  await tick(30000);
  expect(screen.getByRole("note").textContent).toBe("Checking picture support…");
});

test("late replies for a previous model cannot change the selected model", async () => {
  let answer!: (value: boolean) => void;
  check.mockImplementationOnce(() => new Promise<boolean>(resolve => { answer = resolve; })).mockResolvedValue(false);
  const view = render(<PictureSupport provider="chatgpt_plan" model="old" account="account-a" />);
  await tick(500);
  view.rerender(<PictureSupport provider="chatgpt_plan" model="new" account="account-b" />);
  await tick(500);
  await act(async () => answer(true));
  expect(screen.getByRole("note").textContent).toContain("Text only");
});

test.each([
  [true, "يقرأ الصور"],
  [false, "نص فقط — يستخدم ميزان الفحوص الدقيقة للرسم"],
  [null, "فحص دعم الصور قيد التنفيذ…"],
])("shows picture support in Arabic: %s", (metadata, text) => {
  localStorage.setItem("qiyas.lang", "ar");
  render(<LanguageProvider><PictureSupport provider="claude_code" model="sonnet" metadata={metadata} ready={false} /></LanguageProvider>);
  expect(screen.getByRole("note").textContent).toBe(text);
});
