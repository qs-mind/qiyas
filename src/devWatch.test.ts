import tauriIgnore from "../.taurignore?raw";
import { expect, test } from "vitest";
import config from "../vite.config";

test("development evidence, nested worktrees and build output never reload the app", () => {
  const ignored = config.server?.watch?.ignored;
  expect(ignored).toEqual(expect.arrayContaining(["**/.local/**", "**/target/**"]));
  expect(tauriIgnore).toContain(".local/");
});
