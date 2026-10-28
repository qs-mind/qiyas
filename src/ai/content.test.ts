import { expect, test } from "vitest";
import { plainDashes } from "./content";

test("saved Mizan words use plain dashes, keeping surrounding spaces", () => {
  expect(plainDashes("Take-off — ready – checked―done—yes–ok")).toBe("Take-off - ready - checked-done-yes-ok");
  expect(plainDashes("حصر — جاهز – راجع―تم")).toBe("حصر - جاهز - راجع-تم");
  expect(plainDashes("—start\nend–\n―")).toBe("-start\nend-\n-");
  expect(plainDashes("1-2: unchanged")).toBe("1-2: unchanged");
});
