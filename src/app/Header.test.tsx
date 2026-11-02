// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { redo, undo } from "../api";
import type { Ribbon } from "../shell/Ribbon";
import { DEFAULT_RIBBON, QUICK_ACCESS, isRibbonCommand, isRibbonTool, type RibbonAction } from "../shell/ribbonLayout";
import { Header } from "./Header";

const ribbon = vi.hoisted(() => ({ props: null as ComponentProps<typeof Ribbon> | null }));
vi.mock("../shell/Ribbon", () => ({ Ribbon: (props: ComponentProps<typeof Ribbon>) => { ribbon.props = props; return null; } }));
afterEach(cleanup);

function setup(drawing = true, toggles = false) {
  const calls: unknown[][] = [];
  const spy = (name: string) => (...args: unknown[]) => { calls.push([name, ...args]); };
  const props = {
    info: drawing ? { sheets: [{ is_model: false }] } : null,
    onModel: drawing,
    measure: { cancel: spy("cancel") },
    shell: {
      theme: "dark", viewport: { current: { fitExtents: spy("fitExtents") } }, focusDrawing: spy("focusDrawing"),
      commandAliases: {}, setAboutOpen: spy("about"), setSettingsOpen: spy("settings"),
    },
    toolChoice: { tool: "line", chooseTool: spy("tool") },
    commands: { history: spy("history"), apply: spy("apply") },
    sheets: {
      batch: drawing ? { imageTable: [1] } : null,
      split: toggles, setSplit: spy("split"), arabicReversed: toggles, chooseArabicReversed: spy("arabicReversed"),
      ucsIcon: toggles, chooseUcsIcon: spy("ucsIcon"), saveView: spy("saveView"), deleteView: spy("deleteView"),
    },
    layers: { setLayerStatesOpen: spy("layerStates"), setTranslatorOpen: spy("translator"), groupSelect: toggles, setGroupSelect: spy("groupSelect") },
    attach: { setAttachOpen: spy("attachPdf"), setAttachImageOpen: spy("attachImage"), setPending: spy("pending"), setPickedInsert: spy("pickedInsert") },
    blocks: { setBlockPaletteOpen: spy("blockPalette"), openAttributeManager: spy("attributeManager"), openDesignCenter: spy("designCenter"), openToolPalettes: spy("toolPalettes") },
    annotate: { setFindOpen: spy("find"), openDimStyle: spy("dimStyle"), updateFields: spy("updateFields") },
    drawings: {
      openFile: spy("open"), setNewOpen: spy("new"), openPath: spy("openPath"), saveFile: spy("save"), saveFileAs: spy("saveAs"),
      exportFile: spy("exportTakeoff"), setXrefsOpen: spy("xrefs"), setExtractOpen: spy("extract"), setSheetSetShown: spy("sheetSet"),
      recoverFile: spy("recover"), runScriptFile: spy("runScript"), scriptLine: null,
    },
    takeoff: { importBoqFile: spy("importBoq"), compareNow: spy("compare"), exportConflictsFile: spy("exportConflicts") },
    plot: { openPlot: spy("plot"), quickPrint: spy("quickPrint"), printAll: spy("printAll"), openStyles: spy("plotStyles") },
    options: { ribbon: null, openAudit: spy("audit"), openPurge: spy("purge"), setOptionsOpen: spy("options"), setCustomizeOpen: spy("customize"), setAliasesOpen: spy("aliases") },
    parametric: { openParameters: spy("parameters"), setBarHidden: spy("barHidden") },
    onConnect: spy("connect"),
  } as unknown as ComponentProps<typeof Header>;
  render(<Header {...props} />);
  return { calls, ribbon: ribbon.props! };
}

const actionCalls = {
  new: [["new", true]], open: [["open"]], save: [["save"]], saveAs: [["saveAs"]],
  exportTakeoff: [["exportTakeoff"]], importBoq: [["importBoq"]], compare: [["compare"]], exportConflicts: [["exportConflicts"]],
  undo: [["history", undo]], redo: [["history", redo]],
  plot: [["plot"]], quickPrint: [["quickPrint"]], printAll: [["printAll"]], plotStyles: [["plotStyles"]],
  zoomExtents: [["fitExtents"], ["focusDrawing"]], xrefs: [["xrefs", true]],
  attachPdf: [["pickedInsert", null], ["attachPdf", true]], attachImage: [["pickedInsert", null], ["attachImage", true]],
  calibrate: [["cancel"], ["pending", { kind: "calibrate", first: null }], ["focusDrawing"]],
  extract: [["extract", true]], find: [["find", true]], dimStyle: [["dimStyle"]], updateFields: [["updateFields"]],
  parameters: [["parameters"]], constraintBar: [["barHidden", false]],
  blockPalette: [["blockPalette", true]], writeBlock: [["blockPalette", true]], attributeManager: [["attributeManager"]],
  designCenter: [["designCenter"]], toolPalettes: [["toolPalettes"]], layerStates: [["layerStates", true]], translateLayers: [["translator", true]],
  groupSelect: [["groupSelect", true]], split: [["split", true]], arabicReversed: [["arabicReversed", true]], ucsIcon: [["ucsIcon", true]],
  ucsWorld: [["apply", { type: "ucs", origin: [0, 0], x_axis: [1, 0] }]], sheetSet: [["sheetSet", true]],
  recover: [["recover"]], runScript: [["runScript"]], audit: [["audit"]], purge: [["purge"]],
  options: [["options", true]], customize: [["customize", true]], aliases: [["aliases", true]],
} satisfies Record<RibbonAction, unknown[][]>;

const commands = [...new Set([...QUICK_ACCESS, ...DEFAULT_RIBBON.tabs.flatMap((tab) => tab.groups.flatMap((group) => group.buttons))]
  .map((button) => button.command).filter(isRibbonCommand))];

test.each(commands)("the default ribbon's %s reaches its App handler", (command) => {
  const { calls, ribbon } = setup();
  expect(ribbon.isEnabled(command)).toBe(true);
  ribbon.onAction(command);
  expect(calls).toEqual(isRibbonTool(command) ? [["tool", command]] : actionCalls[command]);
});

test("drawing-dependent actions stay disabled without a drawing", () => {
  const { ribbon } = setup(false);
  const always = new Set(["new", "open", "undo", "redo", "zoomExtents", "groupSelect", "ucsIcon", "sheetSet", "recover", "runScript", "plotStyles", "options", "customize", "aliases"]);
  for (const command of commands) {
    expect(ribbon.isEnabled(command), command).toBe(isRibbonTool(command) || always.has(command));
  }
});

test("toggle commands read the current state, and the selected tool stays pressed", () => {
  const { calls, ribbon } = setup(true, true);
  for (const command of ["split", "groupSelect", "arabicReversed", "ucsIcon"] as const) {
    expect(ribbon.isPressed(command)).toBe(true);
    ribbon.onAction(command);
    expect(calls.at(-1)).toEqual([command, false]);
  }
  expect(ribbon.isPressed("line")).toBe(true);
  expect(ribbon.isPressed("select")).toBe(false);
});
