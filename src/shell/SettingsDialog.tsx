import { uiError, userError } from "../i18n/errors";
import { SupportLog } from "./SupportLog";
import { useEffect, useRef, useState } from "react";
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
import { useLanguage, useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { Tabs } from "@base-ui/react/tabs";
import { ModelPicker } from "./ModelPicker";
import { plainMethod } from "./methodName";
import { EffortChoices } from "./SettingsChoices";
import { ProviderLogo } from "./ProviderLogo";
import { SubscriptionSettings } from "./SubscriptionSettings";


// Names only. Rules are authored by Mizan and stored with each drawing.
const STANDARD_METHODS = ["POMI", "NRM2", "SMM7", "CESMM4"];
const LOCAL_METHODS = ["Egypt", "KSA", "UAE"];
const OTHER_METHOD = "\u0000other";
type SettingsWrite = { update: (settings: Settings) => Settings; validate?: (next: Settings) => Promise<void> };

const SECTIONS = ["mizan", "measurement", "general", "connections", "support"] as const;
const PROVIDERS: Provider[] = ["chatgpt_plan", "claude_code", "anthropic", "openai_compatible"];
const PROVIDER_LABELS = { chatgpt_plan: "providerChatgpt", claude_code: "providerClaudeCode", anthropic: "providerAnthropic", openai_compatible: "providerOpenai" };

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
  const { lang } = useLanguage();
  const sheet = useRef<HTMLDivElement>(null);
  const [closing, setClosing] = useState(false);
  const [connectionVersion, setConnectionVersion] = useState(0);
  const [port, setPort] = useState<string | null>(null);
  const [modelContainer, setModelContainer] = useState<HTMLDivElement | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [savedAddress, setSavedAddress] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string | null>>({});
  const keyRead = useRef(0);
  const current = useRef<Settings | null>(null);
  const persisted = useRef<Settings | null>(null);
  const writes = useRef(Promise.resolve());
  const revision = useRef(0);
  const failedWrites = useRef<Record<string, SettingsWrite>>({});
  const failureVersion = useRef(0);
  const fieldRevisions = useRef<Record<string, number>>({});
  const savedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(savedTimer.current), []);
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
      .then((s) => { current.current = persisted.current = s.measurement_method ? { ...s, measurement_method: plainMethod(s.measurement_method) } : s; setDraft(current.current); setSavedAddress(s.providers.openai_compatible.base_url ?? null); })
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
    const read = ++keyRead.current;
    setKeySaved(false);
    // No keychain reads as no key; saving one then says why it can't.
    hasApiKey(provider)
      .then((saved) => { if (current && read === keyRead.current) setKeySaved(saved); })
      .catch(() => { if (current && read === keyRead.current) setKeySaved(false); });
    return () => { current = false; };
  }, [provider]);

  function confirmSaved() {
    setSaved(true);
    clearTimeout(savedTimer.current);
    savedTimer.current = setTimeout(() => setSaved(false), 2200);
  }

  // Serialize patches against the last successful write, never an invalid draft.
  function apply(update: (settings: Settings) => Settings, field = "general", validate?: (next: Settings) => Promise<void>) {
    if (!current.current) return;
    const next = update(current.current);
    current.current = next;
    setDraft(next);
    setSaved(false);
    const version = ++revision.current;
    fieldRevisions.current[field] = version;
    delete failedWrites.current[field];
    setFieldErrors(errors => ({ ...errors, [field]: null }));
    writes.current = writes.current.then(async () => {
      if (!persisted.current) return;
      const next = update(persisted.current);
      try {
        await validate?.(next);
        await setSettings(next);
        persisted.current = next;
        if (field === "address") setConnectionVersion(version);
        onSaved?.(next);
        if (version === fieldRevisions.current[field]) setFieldErrors(errors => ({ ...errors, [field]: null }));
        if (version === revision.current) confirmSaved();
        if (field === "connections") void mcpStatus()
          .then(status => { if (version === fieldRevisions.current[field]) setMcp(status); })
          .catch(() => { if (version === fieldRevisions.current[field]) setMcp(null); });
      } catch (e) {
        if (version === fieldRevisions.current[field]) {
          failureVersion.current += 1;
          failedWrites.current[field] = { update, validate };
          setFieldErrors(errors => ({ ...errors, [field]: uiError(e) }));
        }
      }
    });
  }
  const change = (patch: Partial<Settings>, field = "general") => apply(s => ({ ...s, ...patch }), field);
  const subscription = provider === "chatgpt_plan" || provider === "claude_code";
  const profile = draft ? (draft.providers[draft.provider] ?? { model: "" }) : undefined;
  const keySavedHere = keySaved && (draft?.provider === "anthropic" || (profile?.base_url ?? null) === savedAddress);
  function changeProfile(patch: Partial<ProviderProfile>) {
    const selected = current.current?.provider;
    if (!selected) return;
    const typedKey = key;
    const address = current.current?.providers[selected]?.base_url ?? null;
    apply(s => ({ ...s, providers: { ...s.providers, [selected]: { model: "", ...s.providers[selected], ...patch } } }),
      patch.model !== undefined ? "model" : patch.base_url !== undefined ? "address" : "effort", patch.model !== undefined && !subscription ? async next => {
        const profile = next.providers[selected]!;
        if (selected === "openai_compatible" && (profile.base_url ?? null) !== address) throw userError(t("settings.addressInvalid"));
        if (!profile.model.trim()) throw userError(t("ai.error.no_model"));
        const list = await aiModels(selected, profile.base_url ?? null, typedKey || undefined);
        if (list.from_service && !list.models.some(m => m.id === profile.model)) throw userError(t("ai.error.model_not_found"));
      } : undefined);
  }
  function editAddress(value: string) {
    if (!current.current) return;
    const base_url = value || null;
    const next = { ...current.current, providers: { ...current.current.providers,
      openai_compatible: { ...current.current.providers.openai_compatible, base_url } } };
    current.current = next;
    setDraft(next);
    setKey("");
    setSaved(false);
    revision.current += 1;
    try {
      if (value && !["https:", "http:"].includes(new URL(value).protocol)) return;
      changeProfile({ base_url, image_input: null });
    } catch { /* Keep incomplete addresses editable until the field loses focus. */ }
  }

  async function close() {
    setClosing(true);
    const previousFailure = failureVersion.current;
    await writes.current;
    if (failureVersion.current !== previousFailure) setClosing(false);
    else onClose();
  }

  async function connectKey() {
    if (!draft || subscription || !key.trim()) return;
    const selected = draft.provider;
    const address = profile?.base_url ?? null;
    const typedKey = key.trim();
    setConnecting(true);
    keyRead.current += 1;
    setFieldErrors(errors => ({ ...errors, key: null }));
    try {
      await writes.current;
      if (selected === "openai_compatible" && persisted.current?.providers.openai_compatible.base_url !== address) {
        throw userError(t("settings.addressInvalid"));
      }
      const model = persisted.current?.providers[selected]?.model;
      if (model?.trim()) {
        const list = await aiModels(selected, address, typedKey);
        if (list.from_service && !list.models.some(item => item.id === model)) throw userError(t("ai.error.model_not_found"));
      }
      await setApiKey(selected, typedKey);
      if (current.current?.provider === selected && (current.current.providers[selected]?.base_url ?? null) === address) {
        setKey(""); setKeySaved(true);
        if (selected === "openai_compatible") setSavedAddress(address);
        confirmSaved();
      }
    } catch (e) {
      setFieldErrors(errors => ({ ...errors, key: uiError(e) }));
    } finally { setConnecting(false); }
  }
  function inlineError(field: string) {
    const failed = failedWrites.current[field];
    return fieldErrors[field] ? <div className="settings-field-error">
      <p id={`settings-error-${field}`} className="error" role="alert">{fieldErrors[field]}</p>
      {failed && <button type="button" onClick={() => apply(failed.update, field, failed.validate)}>{t("ai.failure.retry")}</button>}
    </div> : null;
  }

  async function removeKey() {
    if (!draft) return;
    setConnecting(true);
    keyRead.current += 1;
    try {
      await deleteApiKey(draft.provider);
      setKeySaved(false);
      setFieldErrors(errors => ({ ...errors, key: null }));
      confirmSaved();
    } catch (e) {
      setFieldErrors(errors => ({ ...errors, key: uiError(e) }));
    } finally { setConnecting(false); }
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
    <ModalDialog labelledBy="settings-title" className="settings-sections" dismissible={!connecting} onClose={() => void close()}>
      <div ref={sheet} className="settings-sheet" dir={lang === "ar" ? "rtl" : "ltr"} lang={lang}>
        <Tabs.Root className="settings-body" defaultValue="mizan" onValueChange={() => { if (sheet.current?.parentElement) sheet.current.parentElement.scrollTop = 0; }}>
          <header className="settings-head">
            <div className="settings-title-row">
              <h2 id="settings-title" className="heading">{t("settings")}</h2>
              <span className="settings-saved" role="status">{saved ? t("settings.saved") : ""}</span>
              <button type="button" className="settings-close" aria-label={t("settings.close")} disabled={closing || connecting} onClick={() => void close()}>×</button>
            </div>
            <Tabs.List className="settings-nav" aria-label={t("settings")}>
              {SECTIONS.map(id => <Tabs.Tab key={id} value={id} className="settings-nav-item">{t(`settings.section.${id}`)}</Tabs.Tab>)}
              <Tabs.Indicator className="settings-tab-indicator" />
            </Tabs.List>
          </header>
          <Tabs.Panel value="mizan" keepMounted className="settings-section">
            {draft && profile && (
              <>
                <section className="settings-group">
                  <h3>{t("settings.aiConnection")}</h3>
                  <p>{t("settings.aiConnectionHint")}</p>
                  <div className="settings-provider-list" role="radiogroup" aria-label={t("settings.provider")}>
                    {PROVIDERS.map(id => <div key={id} className={`settings-provider ${provider === id ? "selected" : ""}`}>
                      <label className="settings-choice provider-choice">
                        <input type="radio" name="provider" disabled={connecting} aria-describedby={`provider-hint-${id}`} aria-label={t(`settings.${PROVIDER_LABELS[id]}`)} value={id} checked={provider === id}
                          onChange={() => {
                            for (const field of ["model", "effort", "address", "key", "executable"]) {
                              fieldRevisions.current[field] = -1;
                              delete failedWrites.current[field];
                            }
                            setFieldErrors(errors => ({ ...errors, model: null, effort: null, address: null, key: null, executable: null }));
                            change({ provider: id }, "connection"); setKey(""); setShowKey(false);
                          }} />
                        <ProviderLogo provider={id} />
                        <span><strong>{t(`settings.${PROVIDER_LABELS[id]}`)}</strong><span id={`provider-hint-${id}`}>{t(`settings.providerDescription.${id}`)}</span></span>
                      </label>
                      {provider === id && <div className="settings-provider-setup">
                        {subscription ? <SubscriptionSettings modelContainer={modelContainer} provider={id} model={profile.model} onModel={model => changeProfile({ model, image_input: undefined })}
                          effort={profile.effort} onEffort={effort => changeProfile({ effort })}
                          executable={draft.claude_executable ?? ""} onExecutable={claude_executable => change({ claude_executable }, "executable")} /> : <>
                          {id === "openai_compatible" && <>
                            <div className="settings-provider-chips" role="group" aria-label={t("settings.apiService")}>
                              <button type="button" disabled={connecting} aria-pressed={profile.base_url === "https://api.openai.com/v1"} onClick={() => { changeProfile({ base_url: "https://api.openai.com/v1", image_input: null }); setKey(""); }}><ProviderLogo provider="openai_compatible" /><bdi>OpenAI</bdi></button>
                              <button type="button" disabled={connecting} aria-pressed={profile.base_url !== "https://api.openai.com/v1"} onClick={() => { changeProfile({ base_url: null, image_input: null }); setKey(""); }}><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="M9 3v4m6-4v4M7 7h10v4a5 5 0 0 1-10 0zm5 9v5" /></svg>{t("settings.compatible")}</button>
                            </div>
                            <label><span>{t("settings.baseUrl")}</span>
                              <input disabled={connecting} dir="ltr" type="url" placeholder="https://api.openai.com/v1" value={profile.base_url ?? ""}
                                aria-invalid={!!fieldErrors.address} aria-describedby={fieldErrors.address ? "settings-error-address" : undefined}
                                onChange={e => editAddress(e.target.value)}
                                onBlur={e => {
                                  const value = e.target.value.trim();
                                  try { if (value && !["https:", "http:"].includes(new URL(value).protocol)) throw new Error(); }
                                  catch { setFieldErrors(errors => ({ ...errors, address: t("settings.addressInvalid") })); return; }
                                  setFieldErrors(errors => ({ ...errors, address: null }));
                                }} />
                            </label>{inlineError("address")}
                          </>}
                          <div className="field">
                            <label htmlFor="settings-key">{t("settings.apiKey")}</label>
                            <div className="row">
                              <input disabled={connecting} id="settings-key" dir="ltr" type={showKey ? "text" : "password"} autoComplete="off" spellCheck={false}
                                aria-invalid={!!fieldErrors.key} aria-describedby={fieldErrors.key ? "settings-error-key" : undefined}
                                placeholder={keySavedHere ? "••••••••" : ""} value={key} onChange={e => setKey(e.target.value)} />
                              <button type="button" aria-pressed={showKey} onClick={() => setShowKey(!showKey)}>{t(showKey ? "settings.hideKey" : "settings.showKey")}</button>
                              <button type="button" className="primary" disabled={!key.trim() || connecting} onClick={() => void connectKey()}>{t("settings.connectKey")}</button>
                            </div>
                            {keySavedHere && <div className="settings-connection-row"><span className="settings-status" data-state="ok">{t("settings.keyConnected")}</span><button type="button" disabled={connecting} onClick={removeKey}>{t("settings.remove")}</button></div>}
                            {inlineError("key")}
                          </div>
                        </>}
                        {id === "claude_code" && inlineError("executable")}
                      </div>}
                    </div>)}
                  </div>
                  {inlineError("connection")}
                </section>
                <div className="settings-model-slot" ref={setModelContainer} />
                <section className="settings-group">
                {!subscription && <div className="field">
                  <h3>{t("settings.model")}</h3><p>{t("settings.modelHint")}</p>
                  <ModelPicker
                    key={draft.provider}
                    provider={draft.provider}
                    baseUrl={profile.base_url ?? null}
                    value={profile.model}
                    typedKey={key}
                    error={fieldErrors.model}
                    onRetry={failedWrites.current.model ? () => { const failed = failedWrites.current.model; apply(failed.update, "model", failed.validate); } : undefined}
                    connectionVersion={connectionVersion}
                    onChange={(model) => changeProfile({ model, image_input: undefined })}
                  />
                </div>}
                {!subscription && <EffortChoices
                  values={EFFORTS.filter(x => draft.provider === "anthropic" || !["xhigh", "max"].includes(x))}
                  value={profile.effort ?? (draft.provider === "anthropic" ? "medium" : "low")}
                  onChange={effort => changeProfile({ effort: effort as Effort })} />}
                  {subscription && inlineError("model")}
                  {inlineError("effort")}
                </section>
                <section className="settings-group">
                  <h3>{t("settings.howMizanWorks")}</h3>
                  <p>{t("settings.howMizanWorksHint")}</p>
                  <fieldset className="settings-choices">
                    <legend>{t("settings.autonomy")}</legend>
                    {(["autonomous", "per_phase", "per_job"] as const).map(value => <label key={value} className="settings-choice">
                      <input type="radio" name="autonomy" aria-label={t(value === "autonomous" ? "settings.autonomous" : value === "per_phase" ? "settings.perPhase" : "settings.perJob")} checked={(draft.autonomy ?? "autonomous") === value} onChange={() => change({ autonomy: value }, "autonomy")} />
                      <span><strong>{t(value === "autonomous" ? "settings.autonomous" : value === "per_phase" ? "settings.perPhase" : "settings.perJob")}</strong><span>{t(`settings.autonomyHint.${value}`)}</span></span>
                    </label>)}
                  </fieldset>
                  {inlineError("autonomy")}
                  <div className="settings-connection-row">
                    <label htmlFor="settings-concurrency">{t("workspace.limit")}</label>
                    <div className="settings-stepper">
                      <button type="button" aria-label={t("settings.fewerDrawings")} disabled={(draft.max_concurrent_turns ?? 3) <= 1} onClick={() => change({ max_concurrent_turns: (draft.max_concurrent_turns ?? 3) - 1 }, "concurrency")}>−</button>
                      <input id="settings-concurrency" type="number" min={1} max={8} value={draft.max_concurrent_turns ?? 3}
                        onChange={e => { const n = Number(normalizeDigits(e.target.value)); if (Number.isInteger(n) && n >= 1 && n <= 8) change({ max_concurrent_turns: n }, "concurrency"); }} />
                      <button type="button" aria-label={t("settings.moreDrawings")} disabled={(draft.max_concurrent_turns ?? 3) >= 8} onClick={() => change({ max_concurrent_turns: (draft.max_concurrent_turns ?? 3) + 1 }, "concurrency")}>+</button>
                    </div>
                  </div>
                  {inlineError("concurrency")}
                </section>
              </>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="measurement" keepMounted className="settings-section">
            {draft && (
              <section className="settings-group">
                <h3>{t("settings.measurementMethod")}</h3><p>{t("settings.measurementHint")}</p>
                <label>
                  <span>{t("settings.measurementMethod")}</span>
                  <select
                    value={customMethod ? OTHER_METHOD : draft.measurement_method ?? ""}
                    onChange={(e) => {
                      setOtherMethod(e.target.value === OTHER_METHOD);
                      change({ measurement_method: e.target.value === OTHER_METHOD ? null : e.target.value || null }, "measurement");
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
                    onChange={(e) => change({ measurement_method: e.target.value.trim() ? e.target.value : null }, "measurement")} />
                </label>}
                {inlineError("measurement")}
                <label>
                  <span>{t("settings.country")}</span>
                  <input dir="auto" list="settings-countries" maxLength={60} value={draft.country ?? ""} placeholder={t("settings.countryHint")}
                    onChange={(e) => change({ country: e.target.value || null }, "country")} />
                  <datalist id="settings-countries">
                    <option value="Egypt" /><option value="Saudi Arabia" /><option value="UAE" /><option value="Kuwait" />
                  </datalist>
                </label>
                {inlineError("country")}
              </section>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="general" keepMounted className="settings-section">
            {draft && (
              <section className="settings-group"><h3>{t("settings.section.general")}</h3><p>{t("settings.generalHint")}</p>
              <label>
                <span>{t("settings.autosave")}</span>
                <input
                  dir="ltr"
                  inputMode="numeric"
                  value={minutes ?? String(draft.autosave_minutes ?? 5)}
                  aria-invalid={!!fieldErrors.minutes} aria-describedby={fieldErrors.minutes ? "settings-error-minutes" : undefined}
                  onChange={e => {
                    const typed = normalizeDigits(e.target.value); setMinutes(typed);
                    setSaved(false); revision.current += 1;
                    if (!typed.trim()) { setFieldErrors(errors => ({ ...errors, minutes: null })); return; }
                    const n = Number(typed);
                    if (!Number.isInteger(n) || n < 0 || n > 120) { setFieldErrors(errors => ({ ...errors, minutes: t("settings.autosaveWhole") })); return; }
                    change({ autosave_minutes: n }, "minutes");
                  }}
                />
              </label>{inlineError("minutes")}</section>
            )}
          </Tabs.Panel>
          <Tabs.Panel value="connections" keepMounted className="settings-section">
            {draft && snippets && (
              <>
                <h3>{t("settings.section.connections")}</h3><p>{t("settings.connectionsHint")}</p>
                <label className="check">
                  <input
                    type="checkbox"
                    checked={draft.mcp_enabled}
                    onChange={(e) => change({ mcp_enabled: e.target.checked }, "connections")}
                  />
                  <span>{t("settings.mcpEnable")}</span>
                </label>
                {mcp && (
                  <p className="settings-status" data-state={mcp.listening ? "ok" : "off"} role="status">
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
                    value={port ?? draft.mcp_port}
                    aria-invalid={!!fieldErrors.connections} aria-describedby={fieldErrors.connections ? "settings-error-connections" : undefined}
                    onChange={e => {
                      const typed = normalizeDigits(e.target.value);
                      setPort(typed);
                      setSaved(false); revision.current += 1;
                      const n = Number(typed);
                      if (!Number.isInteger(n) || n < 1024 || n > 65535) { setFieldErrors(errors => ({ ...errors, connections: t("settings.portInvalid") })); return; }
                      change({ mcp_port: n }, "connections");
                    }}
                  />
                </label>
                {inlineError("connections")}
                <div className="field">
                  <label htmlFor="settings-token">{t("settings.token")}</label>
                  <p>{t("settings.tokenHint")}</p>
                  <span className="row">
                    <input id="settings-token" dir="ltr" readOnly value={draft.mcp_token} />
                    <button type="button" onClick={() => {
                      const bytes = crypto.getRandomValues(new Uint8Array(16));
                      change({ mcp_token: Array.from(bytes, (n) => n.toString(16).padStart(2, "0")).join("") }, "connections");
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
            <h3>{t("settings.section.support")}</h3><p>{t("settings.supportHint")}</p>
            {draft && <label className="check">
              <input type="checkbox" checked={draft.show_technical_details ?? false}
                onChange={(e) => change({ show_technical_details: e.target.checked }, "support")} />
              <span>{t("settings.showTechnicalDetails")}</span>
            </label>}
            {inlineError("support")}
            <SupportLog />
          </Tabs.Panel>
        </Tabs.Root>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        {inlineError("general")}
      </div>
    </ModalDialog>
  );
}
