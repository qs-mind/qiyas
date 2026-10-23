// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Drafting } from "../api";
import { DraftingSettings } from "./DraftingSettings";
import { StatusBar } from "./StatusBar";

const drafting: Drafting = {
  osnap: true,
  snap_modes: {
    end: true,
    mid: true,
    centre: true,
    node: true,
    intersection: true,
    nearest: false,
    perpendicular: false,
  },
  ortho: false,
  polar: false,
  polar_step_deg: 45,
  grid: false,
  grid_spacing: 1,
  grid_snap: false,
  precision: 3,
};

beforeAll(() => {
  // jsdom may not implement modal dialogs.
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

test("the dialog saves the snap modes, the grid and the precision it was given", async () => {
  const onSave = vi.fn().mockResolvedValue(undefined);
  const onClose = vi.fn();
  render(<DraftingSettings drafting={drafting} onSave={onSave} onClose={onClose} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Nearest" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Midpoint" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Show grid" }));
  fireEvent.change(screen.getByRole("spinbutton", { name: "Spacing (drawing units)" }), {
    target: { value: "0.5" },
  });
  fireEvent.change(screen.getByRole("combobox", { name: "Precision" }), { target: { value: "2" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(onSave).toHaveBeenCalledWith({
    ...drafting,
    snap_modes: { ...drafting.snap_modes, nearest: true, mid: false },
    grid: true,
    grid_spacing: 0.5,
    precision: 2,
  });
});

test("a refused save stays open and says why", async () => {
  const onSave = vi.fn().mockRejectedValue("The grid spacing must be above 0");
  const onClose = vi.fn();
  render(<DraftingSettings drafting={drafting} onSave={onSave} onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Check the values you entered, then try again.",
  );
  expect(onClose).not.toHaveBeenCalled();
});

test("the status bar shows the switches pressed, the snap and the cursor at the precision", () => {
  const onToggle = vi.fn();
  render(
    <StatusBar
      cursor={[1.23456, 2]}
      units="metres"
      unitsWarning={false}
      problems={[]}
      message=""
      precision={2}
      snapLabel="Midpoint"
      drafting={{ ...drafting, ortho: true }}
      onToggle={onToggle}
      onDraftingSettings={() => {}}
    />,
  );
  expect(screen.getByRole("button", { name: "OSNAP" }).getAttribute("aria-pressed")).toBe("true");
  expect(screen.getByRole("button", { name: "ORTHO" }).getAttribute("aria-pressed")).toBe("true");
  expect(screen.getByRole("button", { name: "POLAR" }).getAttribute("aria-pressed")).toBe("false");
  expect(screen.getByRole("button", { name: "GRID" }).getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: "GRID" }));
  expect(onToggle).toHaveBeenCalledWith("grid");
  expect(screen.getByText("Midpoint")).toBeTruthy();
  expect(screen.getByText("1.23, 2.00 m")).toBeTruthy();
});
