// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { RibbonLayout, Settings } from "../api";
import { CustomizeUi } from "./CustomizeUi";
import fixture from "./ribbon.default.json";

/** The core's default, cut down: Home (File: Open, Save; History: Undo) and Manage (Run script). */
const CORE: RibbonLayout = {
  tabs: [
    {
      id: "home",
      label: "home",
      groups: [
        {
          id: "file",
          label: "ribbon.file",
          buttons: [
            { kind: "action", command: "open", label: "open" },
            { kind: "action", command: "save", label: "save" },
          ],
        },
        { id: "history", label: "ribbon.history", buttons: [{ kind: "action", command: "undo", label: "undo" }] },
      ],
    },
    {
      id: "manage",
      label: "manage",
      groups: [{ id: "scripts", label: "ribbon.scripts", buttons: [{ kind: "action", command: "runScript", label: "script.run" }] }],
    },
  ],
};

const settings = {
  provider: "anthropic",
  providers: { anthropic: { model: "claude-opus-5-5" }, openai_compatible: { model: "", base_url: null } },
  effort: "high",
  mcp_enabled: false,
  mcp_port: 48123,
  mcp_token: "0123456789abcdef0123456789abcdef",
  language: "en",
  theme: "system",
  aliases: {},
} satisfies Settings;

const api = vi.hoisted(() => ({ getSettings: vi.fn(), setSettings: vi.fn(), getDefaultRibbon: vi.fn() }));
vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function setup(ribbon: RibbonLayout | null) {
  api.getSettings.mockResolvedValue({ ...settings, ribbon });
  api.getDefaultRibbon.mockResolvedValue(CORE);
  api.setSettings.mockResolvedValue(undefined);
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(<CustomizeUi onSaved={onSaved} onClose={onClose} />);
  await screen.findByRole("tree");
  return { onSaved, onClose };
}

const save = () => fireEvent.click(screen.getByRole("button", { name: "Save" }));
const saved = () => api.setSettings.mock.calls[0][0].ribbon as RibbonLayout | null;

test("the ribbon as a tree: hide a button and a group, move a tab and a button, save the layout", async () => {
  const { onSaved, onClose } = await setup(CORE);
  const tree = screen.getByRole("tree");
  expect(within(tree).getAllByRole("treeitem", { name: /./ }).length).toBeGreaterThan(0);
  fireEvent.click(screen.getByRole("checkbox", { name: "Show Open…" }));
  fireEvent.click(screen.getByRole("button", { name: "Move Save up" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Show History" }));
  fireEvent.click(screen.getByRole("button", { name: "Move Manage up" }));
  save();
  await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
  const layout: RibbonLayout = {
    tabs: [
      CORE.tabs[1],
      { id: "home", label: "home", groups: [{ id: "file", label: "ribbon.file", buttons: [{ kind: "action", command: "save", label: "save" }] }] },
    ],
  };
  expect(saved()).toEqual(layout);
  expect(onSaved).toHaveBeenCalledWith(layout);
});

test("what the saved layout left out is listed hidden and can be shown again", async () => {
  await setup({ tabs: [CORE.tabs[0]] });
  const manage = screen.getByRole("checkbox", { name: "Show Manage" }) as HTMLInputElement;
  expect(manage.checked).toBe(false);
  fireEvent.click(manage);
  save();
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  expect(saved()).toEqual(CORE);
});

test("Reset to default saves no layout of the user's; hiding every tab is refused", async () => {
  const { onSaved } = await setup({ tabs: [CORE.tabs[1]] });
  fireEvent.click(screen.getByRole("checkbox", { name: "Show Manage" }));
  save();
  expect((await screen.findByRole("alert")).textContent).toBe("Keep at least one tab with a button on it");
  expect(api.setSettings).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));
  // The window's default: Measure, Draw (the tab with id "home"), Annotate...
  expect((screen.getAllByRole("checkbox", { name: "Show Draw" })[0] as HTMLInputElement).checked).toBe(true);
  save();
  await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalled());
  expect(saved()).toBeNull();
  expect(onSaved).toHaveBeenCalledWith(null);
});

test("the window's own buttons the core can't keep in a layout are named", async () => {
  await setup(null);
  // Options, Customize and Aliases are on the window's default but not the core's.
  expect(screen.getByText(/Options….*Customize….*Aliases…/)).toBeTruthy();
});

test("with the shipped default it shows the new groups: Draw's Edit and Annotation, Measure's Take-off, Dimensions and Markup, no File", async () => {
  api.getSettings.mockResolvedValue({ ...settings, ribbon: null });
  api.getDefaultRibbon.mockResolvedValue(fixture);
  render(<CustomizeUi onSaved={() => {}} onClose={() => {}} />);
  await screen.findByRole("tree");
  for (const name of ["Draw", "Modify", "Edit", "Annotation", "Clipboard", "Layers", "Take-off", "Markup"]) {
    expect(screen.getAllByRole("checkbox", { name: `Show ${name}` }).length).toBeGreaterThan(0);
  }
  expect(screen.queryByRole("checkbox", { name: "Show File" })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: "Show History" })).toBeNull();
});
