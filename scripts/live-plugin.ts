import type { Plugin } from "vite";

// Live updates for the fast app (`pnpm tauri:fast`): the bundle is rebuilt on every change and
// the window reloads itself, while it still runs on the quick production bundle.
// Only in the "live" mode (scripts/live-fast.mjs runs the watching build and the preview server in
// one process; they share the open windows through globalThis). Normal builds carry none of it.
const CLIENT = "live-reload.js";
type Client = { write(chunk: string): unknown };
const shared = globalThis as { qiyasLiveClients?: Set<Client> };
const clients = (shared.qiyasLiveClients ??= new Set<Client>());

export function liveReload(): Plugin {
  let live = false;
  return {
    name: "qiyas-live-reload",
    config(_config, env) {
      live = env.mode === "live";
    },
    transformIndexHtml() {
      return live ? [{ tag: "script", attrs: { src: `/${CLIENT}` }, injectTo: "body" }] : [];
    },
    generateBundle() {
      if (!live) return;
      this.emitFile({
        type: "asset",
        fileName: CLIENT,
        source: 'new EventSource("/__live").onmessage = () => location.reload();\n',
      });
    },
    writeBundle() {
      if (live) for (const window of clients) window.write("data: reload\n\n");
    },
    configurePreviewServer(server) {
      if (!live) return;
      server.middlewares.use("/__live", (_req, res) => {
        res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive" });
        res.write(": live\n\n");
        clients.add(res);
        res.on("close", () => clients.delete(res));
      });
    },
  };
}
