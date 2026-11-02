// @vitest-environment jsdom
import type { ComponentProps } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { SidePanel } from "../app/Panels";
import { AssistantPanel } from "./AssistantPanel";
import * as content from "./content";
import { useStickToBottom } from "./useStickToBottom";

vi.mock("../api", async original => ({ ...(await original<object>()), aiLog: vi.fn() }));
// Output tables have their own observers; this test measures the conversation's observer.
vi.mock("../shell/TakeoffPanel", () => ({ TakeoffPanel: () => null }));
vi.mock("../boq/BoqPanel", () => ({ BoqPanel: () => null }));
vi.mock("../boq/ConflictsPanel", () => ({ ConflictsPanel: () => null }));
vi.mock("../shell/RecognitionPanel", () => ({ RecognitionPanel: () => null }));

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function fixture() {
  type Props = ComponentProps<typeof SidePanel>;
  const assistant = {
    transcript: { items: Array.from({ length: 300 }, (_, i) => ({ kind: "user" as const, text: `Tower floor ${i}` })) },
    busy: false, error: null, send: vi.fn(), cancel: vi.fn(), clear: vi.fn(),
  };
  return {
    info: { path: "/campus.dwg" } as Props["info"], drafting: null,
    shell: { sideTab: "assistant", setSideTab: vi.fn() },
    takeoffState: { items: [], itemTotals: [], selectedId: null, bill: null, generated: null, boqCode: null,
      conflicts: [], compared: false, infoCard: null, setInfoCard: vi.fn() } as unknown as Props["takeoffState"],
    takeoff: {} as Props["takeoff"],
    ai: { assistant, showCard: vi.fn(), undoCard: vi.fn(), openSettings: vi.fn() } as unknown as Props["ai"],
    recognition: { revision: 0, run: vi.fn() },
    conversationHistory: { chats: [], drawing: null, latest: null, pending: false, error: null, resume: vi.fn(), remove: vi.fn() },
  } satisfies Props;
}

test("ten equal SidePanel updates keep one conversation ResizeObserver", () => {
  const observer = vi.spyOn(globalThis, "ResizeObserver");
  const props = fixture();
  const view = render(<SidePanel {...props} />);
  for (let i = 0; i < 10; i++) view.rerender(<SidePanel {...props} info={{ ...props.info! }} recognition={{ ...props.recognition }} />);
  expect(observer).toHaveBeenCalledTimes(1);
});

test("panel updates with unchanged messages do not render ThreadItems again", () => {
  // Every real user ThreadItem calls isolateRuns once in its render body.
  const renders = vi.spyOn(content, "isolateRuns");
  const { ai } = fixture();
  const props = { assistant: ai.assistant, onShow: ai.showCard, onUndo: ai.undoCard, onSettings: ai.openSettings };
  const view = render(<AssistantPanel {...props} />);
  expect(renders).toHaveBeenCalledTimes(300);
  renders.mockClear();
  // A changed header forces the panel itself to render, exercising ThreadItem's own memo.
  for (let i = 0; i < 10; i++) view.rerender(<AssistantPanel {...props} artifacts={<span>{i}</span>} />);
  expect(renders).not.toHaveBeenCalled();
});

test("a thread retains its observer and only updates changed child subscriptions", () => {
  const observer = vi.spyOn(globalThis, "ResizeObserver");
  const observe = vi.spyOn(ResizeObserver.prototype, "observe");
  const unobserve = vi.spyOn(ResizeObserver.prototype, "unobserve");
  const disconnect = vi.spyOn(ResizeObserver.prototype, "disconnect");
  function Thread({ ids }: { ids: number[] }) {
    const thread = useStickToBottom([ids]);
    return <div ref={thread.ref}>{ids.map(id => <p key={id}>{id}</p>)}</div>;
  }
  const view = render(<Thread ids={[1, 2]} />);
  const first = view.container.querySelector("p")!;
  expect(observe).toHaveBeenCalledTimes(3);
  observe.mockClear();
  view.rerender(<Thread ids={[1, 2]} />);
  expect(observe).not.toHaveBeenCalled();
  view.rerender(<Thread ids={[2, 3]} />);
  expect(unobserve).toHaveBeenCalledWith(first);
  expect(observe).toHaveBeenCalledTimes(1);
  expect(observer).toHaveBeenCalledTimes(1);
  expect(disconnect).not.toHaveBeenCalled();
  view.unmount();
  expect(disconnect).toHaveBeenCalledTimes(1);
});
