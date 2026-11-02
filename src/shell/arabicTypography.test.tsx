// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import css from "./theme.css?raw";
import { readingDirection } from "../ai/content";

afterEach(cleanup);

// Keep jsdom away from unrelated browser-only layout rules.
const selectors = [
  ".properties .prop-section h3",
  ".command-input [cmdk-group-heading]",
  ".status .drafting .segment",
  ".table-grid input.field",
];
const typography = selectors.map(selector => {
  const start = css.indexOf(`${selector} {`);
  return css.slice(start, css.indexOf("}", start) + 1);
}).join("\n");
const arabicRules = css.slice(css.indexOf("/* Arabic letters stay joined"));

test.each(["language", "direction"])("Arabic typography overrides headings and fields by %s", scope => {
  const { container } = render(<>
    <style>{typography + arabicRules}</style>
    <section {...(scope === "language" ? { lang: "ar" } : { dir: "rtl" })}>
      <div className="properties"><div className="prop-section"><h3>خصائص</h3></div></div>
      <div className="command-input"><div cmdk-group-heading="">أوامر</div></div>
      <div className="status"><div className="drafting"><button className="segment">تعامد</button></div></div>
      <div className="table-grid"><input className="field" defaultValue="حقل" /></div>
      <em>ملاحظة</em>
    </section>
  </>);
  for (const element of container.querySelectorAll("h3, [cmdk-group-heading], button, input, em")) {
    const style = getComputedStyle(element);
    expect(style.letterSpacing).toBe("normal");
    expect(style.textTransform).toBe("none");
    expect(style.fontStyle).toBe("normal");
  }
});

test("content in an English window gets Arabic typography while English headings stay unchanged", () => {
  const { container } = render(<div lang="en" className="properties">
    <style>{typography + arabicRules}</style>
    <div className="prop-section">
      <h3 {...readingDirection("D1 خصائص")}>D1 خصائص</h3>
      <h3 {...readingDirection("D1 properties")}>D1 properties</h3>
    </div>
  </div>);
  const headings = container.querySelectorAll("h3");
  expect(getComputedStyle(headings[0]).letterSpacing).toBe("normal");
  expect(getComputedStyle(headings[0]).textTransform).toBe("none");
  expect(getComputedStyle(headings[1]).letterSpacing).toBe("0.44px");
  expect(getComputedStyle(headings[1]).textTransform).toBe("uppercase");
});
