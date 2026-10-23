// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { CanvasMenu } from "./CanvasMenu";

afterEach(cleanup);

function setup(more: Partial<Parameters<typeof CanvasMenu>[0]> = {}) {
  const props = {
    hasDrawing: true,
    onTool: vi.fn(),
    onProperties: vi.fn(),
    onZoomExtents: vi.fn(),
    ...more,
  };
  render(
    <CanvasMenu box={<div className="viewport" tabIndex={0} />} {...props}>
      <canvas />
    </CanvasMenu>,
  );
  return props;
}

const rightClick = () => fireEvent.contextMenu(document.querySelector("canvas")!, { clientX: 40, clientY: 50 });

test("a right-click on the drawing opens a menu with Move, Copy, Erase, Properties, Zoom extents and Ask Mizan", async () => {
  setup();
  expect(screen.queryByRole("menu")).toBeNull();
  rightClick();
  const menu = await screen.findByRole("menu", { name: "Drawing commands" });
  expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((i) => i.querySelector("span")!.textContent)).toEqual([
    "Move",
    "Copy",
    "Erase",
    "Properties",
    "Zoom extents",
    "Ask Mizan about this",
  ]);
  // The command line alias is a small chip: M, CO, E, and Z E.
  expect(Array.from(menu.querySelectorAll("kbd")).map((k) => k.textContent)).toEqual(["M", "CO", "E", "Z E"]);
});

test("a row runs its command and closes the menu", async () => {
  const props = setup();
  for (const [name, check] of [
    ["Erase", () => expect(props.onTool).toHaveBeenLastCalledWith("erase")],
    ["Move", () => expect(props.onTool).toHaveBeenLastCalledWith("move")],
    ["Copy", () => expect(props.onTool).toHaveBeenLastCalledWith("copy")],
    ["Properties", () => expect(props.onProperties).toHaveBeenCalledTimes(1)],
    ["Zoom extents", () => expect(props.onZoomExtents).toHaveBeenCalledTimes(1)],
  ] as const) {
    rightClick();
    fireEvent.click(await screen.findByRole("menuitem", { name: new RegExp(name) }));
    check();
    await act(async () => void (await new Promise((r) => setTimeout(r, 30))));
    expect(screen.queryByRole("menu")).toBeNull();
  }
});

test("Escape closes it, and without a drawing the modify rows and Properties are disabled", async () => {
  const props = setup({ hasDrawing: false });
  rightClick();
  const erase = await screen.findByRole("menuitem", { name: /Erase/ });
  expect(erase.getAttribute("aria-disabled")).toBe("true");
  expect(screen.getByRole("menuitem", { name: "Properties" }).getAttribute("aria-disabled")).toBe("true");
  expect(screen.getByRole("menuitem", { name: /Zoom extents/ }).getAttribute("aria-disabled")).toBeNull();
  fireEvent.click(erase);
  expect(props.onTool).not.toHaveBeenCalled();
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  await act(async () => void (await new Promise((r) => setTimeout(r, 30))));
  expect(screen.queryByRole("menu")).toBeNull();
});

test("the user's own aliases are the chips", async () => {
  setup({ aliases: { MV: "MOVE", M: "MOVE", CP: "COPY" } });
  rightClick();
  const menu = await screen.findByRole("menu");
  expect(menu.querySelectorAll("kbd")[0].textContent).toBe("M");
  expect(menu.querySelectorAll("kbd")[1].textContent).toBe("CP");
});

test("Ask Mizan takes the selection to Mizan, and waits for something selected", async () => {
  setup();
  rightClick();
  const ask = await screen.findByRole("menuitem", { name: "Ask Mizan about this" });
  expect(ask.getAttribute("aria-disabled")).toBe("true");
  cleanup();

  const onAskMizan = vi.fn();
  setup({ onAskMizan });
  rightClick();
  fireEvent.click(await screen.findByRole("menuitem", { name: "Ask Mizan about this" }));
  expect(onAskMizan).toHaveBeenCalledTimes(1);
});
