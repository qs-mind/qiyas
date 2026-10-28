// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { Markdown } from "./Markdown";
import theme from "../shell/theme.css?raw";
import fonts from "../fonts/fonts.css?raw";
import markdown from "./markdown.css?raw";

afterEach(cleanup);
test("the founder's marked Arabic words remain intact on the same prose node", () => {
  const sentence = "أهلاً! ما في رسمة مفتوحة حالياً. عندك خيارين:";
  render(<Markdown text={`${sentence}\n\n- ${sentence} test-plan.dwg\n- **أهلاً** بطول 12.5 m في الجدار A-WALL\n\n## حالياً`} />);
  const paragraph = document.querySelector(".md p")!;
  expect(paragraph.textContent).toBe(sentence);
  expect(paragraph.childNodes).toHaveLength(1);
  expect(paragraph.firstChild?.nodeType).toBe(Node.TEXT_NODE);
  expect(document.querySelectorAll("sup, sub, [style]")).toHaveLength(0);
  for (const bdi of document.querySelectorAll("bdi")) {
    expect(bdi.textContent).not.toMatch(/[\u0600-\u06ff]/);
    expect(bdi.getAttribute("dir")).toBe("ltr");
  }
  expect(document.querySelector(".md li")?.getAttribute("dir")).toBe("rtl");
  expect(document.querySelector("h4")?.textContent).toBe("حالياً");
});

test("Arabic prose has a complete Thmanyah face, baseline inlines and no body swash", () => {
  const faces = fonts.match(/@font-face\s*\{[^}]+\}/g)!.filter(face => face.includes('"Thmanyah Arabic"'));
  expect(faces).toHaveLength(3);
  expect(faces.every(face => !face.includes("unicode-range"))).toBe(true);
  expect(faces.every(face => face.includes("thmanyahsans-"))).toBe(true);
  expect(theme).toContain('font-family: "Thmanyah Arabic", "Thmanyah Sans", sans-serif;');
  expect(theme).toContain('font-feature-settings: "swsh" 0;');
  expect(theme).toContain('.md h4 { font-feature-settings: "swsh"; }');
  expect(theme).toContain('vertical-align: baseline;\n  line-height: inherit;');
  expect(theme).toContain('line-height: 1.65;');
  expect(theme).toContain(':is(.md, .msg.user, .question-list):lang(ar) {');
  // The general :lang(ar) rule must not hand children the ranged face again.
  expect(theme).toContain(':is(.md, .msg.user, .question-list):lang(ar) :not(code, pre, kbd, bdi[dir="ltr"]) {\n  font-family: inherit;');
  expect(markdown).not.toMatch(/vertical-align:\s*(super|sub)|translateY/);
  // Keeping every space (pre-wrap) is what raised "مستطيلاً" in the user's bubble; line breaks stay.
  const bubble = theme.match(/\.msg\.user \{[^}]+\}/)![0];
  expect(bubble).toContain("white-space: pre-line;");
  expect(bubble).not.toContain("white-space: pre-wrap");
});

test("grid prose has no line clamp or ellipsis and totals can wrap in narrow panels", () => {
  expect(theme).not.toContain("line-clamp:");
  expect(theme.match(/\.grid-cell \{([^}]+)\}/)?.[1]).not.toContain("ellipsis");
  expect(theme.match(/\.totals li \{([^}]+)\}/)?.[1]).toContain("flex-wrap: wrap");
});
