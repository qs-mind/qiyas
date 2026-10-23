// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { PlotStyle, PlotStyleEntry } from "../api";
import { PlotStyleEditor } from "./PlotStyleEditor";

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

const entry = (name: string, over: Partial<PlotStyleEntry> = {}): PlotStyleEntry => ({
  name,
  localized_name: name,
  description: "",
  color: null,
  color_policy: 1,
  physical_pen: 0,
  virtual_pen: 0,
  screen: 100,
  linepattern_size: 0.5,
  linetype: 31,
  adaptive_linetype: true,
  lineweight: 0,
  fill_style: 73,
  end_style: 4,
  join_style: 5,
  extra: [["custom_key", "kept"]],
  ...over,
});

const CTB: PlotStyle = {
  kind: "ctb",
  description: "Monochrome",
  scale_factor: 1,
  apply_factor: false,
  display_units: 0,
  styles: [
    entry("Color_1"),
    entry("Color_2", { color: [0, 0, 0], lineweight: 1 }),
    entry("Color_3", { screen: 0 }),
  ],
  lineweights: [0, 0.25, 0.5],
  extra: [["unknown_top", "1"]],
};

const STB: PlotStyle = {
  ...CTB,
  kind: "stb",
  styles: [entry("Normal"), entry("Thick", { lineweight: 2 })],
};

function setup(over: Partial<Parameters<typeof PlotStyleEditor>[0]> = {}) {
  const props = {
    initialPath: null as string | null,
    chooseOpen: vi.fn(async () => "/styles/mono.ctb"),
    chooseSave: vi.fn(async () => "/styles/copy.ctb"),
    load: vi.fn(async (path: string) => (path.endsWith(".stb") ? STB : CTB)),
    save: vi.fn(async () => {}),
    onClose: vi.fn(),
    ...over,
  };
  render(<PlotStyleEditor {...props} />);
  return props;
}

const input = (name: string) => screen.getByLabelText(name) as HTMLInputElement;
const select = (name: string) => screen.getByLabelText(name) as HTMLSelectElement;
const button = (name: string) => screen.getByRole("button", { name }) as HTMLButtonElement;

test("nothing open: only Open… and Close work", () => {
  setup();
  expect(screen.getByRole("heading", { name: "Plot styles" })).toBeTruthy();
  expect(screen.getByText("Open a CTB or STB plot style file to edit its pens.")).toBeTruthy();
  expect(button("Save").disabled).toBe(true);
  expect(button("Save as…").disabled).toBe(true);
  fireEvent.click(button("Close"));
  expect(screen.queryByRole("table")).toBeNull();
});

test("a CTB opens with a row per colour: pen colour, screening, lineweight and plot", async () => {
  const props = setup();
  fireEvent.click(button("Open…"));
  await screen.findByRole("table");
  expect(props.load).toHaveBeenCalledWith("/styles/mono.ctb");
  expect(screen.getByText("Colour-dependent (CTB)")).toBeTruthy();
  expect(screen.getAllByRole("row")).toHaveLength(4);
  expect(screen.getByRole("columnheader", { name: "Colour" })).toBeTruthy();

  // Colour 1 plots the object's colour and lineweight.
  expect(input("Object's: Colour 1").checked).toBe(true);
  expect(input("Pen colour: Colour 1").disabled).toBe(true);
  expect(input("Screening %: Colour 1").value).toBe("100");
  expect(select("Lineweight: Colour 1").value).toBe("0");
  expect([...select("Lineweight: Colour 1").options].map((o) => o.text)).toEqual(["Object's", "0.25 mm", "0.50 mm"]);
  expect(input("Plot: Colour 1").checked).toBe(true);
  // Colour 2 is black at 0.25 mm.
  expect(input("Object's: Colour 2").checked).toBe(false);
  expect(input("Pen colour: Colour 2").value).toBe("#000000");
  expect(select("Lineweight: Colour 2").value).toBe("1");
  // Colour 3 does not plot (screening 0).
  expect(input("Plot: Colour 3").checked).toBe(false);
  expect(input("Screening %: Colour 3").disabled).toBe(true);
});

test("pens are edited and saved over the file, keeping what Qiyas doesn't edit", async () => {
  const props = setup({ initialPath: "/styles/mono.ctb" });
  await screen.findByRole("table");
  expect(props.chooseOpen).not.toHaveBeenCalled();

  fireEvent.click(input("Object's: Colour 1"));
  fireEvent.change(input("Pen colour: Colour 1"), { target: { value: "#ff8000" } });
  fireEvent.change(input("Screening %: Colour 1"), { target: { value: "50" } });
  fireEvent.change(select("Lineweight: Colour 2"), { target: { value: "2" } });
  fireEvent.click(input("Object's: Colour 2"));
  fireEvent.click(input("Plot: Colour 2"));
  fireEvent.click(input("Plot: Colour 3"));
  fireEvent.click(button("Save"));

  await screen.findByText("Saved \u2068/styles/mono.ctb\u2069");
  expect(props.save).toHaveBeenCalledWith("/styles/mono.ctb", {
    ...CTB,
    styles: [
      { ...CTB.styles[0], color: [255, 128, 0], screen: 50 },
      { ...CTB.styles[1], color: null, lineweight: 2, screen: 0 },
      // Plotting again is at full strength.
      { ...CTB.styles[2], screen: 100 },
    ],
  });
});

test("Save as writes another file, of the table's kind, and Save goes there after", async () => {
  const props = setup({ initialPath: "/styles/mono.ctb" });
  await screen.findByRole("table");
  fireEvent.change(input("Screening %: Colour 1"), { target: { value: "120" } });
  // Screening is 0 to 100.
  expect(input("Screening %: Colour 1").value).toBe("100");
  fireEvent.click(button("Save as…"));
  await screen.findByText("Saved \u2068/styles/copy.ctb\u2069");
  expect(props.chooseSave).toHaveBeenCalledWith("ctb", "/styles/mono.ctb");
  expect(props.save).toHaveBeenLastCalledWith("/styles/copy.ctb", CTB);
  fireEvent.click(button("Save"));
  await waitFor(() => expect(props.save).toHaveBeenCalledTimes(2));
  expect(props.save).toHaveBeenLastCalledWith("/styles/copy.ctb", CTB);
});

test("an STB lists its named styles, whose names can be changed", async () => {
  const props = setup({ initialPath: "/styles/named.stb", chooseSave: vi.fn(async () => null) });
  await screen.findByRole("table");
  expect(screen.getByText("Named styles (STB)")).toBeTruthy();
  expect(screen.getByRole("columnheader", { name: "Style" })).toBeTruthy();
  expect(input("Style 1").value).toBe("Normal");
  expect(select("Lineweight: Style 2").value).toBe("2");
  fireEvent.change(input("Style 2"), { target: { value: "Heavy" } });
  // Save as cancelled: nothing written.
  fireEvent.click(button("Save as…"));
  await waitFor(() => expect(props.chooseSave).toHaveBeenCalledWith("stb", "/styles/named.stb"));
  expect(props.save).not.toHaveBeenCalled();
  fireEvent.click(button("Save"));
  await waitFor(() =>
    expect(props.save).toHaveBeenCalledWith("/styles/named.stb", {
      ...STB,
      styles: [STB.styles[0], { ...STB.styles[1], name: "Heavy", localized_name: "Heavy" }],
    }),
  );
});

test("a file that can't be read, or saved, says why", async () => {
  const props = setup({
    load: vi.fn(async () => {
      throw "Not a CTB or STB plot style file";
    }),
  });
  fireEvent.click(button("Open…"));
  expect((await screen.findByRole("alert")).textContent).toBe("The file could not be read. Choose another file or a backup.");
  expect(screen.queryByRole("table")).toBeNull();

  vi.mocked(props.load).mockImplementation(async () => CTB);
  vi.mocked(props.save).mockImplementation(async () => {
    throw "Can't write /styles/mono.ctb: Permission denied";
  });
  fireEvent.click(button("Open…"));
  await screen.findByRole("table");
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.click(button("Save"));
  expect((await screen.findByRole("alert")).textContent).toBe("This item cannot be changed here. Check that it is unlocked.");
});

test("Close closes", () => {
  const props = setup();
  fireEvent.click(button("Close"));
  expect(props.onClose).toHaveBeenCalled();
});
