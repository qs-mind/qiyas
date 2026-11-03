// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Recent } from "../api";
import { RecentsMenu } from "./RecentsMenu";

afterEach(cleanup);

const list: Recent[] = [
  { path: "/plans/terminal.dwg", name: "terminal.dwg", folder: "/plans", missing: false },
  { path: "/old/gone.dxf", name: "gone.dxf", folder: "/old", missing: true },
];

test("Open… opens the dialog; the arrow lists the recent drawings", async () => {
  const onOpen = vi.fn();
  const onOpenRecent = vi.fn();
  render(
    <RecentsMenu
      onOpen={onOpen}
      load={async () => list}
      onOpenRecent={onOpenRecent}
      onClear={async () => {}}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Open…" }));
  expect(onOpen).toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Recent drawings" }));
  const terminal = await screen.findByRole("menuitem", { name: /terminal\.dwg/ });
  expect(terminal.textContent).toContain("/plans");
  const gone = screen.getByRole("menuitem", { name: /gone\.dxf/ }) as HTMLButtonElement;
  expect(gone.textContent).toContain("(missing)");
  expect(gone.disabled).toBe(true);

  fireEvent.click(terminal);
  expect(onOpenRecent).toHaveBeenCalledWith("/plans/terminal.dwg");
  expect(screen.queryByRole("menu")).toBeNull();
});

test("Clear recents clears the list and closes the menu", async () => {
  const onClear = vi.fn(async () => {});
  render(
    <RecentsMenu onOpen={() => {}} load={async () => list} onOpenRecent={() => {}} onClear={onClear} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Recent drawings" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Clear recents" }));
  expect(onClear).toHaveBeenCalled();
  await vi.waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
});

test("an empty list says so", async () => {
  render(
    <RecentsMenu onOpen={() => {}} load={async () => []} onOpenRecent={() => {}} onClear={async () => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Recent drawings" }));
  expect(await screen.findByText("No recent drawings")).toBeTruthy();
  expect(screen.queryByRole("menuitem", { name: "Clear recents" })).toBeNull();
});

test("the menu is placed on the window, so the scrolling tabs' row does not clip it", async () => {
  render(<RecentsMenu onOpen={() => {}} load={async () => list} onOpenRecent={() => {}} onClear={async () => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Recent drawings" }));
  const menu = await screen.findByRole("menu");
  expect(menu.style.position).toBe("fixed");
});
