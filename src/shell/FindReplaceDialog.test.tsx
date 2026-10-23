// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { TextMatch } from "../api";
import { FindReplaceDialog, type FindQuery } from "./FindReplaceDialog";

beforeAll(() => {
  // jsdom may not implement dialogs.
  HTMLDialogElement.prototype.show ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(cleanup);

const matches: TextMatch[] = [
  { handle: 10, sheet: "*Model_Space" },
  { handle: 11, sheet: "*Model_Space" },
  { handle: 12, sheet: "*Paper_Space" },
];

function setup() {
  const onFind = vi.fn(async (_q: FindQuery) => ({ count: 4, matches }));
  const onReplaceAll = vi.fn(async (_q: FindQuery, _r: string) => ({ count: 4, skipped: 0 }));
  const onShow = vi.fn();
  const onClose = vi.fn();
  render(<FindReplaceDialog onFind={onFind} onReplaceAll={onReplaceAll} onShow={onShow} onClose={onClose} />);
  return { onFind, onReplaceAll, onShow, onClose };
}

test("find next asks once, then shows each match in turn and round again", async () => {
  const { onFind, onShow } = setup();
  const next = screen.getByRole("button", { name: "Find next" });
  expect((next as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("Find"), { target: { value: "door" } });
  fireEvent.click(screen.getByLabelText("Whole words only"));
  fireEvent.change(screen.getByLabelText("Look in"), { target: { value: "drawing" } });
  fireEvent.click(next);
  await waitFor(() => expect(onShow).toHaveBeenCalledTimes(1));
  expect(onFind).toHaveBeenCalledWith({ find: "door", scope: "drawing", match_case: false, whole_word: true });
  expect(onShow).toHaveBeenLastCalledWith(matches[0]);
  expect(screen.getByRole("status").textContent).toBe("1 of 3 texts · 4 matches");
  fireEvent.click(next);
  await waitFor(() => expect(onShow).toHaveBeenCalledTimes(2));
  fireEvent.click(next);
  await waitFor(() => expect(onShow).toHaveBeenCalledTimes(3));
  fireEvent.click(next);
  await waitFor(() => expect(onShow).toHaveBeenCalledTimes(4));
  expect(onShow).toHaveBeenLastCalledWith(matches[0]);
  expect(onFind).toHaveBeenCalledTimes(1);

  // A changed query finds again.
  fireEvent.click(screen.getByLabelText("Match case"));
  fireEvent.click(next);
  await waitFor(() => expect(onFind).toHaveBeenCalledTimes(2));
  expect(onFind).toHaveBeenLastCalledWith({ find: "door", scope: "drawing", match_case: true, whole_word: true });
});

test("replace all says how many it replaced", async () => {
  const { onReplaceAll } = setup();
  fireEvent.change(screen.getByLabelText("Find"), { target: { value: "باب" } });
  fireEvent.change(screen.getByLabelText("Replace with"), { target: { value: "مدخل" } });
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Replaced 4"));
  expect(onReplaceAll).toHaveBeenCalledWith(
    { find: "باب", scope: "sheet", match_case: false, whole_word: false },
    "مدخل",
  );
});

test("replace all says how many texts it left as they were", async () => {
  const onReplaceAll = vi.fn(async () => ({ count: 2, skipped: 1 }));
  render(<FindReplaceDialog onFind={vi.fn()} onReplaceAll={onReplaceAll} onShow={vi.fn()} onClose={() => {}} />);
  fireEvent.change(screen.getByLabelText("Find"), { target: { value: "DOOR" } });
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(
      "Replaced 2 · 1 left as they were: a text can't be blank or break onto two lines",
    ),
  );
});

test("nothing found says so; the inputs read either way", async () => {
  const onFind = vi.fn(async () => ({ count: 0, matches: [] }));
  const onShow = vi.fn();
  render(<FindReplaceDialog onFind={onFind} onReplaceAll={vi.fn()} onShow={onShow} onClose={() => {}} />);
  expect(screen.getByLabelText("Find").getAttribute("dir")).toBe("auto");
  expect(screen.getByLabelText("Replace with").getAttribute("dir")).toBe("auto");
  fireEvent.change(screen.getByLabelText("Find"), { target: { value: "x" } });
  fireEvent.click(screen.getByRole("button", { name: "Find next" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("No matches"));
  expect(onShow).not.toHaveBeenCalled();
});

test("an error stays in the dialog and Close closes it", async () => {
  const onClose = vi.fn();
  const onFind = vi.fn(async () => {
    throw new Error("No drawing");
  });
  render(<FindReplaceDialog onFind={onFind} onReplaceAll={vi.fn()} onShow={vi.fn()} onClose={onClose} />);
  fireEvent.change(screen.getByLabelText("Find"), { target: { value: "x" } });
  fireEvent.click(screen.getByRole("button", { name: "Find next" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Open a drawing first."));
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalled();
});
