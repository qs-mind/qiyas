/** The ribbon as data: the layout the window draws when the user has none of their own
 *  (`DEFAULT_RIBBON`), and the Customize dialog's tree of it, where tabs, groups and buttons can
 *  be hidden and moved. The core keeps the default too (`get_default_ribbon`) and checks a saved
 *  layout against it: a saved layout may only hold the core default's buttons. */
import type { RibbonButton, RibbonButtonKind, RibbonGroup, RibbonLayout, RibbonTab } from "../api";

const button = <K extends RibbonButtonKind, const C extends string>(kind: K, command: C, label: string) => ({ kind, command, label });
const tool = <const C extends string>(command: C, label: string = command) => button("tool", command, label);
const action = <const C extends string>(command: C, label: string = command) => button("action", command, label);
const toggle = <const C extends string>(command: C, label: string = command) => button("toggle", command, label);
const tools = <const C extends string>(prefix: string, names: C[]) => names.map((n) => tool(n, `${prefix}.${n}`));
/** `main`: how many buttons show before the group's menu. */
const group = <B extends RibbonButton>(id: string, label: string, buttons: B[], main?: number) =>
  main === undefined ? { id, label, buttons } : { id, label, buttons, main };
const tab = <G extends RibbonGroup>(id: string, groups: G[], label = id) => ({ id, label, groups });

/** The dimension tools, every kind, then the dimension style: the Dimensions group of Measure
 *  (its first five on show, the rest behind "⋯") and of Annotate (all of them). */
const dimensions = () => [
  ...tools("dim", [
    "dimLinear",
    "dimAligned",
    "dimAngular",
    "dimRadius",
    "dimDiameter",
    "dimArc",
    "dimOrdinate",
    "dimBaseline",
    "dimContinue",
    "qdim",
    "dimEdit",
    "dimBreak",
    "dimSpace",
    "dimJogLine",
  ]),
  action("dimStyle", "dim.styleButton"),
];

/** The quick-access bar on the ribbon's tabs row (`Ribbon.tsx`): New, Open, Save, Save as, Undo
 *  and Redo, always shown, never part of a layout. */
export const QUICK_ACCESS = [action("new"), action("open"), action("save"), action("saveAs"), action("undo"), action("redo")];

/** The ribbon Qiyas ships: the same as the core's `default_ribbon` (`qiyas-desktop/src/ribbon.rs`);
 *  `ribbon.default.json` pins both (`ribbonLayout.test.ts` and the core's `tests/options.rs`).
 *  Measure first, then Draw (the tab with id "home"), then Annotate; `Ribbon.tsx` shows the
 *  tabs that fit and puts the remainder under "More". New, Open, Save, Save as, Undo and Redo are not
 *  in it: `Ribbon.tsx` keeps them in the quick-access bar beside the tabs. */
const defaultRibbon = {
  tabs: [
    tab("measure", [
      group("measure", "ribbon.measure", (["select", "distance", "area", "count"] as const).map((n) => tool(n))),
      group("inquiry", "ribbon.inquiry", [
        ...(["geometry", "divide", "measureAlong", "idPoint"] as const).map((n) => tool(n)),
        action("extract"),
      ]),
      group("takeoff", "ribbon.takeoff", [action("exportTakeoff"), action("importBoq"), action("compare"), action("exportConflicts")]),
      group("dims", "dim.group", dimensions(), 5),
      group("markup", "ribbon.markup", [
        tool("revcloud", "draw.revcloud"),
        tool("commentCloud", "dim.commentCloud"),
        tool("leader", "dim.leader"),
        tool("mtext", "text.mtext"),
      ]),
    ]),
    tab(
      "home",
      [
        group(
          "draw",
          "draw.group",
          tools("draw", ["line", "polyline", "rectangle", "circle", "arc", "ellipse", "spline", "point", "hatch", "revcloud", "wipeout"]),
        ),
        group("modify", "modify.group", tools("modify", ["move", "copy", "rotate", "scale", "mirror", "stretch", "align", "array", "explode", "erase"])),
        group(
          "edit",
          "ribbon.edit",
          tools("edit", [
            "trim",
            "extend",
            "break",
            "join",
            "lengthen",
            "reverse",
            "offset",
            "fillet",
            "chamfer",
            "blend",
            "pedit",
            "overkill",
            "flatten",
          ]),
        ),
        group("text", "ribbon.annotation", [...tools("text", ["text", "mtext", "textEdit"]), action("find", "text.find")]),
        group("clipboard", "clip.group", tools("clip", ["copyclip", "copybase", "pasteclip", "pasteblock"])),
        group("layers", "layers", [
          action("layerStates", "layer.statesButton"),
          action("translateLayers", "layer.translateButton"),
          ...tools("layerTool", ["group", "ungroup", "matchprop"]),
          toggle("groupSelect", "group.select"),
        ]),
      ],
      "draw.group",
    ),
    tab("annotate", [
      group("dims", "dim.group", dimensions()),
      group("notes", "dim.notes", tools("dim", ["leader", "mleader", "tolerance"])),
      // Revision cloud now; the revision cloud with a comment will be appended here.
      group("markup", "ribbon.markup", [tool("revcloud", "draw.revcloud"), tool("commentCloud", "dim.commentCloud")]),
      group("tables", "table.group", [
        tool("table", "table.table"),
        tool("tableEdit", "table.tableEdit"),
        tool("field", "table.fieldTool"),
        action("updateFields", "table.updateFields"),
      ]),
    ]),
    tab("parametric", [
      group(
        "geometric",
        "param.geometric",
        tools("param", [
          "gcCoincident",
          "gcCollinear",
          "gcConcentric",
          "gcFixed",
          "gcParallel",
          "gcPerpendicular",
          "gcHorizontal",
          "gcVertical",
          "gcTangent",
          "gcSmooth",
          "gcSymmetric",
          "gcEqual",
          "autoConstrain",
        ]),
      ),
      group("dimensional", "param.dimensional", tools("param", ["dcLinear", "dcAligned", "dcAngular", "dcRadius", "dcDiameter"])),
      group("constraints", "param.manage", [
        action("parameters", "param.parameters"),
        tool("delConstraint", "param.delConstraint"),
        action("constraintBar", "param.showBar"),
      ]),
    ]),
    tab("insert", [
      group("block", "block.group", [
        tool("block", "block.createTool"),
        action("blockPalette", "block.insertTool"),
        action("writeBlock", "block.writeTool"),
        tool("refedit", "block.refedit"),
        tool("attdef", "block.attdefTool"),
        tool("textEdit", "block.attedit"),
        action("attributeManager", "block.manager"),
        action("designCenter", "block.designCenter"),
        action("toolPalettes", "block.toolPalettes"),
      ]),
    ]),
    tab("view", [
      group("views", "ribbon.views", [
        action("zoomExtents"),
        tool("zoomWindow"),
        action("views"),
        action("xrefs"),
        action("attachPdf"),
        action("attachImage"),
        action("calibrate"),
      ]),
      group("vports", "vport.group", [
        tool("mview", "vport.mview"),
        action("split", "vport.two"),
        tool("ucs", "ucs.tool"),
        tool("ucsObject", "ucs.object"),
        action("ucsWorld", "ucs.world"),
        action("sheetSet", "sheetSet.button"),
      ]),
      group("display", "ribbon.display", [toggle("arabicReversed"), toggle("ucsIcon")]),
    ]),
    tab("manage", [
      group("scripts", "ribbon.scripts", [action("runScript", "script.run")]),
      group("drawing", "ribbon.drawing", [
        action("audit", "audit.button"),
        action("purge", "purge.button"),
        action("recover", "recover.button"),
      ]),
      group("setup", "ribbon.setup", [
        action("options", "options.button"),
        action("customize", "cui.button"),
        action("aliases", "aliasEdit.button"),
      ]),
    ]),
    tab("output", [
      group("plot", "ribbon.plot", [
        action("plot", "plot.button"),
        action("quickPrint", "plot.quickPrint"),
        action("printAll", "plot.printAll"),
      ]),
      group("plotStyles", "ribbon.plotStyles", [action("plotStyles", "plot.styles")]),
    ]),
  ],
};

export const DEFAULT_RIBBON: RibbonLayout = defaultRibbon;

type DefaultButton = (typeof defaultRibbon)["tabs"][number]["groups"][number]["buttons"][number];
// Views opens a menu with its own load/save/delete interface.
export type RibbonCommand = Exclude<DefaultButton["command"], "views"> | (typeof QUICK_ACCESS)[number]["command"];
export type RibbonTool = Extract<DefaultButton, { kind: "tool" }>["command"];
export type RibbonAction = Exclude<RibbonCommand, RibbonTool | "views">;
export type RibbonHandlers = Record<RibbonAction, { run(): void; enabled?: boolean; pressed?: boolean }>;

const knownCommands = new Set<string>([
  ...QUICK_ACCESS.map((b) => b.command),
  ...DEFAULT_RIBBON.tabs.flatMap((t) => t.groups.flatMap((g) => g.buttons.map((b) => b.command))),
]);
export const isRibbonCommand = (command: string): command is RibbonCommand => command !== "views" && knownCommands.has(command);

const toolCommands = new Set<string>(DEFAULT_RIBBON.tabs.flatMap((t) =>
  t.groups.flatMap((g) => g.buttons.filter((b) => b.kind === "tool").map((b) => b.command))));
export const isRibbonTool = (command: RibbonCommand): command is RibbonTool => toolCommands.has(command);

/** A button's identity: what it runs (a layout may hold one twice). */
export const buttonKey = (b: Pick<RibbonButton, "kind" | "command">) => `${b.kind}:${b.command}`;

/** The buttons a layout holds, by `buttonKey`. */
export const buttonKeys = (layout: RibbonLayout) =>
  new Set(layout.tabs.flatMap((t) => t.groups.flatMap((g) => g.buttons.map(buttonKey))));

export type EditButton = RibbonButton & { hidden: boolean };
export type EditGroup = Omit<RibbonGroup, "buttons"> & { hidden: boolean; buttons: EditButton[] };
export type EditTab = Omit<RibbonTab, "groups"> & { hidden: boolean; groups: EditGroup[] };

/** The Customize dialog's tree: what is on the ribbon (`saved`, or the window's default when
 *  there is none, less the buttons `all` lacks), in its order, then what `all` (the core's
 *  default) has that it leaves out, hidden, where `all` keeps it. */
export function editTree(all: RibbonLayout, saved: RibbonLayout | null): EditTab[] {
  const known = buttonKeys(all);
  const shown = saved ?? DEFAULT_RIBBON;
  const tree: EditTab[] = shown.tabs.map((t) => ({
    ...t,
    hidden: false,
    groups: t.groups.map((g) => ({
      ...g,
      hidden: false,
      buttons: g.buttons.filter((b) => known.has(buttonKey(b))).map((b) => ({ ...b, hidden: false })),
    })),
  }));
  for (const t of all.tabs) {
    const mine = tree.find((x) => x.id === t.id);
    if (!mine) {
      tree.push({
        ...t,
        hidden: true,
        groups: t.groups.map((g) => ({ ...g, hidden: false, buttons: g.buttons.map((b) => ({ ...b, hidden: false })) })),
      });
      continue;
    }
    for (const g of t.groups) {
      const theirs = mine.groups.find((x) => x.id === g.id);
      if (!theirs) {
        mine.groups.push({ ...g, hidden: true, buttons: g.buttons.map((b) => ({ ...b, hidden: false })) });
        continue;
      }
      const have = new Set(theirs.buttons.map(buttonKey));
      for (const b of g.buttons) if (!have.has(buttonKey(b))) theirs.buttons.push({ ...b, hidden: true });
    }
  }
  return tree;
}

/** The layout a tree saves: what is not hidden, without groups left empty or tabs left with no
 *  group; null when no tab is left. */
export function layoutOf(tree: EditTab[]): RibbonLayout | null {
  const tabs = tree
    .filter((t) => !t.hidden)
    .map(({ id, label, groups }) => ({
      id,
      label,
      groups: groups
        .filter((g) => !g.hidden)
        .map(({ id, label, buttons, main }) => ({
          id,
          label,
          buttons: buttons.filter((b) => !b.hidden).map(({ kind, command, label }) => ({ kind, command, label })),
          ...(main === undefined ? {} : { main }),
        }))
        .filter((g) => g.buttons.length > 0),
    }))
    .filter((t) => t.groups.length > 0);
  return tabs.length > 0 ? { tabs } : null;
}

/** Number of tabs that fit, reserving More only when at least one tab overflows. */
export function fittingTabs(widths: readonly number[], available: number, moreWidth: number, gap = 2): number {
  const width = (count: number) => widths.slice(0, count).reduce((sum, value) => sum + value, 0) + Math.max(0, count - 1) * gap;
  if (width(widths.length) <= available) return widths.length;
  let count = widths.length - 1;
  while (count > 0 && width(count) + gap + moreWidth > available) count--;
  return Math.max(0, count);
}
