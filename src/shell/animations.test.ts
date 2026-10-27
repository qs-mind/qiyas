import { expect, test } from "vitest";
import css from "./theme.css?raw";

// An animation that never ends restyles or repaints the page for as long as it is on screen: a
// box-shadow ring on one 7 px dot cost 6% of a core, the Mizan mark in the panel's heading 9%
// (W2). A looping animation may move only what the compositor can move, or a stroke offset.
const CHEAP = new Set(["transform", "opacity", "stroke-dashoffset"]);

test("every animation that loops forever moves only transform, opacity or a stroke offset", () => {
  const looping = new Set([...css.matchAll(/animation:\s*([\w-]+)[^;]*\binfinite\b/g)].map((m) => m[1]));
  expect(looping.size).toBeGreaterThan(0);
  for (const name of looping) {
    const block = new RegExp(`@keyframes\\s+${name}\\s*\\{`).exec(css);
    expect(block, `@keyframes ${name}`).not.toBeNull();
    // The keyframes' body: to the brace that closes it.
    let depth = 1;
    let i = block!.index + block![0].length;
    const start = i;
    for (; i < css.length && depth > 0; i++) depth += css[i] === "{" ? 1 : css[i] === "}" ? -1 : 0;
    const body = css.slice(start, i - 1);
    const properties = [...body.matchAll(/([a-z-]+)\s*:/g)].map((m) => m[1]);
    expect(properties.length, `${name} has declarations`).toBeGreaterThan(0);
    for (const property of properties) expect(CHEAP.has(property), `${name} animates ${property}`).toBe(true);
  }
});
