import { uiError } from "../i18n/errors";
import { SupportLog } from "./SupportLog";
import { useEffect, useState } from "react";
import {
  aiModels,
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
  type ProviderProfile,
  type Settings,
} from "../api";
import { normalizeDigits } from "../tools/commandParser";
import { useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { Tabs } from "@base-ui/react/tabs";
import { ModelPicker } from "./ModelPicker";
import { plainMethod } from "./methodName";
import { SubscriptionSettings } from "./SubscriptionSettings";


// Names only. Rules are authored by Mizan and stored with each drawing.
const STANDARD_METHODS = ["POMI", "NRM2", "SMM7", "CESMM4"];
const LOCAL_METHODS = ["Egypt", "KSA", "UAE"];
const OTHER_METHOD = "\u0000other";

/** Settings' sections, in the order of the side list. */
const SECTIONS = ["mizan", "measurement", "general", "connections", "support"] as const;
type Section = (typeof SECTIONS)[number];

/** A small line icon for each section (24 grid, 1.5 stroke, the text's colour). */
function SectionIcon({ id }: { id: Section }) {
  const paths: Record<Section, string> = {
    // A balance: Mizan.
    mizan: "M12 4v16M8 20h8M5 7h14M5 7l-2.5 6a2.5 2.5 0 0 0 5 0zM19 7l-2.5 6a2.5 2.5 0 0 0 5 0zM12 4a1 1 0 1 0 0 .01",
    // A ruler.
    measurement: "M3.5 15.5 15.5 3.5l5 5-12 12zM7 12l1.5 1.5M9.5 9.5 12 12M12 7l1.5 1.5M14.5 4.5 17 7",
    // Two sliders.
    general: "M4 7h9M17 7h3M4 17h3M11 17h9M15 5v4M9 15v4",
    // A plug.
    connections: "M9 3v4M15 3v4M7 7h10v4a5 5 0 0 1-10 0zM12 16v5",
    // A question in a circle.
    support: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6v.6M12 17v.01",
  };
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth={1.5}
      strokeLinecap="round" strokeLinejoin="round">
      <path d={paths[id]} />
    </svg>
  );
}

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
export function SettingsDialog({ onClose, onSaved }: { onClose: () => void; onSaved?(settings: Settings): void }) {
  const t = useT();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [savedAddress, setSavedAddress] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
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
  const [otherMethod, setOtherMethod] = useState(false);
  const [methods, setMethods] = useState<MeasurementMethod[]>([]);

  useEffect(() => {
    getSettings()
      .then((s) => { setDraft(s.measurement_method ? { ...s, measurement_method: plainMethod(s.measurement_method) } : s); setSavedAddress(s.providers.openai_compatible.base_url ?? null); })
      .catch((e) => setError(uiError(e)));
    appDir()
      .then(setDir)
      .catch(() => setDir(""));
    mcpStatus()
      .then(setMcp)
      .catch(() => setMcp(null));
    measurementMethods()
      .then(setMethods)
      .catch((e) => setError(uiError(e)));
  }, []);

  const methodNames = [...STANDARD_METHODS, ...LOCAL_METHODS.map((country) => `Local practice – ${country}`), ...methods.map((method) => method.name)];
  const customMethod = otherMethod || !!(draft?.measurement_method && !methodNames.includes(draft.measurement_method));
  const provider = draft?.provider;
  useEffect(() => {
    if (!provider || provider === "chatgpt_plan" || provider === "claude_code") return;
    let current = true;
    setKeySaved(false);
    // No keychain reads as no key; saving one then says why it can't.
    hasApiKey(provider)
      .then((saved) => { if (current) setKeySaved(saved); })
      .catch(() => { if (current) setKeySaved(false); });
    return () => { current = false; };
  }, [provider]);

  const change = (patch: Partial<Settings>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const subscription = provider === "chatgpt_plan" || provider === "claude_code";
  const profile = draft ? (draft.providers[draft.provider] ?? { model: "" }) : undefined;
  const keySavedHere = keySaved && (draft?.provider === "anthropic" || (profile?.base_url ?? null) === savedAddress);
  const changeProfile = (patch: Partial<ProviderProfile>) =>
    setDraft((d) => d ? {
      ...d,
      providers: { ...d.providers, [d.provider]: { model: "", ...d.providers[d.provider], ...patch } },
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
    setSaving(true);
    try {
      const profile = next.providers[next.provider] ?? { model: "" };
      if (!subscription && profile.model.trim()) {
        const list = await aiModels(next.provider, profile.base_url ?? null, key || undefined);
        if (list.from_service && !list.models.some((model) => model.id === profile.model)) {
          setError(t("ai.error.model_not_found"));
          return;
        }
      }
      await setSettings(next);
      onSaved?.(next);
      if (!subscription && key.trim()) await setApiKey(draft.provider, key);
      onClose();
    } catch (e) {
      setError(uiError(e));
    } finally { setSaving(false); }
  }

  async function removeKey() {
    if (!draft) return;
    try {
      await deleteApiKey(draft.provider);
      setKeySaved(false);
      setError(null);
    } catch (e) {
      setError(uiError(e));
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

  const snippets = draft ? mcpSnippets(dir, draft.mcp_token) : null;

  return (
    <ModalDialog labelledBy="settings-title" className="settings-sections" onClose={onClose}>
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
        <Tabs.Root className="settings-body" orientation="vertical" defaultValue="mizan">
          <Tabs.List className="settings-nav">
            {SECTIONS.map((id) => (
              <Tabs.Tab key={id} value={id} className="settings-nav-item">
                <SectionIcon id={id} />
                <span>{t(`settings.section.${id}`)}</span>
              </Tabs.Tab>
            ))}
          </Tabs.List>
          <Tabs.Panel value="mizan" keepMounted className="settings-section">
            {draft && profile && (
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
                    <option value="chatgpt_plan">{t("settings.providerChatgpt")}</option>
                    <option value="claude_code">{t("settings.providerClaudeCode")}</option>
                  </select>
                </label>
                <p>{t(`settings.providerDescription.${draft.provider}`)}</p>
                {subscription && <SubscriptionSettings provider={draft.provider} model={profile.model} onModel={(model) => changeProfile({ model, image_input: undefined })}
                  effort={profile.effort} onEffort={(effort) => changeProfile({ effort })}
                  executable={draft.claude_executable ?? ""} onExecutable={(claude_executable) => change({ claude_executable })} />}
                {!subscription && <div className="field">
                  <span>{t("settings.model")}</span>
                  <ModelPicker
                    key={draft.provider}
                    provider={draft.provider}
                    baseUrl={profile.base_url ?? null}
                    value={profile.model}
                    typedKey={key}
                    onChange={(model) => changeProfile({ model, image_input: undefined })}
                    onImageInputChange={(image_input) => { if (image_input !== (profile.image_input ?? null)) changeProfile({ image_input }); }}
                  />
                </div>}
                <label>
                  <span>{t("settings.imageInput")}</span>
                  <select value={profile.image_input == null ? "unknown" : String(profile.image_input)}
                    onChange={(e) => changeProfile({ image_input: e.target.value === "unknown" ? null : e.target.value === "true" })}>
                    <option value="unknown">{t("settings.imageUnknown")}</option>
                    <option value="true">{t("settings.imageSupported")}</option>
                    <option value="false">{t("settings.imageUnsupported")}</option>
                  </select>
                </label>
                {profile.image_input !== true && <p className="note">{t(profile.image_input === false ? "ai.noVision" : "ai.unknownVision")}</p>}
                {draft.provider === "openai_compatible" && (
                  <label>
                    <span>{t("settings.baseUrl")}</span>
                    <input
                      dir="ltr"
                      type="url"
                      placeholder="http://localhost:11434/v1"
                      value={profile.base_url ?? ""}
                      onChange={(e) => { changeProfile({ base_url: e.target.value || null, image_input: null }); setKey(""); }}
                    />
                  </label>
                )}
                {!subscription && <label>
                  <span>{t("settings.effort")}</span>
                  <select
                    value={profile.effort ?? (draft.provider === "anthropic" ? "medium" : "low")}
                    onChange={(e) => changeProfile({ effort: e.target.value as Effort })}
                  >
                    {EFFORTS.filter((x) => draft.provider === "anthropic" || !["xhigh", "max"].includes(x)).map((x) => (
                      <option key={x} value={x}>
                        {t(`effort.${x}`)}
                      </option>
                    ))}
                  </select>
                </label>}
                {!subscription && <div className="field">
                  <span>
                    <label htmlFor="settings-key">{t("settings.apiKey")}</label>
                    {keySavedHere && <span className="badge">{t("settings.saved")}</span>}
                  </span>
                  <span className="row">
                    <input
                      id="settings-key"
                      dir="ltr"
                      type="password"
                      autoComplete="off"
                      placeholder={keySavedHere ? "••••••••" : ""}
                      value={key}
                      onChange={(e) => setKey(e.target.value)}
                    />
                    {keySavedHere && (
                      <button type="button" onClick={removeKey}>
                        {t("settings.remove")}
                      </button>
                    )}
                  </span>
                </div>}
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
                  <span>{t("workspace.limit")}</span>
                  <input type="number" min={1} max={8} value={draft.max_concurrent_turns ?? 3}
                    onChange={event => change({ max_concurrent_turns: Number(event.target.value) })} />
                </label>
              </>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="measurement" keepMounted className="settings-section">
            {draft && (
              <>
                <label>
                  <span>{t("settings.measurementMethod")}</span>
                  <select
                    value={customMethod ? OTHER_METHOD : draft.measurement_method ?? ""}
                    onChange={(e) => {
                      setOtherMethod(e.target.value === OTHER_METHOD);
                      change({ measurement_method: e.target.value === OTHER_METHOD ? "" : e.target.value || null });
                    }}
                  >
                    <option value="">{t("settings.geometric")}</option>
                    {STANDARD_METHODS.map((name) => <option key={name} value={name}>{name}</option>)}
                    {LOCAL_METHODS.map((country) => <option key={country} value={`Local practice – ${country}`}>{t(`settings.method${country}`)}</option>)}
                    {methods.filter((method) => !STANDARD_METHODS.includes(method.name) && !LOCAL_METHODS.some((country) => method.name === `Local practice – ${country}`)).map((method) => <option key={method.name} value={method.name}>{method.name}</option>)}
                    <option value={OTHER_METHOD}>{t("settings.methodOther")}</option>
                  </select>
                </label>
                {customMethod && <label>
                  <span>{t("settings.methodName")}</span>
                  <input dir="auto" value={draft.measurement_method ?? ""} placeholder="ARM4, MMHW"
                    onChange={(e) => change({ measurement_method: e.target.value })} />
                </label>}
                <label>
                  <span>{t("settings.country")}</span>
                  <input dir="auto" list="settings-countries" value={draft.country ?? ""} placeholder={t("settings.countryHint")}
                    onChange={(e) => change({ country: e.target.value || null })} />
                  <datalist id="settings-countries">
                    <option value="Egypt" /><option value="Saudi Arabia" /><option value="UAE" /><option value="Kuwait" />
                  </datalist>
                </label>
              </>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="general" keepMounted className="settings-section">
            {draft && (
              <label>
                <span>{t("settings.autosave")}</span>
                <input
                  dir="ltr"
                  inputMode="numeric"
                  value={minutes ?? String(draft.autosave_minutes ?? 5)}
                  onChange={(e) => setMinutes(e.target.value)}
                />
              </label>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="connections" keepMounted className="settings-section">
            {draft && snippets && (
              <>
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
                      : (mcp.error ? uiError(mcp.error) : t("settings.mcpStopped"))}
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
                    <button type="button" onClick={() => {
                      const bytes = crypto.getRandomValues(new Uint8Array(16));
                      change({ mcp_token: Array.from(bytes, (n) => n.toString(16).padStart(2, "0")).join("") });
                    }}>{t("settings.rotateToken")}</button>
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
              </>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="support" keepMounted className="settings-section">
            {draft && <label className="check">
              <input type="checkbox" checked={draft.show_technical_details ?? false}
                onChange={(e) => change({ show_technical_details: e.target.checked })} />
              <span>{t("settings.showTechnicalDetails")}</span>
            </label>}
            <SupportLog />
          </Tabs.Panel>
        </Tabs.Root>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={onClose}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!draft || saving}>
            {t("settings.save")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
