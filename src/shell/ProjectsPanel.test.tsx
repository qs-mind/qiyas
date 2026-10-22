// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ProjectsPanel } from "./ProjectsPanel";
import { LanguageProvider } from "../i18n";
const command = vi.fn();
const history = vi.fn();
vi.mock("../api", () => ({ projectCommand: (input: unknown) => command(input), projectHistory: (redo: boolean) => history(redo) }));
vi.mock("../fileDialogs", () => ({ open: vi.fn().mockResolvedValue("/plans/b.dxf") }));
afterEach(() => { cleanup(); vi.resetAllMocks(); localStorage.clear(); });
const project = { path: "/data/projects/Test.qss", set: { name: "Test", project: {
  method: null, boq: [], queries: [], warnings: [], checked: false, register: [], drawings: [],
} }, entries: [{ path: "/plans/a.dxf", full: "/plans/a.dxf", missing: false }] };
function setup() {
  command.mockImplementation(async (input: { kind: string }) => {
    if (input.kind === "list") return { type: "project", result: { projects: [{ name: "Test", path: project.path, drawings: 1 }] } };
    return { type: "project", result: project, ...(input.kind === "run" ? { outputs: { workbook: "/out/project.xlsx", morning_report: "/out/morning-report.md" } } : {}) };
  });
}
test("lists and runs a project without an open drawing, exposing output locations", async () => {
  setup(); render(<ProjectsPanel active />);
  fireEvent.click(await screen.findByRole("button", { name: "Test" }));
  const run = await screen.findByRole("button", { name: "Run take-off" });
  await waitFor(() => expect((run as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(run);
  await screen.findByText("/out/project.xlsx");
  expect(command).toHaveBeenCalledWith({ kind: "run", path: project.path });
  expect(screen.getByText("/out/morning-report.md")).toBeTruthy();
});
test("Arabic project controls and run errors remain visible", async () => {
  setup(); localStorage.setItem("qiyas.lang", "ar");
  render(<LanguageProvider><ProjectsPanel active /></LanguageProvider>);
  fireEvent.click(await screen.findByRole("button", { name: "Test" }));
  const run = await screen.findByRole("button", { name: "تشغيل الحصر" });
  await waitFor(() => expect((run as HTMLButtonElement).disabled).toBe(false));
  command.mockRejectedValueOnce(new Error("Unreadable drawing"));
  fireEvent.click(run);
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Error: Unreadable drawing");
  await waitFor(() => expect((run as HTMLButtonElement).disabled).toBe(false));
});
test("project history refreshes the saved project", async () => {
  setup(); history.mockResolvedValue(true); render(<ProjectsPanel active />);
  fireEvent.click(await screen.findByRole("button", { name: "Test" }));
  await screen.findByRole("button", { name: "Run take-off" });
  await waitFor(() => expect((screen.getByRole("button", { name: "Undo" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  await waitFor(() => expect(history).toHaveBeenCalledWith(false));
  await waitFor(() => expect(command.mock.calls.filter(([c]) => c.kind === "open").length).toBe(2));
});
