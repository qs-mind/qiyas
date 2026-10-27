/** "Connect an AI app" (W4, note 19): one place to let an outside AI app (Claude Code, Claude
 *  Desktop) work on the open drawing directly instead of through screenshots. One button turns the
 *  MCP server on; the commands to connect are then right there to copy. Nothing about the security
 *  changed: the server listens on 127.0.0.1 only, needs the secret token and refuses other web
 *  origins. It is off until someone turns it on. */
import { useEffect, useState } from "react";
import { appDir, getSettings, mcpStatus, setSettings, type McpStatus, type Settings } from "../api";
import { useT } from "../i18n";
import { uiError } from "../i18n/errors";
import { ModalDialog } from "../ui/ModalDialog";
import { mcpSnippets } from "./SettingsDialog";

const newToken = () => Array.from(crypto.getRandomValues(new Uint8Array(16)), (n) => n.toString(16).padStart(2, "0")).join("");

export function ConnectAiDialog({ onClose, onSettings }: { onClose(): void; onSettings(): void }) {
  const t = useT();
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [dir, setDir] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    getSettings().then(setSettingsState).catch((e) => setError(uiError(e)));
    mcpStatus().then(setStatus).catch(() => setStatus(null));
    appDir().then(setDir).catch(() => setDir(""));
  }, []);

  const on = settings?.mcp_enabled ?? false;

  /** Turns the server on or off through the same settings the Settings dialog saves. */
  async function turn(enable: boolean) {
    if (!settings) return;
    setBusy(true);
    setError(null);
    try {
      const next = { ...settings, mcp_enabled: enable, mcp_token: settings.mcp_token || newToken() };
      await setSettings(next);
      setSettingsState(next);
      setStatus(await mcpStatus().catch(() => null));
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  async function copy(name: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(name);
    } catch (e) {
      setError(uiError(e));
    }
  }

  const snippets = settings && on ? mcpSnippets(dir, settings.mcp_token) : null;
  return (
    <ModalDialog labelledBy="connect-title" onClose={onClose}>
      <div className="connect-ai">
        <h2 id="connect-title" className="heading">{t("connect.title")}</h2>
        <p>{t("connect.lead")}</p>
        {settings && (
          <p className="connect-state" role="status">
            {!on
              ? t("connect.off")
              : status?.listening
                ? t("settings.mcpListening", { addr: status.addr ?? "" })
                : status?.error
                  ? uiError(status.error)
                  : t("settings.mcpStopped")}
          </p>
        )}
        {snippets && (
          <>
            <p>{t("connect.steps")}</p>
            <div className="snippet">
              <span>{t("settings.claudeCode")}</span>
              <pre dir="ltr">{snippets.claudeCode}</pre>
              <button type="button" onClick={() => copy("code", snippets.claudeCode)}>
                {copied === "code" ? t("settings.copied") : t("settings.copy")}
              </button>
            </div>
            <div className="snippet">
              <span>{t("settings.claudeDesktop")}</span>
              <pre dir="ltr">{snippets.claudeDesktop}</pre>
              <button type="button" onClick={() => copy("desktop", snippets.claudeDesktop)}>
                {copied === "desktop" ? t("settings.copied") : t("settings.copy")}
              </button>
            </div>
            <p className="muted">{t("connect.warn")}</p>
          </>
        )}
        {error && <p className="error" role="alert">{error}</p>}
        <div className="buttons">
          <button type="button" onClick={() => { onClose(); onSettings(); }}>{t("connect.more")}</button>
          {on ? (
            <button type="button" disabled={busy} onClick={() => void turn(false)}>{t("connect.turnOff")}</button>
          ) : (
            <button type="button" className="primary" disabled={!settings || busy} onClick={() => void turn(true)}>
              {t(busy ? "connect.working" : "connect.turnOn")}
            </button>
          )}
          <button type="button" onClick={onClose}>{t("connect.close")}</button>
        </div>
      </div>
    </ModalDialog>
  );
}
