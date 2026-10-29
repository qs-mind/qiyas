// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { PlanDock, latestDockPlan } from "./PlanDock";
import type { Item } from "./messages";

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

test("Show in chat scrolls the original thread item", () => {
  const original = document.createElement("section");
  original.id = props.threadItemId;
  original.scrollIntoView = vi.fn();
  document.body.append(original);
  try {
    render(<PlanDock {...props} />);
    fireEvent.click(toggle());
    fireEvent.click(screen.getByRole("button", { name: "Show in chat" }));
    expect(original.scrollIntoView).toHaveBeenCalledExactlyOnceWith({ block: "nearest", behavior: "instant" });
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
