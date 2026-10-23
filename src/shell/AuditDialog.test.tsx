// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { AuditReport, Command, Outcome } from "../api";
import { AuditDialog } from "./AuditDialog";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

const found: AuditReport = {
  fix: false,
  counts: { missing_layer: 3, empty_block: 1 },
  repaired: 0,
  items: [
    { kind: "missing_layer", name: "جدار", count: 3, handles: [1, 2, 3], line: "3 entities on layer جدار, which the drawing does not have", fixed: false },
    { kind: "empty_block", name: "_None", count: 1, handles: [], line: "Block _None is empty", fixed: false },
  ],
};

const audited = (report: AuditReport): Outcome => ({ type: "audited", report });

test("Audit… lists the counts and items; Fix repairs and says how many", async () => {
  const look = vi.fn(async (_: Command) => audited(found));
  const fixed: AuditReport = { ...found, fix: true, repaired: 3, items: [{ ...found.items[0], fixed: true }, found.items[1]] };
  const apply = vi.fn(async (_: Command) => audited(fixed));
  render(<AuditDialog look={look} apply={apply} onClose={() => {}} />);
  expect(look).toHaveBeenCalledWith({ type: "audit", fix: false });
  const table = await screen.findByRole("table", { name: "Problems found" });
  expect(table.textContent).toContain("Entities on a missing layer");
  expect(table.textContent).toContain("3");
  expect(screen.getByText("جدار").tagName).toBe("BDI");
  // Listed only: no fix for it.
  expect(table.textContent).toContain("Empty block");
  fireEvent.click(screen.getByRole("button", { name: "Fix" }));
  expect(await screen.findByText("Fixed 3 (one undo step)")).toBeTruthy();
  expect(apply).toHaveBeenCalledWith({ type: "audit", fix: true });
  expect(screen.getAllByText("Fixed").length).toBe(1);
  expect(screen.queryByRole("button", { name: "Fix" })).toBeNull();
});

test("a clean drawing says so and offers no fix; a core error is shown", async () => {
  const { unmount } = render(
    <AuditDialog look={async () => audited({ fix: false, counts: {}, repaired: 0, items: [] })} apply={vi.fn()} onClose={() => {}} />,
  );
  expect(await screen.findByText("No problems found")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Fix" })).toBeNull();
  unmount();

  render(
    <AuditDialog
      look={async () => audited(found)}
      apply={async () => {
        throw "No drawing is open";
      }}
      onClose={() => {}}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Fix" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Open a drawing first.");
});

test("only listed problems (an empty block): nothing to fix", async () => {
  render(
    <AuditDialog
      look={async () => audited({ ...found, counts: { empty_block: 1 }, items: [found.items[1]] })}
      apply={vi.fn()}
      onClose={() => {}}
    />,
  );
  await screen.findByRole("table", { name: "Problems found" });
  expect(screen.queryByRole("button", { name: "Fix" })).toBeNull();
});
