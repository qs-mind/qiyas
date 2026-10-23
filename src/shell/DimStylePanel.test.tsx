// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { DimStyleInfo } from "../api";
import { LanguageProvider } from "../i18n";
import { DimStylePanel } from "./DimStylePanel";
import { ToleranceDialog } from "./ToleranceDialog";

beforeAll(() => {
  // jsdom may not implement modal dialogs.
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

const fields = {
  arrow_size: 2.5,
  text_height: 2.5,
  precision: 2,
  ext_offset: 0.625,
  ext_extension: 1.25,
  gap: 0.625,
  scale: 1,
  show_units: true,
};
const styles: DimStyleInfo[] = [
  { name: "QS-50", fields: { ...fields, scale: 50, precision: 3, show_units: false } },
  { name: "Standard", fields },
];

function panel(onSave = vi.fn(async () => {}), onScale = vi.fn(async () => {})) {
  render(
    <LanguageProvider>
      <DimStylePanel
        styles={styles}
        current="Standard"
        annotationScale="1:1"
        onSave={onSave}
        onScale={onScale}
        onClose={() => {}}
      />
    </LanguageProvider>,
  );
  return { onSave, onScale };
}

const field = (name: string) => screen.getByRole("spinbutton", { name }) as HTMLInputElement;

test("shows the current style's fields and saves an edit to it", async () => {
  const { onSave } = panel();
  expect((screen.getByRole("combobox", { name: "Style" }) as HTMLSelectElement).value).toBe("Standard");
  expect(field("Text height").value).toBe("2.5");
  expect(field("Precision (decimal places)").value).toBe("2");
  fireEvent.change(field("Text height"), { target: { value: "3.5" } });
  fireEvent.click(screen.getByRole("button", { name: "Save style" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith("Standard", { ...fields, text_height: 3.5 }, false));
  // The current style is current already.
  expect(screen.getByRole("button", { name: "Make current" })).toHaveProperty("disabled", true);
});

test("chooses another style, makes it current; a bad value can't be saved", async () => {
  const { onSave } = panel();
  fireEvent.change(screen.getByRole("combobox", { name: "Style" }), { target: { value: "QS-50" } });
  expect(field("Scale").value).toBe("50");
  fireEvent.click(screen.getByRole("button", { name: "Make current" }));
  await waitFor(() =>
    expect(onSave).toHaveBeenCalledWith(
      "QS-50",
      { ...fields, scale: 50, precision: 3, show_units: false },
      true,
    ),
  );
  fireEvent.change(field("Text height"), { target: { value: "0" } });
  expect(screen.getByRole("button", { name: "Save style" })).toHaveProperty("disabled", true);
});

test("creates a style from the current one, sets the annotation scale, shows errors", async () => {
  const onSave = vi.fn(async () => {});
  const onScale = vi.fn(async () => {
    throw "big is no scale";
  });
  panel(onSave, onScale);
  fireEvent.change(screen.getByRole("textbox", { name: "New style name" }), { target: { value: "QS-100" } });
  fireEvent.click(screen.getByRole("button", { name: "New from current" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith("QS-100", fields, false));
  fireEvent.change(screen.getByRole("combobox", { name: "Annotation scale" }), { target: { value: "big" } });
  fireEvent.click(screen.getByRole("button", { name: "Set scale" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Check the values you entered, then try again."));
  expect(onScale).toHaveBeenCalledWith("big");
});

test("Show units is on by default, saved with the style, and follows the chosen style", async () => {
  const { onSave } = panel();
  const units = () => screen.getByRole("checkbox", { name: /Show units/ }) as HTMLInputElement;
  expect(units().checked).toBe(true);
  fireEvent.click(units());
  expect(units().checked).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Save style" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith("Standard", { ...fields, show_units: false }, false));
  // QS-50 has it off; switching the style shows that, and turning it on is saved.
  fireEvent.change(screen.getByRole("combobox", { name: "Style" }), { target: { value: "QS-50" } });
  expect(units().checked).toBe(false);
  fireEvent.click(units());
  fireEvent.click(screen.getByRole("button", { name: "Save style" }));
  await waitFor(() =>
    expect(onSave).toHaveBeenLastCalledWith("QS-50", { ...fields, scale: 50, precision: 3, show_units: true }, false),
  );
  // A new style made from the current one starts with the current style's switch.
  fireEvent.change(screen.getByRole("textbox", { name: "New style name" }), { target: { value: "QS-100" } });
  fireEvent.click(screen.getByRole("button", { name: "New from current" }));
  await waitFor(() => expect(onSave).toHaveBeenLastCalledWith("QS-100", fields, false));
});

test("the tolerance dialog writes AutoCAD's frame", async () => {
  const onSubmit = vi.fn(async () => {});
  render(
    <LanguageProvider>
      <ToleranceDialog onSubmit={onSubmit} onClose={() => {}} />
    </LanguageProvider>,
  );
  fireEvent.change(screen.getByRole("textbox", { name: "Tolerance" }), { target: { value: "0.5" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Datum 1" }), { target: { value: "A" } });
  fireEvent.click(screen.getByRole("button", { name: "Place" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledWith("{\\Fgdt;j}%%v0.5%%v%%vA%%v%%v"));
});
