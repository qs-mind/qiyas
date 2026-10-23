// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { FieldKind } from "../api";
import { LanguageProvider } from "../i18n";
import { FieldDialog, fieldCode } from "./FieldDialog";

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

function dialog(target: { handle: number; kind: string } | null = null, mode: "text" | "cell" = "text") {
  const onSubmit = vi.fn(async (_f: FieldKind) => {});
  const onPick = vi.fn();
  const view = render(
    <LanguageProvider>
      <FieldDialog mode={mode} target={target} picking={false} onPick={onPick} onSubmit={onSubmit} onClose={() => {}} />
    </LanguageProvider>,
  );
  return { onSubmit, onPick, view };
}

const kind = () => screen.getByRole("combobox", { name: "Field" }) as HTMLSelectElement;
const insert = () => screen.getByRole("button", { name: "Insert" }) as HTMLButtonElement;

test("the field codes are AutoCAD's, as the core writes them", () => {
  expect(fieldCode({ kind: "date" })).toBe('%<\\AcVar Date \\f "yyyy-MM-dd">%');
  expect(fieldCode({ kind: "file_name" })).toBe("%<\\AcVar Filename>%");
  expect(fieldCode({ kind: "area", target: { handle: 42, chain: [] } })).toBe(
    '%<\\AcObjProp Object(%<\\_ObjId 2A>%).Area \\f "%lu2%pr3">%',
  );
  expect(fieldCode({ kind: "length", target: { handle: 255, chain: [] } })).toBe(
    '%<\\AcObjProp Object(%<\\_ObjId FF>%).Length \\f "%lu2%pr3">%',
  );
  expect(fieldCode({ kind: "count", block: "DOOR" })).toBe('%<\\AcVar QiyasCount "DOOR">%');
});

test("a date or the file name goes in at once", async () => {
  const { onSubmit } = dialog();
  expect(kind().value).toBe("date");
  fireEvent.click(insert());
  await waitFor(() => expect(onSubmit).toHaveBeenCalledWith({ kind: "date" }));
  cleanup();
  const again = dialog();
  fireEvent.change(kind(), { target: { value: "file_name" } });
  fireEvent.click(insert());
  await waitFor(() => expect(again.onSubmit).toHaveBeenCalledWith({ kind: "file_name" }));
});

test("an area or a length reads a picked entity: Pick asks for it", async () => {
  const { onSubmit, onPick, view } = dialog();
  fireEvent.change(kind(), { target: { value: "area" } });
  expect(insert().disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Pick object" }));
  expect(onPick).toHaveBeenCalled();
  // Back with the pick (the dialog keeps the kind chosen).
  view.rerender(
    <LanguageProvider>
      <FieldDialog
        mode="text"
        target={{ handle: 42, kind: "LwPolyline" }}
        picking={false}
        onPick={onPick}
        onSubmit={onSubmit}
        onClose={() => {}}
      />
    </LanguageProvider>,
  );
  expect(kind().value).toBe("area");
  expect(screen.getByText(/LwPolyline 42/)).toBeTruthy();
  fireEvent.click(insert());
  await waitFor(() =>
    expect(onSubmit).toHaveBeenCalledWith({ kind: "area", target: { handle: 42, chain: [] } }),
  );
});

test("a count names its block", async () => {
  const { onSubmit } = dialog();
  fireEvent.change(kind(), { target: { value: "count" } });
  expect(insert().disabled).toBe(true);
  fireEvent.change(screen.getByRole("textbox", { name: "Block" }), { target: { value: "DOOR" } });
  fireEvent.click(insert());
  await waitFor(() => expect(onSubmit).toHaveBeenCalledWith({ kind: "count", block: "DOOR" }));
});
