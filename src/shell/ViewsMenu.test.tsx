// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { NamedView } from "../api";
import { ViewsMenu } from "./ViewsMenu";

afterEach(cleanup);

const hall: NamedView = { name: "Hall", bounds: [[0, 0], [4, 3]] };

test("choosing a view shows it", async () => {
  const onGo = vi.fn();
  render(
    <ViewsMenu load={async () => [hall]} onGo={onGo} onSave={async () => {}} onDelete={async () => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /Views/ }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Hall" }));
  expect(onGo).toHaveBeenCalledWith(hall);
  expect(screen.queryByRole("menu")).toBeNull();
});

test("Save current view… asks for a name, then saves", async () => {
  const onSave = vi.fn(async () => {});
  render(
    <ViewsMenu load={async () => []} onGo={() => {}} onSave={onSave} onDelete={async () => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /Views/ }));
  expect(await screen.findByText("No named views")).toBeTruthy();
  fireEvent.click(screen.getByRole("menuitem", { name: "Save current view…" }));
  const save = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("View name"), { target: { value: " Kitchen " } });
  fireEvent.click(save);
  expect(onSave).toHaveBeenCalledWith("Kitchen");
  await vi.waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
});

test("a refused save shows why and stays open", async () => {
  const onSave = vi.fn(async () => {
    throw "Named views are saved on the Model sheet";
  });
  render(
    <ViewsMenu load={async () => []} onGo={() => {}} onSave={onSave} onDelete={async () => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /Views/ }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Save current view…" }));
  fireEvent.change(screen.getByLabelText("View name"), { target: { value: "A" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect((await screen.findByRole("alert")).textContent).toContain("The file could not be written. Check the folder and available space.");
  expect(screen.getByRole("menu")).toBeTruthy();
});

test("× deletes a view and the list is read again", async () => {
  let views = [hall];
  const onDelete = vi.fn(async (name: string) => {
    views = views.filter((v) => v.name !== name);
  });
  render(
    <ViewsMenu load={async () => views} onGo={() => {}} onSave={async () => {}} onDelete={onDelete} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /Views/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Delete Hall" }));
  expect(onDelete).toHaveBeenCalledWith("Hall");
  expect(await screen.findByText("No named views")).toBeTruthy();
});

test("the menu is placed on the window under the button, so the ribbon's scrolling panel never clips it", async () => {
  const rect = { top: 40, bottom: 64, left: 300, right: 380, width: 80, height: 24, x: 300, y: 40 };
  const spy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ ...rect, toJSON() {} });
  try {
    for (const dir of ["ltr", "rtl"]) {
      document.documentElement.dir = dir;
      render(<ViewsMenu load={async () => []} onGo={() => {}} onSave={async () => {}} onDelete={async () => {}} />);
      fireEvent.click(screen.getByRole("button", { name: /Views/ }));
      const menu = await screen.findByRole("menu");
      expect(menu.style.position).toBe("fixed");
      expect(menu.style.top).toBe("64px");
      // Its start edge under the button's: the left in English, the right in Arabic.
      if (dir === "ltr") expect(menu.style.left).toBe("300px");
      else expect(menu.style.right).toBe(`${window.innerWidth - 380}px`);
      cleanup();
    }
  } finally {
    spy.mockRestore();
    document.documentElement.dir = "ltr";
  }
});
