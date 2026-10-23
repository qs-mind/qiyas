import { describe, expect, test } from "vitest";
import type { LayerInfo } from "../api";
import { buildLayerTree, groupKey, ROOT } from "./layerGroups";

const layer = (name: string): LayerInfo => ({ name, color: [255, 0, 0], off: false, frozen: false, locked: false });
const many = (names: string[]) => buildLayerTree(names.map(layer), true);

describe("groupKey", () => {
  test("a discipline prefix is one or two letters and a hyphen, in capitals", () => {
    expect(groupKey("A-WALL")).toBe("A-");
    expect(groupKey("a-door")).toBe("A-");
    expect(groupKey("AR-ROOM")).toBe("AR-");
  });
  test("an xref's layer is grouped by the xref, whatever its layer is called", () => {
    expect(groupKey("PLAN|A-DOOR")).toBe("PLAN");
    expect(groupKey("01-IVORY|S-COLUMN")).toBe("01-IVORY");
  });
  test("names with no prefix of the kind have none", () => {
    expect(groupKey("0")).toBeNull();
    expect(groupKey("Defpoints")).toBeNull();
    expect(groupKey("WALLS-OLD")).toBeNull();
    expect(groupKey("COMMENTS")).toBeNull();
  });
});

describe("buildLayerTree", () => {
  const names = ["0", "A-DOOR", "A-ROOM", "A-WALL", "A-WINDOW", "COMMENTS", "E-LIGHT", "E-POWER", "S-COLUMN", "S-BEAM", "S-SLAB", "Z-TEMP"];

  test("a short list stays flat", () => {
    const tree = buildLayerTree(names.slice(0, 8).map(layer));
    expect(tree.groups).toEqual([]);
    expect(tree.root).toHaveLength(8);
  });

  test("a long list is grouped by prefix, each group where its first layer is; a lone layer stays at the top", () => {
    const tree = many(names);
    expect(tree.groups).toEqual(["group:A-", "group:E-", "group:S-"]);
    expect(tree.root).toEqual(["layer:0", "group:A-", "layer:COMMENTS", "group:E-", "group:S-", "layer:Z-TEMP"]);
    const children = (id: string) => (tree.nodes[id] as { children: string[] }).children;
    expect(children("group:A-")).toEqual(["layer:A-DOOR", "layer:A-ROOM", "layer:A-WALL", "layer:A-WINDOW"]);
    expect(children("group:S-")).toEqual(["layer:S-COLUMN", "layer:S-BEAM", "layer:S-SLAB"]);
    expect(tree.nodes[ROOT]).toMatchObject({ children: tree.root });
  });

  test("a prefix with one layer is no group", () => {
    const tree = many([...names.slice(0, 11), "Q-ONLY"]);
    expect(tree.root).toContain("layer:Q-ONLY");
    expect(tree.groups).not.toContain("group:Q-");
  });

  test("the layers of an xref are under it", () => {
    const tree = many([...names, "PLAN|A-DOOR", "PLAN|A-WALL", "OTHER|A-DOOR"]);
    expect(tree.groups).toContain("group:PLAN");
    expect(tree.nodes["group:PLAN"]).toMatchObject({ children: ["layer:PLAN|A-DOOR", "layer:PLAN|A-WALL"] });
    expect(tree.root).toContain("layer:OTHER|A-DOOR");
    // An xref's layer never joins the drawing's own A- group.
    expect((tree.nodes["group:A-"] as { children: string[] }).children).toHaveLength(4);
  });
});

// The twelfth layer used to change the layout without the user's choice.
test("any number of layers stays flat until folders are requested", () => {
  const layers = Array.from({ length: 100 }, (_, i) => layer(`A-${i}`));
  const tree = buildLayerTree(layers);
  expect(tree.groups).toEqual([]);
  expect(tree.root).toHaveLength(100);
});
