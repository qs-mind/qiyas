/** Formulas a model writes as LaTeX (`$2\pi r$`, `$$A = \pi r^2$$`) read as plain text in the
 *  thread: the panel has no maths renderer, and raw TeX is noise to an engineer. Only spans that
 *  hold TeX (a backslash, ^ or _) change, so prices like "$5 and $10" stay as written; code
 *  (fenced or `inline`) is never touched. */

const SYMBOLS: Record<string, string> = {
  pi: "π",
  times: "×",
  cdot: "·",
  div: "÷",
  pm: "±",
  approx: "≈",
  le: "≤",
  leq: "≤",
  ge: "≥",
  geq: "≥",
  neq: "≠",
  ne: "≠",
  theta: "θ",
  alpha: "α",
  beta: "β",
  Delta: "Δ",
  sum: "Σ",
  degree: "°",
  circ: "°",
  infty: "∞",
};
const SUPERSCRIPT: Record<string, string> = { "2": "²", "3": "³" };

/** One TeX formula as plain text. */
export function texToText(tex: string): string {
  let s = tex;
  // \frac{a}{b} → a/b and \sqrt{x} → √(x), innermost first.
  for (let i = 0; i < 5; i += 1) {
    const next = s
      .replace(/\\[dt]?frac\{([^{}]*)\}\{([^{}]*)\}/g, "($1)/($2)")
      .replace(/\\sqrt\{([^{}]*)\}/g, "√($1)");
    if (next === s) break;
    s = next;
  }
  s = s
    .replace(/\\(text|mathrm|mathbf|operatorname)\{([^{}]*)\}/g, "$2")
    .replace(/\\([A-Za-z]+)/g, (_, name: string) => SYMBOLS[name] ?? (name === "left" || name === "right" ? "" : name))
    .replace(/\^\{?([23])\}?/g, (_, d: string) => SUPERSCRIPT[d])
    .replace(/\^\{([^{}]*)\}/g, "^($1)")
    .replace(/_\{([^{}]*)\}/g, "$1")
    .replace(/\\[,;:! ]/g, " ")
    .replace(/[{}]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return s;
}

const TEX = /[\\^_]/;

/** `text` with its TeX formulas as plain text, code left as it is. */
export function plainMath(text: string): string {
  // Split off fenced blocks and inline code; odd parts are code.
  const parts = text.split(/(```[\s\S]*?```|`[^`\n]*`)/);
  return parts
    .map((part, i) => {
      if (i % 2 === 1) return part;
      return part
        .replace(/\$\$([\s\S]+?)\$\$/g, (whole, tex: string) => (TEX.test(tex) ? texToText(tex) : whole))
        .replace(/\$([^$\n]+?)\$/g, (whole, tex: string) => (TEX.test(tex) ? texToText(tex) : whole));
    })
    .join("");
}
