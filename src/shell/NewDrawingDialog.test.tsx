// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { lastNewUnits, NewDrawingDialog } from "./NewDrawingDialog";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

async function open() {
  const onCreate = vi.fn();
  const onClose = vi.fn();
  render(
    <LanguageProvider>
      <NewDrawingDialog onCreate={onCreate} onClose={onClose} />
    </LanguageProvider>,
  );
  await act(async () => {});
  return { onCreate, onClose };
}

test("metres the first time; Create makes it in the units chosen and keeps them for next time", async () => {
  expect(lastNewUnits()).toBe("m");
  const { onCreate } = await open();
  expect(screen.getByRole("radio", { name: "Metres" }).getAttribute("aria-checked")).toBe("true");
  fireEvent.click(screen.getByRole("radio", { name: "Millimetres" }));
  fireEvent.click(screen.getByRole("button", { name: "Create" }));
  expect(onCreate).toHaveBeenCalledWith("mm");
  expect(lastNewUnits()).toBe("mm");
});

test("Cancel makes nothing", async () => {
  const { onCreate, onClose } = await open();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onClose).toHaveBeenCalled();
  expect(onCreate).not.toHaveBeenCalled();
});
