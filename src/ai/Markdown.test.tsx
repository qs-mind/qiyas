// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";

describe("Markdown", () => {
  it("gives each paragraph its own reading direction", () => {
    const { container } = render(<Markdown text={"هذه فقرة عربية طويلة تشرح حساب مساحة الجدار بالتفصيل.\n\nThe wall area."} />);
    expect(container.querySelector(".md")?.getAttribute("dir")).toBe("rtl");
    const paragraphs = container.querySelectorAll("p");
    expect(paragraphs[0].getAttribute("dir")).toBe("rtl");
    expect(paragraphs[1].getAttribute("dir")).toBe("ltr");
  });

  it.each(["#", "##", "###", "####"])("gives %s headings their own reading direction", (prefix) => {
    const { container } = render(<Markdown text={`${prefix} مساحة الجدار\n\n${prefix} Wall area`} />);
    const headings = container.querySelectorAll("h4");
    expect(headings[0].getAttribute("dir")).toBe("rtl");
    expect(headings[1].getAttribute("dir")).toBe("ltr");
  });

  it("never shows the body that publishes a measurement method", () => {
    render(<Markdown text="Measured to POMI (RICS) rules and the RICS NRM2 note." />);
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/RICS/);
    expect(screen.getByText(/Measured to POMI rules and the NRM2 note\./)).toBeTruthy();
  });
});
