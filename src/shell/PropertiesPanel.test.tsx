// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Current, EntityInfo } from "../api";
import { PropertiesPanel } from "./PropertiesPanel";

afterEach(cleanup);

const door: EntityInfo = {
  handle: 0x2a,
  kind: "Insert",
  layer: "A-DOOR",
  layer_short: "A-DOOR",
  length: null,
  area: null,
  block_name: "DOOR",
  text: null,
  inside_block: null,
  chain: [],
  block_path: [],
  color: "ACI 4",
  color_rgb: [0, 255, 255],
  linetype: "ByLayer",
  lineweight: 35,
  radius: null,
  attributes: [
    ["MARK", "D1"],
    ["FIRE", "60 min"],
  ],
  height: null,
  style: null,
};

/** The panel's label and value pairs, attributes apart. */
const rows = () =>
  [...document.querySelectorAll(".properties .prop-section:not(.attributes) dl > div")].map((d) => [
    d.querySelector("dt")!.textContent,
    d.querySelector("dd")!.textContent,
  ]);

test("a block reference shows its handle, colour, weight and attributes", () => {
  render(<PropertiesPanel entity={door} units="metres" precision={3} sheet="Model" />);
  expect(rows()).toEqual([
    ["Handle", "42"],
    ["Type", "Insert"],
    ["Layer", "A-DOOR"],
    ["Colour", "ACI 4"],
    ["Linetype", "ByLayer"],
    ["Lineweight", "0.35 mm"],
    ["Sheet", "Model"],
    ["Block", "DOOR"],
  ]);
  const swatch = document.querySelector(".properties .swatch") as HTMLElement;
  expect(swatch.style.background).toBe("rgb(0, 255, 255)");
  const attributes = [...document.querySelectorAll(".attributes dl > div")].map((d) => d.textContent);
  expect(attributes).toEqual(["MARKD1", "FIRE60 min"]);
});

test("sizes are in metres with the precision, ByBlock and texts read out", () => {
  const arc: EntityInfo = {
    ...door,
    kind: "Arc",
    block_name: null,
    attributes: [],
    color: "ByBlock",
    lineweight: -1,
    length: 6283.1853,
    radius: 2000,
    block_path: ["PLAN", "DOOR"],
  };
  render(<PropertiesPanel entity={arc} units="millimetres" precision={2} sheet="Model" />);
  const got = Object.fromEntries(rows());
  expect(got.Colour).toBe("ByBlock");
  expect(got.Lineweight).toBe("ByLayer");
  expect(got.Length).toBe("6.28 m");
  expect(got.Radius).toBe("2.00 m");
  expect(got.Inside).toBe("PLAN › DOOR");
  expect(document.querySelector(".attributes")).toBeNull();
  cleanup();

  const text: EntityInfo = {
    ...arc,
    kind: "Text",
    text: "غرفة",
    height: 0.25,
    style: "ARIAL",
    length: null,
    radius: null,
  };
  render(<PropertiesPanel entity={text} units="metres" precision={3} sheet="Layout1" />);
  const t = Object.fromEntries(rows());
  expect([t.Text, t.Height, t.Style, t.Sheet]).toEqual(["غرفة", "0.250 m", "ARIAL", "Layout1"]);
});

test("nothing picked says how to pick, and the panel collapses", () => {
  render(<PropertiesPanel entity={null} units="metres" precision={3} sheet="Model" />);
  expect(screen.getByText("Pick one entity with Select to see its properties.")).toBeTruthy();
  const toggle = screen.getByRole("button", { name: "Properties" });
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText("Pick one entity with Select to see its properties.")).toBeNull();
});

test("a bound xref's layer shows by its own name, the full name in its tooltip", () => {
  const wall: EntityInfo = { ...door, layer: "01-IVORY$0$A-WALL", layer_short: "A-WALL" };
  render(<PropertiesPanel entity={wall} units="metres" precision={3} sheet="Model" />);
  expect(Object.fromEntries(rows()).Layer).toBe("A-WALL");
  expect(screen.getByText("A-WALL").getAttribute("title")).toBe("01-IVORY$0$A-WALL");
});

const editing = {
  layers: ["0", "A-DOOR", "A-WALL"],
  linetypes: ["Continuous", "DASHED"],
};

/** The field labelled `name`. */
const field = (name: string) => screen.getByLabelText(name) as HTMLInputElement & HTMLSelectElement;
const enter = (name: string, value: string) => {
  fireEvent.change(field(name), { target: { value } });
  fireEvent.keyDown(field(name), { key: "Enter" });
};

test("a drawing's own entity is edited in place: general properties and a circle's radius in metres", () => {
  const onSet = vi.fn();
  const circle: EntityInfo = {
    ...door,
    kind: "Circle",
    block_name: null,
    attributes: [],
    layer: "A-WALL",
    layer_short: "A-WALL",
    color: "ByLayer",
    linetype: "ByLayer",
    lineweight: -1,
    radius: 2000,
    length: 12566.37,
  };
  render(<PropertiesPanel entity={circle} units="millimetres" precision={3} sheet="Model" {...editing} onSet={onSet} />);
  fireEvent.change(field("Layer"), { target: { value: "A-DOOR" } });
  expect(onSet).toHaveBeenLastCalledWith("layer", "A-DOOR");
  expect(field("Colour").value).toBe("ByLayer");
  enter("Colour", "ACI 3");
  expect(onSet).toHaveBeenLastCalledWith("color", "ACI 3");
  fireEvent.change(field("Linetype"), { target: { value: "DASHED" } });
  expect(onSet).toHaveBeenLastCalledWith("linetype", "DASHED");
  fireEvent.change(field("Lineweight"), { target: { value: "25" } });
  expect(onSet).toHaveBeenLastCalledWith("lineweight", 25);
  // Shown and typed in metres, sent in drawing units.
  expect(field("Radius (m)").value).toBe("2.000");
  enter("Radius (m)", "3");
  expect(onSet).toHaveBeenLastCalledWith("radius", 3000);
  // A value that is no number sends nothing.
  onSet.mockClear();
  enter("Radius (m)", "abc");
  expect(onSet).not.toHaveBeenCalled();
});

test("texts, polylines and block references show their own fields", () => {
  const onSet = vi.fn();
  const text: EntityInfo = { ...door, kind: "Text", block_name: null, attributes: [], text: "A", height: 0.25, rotation: 0 };
  const { rerender } = render(
    <PropertiesPanel entity={text} units="metres" precision={3} sheet="Model" {...editing} onSet={onSet} />,
  );
  enter("Text", "B");
  expect(onSet).toHaveBeenLastCalledWith("text", "B");
  enter("Height (m)", "0.5");
  expect(onSet).toHaveBeenLastCalledWith("text_height", 0.5);
  enter("Rotation (°)", "90");
  expect(onSet).toHaveBeenLastCalledWith("text_rotation", 90);

  const pline: EntityInfo = { ...door, kind: "LwPolyline", block_name: null, attributes: [], closed: true, global_width: 0 };
  rerender(<PropertiesPanel entity={pline} units="metres" precision={3} sheet="Model" {...editing} onSet={onSet} />);
  fireEvent.click(field("Closed"));
  expect(onSet).toHaveBeenLastCalledWith("closed", false);
  enter("Global width (m)", "0.2");
  expect(onSet).toHaveBeenLastCalledWith("global_width", 0.2);

  rerender(
    <PropertiesPanel entity={{ ...door, scale: 1, rotation: 0 }} units="metres" precision={3} sheet="Model" {...editing} onSet={onSet} />,
  );
  enter("Scale", "2");
  expect(onSet).toHaveBeenLastCalledWith("insert_scale", 2);
  enter("Rotation (°)", "45");
  expect(onSet).toHaveBeenLastCalledWith("insert_rotation", 45);
});

test("an xref's entity or one inside a block stays read-only", () => {
  const onSet = vi.fn();
  const inXref: EntityInfo = { ...door, xref: "PLAN", chain: [7] };
  const { rerender } = render(
    <PropertiesPanel entity={inXref} units="metres" precision={3} sheet="Model" {...editing} onSet={onSet} />,
  );
  expect(document.querySelector(".properties input, .properties select")).toBeNull();
  expect(Object.fromEntries(rows()).Layer).toBe("A-DOOR");
  rerender(
    <PropertiesPanel entity={{ ...door, chain: [7] }} units="metres" precision={3} sheet="Model" {...editing} onSet={onSet} />,
  );
  expect(document.querySelector(".properties input, .properties select")).toBeNull();
});

test("with nothing picked the palette edits what new entities are drawn with", () => {
  const onSetCurrent = vi.fn();
  const current: Current = { layer: "0", color: "ByLayer", linetype: "ByLayer", lineweight: -1 };
  render(
    <PropertiesPanel
      entity={null}
      units="metres"
      precision={3}
      sheet="Model"
      {...editing}
      current={current}
      onSetCurrent={onSetCurrent}
    />,
  );
  expect(screen.getByText("New entities")).toBeTruthy();
  fireEvent.change(field("Layer"), { target: { value: "A-WALL" } });
  expect(onSetCurrent).toHaveBeenLastCalledWith({ ...current, layer: "A-WALL" });
  enter("Colour", "#ff0000");
  expect(onSetCurrent).toHaveBeenLastCalledWith({ ...current, color: "#ff0000" });
  fireEvent.change(field("Linetype"), { target: { value: "ByBlock" } });
  expect(onSetCurrent).toHaveBeenLastCalledWith({ ...current, linetype: "ByBlock" });
  fireEvent.change(field("Lineweight"), { target: { value: "-3" } });
  expect(onSetCurrent).toHaveBeenLastCalledWith({ ...current, lineweight: -3 });
});

test("a paper-space viewport's scale and centre are edited in place", () => {
  const onSet = vi.fn();
  const vp: EntityInfo = {
    ...door,
    kind: "Viewport",
    block_name: null,
    attributes: [],
    viewport_scale: 0.02,
    viewport_centre: [11, 4.5],
  };
  render(<PropertiesPanel entity={vp} units="metres" precision={3} sheet="Layout1" {...editing} onSet={onSet} />);
  enter("Viewport scale", "0.01");
  expect(onSet).toHaveBeenLastCalledWith("viewport_scale", 0.01);
  enter("View centre (x, y)", "20, 7.5");
  expect(onSet).toHaveBeenLastCalledWith("viewport_centre", [20, 7.5]);
  // Not a point: nothing sent.
  onSet.mockClear();
  enter("View centre (x, y)", "20");
  expect(onSet).not.toHaveBeenCalled();
});

test("asking to reveal it opens a folded palette and takes the focus", () => {
  const { rerender } = render(<PropertiesPanel entity={door} units="metres" precision={3} sheet="Model" reveal={0} />);
  const title = screen.getByRole("button", { name: "Properties" });
  fireEvent.click(title);
  expect(title.getAttribute("aria-expanded")).toBe("false");
  // The canvas menu's Properties counts the request up.
  rerender(<PropertiesPanel entity={door} units="metres" precision={3} sheet="Model" reveal={1} />);
  expect(title.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(title);
});

const wallLine: EntityInfo = {
  ...door,
  kind: "Line",
  block_name: null,
  attributes: [],
  layer: "A-WALL",
  layer_short: "A-WALL",
  start: [0, 0],
  end: [3000, 4000],
  length: 5000,
};

test("a line shows General and Geometry sections with metres and degrees", () => {
  render(<PropertiesPanel entity={wallLine} units="millimetres" precision={3} sheet="Model" />);
  expect([...document.querySelectorAll(".prop-section h3 button")].map((b) => b.textContent)).toEqual([
    "General",
    "Geometry",
  ]);
  const got = Object.fromEntries(rows());
  expect(got["Start X"]).toBe("0.000 m");
  expect(got["End Y"]).toBe("4.000 m");
  expect(got["Delta X"]).toBe("3.000 m");
  expect(got.Length).toBe("5.000 m");
  expect(got.Angle).toBe("53.130°");
});

test("a section folds, and stays folded the next time", () => {
  localStorage.clear();
  const { unmount } = render(<PropertiesPanel entity={wallLine} units="metres" precision={3} sheet="Model" />);
  const geometry = screen.getByRole("button", { name: "Geometry" });
  expect(geometry.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(geometry);
  expect(geometry.getAttribute("aria-expanded")).toBe("false");
  expect(Object.fromEntries(rows()).Length).toBeUndefined();
  expect(Object.fromEntries(rows()).Layer).toBe("A-WALL");
  unmount();
  render(<PropertiesPanel entity={wallLine} units="metres" precision={3} sheet="Model" />);
  expect(screen.getByRole("button", { name: "Geometry" }).getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: "Geometry" }));
  expect(Object.fromEntries(rows()).Length).toBe("5000.000 m");
  localStorage.clear();
});

test("clicking a value copies the number without its unit", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  render(<PropertiesPanel entity={wallLine} units="metres" precision={2} sheet="Model" />);
  const length = screen.getByText("5000.00 m");
  fireEvent.click(length);
  expect(writeText).toHaveBeenCalledWith("5000.00");
  expect(await screen.findByTitle("Copied")).toBeTruthy();
});

test("a polyline's closed flag and width stay editable beside the read-only vertices and perimeter", () => {
  const onSet = vi.fn();
  const room: EntityInfo = {
    ...door,
    kind: "LwPolyline",
    block_name: null,
    attributes: [],
    vertex_count: 4,
    closed: true,
    global_width: 0,
    length: 14,
    area: 12,
  };
  render(<PropertiesPanel entity={room} units="metres" precision={3} sheet="Model" {...editing} onSet={onSet} />);
  const got = Object.fromEntries(rows());
  expect(got.Vertices).toBe("4");
  expect(got.Perimeter).toBe("14.000 m");
  expect(got.Area).toBe("12.000 m²");
  fireEvent.click(field("Closed"));
  expect(onSet).toHaveBeenLastCalledWith("closed", false);
});

test("several entities show the count, the sums and what they share", () => {
  const wall = (length: number): EntityInfo => ({ ...wallLine, length });
  const room: EntityInfo = { ...wallLine, kind: "LwPolyline", length: 14, area: 12, layer: "A-ROOM", layer_short: "A-ROOM" };
  render(<PropertiesPanel entity={null} many={[wall(10), wall(5), room]} units="metres" precision={2} sheet="Model" />);
  expect([...document.querySelectorAll(".prop-section h3 button")].map((b) => b.textContent)).toEqual([
    "Selection",
    "General",
  ]);
  const got = Object.fromEntries(rows());
  expect(got.Selected).toBe("3");
  expect(got.Types).toBe("Line ×2, LwPolyline ×1");
  expect(got["Total length"]).toBe("29.00 m");
  expect(got["Total area"]).toBe("12.00 m²");
  // The layers differ, the colour does not.
  expect(got.Layer).toBe("Varies");
  expect(got.Colour).toBe("ACI 4");
  expect(got.Handle).toBeUndefined();
  expect(document.querySelector(".properties input, .properties select")).toBeNull();
});
