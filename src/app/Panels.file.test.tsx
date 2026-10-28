// @vitest-environment jsdom
import { useState, type ComponentProps } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { SidePanel } from "./Panels";
import type { SideTab } from "./useShell";
import { EMPTY } from "../ai/messages";
import { OutputsMenu } from "../shell/OutputsMenu";
import { outputList } from "../shell/outputs";
import type { OutputFile } from "../shell/outputFiles";
import { openOutputFile } from "../shell/outputFileActions";
import en from "../i18n/en.json";

vi.mock("../api", async original => ({ ...(await original<object>()), aiLog: vi.fn(),
  fileViewInfo: vi.fn(async (path: string) => ({ kind: "text", name: path.split("/").pop(), size_bytes: 12, pages: null })),
  fileViewBytes: vi.fn(async (path: string) => new TextEncoder().encode(`Contents of ${path}`).buffer),
}));
vi.mock("../shell/outputFileActions", () => ({ openOutputFile: vi.fn(), revealOutputFile: vi.fn() }));
vi.mock("../shell/TakeoffPanel", () => ({ TakeoffPanel: () => <p>Take-off rows</p> }));
vi.mock("../boq/BoqPanel", () => ({ BoqPanel: () => null }));
vi.mock("../boq/ConflictsPanel", () => ({ ConflictsPanel: () => null }));
vi.mock("../shell/RecognitionPanel", () => ({ RecognitionPanel: () => null }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

test("SidePanel shows selected files inside, preserves the draft, switches outputs and returns from back and close", async () => {
  type Props = ComponentProps<typeof SidePanel>;
  const files = [{ path: "/one.txt", name: "one.txt" }, { path: "/two.txt", name: "two.txt" }];
  const state = { items: [{}], itemTotals: [], selectedId: null, bill: null, generated: null, boqCode: null, conflicts: [],
    compared: false, infoCard: null, setInfoCard: vi.fn() } as unknown as Props["takeoffState"];
  const ai = { assistant: { transcript: EMPTY, busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn() },
    showCard: vi.fn(), undoCard: vi.fn(), openSettings: vi.fn() } as unknown as Props["ai"];
  function Window() {
    const [sideTab, setSideTab] = useState<SideTab>("assistant");
    const [file, setFile] = useState<OutputFile | null>(null);
    const openFile = (next: OutputFile) => { setSideTab("assistant"); setFile(next); };
    return <SidePanel info={null} drafting={null} shell={{ sideTab, setSideTab }} takeoffState={state}
      takeoff={{} as Props["takeoff"]} ai={ai} recognition={{ revision: 0, run: vi.fn() }}
      conversationHistory={{ chats: [], drawing: null, latest: null, pending: false, error: null, resume: vi.fn(), remove: vi.fn() }}
      files={files} shownFile={file} onOpenFile={openFile} onCloseFile={() => setFile(null)}
      artifacts={<OutputsMenu outputs={outputList(state)} files={files} shown={null} onOpen={setSideTab} onOpenFile={openFile} />} />;
  }
  render(<Window />);
  fireEvent.change(screen.getByLabelText(en["ai.placeholder"]), { target: { value: "Keep my draft" } });
  fireEvent.click(screen.getByRole("button", { name: en["outputs.countLabel"].replace("{n}", "4") }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "one.txt" }));
  expect(await screen.findByText("Contents of /one.txt")).toBeTruthy();
  expect(openOutputFile).not.toHaveBeenCalled();
  expect((screen.getByLabelText(en["outputs.ask.file"]) as HTMLTextAreaElement).value).toBe("Keep my draft");
  expect(document.querySelector(".back-strip")).not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: en["outputs.switch"] }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "two.txt" }));
  expect(await screen.findByText("Contents of /two.txt")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["outputs.switch"] }));
  fireEvent.click(await screen.findByRole("menuitemradio", { name: /^Take-off(?! report)/ }));
  expect(screen.getByText("Take-off rows").closest("[hidden]")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: en["outputs.switch"] }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "one.txt" }));
  expect(await screen.findByText("Contents of /one.txt")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["outputs.close"] }));
  expect((screen.getByLabelText(en["ai.placeholder"]) as HTMLTextAreaElement).value).toBe("Keep my draft");
  fireEvent.click(screen.getByRole("button", { name: en["outputs.countLabel"].replace("{n}", "4") }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "two.txt" }));
  expect(await screen.findByText("Contents of /two.txt")).toBeTruthy();
  fireEvent.click(document.querySelector(".back-strip")!);
  expect(screen.queryByText("Contents of /two.txt")).toBeNull();
  expect((screen.getByLabelText(en["ai.placeholder"]) as HTMLTextAreaElement).value).toBe("Keep my draft");
});
