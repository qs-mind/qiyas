// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { LayerInfo } from "../api";
import { LayersPanel } from "./LayersPanel";

afterEach(() => { cleanup(); localStorage.clear(); });

const layer = (name: string, frozen = false): LayerInfo => ({
  name,
  color: [255, 0, 0],
  off: false,
  frozen,
  locked: false,
});

test("layers panel toggles and isolates", () => {
  const onToggle = vi.fn();
  const onIsolate = vi.fn();
  render(
    <LayersPanel
      layers={[layer("A-WALL"), layer("A-DOOR"), layer("X-OLD", true)]}
      hidden={new Set(["A-DOOR"])}
      onToggle={onToggle}
      onIsolate={onIsolate}
      onShowAll={() => {}}
    />,
  );
  const button = (name: string) => screen.getByRole("button", { name }) as HTMLButtonElement;

  const doorEye = button("Visible: A-DOOR");
  expect(doorEye.getAttribute("aria-pressed")).toBe("false");
  expect(button("Visible: A-WALL").getAttribute("aria-pressed")).toBe("true");

  fireEvent.click(doorEye);
  expect(onToggle).toHaveBeenCalledWith("A-DOOR");
  fireEvent.click(button("Isolate: A-WALL"));
  expect(onIsolate).toHaveBeenCalledWith("A-WALL");

  // A frozen layer stays hidden whatever the user does: no eye, no isolate into a blank view.
  expect(button("Visible: X-OLD").disabled).toBe(true);
  expect(button("Isolate: X-OLD").disabled).toBe(true);
});

test("while a layer change runs the buttons wait: aria-disabled, clicks ignored", () => {
  const onToggle = vi.fn();
  const onIsolate = vi.fn();
  render(
    <LayersPanel
      layers={[layer("A-WALL")]}
      hidden={new Set()}
      busy
      onToggle={onToggle}
      onIsolate={onIsolate}
      onShowAll={() => {}}
    />,
  );
  const eye = screen.getByRole("button", { name: "Visible: A-WALL" });
  const isolate = screen.getByRole("button", { name: "Isolate: A-WALL" });
  expect(eye.getAttribute("aria-disabled")).toBe("true");
  expect(isolate.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(eye);
  fireEvent.click(isolate);
  expect(onToggle).not.toHaveBeenCalled();
  expect(onIsolate).not.toHaveBeenCalled();
});

test("a white layer's swatch is Ink on the light theme and white on the dark one", () => {
  const white: LayerInfo = { ...layer("A-TEXT"), color: [255, 255, 255] };
  const props = { layers: [white], hidden: new Set<string>(), onToggle() {}, onIsolate() {}, onShowAll() {} };
  const { container, rerender } = render(<LayersPanel {...props} theme="light" />);
  const swatch = () => container.querySelector<HTMLElement>(".swatch")!;
  expect(swatch().style.background).toBe("rgb(14, 15, 18)");
  rerender(<LayersPanel {...props} theme="dark" />);
  expect(swatch().style.background).toBe("rgb(255, 255, 255)");
});

test("in Arabic, a Latin layer name is isolated so it reads in its own direction", () => {
  const name = "01-IVORY TOWER$0$A-FURN";
  document.documentElement.dir = "rtl";
  try {
    render(
      <LayersPanel
        layers={[layer(name)]}
        hidden={new Set()}
        onToggle={() => {}}
        onIsolate={() => {}}
        onShowAll={() => {}}
      />,
    );
    const shown = screen.getByText(name);
    expect(shown.tagName).toBe("BDI");
    expect(shown.closest(".name")).not.toBeNull();
  } finally {
    document.documentElement.dir = "";
  }
});

test("make current marks the current layer; an xref's layer can't be current", () => {
  const onMakeCurrent = vi.fn();
  render(
    <LayersPanel
      layers={[layer("0"), layer("A-WALL"), layer("PLAN|A-DOOR")]}
      hidden={new Set()}
      current="A-WALL"
      onToggle={() => {}}
      onIsolate={() => {}}
      onShowAll={() => {}}
      onMakeCurrent={onMakeCurrent}
    />,
  );
  const button = (name: string) => screen.getByRole("button", { name }) as HTMLButtonElement;
  expect(button("Make current: A-WALL").getAttribute("aria-pressed")).toBe("true");
  expect(button("Make current: 0").getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(button("Make current: 0"));
  expect(onMakeCurrent).toHaveBeenCalledWith("0");
  expect(button("Make current: PLAN|A-DOOR").disabled).toBe(true);
});

test("a layer's properties open under it and each edit goes to the core", () => {
  const onSetLayer = vi.fn();
  const onRenameLayer = vi.fn();
  const onDeleteLayer = vi.fn();
  const wall: LayerInfo = {
    ...layer("A-WALL"),
    color_name: "ACI 1",
    linetype: "Continuous",
    lineweight: -3,
    plot: true,
  };
  render(
    <LayersPanel
      layers={[wall, layer("PLAN|A-DOOR")]}
      hidden={new Set()}
      linetypes={["Continuous", "DASHED"]}
      onToggle={() => {}}
      onIsolate={() => {}}
      onShowAll={() => {}}
      onSetLayer={onSetLayer}
      onRenameLayer={onRenameLayer}
      onDeleteLayer={onDeleteLayer}
    />,
  );
  // An xref's layer is the xref's: nothing to edit.
  expect(screen.queryByRole("button", { name: "Layer properties: PLAN|A-DOOR" })).toBeNull();
  const open = screen.getByRole("button", { name: "Layer properties: A-WALL" });
  expect(screen.queryByRole("checkbox", { name: "Frozen" })).toBeNull();
  fireEvent.click(open);
  expect(open.getAttribute("aria-expanded")).toBe("true");

  fireEvent.click(screen.getByRole("checkbox", { name: "Frozen" }));
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "frozen", true);
  fireEvent.click(screen.getByRole("checkbox", { name: "On" }));
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "on", false);
  fireEvent.click(screen.getByRole("checkbox", { name: "Locked" }));
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "locked", true);
  fireEvent.click(screen.getByRole("checkbox", { name: "Plot" }));
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "plot", false);

  const colour = screen.getByRole("textbox", { name: "Colour" }) as HTMLInputElement;
  expect(colour.value).toBe("ACI 1");
  fireEvent.change(colour, { target: { value: "ACI 5" } });
  fireEvent.keyDown(colour, { key: "Enter" });
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "color", "ACI 5");

  fireEvent.change(screen.getByRole("combobox", { name: "Linetype" }), { target: { value: "DASHED" } });
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "linetype", "DASHED");
  fireEvent.change(screen.getByRole("combobox", { name: "Lineweight" }), { target: { value: "35" } });
  expect(onSetLayer).toHaveBeenLastCalledWith("A-WALL", "lineweight", 35);

  const name = screen.getByRole("textbox", { name: "Rename" });
  fireEvent.change(name, { target: { value: "S-WALL" } });
  fireEvent.keyDown(name, { key: "Enter" });
  expect(onRenameLayer).toHaveBeenCalledWith("A-WALL", "S-WALL");
  fireEvent.click(screen.getByRole("button", { name: "Delete layer A-WALL" }));
  expect(onDeleteLayer).toHaveBeenCalledWith("A-WALL");
});

test("new layer, layer states and the translator from the panel's foot", () => {
  const onNewLayer = vi.fn();
  const onLayerStates = vi.fn();
  const onTranslate = vi.fn();
  render(
    <LayersPanel
      layers={[layer("0")]}
      hidden={new Set()}
      onToggle={() => {}}
      onIsolate={() => {}}
      onShowAll={() => {}}
      onNewLayer={onNewLayer}
      onLayerStates={onLayerStates}
      onTranslate={onTranslate}
    />,
  );
  const name = screen.getByRole("textbox", { name: "New layer" });
  fireEvent.change(name, { target: { value: " S-COLS " } });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  expect(onNewLayer).toHaveBeenCalledWith("S-COLS");
  fireEvent.click(screen.getByRole("button", { name: "Layer states…" }));
  expect(onLayerStates).toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Match layer names" }));
  expect(onTranslate).toHaveBeenCalled();
});

test("on a paper sheet with viewports a VP freeze column freezes a layer in the chosen viewport", () => {
  const onViewportFrozen = vi.fn();
  const props = {
    layers: [layer("A-WALL"), layer("A-DOOR")],
    hidden: new Set<string>(),
    onToggle() {},
    onIsolate() {},
    onShowAll() {},
    onViewportFrozen,
  };
  const { rerender } = render(<LayersPanel {...props} viewports={[]} />);
  expect(screen.queryByRole("checkbox", { name: "VP freeze: A-WALL" })).toBeNull();
  rerender(
    <LayersPanel
      {...props}
      viewports={[
        { handle: 9, frozen: ["A-WALL"] },
        { handle: 12, frozen: [] },
      ]}
    />,
  );
  const wall = screen.getByRole("checkbox", { name: "VP freeze: A-WALL" }) as HTMLInputElement;
  expect(wall.checked).toBe(true);
  fireEvent.click(wall);
  expect(onViewportFrozen).toHaveBeenLastCalledWith(9, "A-WALL", false);
  fireEvent.change(screen.getByRole("combobox", { name: "Viewport" }), { target: { value: "12" } });
  const door = screen.getByRole("checkbox", { name: "VP freeze: A-DOOR" }) as HTMLInputElement;
  expect(door.checked).toBe(false);
  fireEvent.click(door);
  expect(onViewportFrozen).toHaveBeenLastCalledWith(12, "A-DOOR", true);
});

/** The tree moves focus on the next tick. */
const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 40))));
/** A key pressed and let go, as the tree's hotkeys follow both. */
const press = (el: Element, key: string) => {
  fireEvent.keyDown(el, { key, code: key });
  fireEvent.keyUp(el, { key, code: key });
};

const NAMES = ["0", "A-DOOR", "A-ROOM", "A-WALL", "A-WINDOW", "COMMENTS", "E-LIGHT", "E-POWER", "S-COLUMN", "S-BEAM", "S-SLAB", "Z-TEMP"];
const many = () => NAMES.map((n) => layer(n));
const rowOf = (name: string) =>
  screen.getAllByRole("treeitem").find((r) => r.getAttribute("aria-label") === name) as HTMLElement;

test("a short list is a flat tree of layers, each a row with its level and position", () => {
  render(<LayersPanel layers={[layer("A-WALL"), layer("A-DOOR")]} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />);
  const rows = screen.getAllByRole("treeitem");
  expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual(["A-WALL", "A-DOOR"]);
  expect(rows.map((r) => r.getAttribute("aria-level"))).toEqual(["1", "1"]);
  expect(rows[1].getAttribute("aria-posinset")).toBe("2");
  expect(rows[1].getAttribute("aria-setsize")).toBe("2");
});

test("a long list groups its layers by prefix: the groups are open, the layers under them one level in", () => {
  render(<LayersPanel layers={many()} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />);
  if (!(screen.getByRole("checkbox", { name: "Show folders" }) as HTMLInputElement).checked) fireEvent.click(screen.getByRole("checkbox", { name: "Show folders" }));
  expect(screen.getAllByRole("treeitem").map((r) => r.getAttribute("aria-label"))).toEqual([
    "0", "A-", "A-DOOR", "A-ROOM", "A-WALL", "A-WINDOW", "COMMENTS", "E-", "E-LIGHT", "E-POWER", "S-", "S-COLUMN", "S-BEAM", "S-SLAB", "Z-TEMP",
  ]);
  expect(rowOf("A-").getAttribute("aria-expanded")).toBe("true");
  expect(rowOf("A-").textContent).toContain("4");
  expect(rowOf("A-WALL").getAttribute("aria-level")).toBe("2");
  expect(rowOf("COMMENTS").getAttribute("aria-level")).toBe("1");
  // Every layer keeps its controls.
  expect(screen.getByRole("button", { name: "Visible: A-WALL" })).toBeTruthy();
});

test("the arrows, Home and End move between rows; Right opens a group and Left closes it", async () => {
  render(<LayersPanel layers={many()} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />);
  if (!(screen.getByRole("checkbox", { name: "Show folders" }) as HTMLInputElement).checked) fireEvent.click(screen.getByRole("checkbox", { name: "Show folders" }));
  const rows = () => screen.getAllByRole("treeitem");
  // One row is the tab stop.
  expect(rows().filter((r) => r.tabIndex === 0)).toHaveLength(1);
  rowOf("0").focus();
  await settle();
  press(rowOf("0"), "ArrowDown");
  await settle();
  expect(document.activeElement).toBe(rowOf("A-"));
  press(rowOf("A-"), "ArrowDown");
  await settle();
  expect(document.activeElement).toBe(rowOf("A-DOOR"));
  press(rowOf("A-DOOR"), "ArrowUp");
  await settle();
  expect(document.activeElement).toBe(rowOf("A-"));
  // Left closes the open group, and its layers leave the tree.
  press(rowOf("A-"), "ArrowLeft");
  await settle();
  expect(rowOf("A-").getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("treeitem", { name: "A-DOOR" })).toBeNull();
  press(rowOf("A-"), "ArrowRight");
  await settle();
  expect(rowOf("A-").getAttribute("aria-expanded")).toBe("true");
  expect(rowOf("A-DOOR")).toBeTruthy();
  press(document.activeElement!, "End");
  await settle();
  expect(document.activeElement).toBe(rowOf("Z-TEMP"));
  press(rowOf("Z-TEMP"), "Home");
  await settle();
  expect(document.activeElement).toBe(rowOf("0"));
  // A layer's Left goes up to its group.
  fireEvent.click(rowOf("A-WALL"));
  await settle();
  press(rowOf("A-WALL"), "ArrowLeft");
  await settle();
  expect(document.activeElement).toBe(rowOf("A-"));
});

test("a folded group stays folded: it is remembered on this computer", async () => {
  localStorage.clear();
  const first = render(<LayersPanel layers={many()} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />);
  if (!(screen.getByRole("checkbox", { name: "Show folders" }) as HTMLInputElement).checked) fireEvent.click(screen.getByRole("checkbox", { name: "Show folders" }));
  fireEvent.click(rowOf("S-"));
  await settle();
  expect(rowOf("S-").getAttribute("aria-expanded")).toBe("false");
  first.unmount();
  render(<LayersPanel layers={many()} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />);
  if (!(screen.getByRole("checkbox", { name: "Show folders" }) as HTMLInputElement).checked) fireEvent.click(screen.getByRole("checkbox", { name: "Show folders" }));
  expect(rowOf("S-").getAttribute("aria-expanded")).toBe("false");
  expect(rowOf("A-").getAttribute("aria-expanded")).toBe("true");
  localStorage.clear();
});

test("a click selects a layer's row; Enter or a double click on it makes the layer current", async () => {
  const onMakeCurrent = vi.fn();
  render(
    <LayersPanel layers={[layer("A-WALL"), layer("A-DOOR"), layer("PLAN|A-DOOR")]} hidden={new Set()} current="A-WALL" onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} onMakeCurrent={onMakeCurrent} />,
  );
  fireEvent.click(rowOf("A-DOOR"));
  await settle();
  expect(rowOf("A-DOOR").getAttribute("aria-selected")).toBe("true");
  expect(rowOf("A-WALL").getAttribute("aria-selected")).toBe("false");
  fireEvent.keyDown(rowOf("A-DOOR"), { key: "Enter" });
  expect(onMakeCurrent).toHaveBeenLastCalledWith("A-DOOR");
  onMakeCurrent.mockClear();
  fireEvent.doubleClick(rowOf("A-DOOR"));
  expect(onMakeCurrent).toHaveBeenCalledWith("A-DOOR");
  // The current layer, an xref's layer: nothing to make.
  onMakeCurrent.mockClear();
  fireEvent.keyDown(rowOf("A-WALL"), { key: "Enter" });
  fireEvent.keyDown(rowOf("PLAN|A-DOOR"), { key: "Enter" });
  expect(onMakeCurrent).not.toHaveBeenCalled();
  // Pressing a button in the row is not a double click on the row.
  fireEvent.doubleClick(screen.getByRole("button", { name: "Visible: A-DOOR" }));
  expect(onMakeCurrent).not.toHaveBeenCalled();
});

test("the arrows in a layer's properties are the properties': the tree's focus stays", async () => {
  const wall: LayerInfo = { ...layer("A-WALL"), color_name: "ACI 1", linetype: "Continuous", lineweight: -3, plot: true };
  render(
    <LayersPanel layers={[wall, layer("A-DOOR")]} hidden={new Set()} linetypes={["Continuous", "DASHED"]} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} onSetLayer={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Layer properties: A-WALL" }));
  const linetype = screen.getByRole("combobox", { name: "Linetype" });
  linetype.focus();
  const before = rowOf("A-WALL").tabIndex;
  press(linetype, "ArrowDown");
  await settle();
  expect(document.activeElement).toBe(linetype);
  expect(rowOf("A-WALL").tabIndex).toBe(before);
});

test("the row's buttons are in the row for the pointer and the keyboard; a name is whole in its tooltip", () => {
  render(<LayersPanel layers={[layer("A-WALL-FULL-DETAIL-LONG")]} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} onMakeCurrent={() => {}} />);
  const row = rowOf("A-WALL-FULL-DETAIL-LONG");
  expect(row.querySelector(".name")!.getAttribute("title")).toBe("A-WALL-FULL-DETAIL-LONG");
  expect(row.querySelectorAll("button")).toHaveLength(3);
});

test("an isolated layer says so, its isolate button is pressed, and Un-isolate puts it back", () => {
  const onUnisolate = vi.fn();
  render(
    <LayersPanel
      layers={[layer("A-WALL"), layer("A-DOOR"), layer("QIYAS-AREA")]}
      hidden={new Set(["A-WALL", "A-DOOR"])}
      onToggle={() => {}}
      onIsolate={() => {}}
      isolated="QIYAS-AREA"
      onUnisolate={onUnisolate}
      onShowAll={() => {}}
    />,
  );
  expect(screen.getByRole("status").textContent).toContain("Isolated: QIYAS-AREA");
  expect(screen.getByRole("button", { name: "Isolate: QIYAS-AREA" }).getAttribute("aria-pressed")).toBe("true");
  expect(screen.getByRole("button", { name: "Isolate: A-WALL" }).getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: "Un-isolate" }));
  expect(onUnisolate).toHaveBeenCalledTimes(1);
});

test("nothing isolated shows no isolation line", () => {
  render(
    <LayersPanel layers={[layer("A-WALL")]} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />,
  );
  expect(screen.queryByText(/Isolated/)).toBeNull();
});

test("folders are an explicit choice and Arabic reverses the expand and collapse arrows", async () => {
  localStorage.clear();
  localStorage.setItem("qiyas.lang", "ar");
  const { LanguageProvider } = await import("../i18n");
  render(<LanguageProvider><LayersPanel layers={many()} hidden={new Set()} onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} /></LanguageProvider>);
  expect(screen.getAllByRole("treeitem")).toHaveLength(NAMES.length);
  expect(rowOf("A-")).toBeUndefined();
  fireEvent.click(screen.getByRole("checkbox", { name: "عرض المجلدات" }));
  await settle();
  rowOf("A-").focus();
  await settle();
  press(rowOf("A-"), "ArrowRight");
  await settle();
  expect(rowOf("A-").getAttribute("aria-expanded")).toBe("false");
  press(rowOf("A-"), "ArrowLeft");
  await settle();
  expect(rowOf("A-").getAttribute("aria-expanded")).toBe("true");
  expect(localStorage.getItem("qiyas.layers.folders")).toBe("true");
  document.documentElement.lang = "en";
  document.documentElement.dir = "ltr";
});
