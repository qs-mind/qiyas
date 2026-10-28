// `pnpm tauri:fast` front end: the quick production bundle, rebuilt on every change, with the
// window reloading itself (see scripts/live-plugin.ts).
import { build, preview } from "vite";

const watcher = await build({ mode: "live", build: { watch: {}, emptyOutDir: false }, logLevel: "warn" });
// The folder is never emptied, so a window reloading mid-build still finds its files.
// Serve only once the first bundle is written, so the window never opens on an empty folder.
await new Promise((resolve, reject) => {
  watcher.on("event", (event) => {
    if (event.code === "END") resolve();
    if (event.code === "ERROR") { event.result?.close(); console.error(event.error); }
  });
  watcher.on("close", reject);
});
const server = await preview({ mode: "live", preview: { port: 1420, strictPort: true } });
server.printUrls();
console.log("Live updates on: the window reloads after each change.");
