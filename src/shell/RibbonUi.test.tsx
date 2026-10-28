// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Ribbon } from "./Ribbon";
import { HOVER_DELAY } from "./Tooltip";

const props = { active: "select" as const, onTool() {}, onOpen() {}, onZoomExtents() {} };

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.setItem("qiyas.ribbon.collapsed", "0");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  localStorage.removeItem("qiyas.ribbon.collapsed");
});

const wait = (ms: number) => act(() => void vi.advanceTimersByTime(ms));
/** Time passes and the promises it was waiting for settle (a popup closing, the focus returning). */
const settle = (ms = 50) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
/** The pointer comes onto a button: Base UI hears it on the box round the button (`.rb-tip`), as a
 *  `mouseenter` and a move. */
const enter = (el: HTMLElement) => {
  const box = el.closest(".rb-tip") ?? el;
  act(() => {
    fireEvent.mouseEnter(box);
    fireEvent.mouseMove(box);
  });
};
const leave = (el: HTMLElement) => act(() => void fireEvent.mouseLeave(el.closest(".rb-tip") ?? el));
const hover = (el: HTMLElement) => {
  enter(el);
  wait(HOVER_DELAY + 50);
};

test("the pointer resting on a button shows a tooltip: name, what it does, its alias; leaving hides it", () => {
  render(<Ribbon {...props} />);
  fireEvent.click(screen.getByRole("tab", { name: "Draw" }));
  const trim = screen.getByRole("button", { name: "Trim" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  enter(trim);
  // After a short delay, not at once.
  expect(screen.queryByRole("tooltip")).toBeNull();
  wait(HOVER_DELAY + 50);
  const tip = screen.getByRole("tooltip");
  expect(tip.querySelector("b")!.textContent).toBe("Trim");
  expect(tip.textContent).toContain("Cut objects back to the edge of another object.");
  expect(tip.querySelector("kbd")!.textContent).toBe("Alias · TR");
  expect(trim.getAttribute("aria-describedby")).toBe(tip.id);
  leave(trim);
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(trim.getAttribute("aria-describedby")).toBeNull();
});

test("a command without an alias has no alias line; the ellipsis is not in the name", () => {
  render(<Ribbon {...props} onOpen={() => {}} />);
  hover(screen.getByRole("button", { name: "Select" }));
  expect(screen.getByRole("tooltip").querySelector("kbd")).toBeNull();
  leave(screen.getByRole("button", { name: "Select" }));
  hover(screen.getByRole("button", { name: "Open…" }));
  expect(screen.getByRole("tooltip").querySelector("b")!.textContent).toBe("Open");
});

test("the user's own alias shows in the tooltip", () => {
  render(<Ribbon {...props} aliases={{ L: "LINE", LN: "LINE" }} />);
  fireEvent.click(screen.getByRole("tab", { name: "Draw" }));
  hover(screen.getByRole("button", { name: "Line" }));
  expect(screen.getByRole("tooltip").querySelector("kbd")!.textContent).toBe("Alias · L · LN");
});

test("the keyboard's focus shows it, Escape, blur and a click hide it", () => {
  render(<Ribbon {...props} />);
  const select = screen.getByRole("button", { name: "Select" });
  fireEvent.keyDown(document.body, { key: "Tab" });
  act(() => select.focus());
  wait(300);
  expect(screen.getByRole("tooltip").textContent).toContain("Select");
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  // Again, then focus moves away.
  act(() => select.blur());
  fireEvent.keyDown(document.body, { key: "Tab" });
  act(() => select.focus());
  wait(300);
  expect(screen.queryByRole("tooltip")).not.toBeNull();
  act(() => select.blur());
  // Base UI looks where the focus went on the next tick.
  wait(50);
  expect(screen.queryByRole("tooltip")).toBeNull();
  // A click hides the one the pointer brought.
  hover(select);
  expect(screen.queryByRole("tooltip")).not.toBeNull();
  fireEvent.click(select);
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("focus a click brought shows no tooltip", () => {
  render(<Ribbon {...props} />);
  const select = screen.getByRole("button", { name: "Select" });
  fireEvent.pointerDown(select);
  fireEvent.mouseDown(select);
  act(() => select.focus());
  wait(500);
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("a disabled button has a tooltip that says it is coming soon", () => {
  render(<Ribbon {...props} />);
  const save = screen.getByRole("button", { name: "Save" });
  expect((save as HTMLButtonElement).disabled).toBe(true);
  hover(save.parentElement!);
  const tip = screen.getByRole("tooltip");
  expect(tip.textContent).toContain("Coming soon");
  expect(tip.querySelector("b")!.textContent).toBe("Save");
});

test("moving from one button to the next while a card shows moves it at once", () => {
  render(<Ribbon {...props} />);
  hover(screen.getByRole("button", { name: "Distance" }));
  expect(screen.getByRole("tooltip").querySelector("b")!.textContent).toBe("Distance");
  leave(screen.getByRole("button", { name: "Distance" }));
  enter(screen.getByRole("button", { name: "Area" }));
  expect(screen.getByRole("tooltip").querySelector("b")!.textContent).toBe("Area");
});

test("in Arabic the tooltip is Arabic, the alias stays what is typed", async () => {
  const { LanguageProvider } = await import("../i18n");
  localStorage.setItem("qiyas.lang", "ar");
  try {
    render(
      <LanguageProvider>
        <Ribbon {...props} />
      </LanguageProvider>,
    );
    fireEvent.click(screen.getByRole("tab", { name: "رسم" }));
    hover(screen.getByRole("button", { name: "قص" }));
    const tip = screen.getByRole("tooltip");
    expect(tip.textContent).toContain("قص الأجزاء الزائدة عند حافة كائن آخر.");
    expect(tip.querySelector("kbd")!.textContent).toBe("الاختصار · TR");
  } finally {
    localStorage.clear();
    document.documentElement.dir = "ltr";
    document.documentElement.lang = "en";
  }
});

test("More holds Parametric, Insert, View, Manage and Output; choosing one shows it as the selected tab", () => {
  render(<Ribbon {...props} />);
  const tabs = () => screen.getAllByRole("tab").map((t) => t.textContent);
  expect(tabs()).toEqual(["Measure", "Draw", "Annotate", "More"]);
  const more = screen.getByRole("tab", { name: "More" });
  expect(more.getAttribute("aria-haspopup")).toBe("menu");
  expect(more.getAttribute("aria-selected")).toBe("false");
  fireEvent.click(more);
  const menu = screen.getByRole("menu", { name: "More" });
  expect(within(menu).getAllByRole("menuitemradio").map((i) => i.textContent)).toEqual([
    "Parametric",
    "Insert",
    "View",
    "Manage",
    "Output",
  ]);
  fireEvent.click(within(menu).getByRole("menuitemradio", { name: "View" }));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(tabs()).toEqual(["Measure", "Draw", "Annotate", "View"]);
  const view = screen.getByRole("tab", { name: "View" });
  expect(view.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("group", { name: "Viewports" })).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Measure" }).getAttribute("aria-selected")).toBe("false");
  // The menu marks the one on show.
  fireEvent.click(view);
  expect(screen.getByRole("menuitemradio", { name: "View" }).getAttribute("aria-checked")).toBe("true");
  expect(screen.getByRole("menuitemradio", { name: "Insert" }).getAttribute("aria-checked")).toBe("false");
});

test("the More menu is reached by the keyboard: ArrowDown opens it, the arrows move, Escape closes it and gives the focus back", async () => {
  render(<Ribbon {...props} />);
  const more = screen.getByRole("tab", { name: "More" });
  act(() => more.focus());
  fireEvent.keyDown(more, { key: "ArrowDown" });
  await settle();
  const items = screen.getAllByRole("menuitemradio");
  expect(document.activeElement).toBe(items[0]);
  fireEvent.keyDown(items[0], { key: "ArrowDown" });
  await settle();
  expect(document.activeElement).toBe(items[1]);
  fireEvent.keyDown(items[1], { key: "End" });
  await settle();
  expect(document.activeElement).toBe(items[4]);
  fireEvent.keyDown(items[4], { key: "ArrowDown" });
  await settle();
  expect(document.activeElement).toBe(items[0]);
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  await settle();
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(more);
});

test("a click outside closes a flyout", () => {
  render(<Ribbon {...props} />);
  fireEvent.click(screen.getByRole("tab", { name: "More" }));
  expect(screen.getByRole("menu")).toBeTruthy();
  fireEvent.mouseDown(document.body);
  expect(screen.queryByRole("menu")).toBeNull();
});

test("a tab layout with four tabs or fewer shows them all, with no More", () => {
  render(
    <Ribbon
      {...props}
      layout={{
        tabs: ["a", "b", "c", "d"].map((id) => ({
          id,
          label: id,
          groups: [{ id: "g", label: "g", buttons: [{ kind: "tool", command: "line", label: "draw.line" }] }],
        })),
      }}
    />,
  );
  expect(screen.getAllByRole("tab")).toHaveLength(4);
  expect(screen.queryByRole("tab", { name: "More" })).toBeNull();
});

test("Measure's Dimensions shows its first five; the rest, and the style, are behind its ⋯ with their icons and names", () => {
  const onTool = vi.fn();
  const onDimStyle = vi.fn();
  render(<Ribbon {...props} onTool={onTool} onDimStyle={onDimStyle} />);
  const dims = screen.getByRole("group", { name: "Dimensions" });
  expect(within(dims).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Linear",
    "Aligned",
    "Angular",
    "Radius",
    "Diameter",
    "More in Dimensions",
  ]);
  expect(within(dims).queryByRole("button", { name: "Arc length" })).toBeNull();
  const more = within(dims).getByRole("button", { name: "More in Dimensions" });
  expect(more.getAttribute("aria-haspopup")).toBe("menu");
  fireEvent.click(more);
  expect(more.getAttribute("aria-expanded")).toBe("true");
  const menu = screen.getByRole("menu", { name: "Dimensions" });
  const items = Array.from(menu.querySelectorAll<HTMLElement>('[role^="menuitem"]'));
  expect(items.map((i) => i.textContent)).toEqual([
    "Arc length",
    "Ordinate",
    "Baseline",
    "Continue",
    "Quick",
    "Edit dimension",
    "Break",
    "Space",
    "Jog line",
    "Dimension style…",
  ]);
  for (const i of items) expect(i.querySelector("svg")).toBeTruthy();
  fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: "Arc length" }));
  expect(onTool).toHaveBeenLastCalledWith("dimArc");
  expect(screen.queryByRole("menu")).toBeNull();
  // The style is an action.
  fireEvent.click(more);
  fireEvent.click(screen.getByRole("menuitem", { name: "Dimension style…" }));
  expect(onDimStyle).toHaveBeenCalled();
  // Disabled without a handler, as on the bar.
  fireEvent.click(more);
  expect(screen.getByRole("menuitem", { name: "Dimension style…" }).getAttribute("aria-disabled")).toBeNull();
});

test("a flyout's tool shows pressed, and Escape closes it back onto its ⋯", async () => {
  render(<Ribbon {...props} active="dimArc" />);
  const more = screen.getByRole("button", { name: "More in Dimensions" });
  act(() => more.focus());
  fireEvent.keyDown(more, { key: "ArrowDown" });
  await settle();
  expect(screen.getByRole("menuitemcheckbox", { name: "Arc length" }).getAttribute("aria-checked")).toBe("true");
  expect(document.activeElement).toBe(screen.getAllByRole("menuitemcheckbox")[0]);
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  await settle();
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(more);
});

test("Annotate's Dimensions shows all of them with no ⋯, and Markup is a group of its own to add to", () => {
  render(<Ribbon {...props} />);
  fireEvent.click(screen.getByRole("tab", { name: "Annotate" }));
  const dims = screen.getByRole("group", { name: "Dimensions" });
  expect(within(dims).getAllByRole("button")).toHaveLength(15);
  expect(within(dims).queryByRole("button", { name: /More in/ })).toBeNull();
  expect(screen.getAllByRole("group").map((g) => g.getAttribute("aria-label"))).toEqual(["Dimensions", "Leaders", "Markup", "Tables"]);
  expect(within(screen.getByRole("group", { name: "Markup" })).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Revision cloud",
    "Comment cloud",
  ]);
});

/** A panel as wide as `room` buttons of 100px, in a window with no layout of its own. */
function withRoom(room: number, body: () => void) {
  const scroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollWidth");
  const client = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains("ribbon-panel") ? this.querySelectorAll(".rb").length * 100 : 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains("ribbon-panel") ? room * 100 : 0;
    },
  });
  try {
    body();
  } finally {
    if (scroll) Object.defineProperty(HTMLElement.prototype, "scrollWidth", scroll);
    else delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollWidth;
    if (client) Object.defineProperty(HTMLElement.prototype, "clientWidth", client);
    else delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
  }
}

const panelButtons = () => document.querySelectorAll(".ribbon-panel .rb").length;

test("as the panel narrows, groups put more buttons behind their ⋯ until it fits, with nothing lost", () => {
  // Measure: 4 + 5 + 4 + (5 and the ⋯) + 4 = 23 buttons.
  withRoom(100, () => {
    render(<Ribbon {...props} />);
    expect(panelButtons()).toBe(23);
    expect(screen.queryByRole("button", { name: "More in Inquiry" })).toBeNull();
  });
  cleanup();
  withRoom(15, () => {
    render(<Ribbon {...props} />);
    expect(panelButtons()).toBeLessThanOrEqual(15);
    // Every command is still reachable: on show, or in a flyout.
    const reach = new Set<string>();
    for (const b of document.querySelectorAll(".ribbon-panel .rb")) {
      const name = b.getAttribute("aria-label")!;
      if (name.startsWith("More in ")) {
        fireEvent.click(b);
        for (const i of document.querySelectorAll('[role^="menuitem"]')) reach.add(i.textContent!);
        fireEvent.keyDown(document.body, { key: "Escape" });
      } else reach.add(name);
    }
    for (const name of ["Select", "Distance", "Area", "Count", "Geometry", "Divide", "Measure along", "ID point", "Extract data", "Export take-off", "Import BOQ", "Compare", "Export conflicts", "Linear", "Dimension style…", "Revision cloud", "Comment cloud", "Leader", "MText"]) {
      expect(reach.has(name), name).toBe(true);
    }
  });
  cleanup();
  // The window widens again: the groups show what they want to show.
  withRoom(100, () => {
    render(<Ribbon {...props} />);
    expect(panelButtons()).toBe(23);
  });
});

test("a panel too narrow even for that keeps its scrollbar rather than losing buttons", () => {
  withRoom(2, () => {
    render(<Ribbon {...props} />);
    // Every group is down to its ⋯ (or its one button).
    expect(panelButtons()).toBeLessThanOrEqual(6);
    expect(screen.getAllByRole("group")).toHaveLength(5);
  });
});

test("the views menu stays on the bar however narrow it is, and a group's ⋯ never holds it", () => {
  withRoom(3, () => {
    render(
      <Ribbon
        {...props}
        loadViews={async () => []}
        onGoToView={() => {}}
        onSaveView={async () => {}}
        onDeleteView={async () => {}}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "View" }));
    expect(screen.getByRole("button", { name: "Views" }).getAttribute("aria-haspopup")).toBe("menu");
  });
});

test("the toggles are icon buttons with a pressed state", () => {
  const onGroupSelect = vi.fn();
  render(<Ribbon {...props} groupSelect onGroupSelect={onGroupSelect} />);
  fireEvent.click(screen.getByRole("tab", { name: "Draw" }));
  const toggle = screen.getByRole("button", { name: "Group selection" });
  expect(toggle.getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(toggle);
  expect(onGroupSelect).toHaveBeenCalledWith(false);
});

test("Space never presses a focused ribbon button", () => {
  const onTool = vi.fn();
  render(<Ribbon {...props} onTool={onTool} />);
  const select = screen.getByRole("button", { name: "Select" });
  const down = fireEvent.keyDown(select, { key: " " });
  const up = fireEvent.keyUp(select, { key: " " });
  expect(down).toBe(false);
  expect(up).toBe(false);
});

test("Escape that closes a flyout does not also reach the window's Escape", async () => {
  const onWindowKey = vi.fn();
  window.addEventListener("keydown", onWindowKey);
  try {
    render(<Ribbon {...props} />);
    const more = screen.getByRole("tab", { name: "More" });
    act(() => more.focus());
    fireEvent.keyDown(more, { key: "ArrowDown" });
    await settle();
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await settle();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(onWindowKey).not.toHaveBeenCalledWith(expect.objectContaining({ key: "Escape" }));
  } finally {
    window.removeEventListener("keydown", onWindowKey);
  }
});
