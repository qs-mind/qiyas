import { hotkeysCoreFeature, selectionFeature, syncDataLoaderFeature } from "@headless-tree/core";
import { useTree } from "@headless-tree/react";
import { memo, useEffect, useMemo, useState, type CSSProperties, type KeyboardEvent } from "react";
import type { LayerInfo, LayerProp, ViewportInfo } from "../api";
import { useT } from "../i18n";
import type { Theme } from "../viewport/gl";
import { buildLayerTree, ROOT, type LayerNode } from "./layerGroups";
import { LINEWEIGHTS, weightLabel } from "./lineweights";
import { swatchColor } from "./swatch";

type Props = {
  layers: LayerInfo[];
  hidden: Set<string>;
  /** Drives the swatches: white shows as Ink on the light theme, black as Paper on the dark one. */
  theme?: Theme;
  /** A layer change is talking to the core: the eye and isolate buttons wait (and say so). */
  busy?: boolean;
  onToggle(name: string): void;
  onIsolate(name: string): void;
  /** The layer isolated (LAYISO), if one is, and putting back what isolating hid. */
  isolated?: string | null;
  onUnisolate?(): void;
  onShowAll(): void;
  /** The layer new entities are drawn on, and making another one that. Without a handler there
   *  is no button. */
  current?: string;
  onMakeCurrent?(name: string): void;
  /** The drawing's linetypes, for a layer's linetype. */
  linetypes?: string[];
  /** Edits a layer of the drawing itself (not an xref's): without a handler its properties
   *  don't open. */
  onSetLayer?(name: string, prop: LayerProp, value: boolean | string | number): void;
  onRenameLayer?(from: string, to: string): void;
  onDeleteLayer?(name: string): void;
  onNewLayer?(name: string): void;
  /** Open the layer states and layer translator dialogs. */
  onLayerStates?(): void;
  onTranslate?(): void;
  /** The paper-space viewports of the sheet on screen: with some, a VP freeze column freezes a
   *  layer in the chosen one. */
  viewports?: ViewportInfo[];
  onViewportFrozen?(viewport: number, layer: string, frozen: boolean): void;
};

/** Runs `commit` on Enter. */
const onEnter = (commit: () => void) => (e: KeyboardEvent<HTMLInputElement>) => {
  if (e.key === "Enter") {
    e.preventDefault();
    commit();
  }
};

type LayerPropsProps = {
  layer: LayerInfo;
  linetypes: string[];
  onSet(prop: LayerProp, value: boolean | string | number): void;
  onRename?(to: string): void;
  onDelete?(): void;
};

/** One layer's properties, open under its row: on, frozen, locked, plot, colour, linetype,
 *  lineweight, its name and deleting it. */
function LayerProps({ layer, linetypes, onSet, onRename, onDelete }: LayerPropsProps) {
  const t = useT();
  const [color, setColor] = useState(layer.color_name ?? "");
  const [name, setName] = useState(layer.name);
  const commitColor = () => color.trim() !== "" && color.trim() !== layer.color_name && onSet("color", color.trim());
  const commitName = () => name.trim() !== "" && name.trim() !== layer.name && onRename?.(name.trim());
  const check = (prop: LayerProp, label: string, checked: boolean, value: (on: boolean) => boolean = (on) => on) => (
    <label className="check">
      <input type="checkbox" checked={checked} onChange={(e) => onSet(prop, value(e.currentTarget.checked))} />
      <span>{t(label)}</span>
    </label>
  );
  const types = layer.linetype && !linetypes.includes(layer.linetype) ? [layer.linetype, ...linetypes] : linetypes;
  return (
    <div className="layer-props" role="group" aria-label={`${t("layer.properties")}: ${layer.name}`}>
      {check("on", "layer.on", !layer.off)}
      {check("frozen", "layer.frozen", layer.frozen)}
      {check("locked", "layer.locked", layer.locked)}
      {check("plot", "layer.plot", layer.plot ?? true)}
      <label>
        <span>{t("props.color")}</span>
        <input
          dir="ltr"
          value={color}
          onChange={(e) => setColor(e.target.value)}
          onKeyDown={onEnter(commitColor)}
          onBlur={commitColor}
        />
      </label>
      <label>
        <span>{t("props.linetype")}</span>
        <select value={layer.linetype ?? ""} onChange={(e) => onSet("linetype", e.target.value)}>
          {types.map((lt) => (
            <option key={lt} value={lt}>
              {lt}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>{t("props.lineweight")}</span>
        <select value={String(layer.lineweight ?? -3)} onChange={(e) => onSet("lineweight", Number(e.target.value))}>
          {[-3, ...LINEWEIGHTS].map((w) => (
            <option key={w} value={String(w)}>
              {weightLabel(w, t)}
            </option>
          ))}
        </select>
      </label>
      {onRename && (
        <label>
          <span>{t("layer.rename")}</span>
          <input dir="auto" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={onEnter(commitName)} />
        </label>
      )}
      {onDelete && (
        <button type="button" className="link" aria-label={t("layer.delete", { name: layer.name })} onClick={onDelete}>
          {t("layer.deleteShort")}
        </button>
      )}
    </div>
  );
}

const STORE = "qiyas.layers.collapsed";

/** The tree's movement keys, which the tree must leave alone while the keyboard is in a layer's
 *  properties: an arrow there moves a drop-down or a caret, not the focus along the rows. Only
 *  `hotkey` and `isEnabled` are ours; what each key does stays the library's. */
const outsideProps = () => !document.activeElement?.closest(".layer-props");
const MOVE_KEYS = {
  focusNextItem: { hotkey: "ArrowDown", isEnabled: outsideProps },
  focusPreviousItem: { hotkey: "ArrowUp", isEnabled: outsideProps },
  expandOrDown: { hotkey: "ArrowRight", isEnabled: outsideProps },
  collapseOrUp: { hotkey: "ArrowLeft", isEnabled: outsideProps },
  focusFirstItem: { hotkey: "Home", isEnabled: outsideProps },
  focusLastItem: { hotkey: "End", isEnabled: outsideProps },
};

/** The groups the user folded, remembered on this computer. */
function storedCollapsed(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(STORE) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** The drawing's layers as a tree, one row each: colour, name, an eye to hide or show it and, when
 *  the pointer or the keyboard is on the row, isolate (show only that one), make current (draw on
 *  it; not an xref's layer, `XREF|LAYER`) and its properties. A layer of the drawing itself opens
 *  its properties under its row (on, frozen, locked, plot, colour, linetype, lineweight, rename,
 *  delete); on a paper sheet with viewports a VP freeze column freezes a layer in the viewport
 *  chosen. A long list is grouped by its names' prefix (A-, S-, E-...) and by xref (`layerGroups`);
 *  Headless Tree gives the keyboard: the arrows, Home and End move between rows, Right and Left
 *  open and close a group, a click or Shift/Ctrl select, Enter or a double click makes a layer
 *  current. The foot adds a layer and opens layer states and the translator.
 *  Memoised: the app re-renders on every pointer move (cursor in the status bar). */
export const LayersPanel = memo(function LayersPanel({
  layers,
  hidden,
  theme = "light",
  busy = false,
  onToggle,
  onIsolate,
  isolated = null,
  onUnisolate,
  onShowAll,
  current,
  onMakeCurrent,
  linetypes = [],
  onSetLayer,
  onRenameLayer,
  onDeleteLayer,
  onNewLayer,
  onLayerStates,
  onTranslate,
  viewports = [],
  onViewportFrozen,
}: Props) {
  const t = useT();
  const [open, setOpen] = useState<string | null>(null);
  const [fresh, setFresh] = useState("");
  const [chosenVp, setChosenVp] = useState<number | null>(null);
  const [collapsed, setCollapsed] = useState(storedCollapsed);
  const viewport = viewports.find((v) => v.handle === chosenVp) ?? viewports[0] ?? null;
  const addLayer = () => {
    const name = fresh.trim();
    if (name === "" || !onNewLayer) return;
    onNewLayer(name);
    setFresh("");
  };

  const tree = useMemo(() => buildLayerTree(layers), [layers]);
  // Kept the same array while nothing changed: the tree takes a new one for a change of state.
  const expanded = useMemo(() => tree.groups.filter((g) => !collapsed.includes(g)), [tree, collapsed]);
  const instance = useTree<LayerNode>({
    rootItemId: ROOT,
    getItemName: (item) => item.getItemData().name,
    isItemFolder: (item) => item.getItemData().kind === "group",
    dataLoader: {
      getItem: (id) => tree.nodes[id],
      getChildren: (id) => (tree.nodes[id] as { children?: string[] }).children ?? [],
    },
    state: { expandedItems: expanded },
    setExpandedItems: (update) => {
      const next = typeof update === "function" ? update(expanded) : update;
      const folded = tree.groups.filter((g) => !next.includes(g));
      setCollapsed(folded);
      try {
        localStorage.setItem(STORE, JSON.stringify(folded));
      } catch {
        // Not remembered; the group is folded for this session.
      }
    },
    hotkeys: MOVE_KEYS,
    features: [syncDataLoaderFeature, selectionFeature, hotkeysCoreFeature],
  });
  // The tree is told when the layers (or which groups are open) change under it.
  useEffect(() => {
    instance.rebuildTree();
  }, [instance, tree, collapsed]);

  /** What one layer's row says and does. */
  function layerRow(item: ReturnType<typeof instance.getItems>[number], l: LayerInfo) {
    // Off and frozen layers stay hidden whatever the user does (the drawing says so).
    const locked = l.off || l.frozen;
    const visible = !locked && !hidden.has(l.name);
    const eyeLabel = `${t("visible")}: ${l.name}`;
    const isolateLabel = `${t("isolate")}: ${l.name}`;
    const currentLabel = `${t("makeCurrent")}: ${l.name}`;
    const propsLabel = `${t("layer.properties")}: ${l.name}`;
    const isCurrent = current?.toUpperCase() === l.name.toUpperCase();
    const own = !l.name.includes("|");
    const expandedProps = open === l.name;
    const vpFrozen = viewport?.frozen.some((n) => n.toUpperCase() === l.name.toUpperCase()) ?? false;
    const canMakeCurrent = !!onMakeCurrent && own && !isCurrent;
    const makeCurrent = () => !busy && canMakeCurrent && onMakeCurrent!(l.name);
    const currentButton = onMakeCurrent && (
      <button
        type="button"
        className="icon"
        aria-label={currentLabel}
        title={currentLabel}
        aria-pressed={isCurrent}
        disabled={!own}
        aria-disabled={busy || undefined}
        onClick={makeCurrent}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="M3 13l2.5-.5L13 5l-2-2-7.5 7.5z" />
          {isCurrent && <path d="M2 15h12" />}
        </svg>
      </button>
    );
    return (
      <div className={expandedProps ? "layer-node open" : "layer-node"} role="none" key={item.getId()}>
        <div
          {...item.getProps()}
          className={visible ? "layer-row" : "layer-row dim"}
          style={{ "--level": item.getItemMeta().level } as CSSProperties}
          onDoubleClick={makeCurrent}
          onKeyDown={(e) => {
            // Enter on the row itself makes the layer current, as a double click does.
            if (e.key === "Enter" && e.target === e.currentTarget) {
              e.preventDefault();
              makeCurrent();
            }
          }}
        >
          <span className="swatch" style={{ background: `rgb(${swatchColor(l.color, theme).join(",")})` }} />
          <span className="name" title={l.name}>
            <bdi>{l.name}</bdi>
          </span>
          {/* The row's buttons are not clicks on the row. The ones that show on hover lie over the
              start of the buttons that stay, so they end where those begin. */}
          <span className="layer-keep" onClick={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
            <span className="layer-more">
              <button
                type="button"
                className="icon"
                aria-label={isolateLabel}
                title={isolateLabel}
                aria-pressed={isolated !== null && isolated.toUpperCase() === l.name.toUpperCase()}
                disabled={locked}
                aria-disabled={busy || undefined}
                onClick={() => !busy && onIsolate(l.name)}
              >
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <circle cx="8" cy="8" r="5.5" />
                  <circle cx="8" cy="8" r="1.5" />
                </svg>
              </button>
              {!isCurrent && currentButton}
              {onSetLayer && own && (
                <button
                  type="button"
                  className="icon"
                  aria-label={propsLabel}
                  title={propsLabel}
                  aria-expanded={expandedProps}
                  onClick={() => setOpen(expandedProps ? null : l.name)}
                >
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path d={expandedProps ? "M4 10l4-4 4 4" : "M4 6l4 4 4-4"} />
                  </svg>
                </button>
              )}
            </span>
            {viewport && own && onViewportFrozen && (
              <input
                type="checkbox"
                className="vp-freeze"
                aria-label={`${t("layer.vpFreeze")}: ${l.name}`}
                title={`${t("layer.vpFreeze")}: ${l.name}`}
                checked={vpFrozen}
                onChange={(e) => onViewportFrozen(viewport.handle, l.name, e.currentTarget.checked)}
              />
            )}
            {isCurrent && currentButton}
            <button
              type="button"
              className="icon"
              aria-label={eyeLabel}
              title={eyeLabel}
              aria-pressed={visible}
              disabled={locked}
              aria-disabled={busy || undefined}
              onClick={() => !busy && onToggle(l.name)}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8 12.1 12.5 8 12.5 1.5 8 1.5 8Z" />
                {visible ? <circle cx="8" cy="8" r="2" /> : <path d="M2.5 13.5l11-11" />}
              </svg>
            </button>
          </span>
        </div>
        {expandedProps && onSetLayer && (
          <LayerProps
            key={`${l.name}:${l.color_name}`}
            layer={l}
            linetypes={linetypes}
            onSet={(prop, value) => onSetLayer(l.name, prop, value)}
            onRename={onRenameLayer && ((to) => onRenameLayer(l.name, to))}
            onDelete={onDeleteLayer && (() => onDeleteLayer(l.name))}
          />
        )}
      </div>
    );
  }

  /** A group's row: its name, how many layers it holds, a chevron that turns when it opens. */
  function groupRow(item: ReturnType<typeof instance.getItems>[number], node: Extract<LayerNode, { kind: "group" }>) {
    const isOpen = item.isExpanded();
    return (
      <div
        {...item.getProps()}
        key={item.getId()}
        className="layer-row layer-group"
        style={{ "--level": item.getItemMeta().level } as CSSProperties}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" className={isOpen ? "chevron open" : "chevron"}>
          <path d="M6 4l4 4-4 4" />
        </svg>
        <span className="name" title={node.name}>
          <bdi>{node.name}</bdi>
        </span>
        <span className="layer-count">{node.children.length}</span>
      </div>
    );
  }

  return (
    <aside className="layers" aria-busy={busy || undefined}>
      <div className="layers-head">
        <h2 className="heading">{t("layers")}</h2>
        <button type="button" className="link" onClick={onShowAll} disabled={hidden.size === 0 || busy}>
          {t("showAll")}
        </button>
      </div>
      {isolated && (
        <p className="layers-isolated" role="status">
          <span>
            {t("layer.isolated")}: <bdi>{isolated}</bdi>
          </span>
          {onUnisolate && (
            <button type="button" className="link" onClick={onUnisolate} disabled={busy}>
              {t("layer.unisolate")}
            </button>
          )}
        </p>
      )}
      {viewport && viewports.length > 1 && (
        <label className="layers-vp">
          <span>{t("layer.viewport")}</span>
          <select value={String(viewport.handle)} onChange={(e) => setChosenVp(Number(e.target.value))}>
            {viewports.map((v, i) => (
              <option key={v.handle} value={String(v.handle)}>
                {t("layer.viewportN", { n: i + 1 })}
              </option>
            ))}
          </select>
        </label>
      )}
      <div {...instance.getContainerProps(t("layers"))} className="layer-tree">
        {instance.getItems().map((item) => {
          const node = item.getItemData();
          return node.kind === "group" ? groupRow(item, node) : layerRow(item, node.layer);
        })}
      </div>
      {(onNewLayer || onLayerStates || onTranslate) && (
        <div className="layers-foot">
          {onNewLayer && (
            <span className="layer-new">
              <input
                dir="auto"
                aria-label={t("layer.new")}
                placeholder={t("layer.new")}
                value={fresh}
                onChange={(e) => setFresh(e.target.value)}
                onKeyDown={onEnter(addLayer)}
              />
              <button type="button" onClick={addLayer} disabled={fresh.trim() === ""}>
                {t("layer.add")}
              </button>
            </span>
          )}
          {onLayerStates && (
            <button type="button" className="link" onClick={onLayerStates}>
              {t("layer.states")}
            </button>
          )}
          {onTranslate && (
            <button type="button" className="link" onClick={onTranslate}>
              {t("layer.translate")}
            </button>
          )}
        </div>
      )}
    </aside>
  );
});
