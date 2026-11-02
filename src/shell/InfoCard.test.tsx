// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AlongPrompt, InfoCard } from "./InfoCard";

afterEach(cleanup);

test("the info card shows each row with its label, and closes", () => {
  const onClose = vi.fn();
  render(
    <InfoCard
      info={{
        title: "Circle",
        rows: [
          ["radius", "1.500 m"],
          ["centre", "3.500, 4.500"],
          ["snap", "end"],
          ["mystery", "?"],
        ],
      }}
      onClose={onClose}
    />,
  );
  expect(screen.getByRole("heading").textContent).toBe("Info: Circle");
  const terms = screen.getAllByRole("term").map((t) => t.textContent);
  expect(terms).toEqual(["Radius", "Centre", "Snap", "mystery"]);
  const values = screen.getAllByRole("definition").map((d) => d.textContent);
  expect(values).toEqual(["1.500 m", "3.500, 4.500", "End", "?"]);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalled();
});

test("the divide prompt starts at 2 parts and takes 2 to 1000", () => {
  const onAnswer = vi.fn();
  render(<AlongPrompt kind="divide" units="metres" onAnswer={onAnswer} onCancel={() => {}} />);
  const place = screen.getByRole("button", { name: "Place points" }) as HTMLButtonElement;
  const input = screen.getByRole("spinbutton", { name: "Number of parts" });
  expect((input as HTMLInputElement).value).toBe("2");
  fireEvent.change(input, { target: { value: "1" } });
  expect(place.disabled).toBe(true);
  fireEvent.change(input, { target: { value: "5" } });
  fireEvent.click(place);
  expect(onAnswer).toHaveBeenCalledWith(5);
});

test("the step is typed in metres and given in drawing units", () => {
  const onAnswer = vi.fn();
  const onCancel = vi.fn();
  render(<AlongPrompt kind="measureAlong" units="millimetres" onAnswer={onAnswer} onCancel={onCancel} />);
  fireEvent.change(screen.getByRole("spinbutton", { name: "Step (m)" }), { target: { value: "0.6" } });
  fireEvent.click(screen.getByRole("button", { name: "Place points" }));
  expect(onAnswer).toHaveBeenCalledWith(600);
  fireEvent.keyDown(screen.getByRole("spinbutton"), { key: "Escape" });
  expect(onCancel).toHaveBeenCalled();
});

test("inquiry text uses RTL after a Latin code and keeps English LTR", () => {
  render(<InfoCard info={{ title: "Tower", rows: [["text", "D1 عدده 3 في A-101"], ["layer", "Tower Walls"]] }} onClose={() => {}} />);
  expect(screen.getByText("D1 عدده 3 في A-101").getAttribute("dir")).toBe("rtl");
  expect(screen.getByText("Tower Walls").getAttribute("dir")).toBe("ltr");
});
