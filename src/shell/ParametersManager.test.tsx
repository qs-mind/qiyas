// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { ParameterInfo } from "../api";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import { ParametersManager } from "./ParametersManager";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

const PARAMS: ParameterInfo[] = [
  { id: 9, name: "d1", kind: "linear", expression: "2*d2 + 1.5", value: 7.5, dimension: 40 },
  { id: 10, name: "d2", kind: "linear", expression: "3", value: 3, dimension: 41 },
  { id: 11, name: "ang1", kind: "angular", expression: "30", value: 30, dimension: 42 },
];

function open(
  onSet: (name: string, expression: string) => Promise<void>,
  parameters: ParameterInfo[] = PARAMS,
  onDelete: (id: number) => void = () => {},
) {
  render(
    <LanguageProvider>
      <ParametersManager parameters={parameters} precision={2} onSet={onSet} onDelete={onDelete} onClose={() => {}} />
    </LanguageProvider>,
  );
}

const field = (name: string) =>
  screen.getByRole("textbox", { name: en["param.expressionOf"].replace("{name}", name) }) as HTMLInputElement;

test("lists name, expression and value; Enter on a changed expression sets it", async () => {
  const onSet = vi.fn(async () => {});
  open(onSet);
  expect(screen.getByRole("heading", { name: en["param.manager"] })).toBeTruthy();
  expect(field("d1").value).toBe("2*d2 + 1.5");
  expect(screen.getByText("7.50")).toBeTruthy();
  expect(screen.getByText("30.00°")).toBeTruthy();
  // Unchanged: nothing sent.
  fireEvent.keyDown(field("d2"), { key: "Enter" });
  expect(onSet).not.toHaveBeenCalled();
  fireEvent.change(field("d2"), { target: { value: " 4 " } });
  fireEvent.keyDown(field("d2"), { key: "Enter" });
  await waitFor(() => expect(onSet).toHaveBeenCalledWith("d2", "4"));
});

test("a refusal (a circle) stays in the dialog", async () => {
  open(async () => {
    throw new Error("These parameters depend on each other in a circle: d2 > d1 > d2");
  });
  fireEvent.change(field("d2"), { target: { value: "d1 - 1" } });
  fireEvent.blur(field("d2"));
  expect((await screen.findByRole("alert")).textContent).toContain("That didn’t work. Technical details are available in Settings.");
});

test("a parameter that can't be worked out says why and can be set again or deleted", async () => {
  const onSet = vi.fn(async () => {});
  const onDelete = vi.fn();
  const broken: ParameterInfo = { ...PARAMS[0], value: null, error: "There is no parameter named d9" };
  open(onSet, [broken, PARAMS[1]], onDelete);
  expect(screen.getByRole("note").textContent).toBe(
    en["param.broken"].replace("{why}", "There is no parameter named d9"),
  );
  expect(screen.getByText("—")).toBeTruthy();
  fireEvent.change(field("d1"), { target: { value: "5" } });
  fireEvent.keyDown(field("d1"), { key: "Enter" });
  await waitFor(() => expect(onSet).toHaveBeenCalledWith("d1", "5"));
  fireEvent.click(screen.getByRole("button", { name: en["param.deleteOf"].replace("{name}", "d1") }));
  expect(onDelete).toHaveBeenCalledWith(9);
});
