// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";

describe("Markdown", () => {
  it("never shows the body that publishes a measurement method", () => {
    render(<Markdown text="Measured to POMI (RICS) rules and the RICS NRM2 note." />);
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/RICS/);
    expect(screen.getByText(/Measured to POMI rules and the NRM2 note\./)).toBeTruthy();
  });
});
