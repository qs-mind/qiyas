// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { OutputsMenu } from "./OutputsMenu";
import { outputGroups, outputList, type OutputInfo } from "./outputs";
import { openOutputFile, revealOutputFile } from "./outputFileActions";
import type { Sheet } from "../api";
import en from "../i18n/en.json";

vi.mock("./outputFileActions", () => ({ openOutputFile: vi.fn().mockResolvedValue(undefined), revealOutputFile: vi.fn().mockResolvedValue(undefined) }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

const outputs: OutputInfo[] = [{ id: "boq", count: 4 }, { id: "takeoff", count: 0 }, { id: "conflicts", count: 0 }, { id: "review", count: null }];
const sheets: Sheet[] = [{ block: "model", name: "Model", is_model: true }, { block: "paper1", name: "Ground floor", is_model: false }];
const file = { path: new URL("../../samples/plan.pdf", import.meta.url).pathname, name: "plan.pdf" };

test("groups paper sheets, written files and populated take-off outputs; counts items, not rows", () => {
  const groups = outputGroups(outputs, sheets, [file]);
  expect(groups).toEqual({ sheets: [sheets[1]], files: [file], takeoff: [outputs[0]], count: 3 });
  expect(outputGroups(outputs, [], []).count).toBe(1);
  expect(outputGroups([], [], []).count).toBe(0);
});

test("the report is available once there are measured items", () => {
  const state = { items: [], bill: null, generated: null, conflicts: [] };
  expect(outputGroups(outputList(state), [], []).count).toBe(0);
  expect(outputGroups(outputList({ ...state, items: [{}] }), [], []).takeoff.map((o) => o.id)).toEqual(["takeoff", "review"]);
  const generated = outputGroups(outputList({ ...state, generated: { items: [{}] } }), [], []);
  expect(generated.takeoff.map((o) => o.id)).toEqual(["boq", "review"]);
  expect(generated.takeoff.find((o) => o.id === "review")?.count).toBeNull();
});

test("the menu labels its groups and count, opens a sheet, and hides empty outputs", async () => {
  const onSheet = vi.fn();
  render(<OutputsMenu outputs={outputs} sheets={sheets} files={[file]} shown={null} onOpen={vi.fn()} onSheet={onSheet} />);
  const trigger = screen.getByRole("button", { name: en["outputs.countLabel"].replace("{n}", "3") });
  expect(trigger.querySelector(".artifacts-count")?.textContent).toBe("3");
  fireEvent.click(trigger);
  const menu = await screen.findByRole("menu");
  expect(within(menu).getAllByRole("group")).toEqual([
    within(menu).getByRole("group", { name: en["outputs.sheets"] }),
    within(menu).getByRole("group", { name: en["outputs.files"] }),
    within(menu).getByRole("group", { name: en["outputs.takeoffSection"] }),
  ]);
  expect(within(menu).queryByText("Model")).toBeNull();
  expect(within(menu).queryByText(en["outputs.empty"])).toBeNull();
  expect(within(menu).queryByText(en["outputs.conflicts"])).toBeNull();
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Ground floor" }));
  expect(onSheet).toHaveBeenCalledWith("paper1");
});

test("files open in the viewer and have a separate Show in folder action", async () => {
  render(<OutputsMenu outputs={[]} files={[file]} shown={null} onOpen={vi.fn()} />);
  fireEvent.click(screen.getByRole("button"));
  fireEvent.click(await screen.findByRole("menuitem", { name: "plan.pdf" }));
  expect(openOutputFile).toHaveBeenCalledWith(file.path);
  fireEvent.click(screen.getByRole("menuitem", { name: en["outputs.revealFile"].replace("{name}", file.name) }));
  expect(revealOutputFile).toHaveBeenCalledWith(file.path);
  expect(openOutputFile).toHaveBeenCalledTimes(1);
});

test("file action failures are explained in the menu", async () => {
  vi.mocked(openOutputFile).mockRejectedValueOnce(new Error("missing"));
  render(<OutputsMenu outputs={[]} files={[file]} shown={null} onOpen={vi.fn()} />);
  fireEvent.click(screen.getByRole("button"));
  fireEvent.click(await screen.findByRole("menuitem", { name: "plan.pdf" }));
  expect((await screen.findByRole("alert")).textContent).toBe(en["outputs.fileActionFailed"]);
});
