import { useEffect, useState } from "react";
import {
  appDir,
  deleteApiKey,
  EFFORTS,
  getSettings,
  hasApiKey,
  mcpStatus,
  measurementMethods,
  setApiKey,
  setSettings,
  type Effort,
  type McpStatus,
  type MeasurementMethod,
  type Provider,
  type Settings,
} from "../api";
import { normalizeDigits } from "../tools/commandParser";
import { useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { ModelPicker } from "./ModelPicker";


/** The MCP bridge next to the app, with the platform's separator and extension. */
function bridgePath(dir: string) {
  return dir.includes("\\") ? `${dir}\\qiyas-mcp.exe` : `${dir}/qiyas-mcp`;
}

/** The commands that connect Claude Code and Claude Desktop to this app's MCP server. */
export function mcpSnippets(dir: string, token: string) {
  const bridge = bridgePath(dir);
  const arg = /\s/.test(bridge) ? `"${bridge}"` : bridge;
  return {
    claudeCode: `claude mcp add qiyas -e QIYAS_MCP_TOKEN=${token} -- ${arg}`,
    claudeDesktop: JSON.stringify(
      { mcpServers: { qiyas: { command: bridge, env: { QIYAS_MCP_TOKEN: token } } } },
      null,
      2,
    ),
  };
}

/** Settings: the AI provider, model, effort and API key, and the MCP server for outside AI apps. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [dir, setDir] = useState("");
  /** A key is in the keychain for the chosen provider. */
  const [keySaved, setKeySaved] = useState(false);
  /** A new key typed here; empty keeps the one saved. */
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  /** The autosave minutes as typed; null while untouched. */
  const [minutes, setMinutes] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [mcp, setMcp] = useState<McpStatus | null>(null);
  const [methods, setMethods] = useState<MeasurementMethod[]>([]);

  useEffect(() => {
    getSettings()
      .then(setDraft)
      .catch((e) => setError(String(e)));
    appDir()
      .then(setDir)
      .catch(() => setDir(""));
    mcpStatus()
      .then(setMcp)
      .catch(() => setMcp(null));
    measurementMethods()
      .then(setMethods)
      .catch((e) => setError(String(e)));
  }, []);

  const provider = draft?.provider;
  useEffect(() => {
    if (!provider) return;
    let current = true;
    setKeySaved(false);
    // No keychain reads as no key; saving one then says why it can't.
    hasApiKey(provider)
      .then((saved) => { if (current) setKeySaved(saved); })
      .catch(() => { if (current) setKeySaved(false); });
    return () => { current = false; };
  }, [provider]);

  const change = (patch: Partial<Settings>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const profile = draft?.providers[draft.provider];
  const changeProfile = (patch: Partial<Settings["providers"][Provider]>) =>
    setDraft((d) => d ? {
      ...d,
      providers: { ...d.providers, [d.provider]: { ...d.providers[d.provider], ...patch } },
    } : d);

  async function save() {
    if (!draft) return;
    // Whole minutes from 0 to 120; left empty, they stay as they were.
    let next = draft;
    const typed = minutes?.trim() ?? "";
    if (typed !== "") {
      const n = Number(normalizeDigits(typed));
      if (!Number.isInteger(n) || n < 0 || n > 120) {
        setError(t("settings.autosaveWhole"));
        return;
      }
      if (n !== (draft.autosave_minutes ?? 5)) next = { ...draft, autosave_minutes: n };
    }
    try {
      await setSettings(next);
      if (key.trim()) await setApiKey(draft.provider, key);
      onClose();
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeKey() {
    if (!draft) return;
    try {
      await deleteApiKey(draft.provider);
      setKeySaved(false);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function copy(name: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(name);
    } catch (e) {
      setError(String(e));
    }
  }

  const snippets = draft ? mcpSnippets(dir, draft.mcp_token) : null;

  return (
    <ModalDialog labelledBy="settings-title" onClose={onClose}>
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 id="settings-title" className="heading">
          {t("settings")}
        </h2>
        {draft && snippets && profile && (
          <>
            <label>
              <span>{t("settings.provider")}</span>
              <select
                value={draft.provider}
                onChange={(e) => {
                  change({ provider: e.target.value as Provider });
                  setKey("");
                }}
              >
                <option value="anthropic">{t("settings.providerAnthropic")}</option>
                <option value="openai_compatible">{t("settings.providerOpenai")}</option>
              </select>
            </label>
            <div className="field">
              <span>{t("settings.model")}</span>
              <ModelPicker
                key={draft.provider}
                provider={draft.provider}
                baseUrl={profile.base_url ?? null}
                value={profile.model}
                onChange={(model) => changeProfile({ model })}
              />
            </div>
            {draft.provider === "openai_compatible" && (
              <label>
                <span>{t("settings.baseUrl")}</span>
                <input
                  dir="ltr"
                  type="url"
                  placeholder="http://localhost:11434/v1"
                  value={profile.base_url ?? ""}
                  onChange={(e) => changeProfile({ base_url: e.target.value || null })}
                />
              </label>
            )}
            <label>
              <span>{t("settings.effort")}</span>
              <select
                value={draft.effort}
                onChange={(e) => change({ effort: e.target.value as Effort })}
              >
                {EFFORTS.map((x) => (
                  <option key={x} value={x}>
                    {t(`effort.${x}`)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("settings.autonomy")}</span>
              <select
                value={draft.autonomy ?? "autonomous"}
                onChange={(e) => change({ autonomy: e.target.value as Settings["autonomy"] })}
              >
                <option value="per_job">{t("settings.perJob")}</option>
                <option value="per_phase">{t("settings.perPhase")}</option>
                <option value="autonomous">{t("settings.autonomous")}</option>
              </select>
            </label>
            <label>
              <span>{t("settings.measurementMethod")}</span>
              <select
                value={draft.measurement_method ?? ""}
                onChange={(e) => change({ measurement_method: e.target.value || null })}
              >
                <option value="">{t("settings.geometric")}</option>
                {draft.measurement_method && !methods.some((m) => m.name === draft.measurement_method) && (
                  <option value={draft.measurement_method}>{draft.measurement_method} {t("settings.methodMissing")}</option>
                )}
                {methods.map((method) => <option key={method.name} value={method.name}>{method.title}</option>)}
              </select>
            </label>
            <label>
              <span>{t("settings.autosave")}</span>
              <input
                dir="ltr"
                inputMode="numeric"
                value={minutes ?? String(draft.autosave_minutes ?? 5)}
                onChange={(e) => setMinutes(e.target.value)}
              />
            </label>
            <div className="field">
              <span>
                <label htmlFor="settings-key">{t("settings.apiKey")}</label>
                {keySaved && <span className="badge">{t("settings.saved")}</span>}
              </span>
              <span className="row">
                <input
                  id="settings-key"
                  dir="ltr"
                  type="password"
                  autoComplete="off"
                  placeholder={keySaved ? "••••••••" : ""}
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                />
                {keySaved && (
                  <button type="button" onClick={removeKey}>
                    {t("settings.remove")}
                  </button>
                )}
              </span>
            </div>

            <fieldset>
              <legend className="heading">{t("settings.mcp")}</legend>
              <label className="check">
                <input
                  type="checkbox"
                  checked={draft.mcp_enabled}
                  onChange={(e) => change({ mcp_enabled: e.target.checked })}
                />
                <span>{t("settings.mcpEnable")}</span>
              </label>
              {mcp && (
                <p className="status" role="status">
                  {mcp.listening
                    ? t("settings.mcpListening", { addr: mcp.addr ?? "" })
                    : (mcp.error ?? t("settings.mcpStopped"))}
                </p>
              )}
              <label>
                <span>{t("settings.port")}</span>
                <input
                  dir="ltr"
                  type="number"
                  min={1024}
                  max={65535}
                  value={draft.mcp_port}
                  onChange={(e) => change({ mcp_port: Number(e.target.value) })}
                />
              </label>
              <div className="field">
                <label htmlFor="settings-token">{t("settings.token")}</label>
                <span className="row">
                  <input id="settings-token" dir="ltr" readOnly value={draft.mcp_token} />
                  <button type="button" onClick={() => copy("token", draft.mcp_token)}>
                    {copied === "token" ? t("settings.copied") : t("settings.copy")}
                  </button>
                </span>
              </div>
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
            </fieldset>
          </>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={onClose}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!draft}>
            {t("settings.save")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
