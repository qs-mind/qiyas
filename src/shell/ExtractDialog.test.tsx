// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { ExtractDialog, toTsv } from "./ExtractDialog";

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

function setup() {
  const props = {
    layers: ["A-DOOR", "A-WALL", "A-WINDOW"],
    onRun: vi.fn(async () => [
      ["DOOR", "D1"],
      ["DOOR", "D2"],
    ]),
    onExport: vi.fn(async () => {}),
    onClose: vi.fn(),
  };
  render(<ExtractDialog {...props} />);
  return props;
}

test("Run emits one filter per layer chosen and the fields, then shows the table", async () => {
  const props = setup();
  const layers = screen.getByRole("listbox") as HTMLSelectElement;
  for (const o of Array.from(layers.options)) o.selected = o.value !== "A-WALL";
  fireEvent.change(layers);
  fireEvent.change(screen.getByRole("combobox", { name: "Type" }), { target: { value: "Insert" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Block name" }), { target: { value: " DOOR " } });
  // Only the block name, and a MARK attribute column.
  for (const name of ["Type", "Layer", "Text", "Length (m)", "Area (m²)"]) {
    fireEvent.click(screen.getByRole("checkbox", { name }));
  }
  fireEvent.change(screen.getByRole("textbox", { name: "Attribute tags, comma-separated" }), {
    target: { value: "MARK, " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Run" }));

  await screen.findByText("2 rows");
  const base = { kind: "Insert", block_name: "DOOR", text_contains: null, limit: 5000 };
  expect(props.onRun).toHaveBeenCalledWith(
    [
      { ...base, layer: "A-DOOR" },
      { ...base, layer: "A-WINDOW" },
    ],
    ["block_name", { attribute: "MARK" }],
    expect.any(Function),
  );
  const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
  expect(headers).toEqual(["Block", "MARK"]);
  expect(screen.getAllByRole("cell").map((c) => c.textContent)).toEqual(["DOOR", "D1", "DOOR", "D2"]);

  fireEvent.click(screen.getByRole("button", { name: "Export…" }));
  await waitFor(() =>
    expect(props.onExport).toHaveBeenCalledWith(["Block", "MARK"], [
      ["DOOR", "D1"],
      ["DOOR", "D2"],
    ]),
  );
});

test("with no layer chosen one filter covers every layer; no field, no run", async () => {
  const props = setup();
  fireEvent.click(screen.getByRole("button", { name: "Run" }));
  await screen.findByText("2 rows");
  expect(props.onRun).toHaveBeenCalledWith(
    [{ kind: null, block_name: null, text_contains: null, limit: 5000 }],
    ["kind", "layer", "block_name", "text", "length", "area"],
    expect.any(Function),
  );
  for (const name of ["Type", "Layer", "Block", "Text", "Length (m)", "Area (m²)"]) {
    fireEvent.click(screen.getByRole("checkbox", { name }));
  }
  expect((screen.getByRole("button", { name: "Run" }) as HTMLButtonElement).disabled).toBe(true);
});

test("copy gives tab-separated lines with the header", () => {
  expect(
    toTsv(
      ["Block", "Text"],
      [
        ["DOOR", "two\tparts"],
        ["WIN", "line\nbreak"],
      ],
    ),
  ).toBe("Block\tText\nDOOR\ttwo parts\nWIN\tline break");
});
