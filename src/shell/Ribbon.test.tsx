// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { Ribbon } from "./Ribbon";
import css from "./theme.css?raw";

afterEach(cleanup);

const props = { active: "select" as const, onTool() {}, onOpen() {}, onZoomExtents() {} };

/** Shows a tab: a click on it, or, for the tabs under "More", the menu and its item. */
function go(name: string) {
  const tab = screen.queryByRole("tab", { name });
  if (tab) {
    fireEvent.click(tab);
    return;
  }
  fireEvent.click(document.querySelector<HTMLElement>(".ribbon-more")!);
  fireEvent.click(screen.getByRole("menuitemradio", { name }));
}

test("the View tab says whether the drawing's Arabic text is reversed", () => {
  const onArabicReversed = vi.fn();
  const { rerender } = render(
    <Ribbon {...props} arabicReversed={false} onArabicReversed={onArabicReversed} />,
  );
  go("View");
  const box = screen.getByRole("button", { name: "Arabic text is reversed" }) as HTMLButtonElement;
  expect(box.getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(box);
  expect(onArabicReversed).toHaveBeenCalledWith(true);

  rerender(<Ribbon {...props} arabicReversed />);
  const off = screen.getByRole("button", { name: "Arabic text is reversed" }) as HTMLButtonElement;
  expect(off.getAttribute("aria-pressed")).toBe("true");
  // No drawing open: nothing to set.
  expect(off.disabled).toBe(true);
});

test("the Measure tab has the inquiry tools and Extract data", () => {
  const onTool = vi.fn();
  const onExtract = vi.fn();
  render(<Ribbon {...props} onTool={onTool} onExtract={onExtract} />);
  go("Measure");
  for (const [name, tool] of [
    ["Geometry", "geometry"],
    ["Divide", "divide"],
    ["Measure along", "measureAlong"],
    ["ID point", "idPoint"],
  ]) {
    fireEvent.click(screen.getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  fireEvent.click(screen.getByRole("button", { name: "Extract data" }));
  expect(onExtract).toHaveBeenCalled();
});

test("Open has the recent drawings beside it, and View has the named views", async () => {
  const loadViews = vi.fn(async () => []);
  render(
    <Ribbon
      {...props}
      loadRecents={async () => []}
      onOpenRecent={() => {}}
      onClearRecents={async () => {}}
      loadViews={loadViews}
      onGoToView={() => {}}
      onSaveView={async () => {}}
      onDeleteView={async () => {}}
    />,
  );
  expect(screen.getByRole("button", { name: "Open…" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Recent drawings" })).toBeTruthy();
  go("View");
  fireEvent.click(screen.getByRole("button", { name: /Views/ }));
  expect(await screen.findByRole("menu", { name: "Views" })).toBeTruthy();
  expect(loadViews).toHaveBeenCalled();
});

test("without a drawing on the Model sheet, Views is disabled", () => {
  render(<Ribbon {...props} />);
  go("View");
  expect((screen.getByRole("button", { name: "Views" }) as HTMLButtonElement).disabled).toBe(true);
});

/** Base UI moves focus a tick after the key. */
const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 30))));

test("the ribbon tabs have a roving tab stop; the arrows move between them, round the ends", async () => {
  render(<Ribbon {...props} />);
  const tab = (name: string) => screen.getByRole("tab", { name });
  const names = ["Measure", "Draw", "Annotate", "More"];
  expect(screen.getAllByRole("tablist")).toHaveLength(1);
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(names);
  expect(names.map((n) => tab(n).tabIndex)).toEqual([0, -1, -1, -1]);
  tab("Measure").focus();
  fireEvent.keyDown(tab("Measure"), { key: "ArrowRight" });
  await settle();
  expect(document.activeElement).toBe(tab("Draw"));
  // Enter and Space press a focused tab (a button's own click); the chosen tab is the tab stop.
  fireEvent.click(tab("Draw"));
  expect(tab("Draw").getAttribute("aria-selected")).toBe("true");
  expect(names.map((n) => tab(n).tabIndex)).toEqual([-1, 0, -1, -1]);
  // "More" is the last stop, and the arrows go round.
  fireEvent.keyDown(tab("Draw"), { key: "End" });
  await settle();
  expect(document.activeElement).toBe(tab("More"));
  fireEvent.keyDown(tab("More"), { key: "ArrowRight" });
  await settle();
  expect(document.activeElement).toBe(tab("Measure"));
});

test("the Home tab has the Draw group", () => {
  const onTool = vi.fn();
  const { rerender } = render(<Ribbon {...props} onTool={onTool} />);
  go("Draw");
  for (const [name, tool] of [
    ["Line", "line"],
    ["Polyline", "polyline"],
    ["Rectangle", "rectangle"],
    ["Circle", "circle"],
    ["Arc", "arc"],
    ["Ellipse", "ellipse"],
    ["Spline", "spline"],
    ["Point", "point"],
    ["Hatch", "hatch"],
    ["Revision cloud", "revcloud"],
    ["Wipeout", "wipeout"],
  ]) {
    fireEvent.click(screen.getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  // The tool on goes back to Select when pressed again.
  rerender(<Ribbon {...props} active="line" onTool={onTool} />);
  const line = screen.getByRole("button", { name: "Line" });
  expect(line.getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(line);
  expect(onTool).toHaveBeenLastCalledWith("select");
});

test("the Home tab has the Modify group", () => {
  const onTool = vi.fn();
  render(<Ribbon {...props} onTool={onTool} />);
  go("Draw");
  const group = screen.getByRole("group", { name: "Modify" });
  expect(group).toBeTruthy();
  for (const [name, tool] of [
    ["Move", "move"],
    ["Copy", "copy"],
    ["Rotate", "rotate"],
    ["Scale", "scale"],
    ["Mirror", "mirror"],
    ["Stretch", "stretch"],
    ["Align", "align"],
    ["Array", "array"],
    ["Explode", "explode"],
    ["Erase", "erase"],
  ]) {
    fireEvent.click(screen.getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
});

test("the Home tab has the Annotation group with Find and replace", () => {
  const onTool = vi.fn();
  const onFind = vi.fn();
  render(<Ribbon {...props} onTool={onTool} onFind={onFind} />);
  go("Draw");
  const group = screen.getByRole("group", { name: "Annotation" });
  for (const [name, tool] of [
    ["Text", "text"],
    ["MText", "mtext"],
    ["Edit text", "textEdit"],
  ]) {
    fireEvent.click(within(group).getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  fireEvent.click(within(group).getByRole("button", { name: "Find and replace" }));
  expect(onFind).toHaveBeenCalled();
});

test("the Annotate tab has the Dimensions group, the dimension style and the Leaders group", () => {
  const onTool = vi.fn();
  const onDimStyle = vi.fn();
  render(<Ribbon {...props} onTool={onTool} onDimStyle={onDimStyle} />);
  go("Annotate");
  const dims = screen.getByRole("group", { name: "Dimensions" });
  for (const [name, tool] of [
    ["Linear", "dimLinear"],
    ["Aligned", "dimAligned"],
    ["Angular", "dimAngular"],
    ["Radius", "dimRadius"],
    ["Diameter", "dimDiameter"],
    ["Arc length", "dimArc"],
    ["Ordinate", "dimOrdinate"],
    ["Baseline", "dimBaseline"],
    ["Continue", "dimContinue"],
    ["Quick", "qdim"],
    ["Edit dimension", "dimEdit"],
    ["Break", "dimBreak"],
    ["Space", "dimSpace"],
    ["Jog line", "dimJogLine"],
  ]) {
    fireEvent.click(within(dims).getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  fireEvent.click(within(dims).getByRole("button", { name: "Dimension style…" }));
  expect(onDimStyle).toHaveBeenCalled();
  const notes = screen.getByRole("group", { name: "Leaders" });
  for (const [name, tool] of [
    ["Leader", "leader"],
    ["Multileader", "mleader"],
    ["Tolerance", "tolerance"],
  ]) {
    fireEvent.click(within(notes).getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
});

test("the Annotate tab has the Tables group: table, edit, field and update fields", () => {
  const onTool = vi.fn();
  const onUpdateFields = vi.fn();
  render(<Ribbon {...props} onTool={onTool} onUpdateFields={onUpdateFields} />);
  go("Annotate");
  const group = screen.getByRole("group", { name: "Tables" });
  for (const [name, tool] of [
    ["Table", "table"],
    ["Edit table", "tableEdit"],
    ["Field", "field"],
  ]) {
    fireEvent.click(within(group).getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  fireEvent.click(within(group).getByRole("button", { name: "Update fields" }));
  expect(onUpdateFields).toHaveBeenCalled();
});

test("the Insert tab has the Block group", () => {
  const onTool = vi.fn();
  const handlers = {
    onInsertBlock: vi.fn(),
    onWriteBlock: vi.fn(),
    onAttributeManager: vi.fn(),
    onDesignCenter: vi.fn(),
    onToolPalettes: vi.fn(),
  };
  render(<Ribbon {...props} onTool={onTool} {...handlers} />);
  go("Insert");
  const group = screen.getByRole("group", { name: "Block" });
  for (const [name, tool] of [
    ["Create block", "block"],
    ["Edit in place", "refedit"],
    ["Define attribute", "attdef"],
    ["Edit attributes", "textEdit"],
  ]) {
    fireEvent.click(within(group).getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  for (const [name, handler] of [
    ["Insert…", handlers.onInsertBlock],
    ["Write block…", handlers.onWriteBlock],
    ["Attribute manager", handlers.onAttributeManager],
    ["Design Center", handlers.onDesignCenter],
    ["Tool palette", handlers.onToolPalettes],
  ] as const) {
    fireEvent.click(within(group).getByRole("button", { name }));
    expect(handler).toHaveBeenCalled();
  }
});

test("Home has a Layers group: layer states, the translator, groups, match properties", () => {
  const onTool = vi.fn();
  const onLayerStates = vi.fn();
  const onTranslateLayers = vi.fn();
  const onGroupSelect = vi.fn();
  render(
    <Ribbon
      {...props}
      onTool={onTool}
      onLayerStates={onLayerStates}
      onTranslateLayers={onTranslateLayers}
      groupSelect
      onGroupSelect={onGroupSelect}
    />,
  );
  go("Draw");
  const layers = within(screen.getByRole("group", { name: "Layers" }));
  fireEvent.click(layers.getByRole("button", { name: "Layer states" }));
  expect(onLayerStates).toHaveBeenCalled();
  fireEvent.click(layers.getByRole("button", { name: "Translate layers" }));
  expect(onTranslateLayers).toHaveBeenCalled();
  for (const [name, tool] of [
    ["Group", "group"],
    ["Ungroup", "ungroup"],
    ["Match properties", "matchprop"],
  ]) {
    fireEvent.click(layers.getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  // They are not the Modify group's.
  expect(within(screen.getByRole("group", { name: "Modify" })).queryByRole("button", { name: "Ungroup" })).toBeNull();
  const box = layers.getByRole("button", { name: "Group selection" });
  expect(box.getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(box);
  expect(onGroupSelect).toHaveBeenCalledWith(false);
});

test("the Home tab has the Clipboard group, and Recover is on Manage beside Audit and Purge", () => {
  const onTool = vi.fn();
  const onRecover = vi.fn();
  render(<Ribbon {...props} onTool={onTool} onRecover={onRecover} />);
  go("Draw");
  const group = screen.getByRole("group", { name: "Clipboard" });
  for (const [name, tool] of [
    ["Copy to clipboard", "copyclip"],
    ["Copy with base point", "copybase"],
    ["Paste", "pasteclip"],
    ["Paste as block", "pasteblock"],
  ]) {
    fireEvent.click(within(group).getByRole("button", { name }));
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  expect(screen.queryByRole("button", { name: "Recover…" })).toBeNull();
  go("Manage");
  const drawing = screen.getByRole("group", { name: "Drawing" });
  expect(within(drawing).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Audit…",
    "Purge…",
    "Recover…",
  ]);
  fireEvent.click(within(drawing).getByRole("button", { name: "Recover…" }));
  expect(onRecover).toHaveBeenCalled();
});

test("the View tab has the Viewports group: MVIEW, two views, the UCS and sheet sets", () => {
  const onTool = vi.fn();
  const onSplit = vi.fn();
  const onUcsWorld = vi.fn();
  const onSheetSet = vi.fn();
  const { rerender } = render(
    <Ribbon {...props} onTool={onTool} onSplit={onSplit} onUcsWorld={onUcsWorld} onSheetSet={onSheetSet} />,
  );
  go("View");
  const group = screen.getByRole("group", { name: "Viewports" });
  fireEvent.click(within(group).getByRole("button", { name: "New viewport" }));
  expect(onTool).toHaveBeenLastCalledWith("mview");
  fireEvent.click(within(group).getByRole("button", { name: "UCS" }));
  expect(onTool).toHaveBeenLastCalledWith("ucs");
  fireEvent.click(within(group).getByRole("button", { name: "UCS from object" }));
  expect(onTool).toHaveBeenLastCalledWith("ucsObject");
  fireEvent.click(within(group).getByRole("button", { name: "World UCS" }));
  expect(onUcsWorld).toHaveBeenCalled();
  fireEvent.click(within(group).getByRole("button", { name: "Two viewports" }));
  expect(onSplit).toHaveBeenCalledWith(true);
  fireEvent.click(within(group).getByRole("button", { name: "Sheet set…" }));
  expect(onSheetSet).toHaveBeenCalled();
  rerender(<Ribbon {...props} split onSplit={onSplit} />);
  fireEvent.click(screen.getByRole("button", { name: "One viewport" }));
  expect(onSplit).toHaveBeenLastCalledWith(false);
});

test("the Manage tab has Audit, Purge, Options, Customize and Aliases", () => {
  const handlers = {
    onAudit: vi.fn(),
    onPurge: vi.fn(),
    onOptions: vi.fn(),
    onCustomize: vi.fn(),
    onAliases: vi.fn(),
  };
  render(<Ribbon {...props} {...handlers} />);
  go("Manage");
  for (const [name, handler] of [
    ["Audit…", handlers.onAudit],
    ["Purge…", handlers.onPurge],
    ["Options…", handlers.onOptions],
    ["Customize…", handlers.onCustomize],
    ["Aliases…", handlers.onAliases],
  ] as const) {
    fireEvent.click(screen.getByRole("button", { name }));
    expect(handler).toHaveBeenCalled();
  }
});

test("a saved layout is drawn as it is: its tabs, groups and buttons in their order, the rest gone", () => {
  const onTool = vi.fn();
  const onCompare = vi.fn();
  render(
    <Ribbon
      {...props}
      onTool={onTool}
      onCompare={onCompare}
      layout={{
        tabs: [
          {
            id: "view",
            label: "view",
            groups: [{ id: "views", label: "ribbon.views", buttons: [{ kind: "tool", command: "zoomWindow", label: "zoomWindow" }] }],
          },
          {
            id: "home",
            label: "home",
            groups: [
              {
                id: "draw",
                label: "draw.group",
                buttons: [
                  { kind: "tool", command: "circle", label: "draw.circle" },
                  { kind: "tool", command: "line", label: "draw.line" },
                ],
              },
              // Moved from Measure to Home.
              { id: "takeoff", label: "ribbon.takeoff", buttons: [{ kind: "action", command: "compare", label: "compare" }] },
            ],
          },
        ],
      }}
    />,
  );
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["View", "Home"]);
  // The first tab is on screen.
  expect(screen.getByRole("button", { name: "Zoom window" })).toBeTruthy();
  go("Home");
  expect(screen.getAllByRole("group").map((g) => g.getAttribute("aria-label"))).toEqual(["Draw", "Take-off"]);
  expect(within(screen.getByRole("group", { name: "Draw" })).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Circle",
    "Line",
  ]);
  expect(screen.queryByRole("button", { name: "Polyline" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Compare" }));
  expect(onCompare).toHaveBeenCalled();
  // The quick-access bar is the window's, whatever the layout.
  expect(within(screen.getByRole("toolbar", { name: "Quick access" })).getAllByRole("button").length).toBe(6);
});

test("the quick-access bar sits after the app name, before the tabs: New, Open (with recents), Save, Save as, Undo, Redo", () => {
  const handlers = { onNew: vi.fn(), onOpen: vi.fn(), onSave: vi.fn(), onSaveAs: vi.fn(), onUndo: vi.fn(), onRedo: vi.fn() };
  render(
    <Ribbon
      {...props}
      {...handlers}
      loadRecents={async () => []}
      onOpenRecent={() => {}}
      onClearRecents={async () => {}}
    />,
  );
  const bar = screen.getByRole("toolbar", { name: "Quick access" });
  expect(within(bar).getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual([
    "New…",
    "Open…",
    "Recent drawings",
    "Save",
    "Save as",
    "Undo",
    "Redo",
  ]);
  for (const [name, handler] of [
    ["New…", handlers.onNew],
    ["Open…", handlers.onOpen],
    ["Save", handlers.onSave],
    ["Save as", handlers.onSaveAs],
    ["Undo", handlers.onUndo],
    ["Redo", handlers.onRedo],
  ] as const) {
    fireEvent.click(within(bar).getByRole("button", { name }));
    expect(handler).toHaveBeenCalledTimes(1);
  }
  // On the tabs' row, after the app name and before the tablist; never in a tab's panel.
  const row = bar.parentElement!;
  expect(row.firstElementChild!.textContent).toBe("Qiyas");
  expect(row.children[1]).toBe(bar);
  expect(row.children[2].getAttribute("role")).toBe("tablist");
  const panel = screen.getByRole("tabpanel");
  for (const name of ["Save", "Undo", "Redo"]) expect(within(panel).queryByRole("button", { name })).toBeNull();
  // Without a drawing, Save and Save as are disabled; Open never is.
  cleanup();
  render(<Ribbon {...props} />);
  const plain = screen.getByRole("toolbar", { name: "Quick access" });
  expect((within(plain).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  expect((within(plain).getByRole("button", { name: "Open…" }) as HTMLButtonElement).disabled).toBe(false);
});

test("Draw is Draw, Modify, Edit, Annotation, Clipboard and Layers; each group is a box of icon buttons with its name under them", () => {
  const { container } = render(<Ribbon {...props} />);
  go("Draw");
  const groups = screen.getAllByRole("group");
  expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual([
    "Draw",
    "Modify",
    "Edit",
    "Annotation",
    "Clipboard",
    "Layers",
  ]);
  for (const g of groups) {
    const [buttons, label] = Array.from(g.children);
    expect(buttons.className).toBe("ribbon-group-buttons");
    expect(label.className).toBe("ribbon-group-label");
    expect(label.textContent).toBe(g.getAttribute("aria-label"));
    // Icon buttons: a name for the screen reader, no words on the button.
    for (const b of within(g).getAllByRole("button")) {
      expect(b.textContent).toBe("");
      expect(b.getAttribute("aria-label")).toBeTruthy();
      expect(b.querySelector("svg")).toBeTruthy();
    }
  }
  // Modify is the transform tools, Edit the 13 edit tools; no row break inside a group.
  expect(within(screen.getByRole("group", { name: "Modify" })).getAllByRole("button")).toHaveLength(10);
  expect(within(screen.getByRole("group", { name: "Edit" })).getAllByRole("button")).toHaveLength(13);
  expect(container.querySelector(".ribbon-break, .sep")).toBeNull();
  const layers = screen.getByRole("group", { name: "Layers" });
  expect(within(layers).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Layer states",
    "Translate layers",
    "Group",
    "Ungroup",
    "Match properties",
    "Group selection",
  ]);
});

test("the Measure tab is first: Measure, Inquiry, Take-off, Dimensions and Markup", () => {
  const handlers = { onExport: vi.fn(), onImportBoq: vi.fn(), onCompare: vi.fn(), onExportConflicts: vi.fn() };
  render(<Ribbon {...props} {...handlers} />);
  expect(screen.getAllByRole("group").map((g) => g.getAttribute("aria-label"))).toEqual([
    "Measure",
    "Inquiry",
    "Take-off",
    "Dimensions",
    "Markup",
  ]);
  const group = screen.getByRole("group", { name: "Take-off" });
  const buttons = within(group).getAllByRole("button");
  expect(buttons).toHaveLength(4);
  // The first-use actions can be identified without waiting for a tooltip.
  for (const button of buttons) {
    expect(button.querySelector(".rb-label")?.textContent).toBe(button.getAttribute("aria-label"));
  }
  buttons.forEach((b) => fireEvent.click(b));
  for (const h of Object.values(handlers)) expect(h).toHaveBeenCalledTimes(1);
  // The headline groups are one row of primary buttons.
  expect(group.firstElementChild!.className).toContain("primary");
  expect(screen.getByRole("group", { name: "Measure" }).firstElementChild!.className).toContain("primary");
  expect(screen.getByRole("group", { name: "Inquiry" }).firstElementChild!.className).not.toContain("primary");
  const markup = within(screen.getByRole("group", { name: "Markup" }));
  expect(markup.getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Revision cloud",
    "Comment cloud",
    "Leader",
    "MText",
  ]);
});

test("in Arabic the ribbon reads right to left: the same order from the start edge, Arabic names, logical sides", () => {
  localStorage.setItem("qiyas.lang", "ar");
  try {
    render(
      <LanguageProvider>
        <Ribbon {...props} />
      </LanguageProvider>,
    );
    expect(document.documentElement.dir).toBe("rtl");
    // The flex rows follow the page direction: the app name, the quick-access bar and the
    // first tab sit at the right, and Draw is the rightmost group (the start edge).
    const bar = screen.getByRole("toolbar", { name: "الوصول السريع" });
    expect(bar.previousElementSibling!.textContent).toBe("قياس");
    expect(bar.nextElementSibling!.getAttribute("role")).toBe("tablist");
    const names = ["أدوات القياس", "استعلام", "الحصر", "الأبعاد", "ملاحظات"];
    expect(screen.getAllByRole("group").map((g) => g.getAttribute("aria-label"))).toEqual(names);
    expect(screen.getAllByRole("group").map((g) => g.lastElementChild!.textContent)).toEqual(names);
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["القياسات", "رسم", "التعليقات", "المزيد"]);
    for (const button of document.querySelectorAll(".rb.primary")) {
      expect(button.querySelector(".rb-label")?.textContent).toBe(button.getAttribute("aria-label"));
    }
  } finally {
    cleanup();
    localStorage.clear();
    document.documentElement.dir = "ltr";
    document.documentElement.lang = "en";
  }
  // The styles use start/end sides only, so Arabic mirrors them, and the panel scrolls sideways
  // instead of wrapping (a right-to-left panel starts scrolled to its right edge).
  const rule = (selector: string) => css.match(new RegExp(`\\n${selector.replace(".", "\\.")} \\{([^}]*)\\}`))![1];
  expect(rule(".quick-access")).toContain("border-inline-end");
  expect(rule(".ribbon-group \\+ \\.ribbon-group")).toContain("border-inline-start");
  const panel = rule(".ribbon-panel");
  expect(panel).toContain("flex-wrap: nowrap");
  expect(panel).toContain("overflow-x: auto");
  // Sized from the tokens, shorter than the 104px of the text ribbon.
  expect(panel).toContain("min-height: calc(var(--rb-std) * 2");
  const grid = rule(".ribbon-group-buttons");
  expect(grid).toContain("grid-auto-flow: column");
  expect(grid).toContain("grid-template-rows: repeat(2, var(--rb-std))");
  for (const r of [rule(".quick-access"), rule(".ribbon-group"), panel]) expect(r).not.toMatch(/\b(left|right)\b/);
});

// The rule `selector { … }` at the top level of theme.css.
const rule = (selector: string) => css.match(new RegExp(`\\n${selector.replace(/[.+]/g, "\\$&")} \\{([^}]*)\\}`))![1];

test("the tabs' row scrolls sideways and stops short of the preferences instead of lying under them", () => {
  // The tabs never wrap and never shrink.
  expect(rule(".ribbon-tabs \\[role=\"tab\"\\]")).toContain("white-space: nowrap");
  expect(rule(".ribbon-tabs > \\*")).toContain("flex-shrink: 0");
  // The row scrolls (thin bar) and its end margin is the overlay's width, from one variable the
  // overlay reads too, so they stay in step. A margin, not padding: padding is only reached at
  // the end of the scroll, so the tabs lay under the overlay until then.
  const row = rule(".ribbon-tabs");
  expect(row).toContain("overflow-x: auto");
  expect(row).toContain("scrollbar-width: thin");
  expect(row).toContain("margin-inline-end: var(--prefs-width)");
  expect(row).not.toContain("padding-inline-end");
  // The reserve is at least the overlay's real width: four icon buttons (language, theme,
  // settings, about), about 145px; at 1024px the row (about 560px) and the overlay fit side by side.
  const reserve = Number(css.match(/:root \{\s*--prefs-width: (\d+)px/)![1]);
  expect(reserve).toBeGreaterThanOrEqual(150);
  expect(reserve + 560).toBeLessThanOrEqual(1024);
  expect(rule(".prefs")).toContain("min-width: var(--prefs-width)");
  expect(css).toMatch(/\.ribbon-tabs::-webkit-scrollbar \{\s*height: 6px/);
  // The prefs take a row of their own under 1000px.
  const stacked = css.match(/@media \(max-width: 1000px\) \{([\s\S]*?)\n\}/)![1];
  expect(stacked).toContain("position: static");
  expect(stacked).toMatch(/\.ribbon-tabs \{\s*margin-inline-end: 0/);
  expect(css).not.toMatch(/max-width: (1500|1020|700|1420)px/);
  // Logical sides only, so Arabic mirrors the row and the overlay.
  for (const r of [row, rule(".prefs"), stacked]) expect(r).not.toMatch(/\b(left|right)\b/);
});

test("the quick-access bar is a toolbar: one tab stop, arrows, Home and End move between its buttons", () => {
  const handlers = { onNew() {}, onSave() {}, onSaveAs() {}, onUndo() {}, onRedo() {} };
  render(
    <Ribbon
      {...props}
      {...handlers}
      loadRecents={async () => []}
      onOpenRecent={() => {}}
      onClearRecents={async () => {}}
    />,
  );
  const bar = screen.getByRole("toolbar", { name: "Quick access" });
  const names = () => within(bar).getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent);
  expect(names()).toEqual(["New…", "Open…", "Recent drawings", "Save", "Save as", "Undo", "Redo"]);
  const stops = () => within(bar).getAllByRole("button").map((b) => b.tabIndex);
  expect(stops()).toEqual([0, -1, -1, -1, -1, -1, -1]);
  const button = (name: string) => within(bar).getByRole("button", { name });
  button("New…").focus();
  fireEvent.keyDown(button("New…"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(button("Open…"));
  fireEvent.keyDown(button("Open…"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(button("Recent drawings"));
  expect(stops()).toEqual([-1, -1, 0, -1, -1, -1, -1]);
  fireEvent.keyDown(button("Recent drawings"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(button("Save"));
  fireEvent.keyDown(button("Save"), { key: "End" });
  expect(document.activeElement).toBe(button("Redo"));
  fireEvent.keyDown(button("Redo"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(button("New…"));
  fireEvent.keyDown(button("New…"), { key: "ArrowLeft" });
  expect(document.activeElement).toBe(button("Redo"));
  fireEvent.keyDown(button("Redo"), { key: "Home" });
  expect(document.activeElement).toBe(button("New…"));
  // The tab stop stays on the last button focused when focus leaves the bar.
  button("Undo").focus();
  fireEvent.keyDown(button("Undo"), { key: "ArrowLeft" });
  screen.getByRole("tab", { name: "Measure" }).focus();
  expect(stops()).toEqual([-1, -1, -1, -1, 0, -1, -1]);
});

test("the quick-access arrows skip a disabled button and mirror in Arabic", () => {
  // Only Open and Undo have handlers here; Save, Save as and Redo are disabled.
  render(<Ribbon {...props} onUndo={() => {}} />);
  const bar = screen.getByRole("toolbar", { name: "Quick access" });
  const button = (name: string) => within(bar).getByRole("button", { name });
  button("Open…").focus();
  fireEvent.keyDown(button("Open…"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(button("Undo"));
  document.documentElement.dir = "rtl";
  try {
    fireEvent.keyDown(button("Undo"), { key: "ArrowRight" });
    expect(document.activeElement).toBe(button("Open…"));
  } finally {
    document.documentElement.dir = "ltr";
  }
});

// Collapsing the ribbon (W4, note 40 interview): double-click a tab; the keyboard keeps working.
const panel = () => document.querySelector(".ribbon-panel");

test("double-clicking a tab collapses the ribbon to its tabs, and again expands it; the choice is kept", () => {
  localStorage.removeItem("qiyas.ribbon.collapsed");
  render(<Ribbon {...props} />);
  expect(panel()).not.toBeNull();
  const measure = screen.getByRole("tab", { name: "Measure" });
  fireEvent.doubleClick(measure);
  expect(panel()).toBeNull();
  expect(localStorage.getItem("qiyas.ribbon.collapsed")).toBe("1");
  expect(document.querySelector(".ribbon")?.classList.contains("collapsed")).toBe(true);
  fireEvent.doubleClick(measure);
  expect(panel()).not.toBeNull();
  expect(localStorage.getItem("qiyas.ribbon.collapsed")).toBe("0");
});

test("a collapsed ribbon shows a tab's panel when the tab is clicked, and Escape or a click elsewhere puts it away", () => {
  localStorage.setItem("qiyas.ribbon.collapsed", "1");
  render(<Ribbon {...props} />);
  expect(panel()).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "Draw" }));
  expect(panel()).not.toBeNull();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(panel()).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "Annotate" }));
  expect(panel()).not.toBeNull();
  fireEvent.pointerDown(document.body);
  expect(panel()).toBeNull();
  localStorage.removeItem("qiyas.ribbon.collapsed");
});

test("a tool chosen from the shown panel puts the panel away again", () => {
  localStorage.setItem("qiyas.ribbon.collapsed", "1");
  const onTool = vi.fn();
  render(<Ribbon {...props} onTool={onTool} />);
  fireEvent.click(screen.getByRole("tab", { name: "Measure" }));
  fireEvent.click(screen.getByRole("button", { name: "Distance" }));
  expect(onTool).toHaveBeenCalledWith("distance");
  expect(panel()).toBeNull();
  localStorage.removeItem("qiyas.ribbon.collapsed");
});

test("without a mouse: Ctrl+F1 and the arrow at the row's end collapse and expand", () => {
  localStorage.removeItem("qiyas.ribbon.collapsed");
  render(<Ribbon {...props} />);
  const arrow = screen.getByRole("button", { name: "Collapse the ribbon" });
  expect(arrow.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(arrow);
  expect(panel()).toBeNull();
  expect(screen.getByRole("button", { name: "Expand the ribbon" }).getAttribute("aria-expanded")).toBe("false");
  fireEvent.keyDown(window, { key: "F1", ctrlKey: true });
  expect(panel()).not.toBeNull();
  fireEvent.keyDown(window, { key: "F1", ctrlKey: true });
  expect(panel()).toBeNull();
  localStorage.removeItem("qiyas.ribbon.collapsed");
});
