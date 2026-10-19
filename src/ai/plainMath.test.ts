import { expect, test } from "vitest";
import { plainMath, texToText } from "./plainMath";

test("TeX formulas read as plain text", () => {
  expect(plainMath("Perimeter: 6.283 m ($2\\pi r$)")).toBe("Perimeter: 6.283 m (2π r)");
  expect(plainMath("Area: 3.142 m² ($\\pi r^2$)")).toBe("Area: 3.142 m² (π r²)");
  expect(plainMath("$$A = \\frac{b h}{2}$$")).toBe("A = (b h)/(2)");
  expect(texToText("\\sqrt{a^2 + b^2}")).toBe("√(a² + b²)");
  expect(texToText("\\text{area} \\times 1.05")).toBe("area × 1.05");
});

test("prices and code are left as written", () => {
  expect(plainMath("between $5 and $10 a metre")).toBe("between $5 and $10 a metre");
  expect(plainMath("type `$2\\pi r$` here")).toBe("type `$2\\pi r$` here");
  expect(plainMath("```\n$x^2$\n```")).toBe("```\n$x^2$\n```");
});
