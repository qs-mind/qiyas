// The Qiyas mark, alive: Mizan's "working" sign. Its lines are the same drawing as
// public/brand/qiyas-mark.svg (the sphere's mesh, one segment each, in order round the centre
// from the top). While Mizan works the mesh draws itself line by line over a faint copy of
// itself, the face (the facet in the Qiyas colours) fills in, then the lines wipe away in the
// same order and it starts again. Pure CSS (theme.css, `.mizan-mark`); with reduced motion it
// stands still, drawn. Decorative: the text beside it says what is happening.
import { useId, type CSSProperties } from "react";

/** The sphere's segments [x1, y1, x2, y2], in drawing order. */
const SEGMENTS: [number, number, number, number][] = [
  [98.61, 19.66, 161.65, 26.25],
  [95.57, 72.76, 158.61, 79.35],
  [130.64, 32.86, 161.65, 26.25],
  [130.64, 32.86, 158.61, 79.35],
  [130.64, 32.86, 182.68, 51.49],
  [161.65, 26.25, 182.68, 51.49],
  [161.65, 26.25, 208.78, 68.66],
  [158.61, 79.35, 182.68, 51.49],
  [123.01, 119.36, 158.61, 79.35],
  [182.68, 51.49, 208.78, 68.66],
  [182.68, 51.49, 206.9, 101.48],
  [158.61, 79.35, 206.9, 101.48],
  [206.9, 101.48, 208.78, 68.66],
  [158.61, 79.35, 171.83, 141.37],
  [208.78, 68.66, 222, 130.68],
  [206.9, 101.48, 222, 130.68],
  [171.83, 141.37, 206.9, 101.48],
  [205.17, 157, 206.9, 101.48],
  [205.17, 157, 222, 130.68],
  [123.01, 119.36, 171.83, 141.37],
  [171.83, 141.37, 205.17, 157],
  [196.26, 188.61, 222, 130.68],
  [196.26, 188.61, 205.17, 157],
  [165.25, 195.22, 205.17, 157],
  [165.25, 195.22, 171.83, 141.37],
  [165.25, 195.22, 196.26, 188.61],
  [116.96, 173.1, 171.83, 141.37],
  [141.39, 220.34, 196.26, 188.61],
  [141.39, 220.34, 165.25, 195.22],
  [116.96, 173.1, 165.25, 195.22],
  [111.82, 210.98, 165.25, 195.22],
  [111.82, 210.98, 141.39, 220.34],
  [116.96, 173.1, 123.01, 119.36],
  [111.82, 210.98, 116.96, 173.1],
  [78.35, 213.75, 141.39, 220.34],
  [78.35, 213.75, 111.82, 210.98],
  [63.25, 184.55, 111.82, 210.98],
  [63.25, 184.55, 116.96, 173.1],
  [63.25, 184.55, 78.35, 213.75],
  [69.83, 130.69, 116.96, 173.1],
  [31.22, 171.34, 78.35, 213.75],
  [31.22, 171.34, 63.25, 184.55],
  [63.25, 184.55, 69.83, 130.69],
  [31.63, 138.83, 63.25, 184.55],
  [31.22, 171.34, 31.63, 138.83],
  [18, 109.32, 31.22, 171.34],
  [69.83, 130.69, 123.01, 119.36],
  [31.63, 138.83, 69.83, 130.69],
  [18, 109.32, 31.63, 138.83],
  [31.63, 138.83, 41.86, 84.2],
  [41.86, 84.2, 69.83, 130.69],
  [18, 109.32, 41.86, 84.2],
  [18, 109.32, 43.74, 51.39],
  [69.83, 130.69, 95.57, 72.76],
  [41.86, 84.2, 43.74, 51.39],
  [41.86, 84.2, 95.57, 72.76],
  [41.86, 84.2, 75.43, 40.26],
  [43.74, 51.39, 75.43, 40.26],
  [43.74, 51.39, 98.61, 19.66],
  [75.43, 40.26, 95.57, 72.76],
  [95.57, 72.76, 123.01, 119.36],
  [75.43, 40.26, 98.61, 19.66],
  [75.43, 40.26, 130.64, 32.86],
  [95.57, 72.76, 130.64, 32.86],
  [98.61, 19.66, 130.64, 32.86],
];
const SPHERE = SEGMENTS.map(([x1, y1, x2, y2]) => `M${x1} ${y1}L${x2} ${y2}`).join("");
const FACE = "182.68,51.49 206.9,101.48 158.61,79.35";

/** `still`: the finished mark, no animation. An animated mark is some sixty SVG lines restyled sixty
 *  times a second for as long as it is on screen (9% of a core idle in Chromium), so it moves only
 *  while something is being waited for. */
export function MizanMark({
  size = "sm",
  className,
  still = false,
}: {
  size?: "sm" | "md";
  className?: string;
  still?: boolean;
}) {
  const id = useId().replace(/:/g, "");
  return (
    <svg
      className={["mizan-mark", `mizan-mark-${size}`, still ? "still" : null, className].filter(Boolean).join(" ")}
      viewBox="0 0 240 240"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id={`mm-face-${id}`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#C9A6E0" />
          <stop offset="1" stopColor="#6E4A9E" />
        </linearGradient>
      </defs>
      <path className="mm-ghost" d={SPHERE} />
      <g className="mm-pen">
        {SEGMENTS.map(([x1, y1, x2, y2], i) => (
          <line
            key={i}
            x1={x1}
            y1={y1}
            x2={x2}
            y2={y2}
            pathLength={1}
            style={{ "--i": (i / SEGMENTS.length).toFixed(3) } as CSSProperties}
          />
        ))}
      </g>
      <polygon className="mm-face" points={FACE} fill={`url(#mm-face-${id})`} />
    </svg>
  );
}

export { SEGMENTS as MIZAN_MARK_SEGMENTS };
