// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AssistantPanel } from "../ai/AssistantPanel";
import { EMPTY } from "../ai/messages";
import { LanguageProvider } from "../i18n";
import en from "../i18n/en.json";
import { ArtifactFrame } from "./ArtifactFrame";
import { filledOutputs, type OutputInfo } from "./outputs";

vi.mock("../api", async (original) => ({ ...(await original<object>()), aiLog: vi.fn(), aiAttachPdf: vi.fn() }));
afterEach(cleanup);

const outputs: OutputInfo[] = [
  { id: "takeoff", count: 3 },
  { id: "boq", count: 12 },
  { id: "conflicts", count: 0 },
  { id: "review", count: null },
];

test("filledOutputs counts the outputs that hold something", () => {
  expect(filledOutputs(outputs)).toBe(2);
});

test("an output has a way back, a menu to the others, its meta and one action", async () => {
  const onBack = vi.fn();
  const onSelect = vi.fn();
  const onClick = vi.fn();
  render(
    <LanguageProvider>
      <ArtifactFrame id="boq" outputs={outputs} meta="plan.dwg · 12 items" action={{ label: "Export", onClick }} onSelect={onSelect} onBack={onBack}>
        <p>the table</p>
      </ArtifactFrame>
    </LanguageProvider>,
  );
  expect(screen.getByRole("region", { name: en["outputs.boq"] })).toBeTruthy();
  expect(screen.getByText("the table")).toBeTruthy();
  expect(screen.getByText("plan.dwg · 12 items")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Export" }));
  expect(onClick).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: en["outputs.back"] }));
  fireEvent.click(screen.getByRole("button", { name: en["outputs.close"] }));
  expect(onBack).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("button", { name: en["outputs.switch"] }));
  fireEvent.click(await screen.findByRole("menuitemradio", { name: /^Conflicts/ }));
  expect(onSelect).toHaveBeenCalledWith("conflicts");
});

test("while an output is shown it takes the conversation's place, the box stays with its draft, and the strip goes back", () => {
  const onBack = vi.fn();
  const assistant = { transcript: EMPTY, busy: false, error: null, send: vi.fn().mockResolvedValue(true), cancel: vi.fn(), clear: vi.fn() };
  const view = (shown: "boq" | null) => (
    <LanguageProvider>
      <AssistantPanel assistant={assistant} onShow={() => {}} onUndo={() => {}} onSettings={() => {}}
        artifact={{ shown, node: <p data-testid="out">output</p>, onBack }} />
    </LanguageProvider>
  );
  const { rerender } = render(view(null));
  fireEvent.change(screen.getByLabelText(en["ai.placeholder"]), { target: { value: "half a thought" } });
  expect(screen.getByTestId("out").closest("[hidden]")).not.toBeNull();
  rerender(view("boq"));
  expect(screen.getByTestId("out").closest("[hidden]")).toBeNull();
  expect(document.querySelector(".chat-slot")?.hasAttribute("hidden")).toBe(true);
  const box = screen.getByLabelText(en["outputs.ask.boq"]) as HTMLTextAreaElement;
  expect(box.value).toBe("half a thought");
  fireEvent.click(document.querySelector(".back-strip")!);
  expect(onBack).toHaveBeenCalledOnce();
  rerender(view(null));
  expect((screen.getByLabelText(en["ai.placeholder"]) as HTMLTextAreaElement).value).toBe("half a thought");
});
