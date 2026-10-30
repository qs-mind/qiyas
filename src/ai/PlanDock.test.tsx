// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { PlanDock, StatusBlock, latestDockPlan } from "./PlanDock";
import { EMPTY, reduce, replay, type Item } from "./messages";

const STORAGE_KEY = "qiyas.planDock.expanded";
const plan: Extract<Item, { kind: "plan" }> = {
  kind: "plan",
  phases: [
    { id: "site", title: "Tower site survey", status: "done" },
    { id: "foundations", title: "Tower foundations", status: "done" },
    { id: "podium", title: "Tower podium alternatives", status: "skipped" },
    { id: "structure", title: "Tower structural frame", status: "active" },
    { id: "facade", title: "Tower facade quantities", status: "pending" },
    { id: "services", title: "Tower MEP coordination", status: "pending" },
    { id: "fitout", title: "Tower fit-out quantities", status: "pending" },
    { id: "boq", title: "Tower BOQ review", status: "pending" },
  ],
};
const props = { plan, busy: true, language: "en" as const, threadItemId: "tower-plan" };
const finished = { ...plan, phases: plan.phases.map((phase) => ({ ...phase, status: phase.status === "skipped" ? "skipped" as const : "done" as const })) };
const toggle = () => screen.getByRole("button", { name: /Plan (?:done )?\d\/8/ });

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  localStorage.removeItem(STORAGE_KEY);
  localStorage.removeItem("qiyas.planDock.frameExpanded");
});

test("collapsed tower plan shows done plus skipped, the active title, and small dots", () => {
  const { container } = render(<PlanDock {...props} />);
  expect(toggle().textContent).toContain("Plan 3/8");
  expect(toggle().textContent).toContain("Tower structural frame");
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  expect(container.querySelector(".dot-loader.dot-sm")).toBeTruthy();
  expect(container.querySelector<HTMLElement>(".plan-dock-progress > span")?.style.inlineSize).toBe("37.5%");
  expect(screen.queryByRole("region")).toBeNull();
  expect(container.querySelector('[aria-live], [role="status"]')).toBeNull();
});

test("click toggles, Escape from a child collapses, and focus returns to the button", () => {
  render(<PlanDock {...props} />);
  fireEvent.click(toggle());
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  expect(document.getElementById(toggle().getAttribute("aria-controls")!)?.hidden).toBe(false);
  expect(document.activeElement).toBe(toggle());
  expect(screen.getAllByRole("listitem")).toHaveLength(8);
  fireEvent.click(toggle());
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(toggle());
  const chat = screen.getByRole("button", { name: "Show in chat" });
  chat.focus();
  fireEvent.keyDown(chat, { key: "Escape" });
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  expect(document.activeElement).toBe(toggle());
  expect(screen.queryByRole("button", { name: "Show in chat" })).toBeNull();
});

test("the chosen fold is remembered on remount", () => {
  const view = render(<PlanDock {...props} />);
  fireEvent.click(toggle());
  expect(localStorage.getItem(STORAGE_KEY)).toBe("true");
  view.unmount();
  render(<PlanDock {...props} />);
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(toggle());
  expect(localStorage.getItem(STORAGE_KEY)).toBe("false");
});

test("approval opens a collapsed dock and Approve calls its handler once", () => {
  const onApprove = vi.fn();
  const onChange = vi.fn();
  const view = render(<PlanDock {...props} busy={false} onApprove={onApprove} onChange={onChange} />);
  view.rerender(<PlanDock {...props} plan={{ ...plan, awaitingApproval: true }} busy={false} onApprove={onApprove} onChange={onChange} />);
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Approve" }));
  expect(onApprove).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Change" }));
  expect(onChange).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(toggle(), { key: "Escape" });
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
});

test("completed phases collapse to done only when Mizan stops working", () => {
  const view = render(<PlanDock {...props} />);
  fireEvent.click(toggle());
  view.rerender(<PlanDock {...props} plan={finished} />);
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  screen.getByRole("button", { name: "Show in chat" }).focus();
  view.rerender(<PlanDock {...props} plan={finished} busy={false} />);
  expect(toggle().textContent).toContain("Plan done 8/8");
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  expect(document.activeElement).toBe(toggle());
  expect(document.querySelector(".dot-loader")).toBeNull();
});

test("no dock for missing or single-phase plans", () => {
  const view = render(<PlanDock {...props} plan={null} />);
  expect(view.container.childElementCount).toBe(0);
  view.rerender(<PlanDock {...props} plan={{ kind: "plan", phases: [plan.phases[0]] }} />);
  expect(view.container.childElementCount).toBe(0);
});

test("Show in chat folds the dock and scrolls the original thread item", () => {
  const original = document.createElement("section");
  original.id = props.threadItemId;
  original.scrollIntoView = vi.fn();
  document.body.append(original);
  try {
    render(<PlanDock {...props} />);
    fireEvent.click(toggle());
    fireEvent.click(screen.getByRole("button", { name: "Show in chat" }));
    expect(original.scrollIntoView).toHaveBeenCalledExactlyOnceWith({ block: "nearest", behavior: "instant" });
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
  } finally { original.remove(); }
});

test("Arabic uses RTL, Arabic digits, and a localized active phase title", () => {
  const arabic = { ...plan, phases: plan.phases.map((phase) => phase.status === "active"
    ? { ...phase, title: "الهيكل الإنشائي لبرج المطار" } : phase) };
  const { container } = render(<PlanDock {...props} plan={arabic} language="ar" />);
  const button = screen.getByRole("button", { name: /الخطة ٣\/٨/ });
  expect(button.textContent).toContain("الهيكل الإنشائي لبرج المطار");
  expect(container.firstElementChild?.getAttribute("dir")).toBe("rtl");
  expect(container.firstElementChild?.getAttribute("lang")).toBe("ar");
});

test("blocked storage does not prevent expanding or collapsing", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("blocked"); });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("blocked"); });
  render(<PlanDock {...props} />);
  fireEvent.click(toggle());
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  fireEvent.keyDown(toggle(), { key: "Escape" });
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
});

test("expanded stale advice does not create another live region", () => {
  const { container } = render(<PlanDock {...props} plan={{ ...plan, stale: true }} />);
  fireEvent.click(toggle());
  expect(screen.getByText("The plan needs an update. Some phases are still unfinished.")).toBeTruthy();
  expect(container.querySelector('[role="status"], [aria-live]')).toBeNull();
});

test("active phase changes scroll only the expanded dock", () => {
  const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
  const view = render(<PlanDock {...props} />);
  fireEvent.click(toggle());
  const body = document.getElementById(toggle().getAttribute("aria-controls")!)!;
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 100 });
  vi.spyOn(body, "getBoundingClientRect").mockReturnValue({ top: 20 } as DOMRect);
  const next = screen.getByText("Tower facade quantities").closest("li")!;
  vi.spyOn(next, "getBoundingClientRect").mockReturnValue({ top: 220 } as DOMRect);
  view.rerender(<PlanDock {...props} plan={{ ...plan, phases: plan.phases.map((phase) => ({
    ...phase, status: phase.id === "facade" ? "active" : phase.status === "active" ? "done" : phase.status,
  })) }} />);
  expect(body.scrollTop).toBe(150);
  expect(scrollIntoView).not.toHaveBeenCalled();
});

test("the latest plan wins; completed work leaves on the next user turn", () => {
  const hospital: typeof plan = { kind: "plan", phases: [
    { id: "structure", title: "Hospital structure", status: "active" },
    { id: "mep", title: "Hospital MEP", status: "pending" },
  ] };
  expect(latestDockPlan([])).toBeNull();
  expect(latestDockPlan([plan, hospital])?.plan).toBe(hospital);
  expect(latestDockPlan([plan, { ...hospital, phases: [hospital.phases[0]] }])).toBeNull();
  expect(latestDockPlan([finished])?.plan).toBe(finished);
  expect(latestDockPlan([finished, { kind: "user", text: "Review the airport terminal" }])).toBeNull();
  expect(latestDockPlan([hospital, { kind: "user", text: "Continue the hospital" }])?.plan).toBe(hospital);
});


test("app statuses stay distinct from the answer and retain their language on replay", () => {
  const status = { type: "status" as const, kind: "open_phases" as const, items: ["المناسيب"], language: "ar" as const };
  let state = reduce(EMPTY, { type: "text", text: "أحتاج المناسيب قبل الإكمال." });
  state = reduce(state, status);
  expect(state.items[0]).toEqual({ kind: "assistant", text: "أحتاج المناسيب قبل الإكمال.", finished: true });
  const item = state.items[1];
  if (item.kind !== "status") throw new Error("Expected app status");
  render(<StatusBlock item={item} />);
  expect(screen.getByRole("status").getAttribute("lang")).toBe("ar");
  expect(screen.getByRole("status").getAttribute("dir")).toBe("rtl");
  expect(screen.getByRole("status").textContent).toContain("مراحل مفتوحة");
  expect(screen.getByRole("status").textContent).not.toContain("أحتاج المناسيب قبل الإكمال.");
  expect(replay([{ type: "event", event: status }]).items).toEqual([item]);
  expect(replay([{ type: "event", event: { type: "text", text: "Old answer" } }]).items)
    .toEqual([{ kind: "assistant", text: "Old answer", finished: true }]);
});

test("save statuses have English and Arabic labels", () => {
  for (const language of ["en", "ar"] as const) {
    render(<StatusBlock item={{ kind: "status", status: { type: "status", kind: "saved", items: ["drawing.dwg"], language } }} />);
    expect(screen.getByRole("status").textContent).toContain(language === "en" ? "Saved" : "تم الحفظ");
    cleanup();
  }
});

const frame = {
  expert_checks: "Check escape routes", raw_data: "Read walls and openings",
  methods: "Measure the enclosure", outside_check: "No door outside the boundary",
};

test.each(["en", "ar"] as const)("expert frame has its own remembered fold and never counts as work in %s", async (language) => {
  const framed = { ...plan, frame, phases: plan.phases.map((phase, i) => i === 3 ? { ...phase, note: "Review 60 doors on A1 sheets" } : phase) };
  const view = render(<PlanDock {...props} plan={framed} language={language} />);
  const title = language === "en" ? "Expert frame" : "إطار الخبير";
  const details = screen.getByText(title).closest("details")!;
  expect(details.open).toBe(false);
  expect(view.container.querySelector(".plan-dock-count")?.textContent).toMatch(language === "en" ? /3\/8/ : /٣\/٨/);
  fireEvent.click(screen.getByText(title));
  await waitFor(() => expect(details.open).toBe(true));
  await waitFor(() => expect(localStorage.getItem("qiyas.planDock.frameExpanded")).toBe("true"));
  expect(screen.getByText(frame.outside_check)).toBeTruthy();
  fireEvent.click(view.container.querySelector(".plan-dock-toggle")!);
  expect(screen.getAllByRole("listitem")).toHaveLength(8);
  expect(screen.getByText("Review 60 doors on A1 sheets")).toBeTruthy();
  view.unmount();
  render(<PlanDock {...props} plan={framed} language={language} />);
  expect(screen.getByText(title).closest("details")?.open).toBe(true);
});

test("a single work step can still show its expert frame", () => {
  const single = { ...plan, frame, phases: [plan.phases[3]] };
  expect(latestDockPlan([single])?.plan).toEqual(single);
  render(<PlanDock {...props} plan={single} />);
  expect(screen.getByText("Expert frame")).toBeTruthy();
  expect(screen.getByRole("button", { name: /Plan 0\/1/ })).toBeTruthy();
});

test("frame and phase notes survive plan updates, approval and history replay", () => {
  const phases = [{ id: "doors", title: "Doors", note: "Review 60 doors", status: "active" as const }];
  const events = [{ type: "plan" as const, frame, phases }, { type: "awaiting_approval" as const, plan: { frame, phases } }];
  let state = EMPTY;
  for (const event of events) state = reduce(state, event);
  expect(state.items[0]).toMatchObject({ frame, phases, awaitingApproval: true });
  const restored = replay(events.map((event) => ({ type: "event" as const, event })));
  expect(restored.items[0]).toMatchObject({ frame, phases });
});
