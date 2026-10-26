/** Canvas work is bounded to text in rendered cells, with a small cache shared by grids. */
const widths = new Map<string, number>();
let context: CanvasRenderingContext2D | null | undefined;

export function textWidth(text: string, style: CSSStyleDeclaration): number {
  // Hidden panels and jsdom have no usable font/layout yet. Measure when they become visible.
  if (!style.fontSize || typeof CanvasRenderingContext2D === "undefined") return 0;
  context ??= document.createElement("canvas").getContext("2d");
  if (!context) return 0;
  const font = `${style.fontStyle || "normal"} ${style.fontWeight || "400"} ${style.fontSize} ${style.fontFamily}`;
  const key = `${font}|${style.fontVariantNumeric}|${style.letterSpacing}|${text}`;
  const cached = widths.get(key);
  if (cached !== undefined) return cached;
  context.font = font;
  let width = context.measureText(text).width;
  if (style.fontVariantNumeric.includes("tabular-nums")) {
    const digit = Math.max(...Array.from("0123456789", (value) => context!.measureText(value).width));
    width = Math.max(width, Array.from(text, (value) => /[0-9]/.test(value) ? digit : context!.measureText(value).width).reduce((a, b) => a + b, 0));
  }
  width += (parseFloat(style.letterSpacing) || 0) * Math.max(0, text.length - 1);
  if (widths.size >= 5000) widths.clear();
  widths.set(key, width);
  return width;
}

export function clearTextWidths() { widths.clear(); }
