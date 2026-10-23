/** Layers stay flat until the user requests folders. Folder names come from shared name
 *  prefixes or external drawing names; no drawing or discipline list is hard-coded. */
import type { LayerInfo } from "../api";

export type LayerNode =
  | { id: string; kind: "layer"; name: string; layer: LayerInfo }
  | { id: string; kind: "group"; name: string; children: string[] };

export type LayerTree = {
  /** Every node by id, the root (`ROOT`) included. */
  nodes: Record<string, LayerNode & { children?: string[] }>;
  /** The root's children, in the order the layers come. */
  root: string[];
  /** The ids of the groups. */
  groups: string[];
};

export const ROOT = "root";
export const layerId = (name: string) => `layer:${name}`;
export const groupId = (key: string) => `group:${key}`;

/** The group a layer name belongs to, or null: an xref's name before the `|`, or the one or two
 *  letters before the first `-` (the AIA discipline designator), in capitals. */
export function groupKey(name: string): string | null {
  const bar = name.indexOf("|");
  if (bar > 0) return name.slice(0, bar);
  const prefix = /^([A-Za-z]{1,2})-/.exec(name);
  return prefix ? `${prefix[1].toUpperCase()}-` : null;
}

export function buildLayerTree(layers: LayerInfo[], folders = false): LayerTree {
  const nodes: LayerTree["nodes"] = {};
  const root: string[] = [];
  const groups: string[] = [];
  const keys = layers.map((l) => (folders ? groupKey(l.name) : null));
  const size = new Map<string, number>();
  for (const key of keys) if (key !== null) size.set(key, (size.get(key) ?? 0) + 1);
  layers.forEach((layer, i) => {
    const id = layerId(layer.name);
    nodes[id] = { id, kind: "layer", name: layer.name, layer };
    const key = keys[i];
    if (key === null || (size.get(key) ?? 0) < 2) {
      root.push(id);
      return;
    }
    const gid = groupId(key);
    if (!nodes[gid]) {
      nodes[gid] = { id: gid, kind: "group", name: key, children: [] };
      root.push(gid);
      groups.push(gid);
    }
    (nodes[gid] as Extract<LayerNode, { kind: "group" }>).children.push(id);
  });
  nodes[ROOT] = { id: ROOT, kind: "group", name: "", children: root };
  return { nodes, root, groups };
}
