import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { liveReload } from "./scripts/live-plugin";

export default defineConfig({
  plugins: [react(), liveReload()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/.local/**", "**/target/**"] },
  },
  test: {
    passWithNoTests: true,
    // Only this checkout's tests: `.local/` holds parallel worktrees and development data.
    exclude: ["**/node_modules/**", "**/dist/**", ".local/**", "target/**"],
    setupFiles: ["./src/test/setup.ts"],
    // `Ribbon.test.tsx` reads the ribbon's rules from `theme.css?raw` (other CSS stays empty).
    css: { include: [/theme\.css/, /(?:fonts|markdown)\.css\?raw/] },
  },
});
