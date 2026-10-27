// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import brand from "../../public/brand/qiyas-mark.svg?raw";
import css from "../shell/theme.css?raw";
import { MizanMark } from "./MizanMark";

afterEach(cleanup);

test("the mark is the brand drawing: the sphere's mesh, drawn line by line, and its face", () => {
  const { container } = render(<MizanMark />);
  const svg = container.querySelector("svg.mizan-mark.mizan-mark-sm")!;
  expect(svg.getAttribute("aria-hidden")).toBe("true");
  // The faint copy is the brand's mesh exactly; the pen draws every one of its segments, in order
  // round the centre, each with its place in the sweep.
  const mesh = svg.querySelector(".mm-ghost")!.getAttribute("d")!;
  const brandMesh = brand.match(/class="sphere" d="([^"]+)"/)![1];
  const segs = (d: string) => d.split("M").filter(Boolean).sort();
  expect(segs(mesh)).toEqual(segs(brandMesh));
  const lines = [...svg.querySelectorAll(".mm-pen line")];
  expect(lines).toHaveLength(segs(brandMesh).length);
  expect(lines.every((l) => l.getAttribute("pathLength") === "1")).toBe(true);
  const places = lines.map((l) => Number((l as SVGElement).style.getPropertyValue("--i")));
  expect(places[0]).toBe(0);
  expect([...places].sort((a, b) => a - b)).toEqual(places);
  expect(places[places.length - 1]).toBeLessThan(1);
  expect(brand).toContain(svg.querySelector(".mm-face")!.getAttribute("points")!);
});

test("two marks on one page keep their own gradient", () => {
  const { container } = render(
    <>
      <MizanMark />
      <MizanMark size="md" />
    </>,
  );
  const ids = [...container.querySelectorAll("linearGradient")].map((g) => g.id);
  expect(new Set(ids).size).toBe(2);
  for (const face of container.querySelectorAll(".mm-face")) {
    expect(ids).toContain(face.getAttribute("fill")!.slice(5, -1));
  }
});

test("the pen draws and wipes; nothing blinks; with reduced motion the mark stands still", () => {
  expect(css).toMatch(/@keyframes mm-draw \{\s*0% \{\s*stroke-dashoffset: 1;/);
  expect(css).not.toMatch(/@keyframes mm-turn/);
  // Reduced motion: drawn and still, the face shown.
  expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\) \{\s*\.mizan-mark \.mm-pen line \{\s*animation: none;\s*stroke-dashoffset: 0;/);
  expect(css).toMatch(/\.mizan-mark \.mm-face \{\s*animation: none;\s*opacity: 1;/);
});

test("a still mark is the finished drawing with no animation: an idle panel must not restyle sixty lines sixty times a second", () => {
  const moving = render(<MizanMark />).container.querySelector("svg")!;
  expect(moving.classList.contains("still")).toBe(false);
  cleanup();
  const resting = render(<MizanMark still />).container.querySelector("svg")!;
  expect(resting.classList.contains("still")).toBe(true);
  expect(css).toMatch(/\.mizan-mark\.still \.mm-pen line \{[^}]*animation: none;[^}]*stroke-dashoffset: 0;/);
  expect(css).toMatch(/\.mizan-mark\.still \.mm-face \{[^}]*animation: none;[^}]*opacity: 1;/);
});
