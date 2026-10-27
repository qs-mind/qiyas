import { describe, expect, it } from "vitest";
import { plainMethod, withoutBody } from "./methodName";

describe("method names without the professional body", () => {
  it("drops the body from a method name", () => {
    expect(plainMethod("POMI (RICS)")).toBe("POMI");
    expect(plainMethod("NRM2 (rics)")).toBe("NRM2");
    expect(plainMethod("NRM2 [RICS]")).toBe("NRM2");
    expect(plainMethod("SMM7")).toBe("SMM7");
    expect(plainMethod(null)).toBeNull();
  });
  it("never leaves an empty name", () => {
    expect(plainMethod("RICS")).toBe("RICS");
  });
  it("takes the body out of a longer text and leaves the rest", () => {
    expect(withoutBody("Measured to POMI (RICS) rules, clause 4.")).toBe("Measured to POMI rules, clause 4.");
    expect(withoutBody("The RICS NRM2 way")).toBe("The NRM2 way");
    expect(withoutBody("no body here")).toBe("no body here");
  });
});
