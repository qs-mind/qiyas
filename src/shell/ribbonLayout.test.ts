import { expect, test } from "vitest";
import type { RibbonLayout } from "../api";
import fixture from "./ribbon.default.json";
import { fittingTabs, buttonKeys, DEFAULT_RIBBON, editTree, layoutOf } from "./ribbonLayout";

/** The core's default, cut down: Home (Draw with Audit, Annotation) and Manage (Run script). */
const CORE: RibbonLayout = {
  tabs: [
    {
      id: "home",
      label: "home",
      groups: [
        {
          id: "draw",
          label: "draw.group",
          buttons: [
            { kind: "tool", command: "line", label: "draw.line" },
            { kind: "action", command: "audit", label: "audit.button" },
          ],
        },
        { id: "text", label: "ribbon.annotation", buttons: [{ kind: "action", command: "find", label: "text.find" }] },
      ],
    },
    {
      id: "manage",
      label: "manage",
      groups: [{ id: "scripts", label: "ribbon.scripts", buttons: [{ kind: "action", command: "runScript", label: "script.run" }] }],
    },
  ],
};

test("the window's default has the eight tabs, Measure first, Draw its six groups, and Manage has Audit, Purge, Recover, Options, Customize and Aliases", () => {
  expect(DEFAULT_RIBBON.tabs.map((t) => t.id)).toEqual([
    "measure",
    "home",
    "annotate",
    "parametric",
    "insert",
    "view",
    "manage",
    "output",
  ]);
  // The tab with id "home" is the Draw tab: its label is the Draw group's.
  const home = DEFAULT_RIBBON.tabs[1];
  expect(home.label).toBe("draw.group");
  expect(home.groups.map((g) => g.id)).toEqual(["draw", "modify", "edit", "text", "clipboard", "layers"]);
  expect(home.groups.find((g) => g.id === "modify")!.buttons).toHaveLength(10);
  expect(home.groups.find((g) => g.id === "edit")!.buttons).toHaveLength(13);
  // New, Open, Save, Save as, Undo and Redo are the quick-access bar's, never a layout's.
  const commands = DEFAULT_RIBBON.tabs.flatMap((t) => t.groups.flatMap((g) => g.buttons.map((b) => b.command)));
  for (const c of ["new", "open", "save", "saveAs", "undo", "redo"]) expect(commands).not.toContain(c);
  const measure = DEFAULT_RIBBON.tabs.find((t) => t.id === "measure")!;
  expect(measure.groups.find((g) => g.id === "takeoff")!.buttons.map((b) => b.command)).toEqual([
    "exportTakeoff",
    "importBoq",
    "compare",
    "exportConflicts",
  ]);
  expect(measure.groups.map((g) => g.id)).toEqual(["measure", "inquiry", "takeoff", "dims", "markup"]);
  // Dimensions: five on show, the other ten and the style behind its "⋯".
  const dims = measure.groups.find((g) => g.id === "dims")!;
  expect([dims.main, dims.buttons.length]).toEqual([5, 15]);
  expect(measure.groups.find((g) => g.id === "markup")!.buttons.map((b) => b.command)).toEqual(["revcloud", "commentCloud", "leader", "mtext"]);
  // Annotate: every dimension type and the style, Leaders, Markup (where the revision cloud with a
  // comment is added), Tables.
  const annotate = DEFAULT_RIBBON.tabs.find((t) => t.id === "annotate")!;
  expect(annotate.groups.map((g) => g.id)).toEqual(["dims", "notes", "markup", "tables"]);
  expect(annotate.groups[0].buttons).toHaveLength(15);
  expect(annotate.groups[0].main).toBeUndefined();
  expect(annotate.groups[2].buttons.map((b) => b.command)).toEqual(["revcloud", "commentCloud"]);
  const parametric = DEFAULT_RIBBON.tabs.find((t) => t.id === "parametric")!;
  expect(parametric.groups.map((g) => g.buttons.length)).toEqual([13, 5, 3]);
  const manage = DEFAULT_RIBBON.tabs.find((t) => t.id === "manage")!;
  expect(manage.groups.flatMap((g) => g.buttons.map((b) => b.command))).toEqual([
    "runScript",
    "audit",
    "purge",
    "recover",
    "options",
    "customize",
    "aliases",
  ]);
});

test("the window's default is ribbon.default.json, which the core's default_ribbon is pinned to too", () => {
  // crates/qiyas-desktop/tests/options.rs compares default_ribbon() with the same file.
  expect(DEFAULT_RIBBON).toEqual(fixture);
});

test("a saved layout shows in its order, and what the core's default has that it leaves out is hidden", () => {
  const saved: RibbonLayout = {
    tabs: [
      {
        id: "home",
        label: "home",
        groups: [{ id: "text", label: "ribbon.annotation", buttons: [{ kind: "action", command: "find", label: "text.find" }] }],
      },
    ],
  };
  const tree = editTree(CORE, saved);
  expect(tree.map((t) => [t.id, t.hidden])).toEqual([
    ["home", false],
    ["manage", true],
  ]);
  expect(tree[0].groups.map((g) => [g.id, g.hidden])).toEqual([
    ["text", false],
    ["draw", true],
  ]);
  // Nothing hidden changes: the same layout back.
  expect(layoutOf(tree)).toEqual(saved);
});

test("with no saved layout the tree is the window's default, less the buttons the core lacks", () => {
  const tree = editTree(CORE, null);
  const keys = (id: string) => tree.find((t) => t.id === id)!.groups.flatMap((g) => g.buttons.filter((b) => !b.hidden).map((b) => b.command));
  // Options, Customize and Aliases are the window's own: the core can't keep them in a layout
  // (nor Purge, which this cut-down core lacks).
  expect(keys("manage")).toEqual(["runScript", "audit"]);
  // Audit, beside Line in the core's data, is hidden there (it shows on Manage in the window's).
  const draw = tree.find((t) => t.id === "home")!.groups.find((g) => g.id === "draw")!;
  expect(draw.buttons.find((b) => b.command === "audit")!.hidden).toBe(true);
  expect(draw.buttons.find((b) => b.command === "line")!.hidden).toBe(false);
  // Every button the layout saves is one the core knows.
  const known = buttonKeys(CORE);
  for (const k of buttonKeys(layoutOf(tree)!)) expect(known.has(k)).toBe(true);
});

test("hidden tabs, groups and buttons are left out; empty groups and tabs go; no tab left is null", () => {
  const tree = editTree(CORE, CORE);
  tree[0].groups[0].buttons[1].hidden = true;
  tree[0].groups[1].buttons[0].hidden = true;
  tree[1].hidden = true;
  expect(layoutOf(tree)).toEqual({
    tabs: [
      {
        id: "home",
        label: "home",
        groups: [{ id: "draw", label: "draw.group", buttons: [{ kind: "tool", command: "line", label: "draw.line" }] }],
      },
    ],
  });
  tree[0].hidden = true;
  expect(layoutOf(tree)).toBeNull();
});

test("a group's \"main\" survives a round trip through the Customize tree; a button may sit in two places", () => {
  const tree = editTree(DEFAULT_RIBBON, null);
  const saved = layoutOf(tree)!;
  expect(saved).toEqual(DEFAULT_RIBBON);
  expect(saved.tabs[0].groups.find((g) => g.id === "dims")!.main).toBe(5);
  // Revision cloud is on Measure's Markup, Annotate's Markup and Draw's Draw group.
  const places = DEFAULT_RIBBON.tabs.flatMap((t) => t.groups.filter((g) => g.buttons.some((b) => b.command === "revcloud")).map((g) => `${t.id}/${g.id}`));
  expect(places).toEqual(["measure/markup", "home/draw", "annotate/markup"]);
});


test("tabs use measured widths, with More only when a tab does not fit", () => {
  expect(fittingTabs([60, 80, 70, 100], 316, 64)).toBe(4);
  expect(fittingTabs([60, 80, 70, 100], 315, 64)).toBe(3);
  expect(fittingTabs([60, 80, 70, 100], 200, 64)).toBe(1);
  expect(fittingTabs([60, 80], 50, 64)).toBe(0);
  expect(fittingTabs([], 100, 64)).toBe(0);
  // Arabic labels may be wider; the order remains the reading order in either direction.
  expect(fittingTabs([100, 90, 120, 110], 316, 80)).toBe(2);
});
