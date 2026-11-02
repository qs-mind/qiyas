import { useEffect, useRef, useState } from "react";
import {
  aiProviderStatus, type AiProviderStatus,
  chatgptAccount, chatgptSignIn, chatgptSignOut, cancelChatgptSignIn, reopenChatgptSignIn,
  claudeCodeStatus, subscriptionLink, chatgptModels, chatgptSelectAccount, chatgptResumePlan,
  type ChatgptAccount, type ClaudeCodeStatus, type Provider, type PlanModel,
} from "../api";
import { createPortal } from "react-dom";
import { locale } from "../i18n/locale";
import { DotLoader } from "./DotLoader";
import { EffortChoices } from "./SettingsChoices";
import { PictureSupport } from "./PictureSupport";
import { useLanguage, useT } from "../i18n";
import { uiError } from "../i18n/errors";

/** Account controls never receive tokens. Claude login stays in the user's terminal. */
const OTHER_MODEL = "\u0000other";

export function SubscriptionSettings({ provider, executable, onExecutable, model = "", onModel, effort, onEffort, modelContainer }: {
  modelContainer?: HTMLElement | null;
  provider: Provider;
  executable: string;
  onExecutable: (value: string) => void;
  model?: string;
  onModel?: (value: string) => void;
  effort?: string | null;
  onEffort?: (value: string | null) => void;
}) {
  const t = useT();
  const { lang } = useLanguage();
  const [connection, setConnection] = useState<AiProviderStatus | null>(null);
  const [switchingAccount, setSwitchingAccount] = useState(false);
  const [account, setAccount] = useState<ChatgptAccount | null>(null);
  const [claude, setClaude] = useState<ClaudeCodeStatus | null>(null);
  const [models, setModels] = useState<PlanModel[]>([]);
  const [otherModel, setOtherModel] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const statusGeneration = useRef(0);
  const pendingSignIn = useRef(false);
  useEffect(() => {
    setBusy(false);
    setSigningIn(false);
    return () => {
      statusGeneration.current += 1;
      if (pendingSignIn.current) {
        pendingSignIn.current = false;
        void cancelChatgptSignIn().catch(() => {});
      }
    };
  }, [provider, executable]);
  useEffect(() => {
    let mounted = true;
    setError(null);
    setClaude(null);
    setOtherModel(false);
    if (provider === "chatgpt_plan") {
      chatgptAccount().then((account) => { if (mounted) setAccount(account); })
        .catch((e) => { if (mounted) setError(uiError(e)); });
    }
    return () => { mounted = false; };
  }, [provider]);

  useEffect(() => {
    let mounted = true;
    setModels([]);
    if (provider === "chatgpt_plan" && account?.signed_in && account.plan_granted && !account.paused) {
      chatgptModels().then((items) => {
        if (mounted) {
          setModels(items);
          if (!items.some((m) => m.id === model)) {
            onModel?.(items[0]?.id ?? "");
            if (effort && !items[0]?.reasoning_efforts?.includes(effort)) onEffort?.(null);
          }
        }
      })
        .catch((e) => { if (mounted) setError(uiError(e)); });
    }
    return () => { mounted = false; };
  }, [provider, account]);

  // Claude Code is read once the path rests (the founder: no check button, the choices are just there).
  useEffect(() => {
    if (provider !== "claude_code") return;
    const timer = window.setTimeout(() => void act(async () => {
      const generation = statusGeneration.current;
      const status = await claudeCodeStatus(executable);
      if (generation !== statusGeneration.current) return;
      setClaude(status);
      if (status.compatible) {
        if (!status.supports_model && model) onModel?.("");
        if (effort && (!status.supports_effort || !status.effort_levels?.includes(effort))) onEffort?.(null);
      }
    }), 500);
    return () => window.clearTimeout(timer);
  }, [provider, executable]);

  useEffect(() => {
    if (provider !== "chatgpt_plan") return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    setConnection(null);
    async function refresh() {
      try {
        const next = await aiProviderStatus();
        if (active && next.provider === provider) setConnection(next);
      } catch { /* Account controls report sign-in errors. */ }
      if (active) timer = setTimeout(refresh, 5000);
    }
    void refresh();
    return () => { active = false; clearTimeout(timer); };
  }, [provider, account]);

  const selectedModel = models.find((item) => item.id === model) ?? models[0];
  const modelAliases = claude?.model_aliases ?? [];
  const customModel = otherModel || !!(model && !modelAliases.includes(model));
  const efforts = provider === "chatgpt_plan" ? selectedModel?.reasoning_efforts ?? []
    : claude?.supports_effort ? claude.effort_levels ?? [] : [];

  async function waitingAction(operation: () => Promise<void>) {
    const generation = statusGeneration.current;
    try { await operation(); } catch (e) {
      if (generation === statusGeneration.current) setError(uiError(e));
    }
  }

  async function act(operation: () => Promise<void>) {
    const generation = statusGeneration.current;
    setBusy(true);
    setError(null);
    try { await operation(); } catch (e) {
      if (generation === statusGeneration.current) setError(uiError(e));
    } finally {
      if (generation === statusGeneration.current) setBusy(false);
    }
  }

  const connectionState = account?.signed_in === false ? "needs_sign_in" : connection?.status;
  const connectionText = connectionState === "ok" || connectionState === "connected" ? t("settings.connected")
    : connectionState === "plan_limit" ? connection?.resets_at
      ? t("settings.allowanceUntil", { time: new Date(connection.resets_at * 1000).toLocaleString(locale(lang)) }) : t("settings.allowanceUsed")
    : connectionState === "spend_blocked" ? t("settings.creditsSwitch")
    : connectionState === "needs_sign_in" || connectionState === "auth" ? t("settings.signedOut")
    : connection?.last_failure ? uiError(JSON.stringify(connection.last_failure)) : t("settings.checkingConnection");
  const modelControls = <section className="settings-group subscription-model">
    <h3>{t("settings.model")}</h3><p>{t("settings.modelHint")}</p>
    <div className="settings-model-choice"><div className="settings-model-input">
    {provider === "claude_code" && claude?.supports_model && <>
        <label>
          <span>{t("settings.model")}</span>
          <select dir="ltr" value={customModel ? OTHER_MODEL : model} disabled={busy} onChange={(e) => {
            setOtherModel(e.target.value === OTHER_MODEL);
            onModel?.(e.target.value === OTHER_MODEL ? "" : e.target.value);
          }}>
            <option value="">{t("settings.subscriptionDefault")}</option>
            {modelAliases.map((alias) => <option key={alias} value={alias}>{alias}</option>)}
            <option value={OTHER_MODEL}>{t("settings.modelOther")}</option>
          </select>
        </label>
        {customModel && <label>
          <span>{t("settings.modelName")}</span>
          <input dir="ltr" value={model} onChange={(e) => onModel?.(e.target.value)} />
        </label>}
    </>}
    {provider === "chatgpt_plan" && account?.signed_in && account.plan_granted && !account.paused && <label>
        <span>{t("settings.chatgptModel")}</span>
        <select dir="ltr" aria-label={t("settings.chatgptModel")} value={selectedModel?.id ?? ""} disabled={(busy && !signingIn) || !models.length} onChange={(e) => {
          onModel?.(e.target.value);
          if (effort && !models.find((m) => m.id === e.target.value)?.reasoning_efforts?.includes(effort)) onEffort?.(null);
        }}>
          {!models.length && <option value="">{t("settings.chatgptNoModels")}</option>}
          {models.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
        </select>
      </label>}
    </div>
    <PictureSupport provider={provider} model={model} executable={executable}
      account={account?.active_id} metadata={selectedModel?.image_input}
      ready={provider === "claude_code" ? !!claude?.compatible : !!account?.signed_in && !!account.plan_granted && !account.paused && models.length > 0} />
    </div>
    {efforts.length > 0 && <EffortChoices values={efforts} value={effort && efforts.includes(effort) ? effort : ""}
      defaultChoice disabled={busy && !signingIn} onChange={value => onEffort?.(value || null)} />}
  </section>;

  return <section className="subscription-settings">
    {provider === "claude_code" && <>
      <p>{t("settings.claudeLogin")}</p>
      <label>
        <span>{t("settings.claudeExecutable")}</span>
        <input dir="ltr" value={executable} disabled={busy} onChange={(e) => { onExecutable(e.target.value); setClaude(null); setOtherModel(false); }} />
      </label>
      <p>{t("settings.claudePathHint")}</p>
      {busy && !claude && <DotLoader pattern="spiral" size="sm" label={t("settings.modelsLoading")} />}
      {claude && <p role="status" className="settings-status" data-state={claude.signed_in && claude.compatible ? "ok" : "auth"}>
        {claude.code ? t("ai.error." + claude.code) : claude.signed_in === true
          ? t("settings.claudeReady") : claude.signed_in === false
            ? t("ai.error.claude_auth") : t("settings.claudeUnknownLogin")}
        {claude.version && <> <bdi>{claude.version}</bdi></>}
      </p>}
      {claude?.installed === false && <button type="button" onClick={() => void act(async () => {
        await subscriptionLink("claude_install");
      })}>{t("settings.claudeInstall")}</button>}
    </>}
    {provider === "chatgpt_plan" && <>
      <div className="settings-connection-row">
        <div className="settings-account">
          <span className="settings-avatar" aria-hidden="true">{account?.display_name?.slice(0, 1).toUpperCase() || "C"}</span>
          <div><strong><bdi>{account?.display_name || t("settings.chatgptAccount")}</bdi></strong>
            <p className="settings-status" data-state={connectionState} role="status">{connectionText}</p>
          </div>
        </div>
      <div className="subscription-actions">
        {(!account?.signed_in || !account.plan_granted || switchingAccount || signingIn || connectionState === "auth" || connectionState === "needs_sign_in") && <button className="chatgpt-sign-in" type="button" disabled={busy || !account} onClick={() => void act(async () => {
          const generation = statusGeneration.current;
          pendingSignIn.current = true;
          setSigningIn(true);
          try {
            setModels([]); onModel?.("");
            const signedInAccount = await chatgptSignIn();
            if (generation === statusGeneration.current) setAccount(signedInAccount);
          } finally {
            if (generation === statusGeneration.current) {
              pendingSignIn.current = false;
              setSigningIn(false);
            }
          }
        })}>{t("settings.chatgptSignIn")}</button>}
        {account?.signed_in && <button type="button" disabled={busy} onClick={() => void act(async () => {
          setModels([]); onModel?.(""); setAccount(await chatgptSignOut());
        })}>{t("settings.chatgptSignOut")}</button>}

        {account?.signed_in && <button type="button" disabled={busy} aria-expanded={switchingAccount} onClick={() => setSwitchingAccount(!switchingAccount)}>{t("settings.switchAccount")}</button>}
      </div>
      </div>
      {switchingAccount && <>
      <label>
        <span>{t("settings.chatgptAccount")}</span>
        <select aria-label={t("settings.chatgptAccount")} value={account?.active_id ?? ""} disabled={busy || !account} onChange={(e) => {
          const id = e.target.value || null;
          setModels([]); onModel?.("");
          void act(async () => { setAccount(await chatgptSelectAccount(id)); });
        }}>
          <option value="">{t("settings.chatgptAddAccount")}</option>
          {account?.accounts?.map((a) => <option key={a.id} value={a.id}>{a.display_name}</option>)}
        </select>
      </label>
      </>}
      {account?.signed_in && !account.plan_granted && <p role="alert">{t("ai.error.chatgpt_scope")}</p>}
      {account?.notice && <p role="status">{t("ai.error." + account.notice)}</p>}
      {account?.paused && !connection?.actions.some(a => a.kind === "retry") && <>
        <p role="alert">{t("settings.chatgpt_paused")}</p>
        <button type="button" disabled={busy} onClick={() => void act(async () => {
          setAccount(await chatgptResumePlan());
        })}>{t("settings.chatgptResume")}</button>
      </>}
      {signingIn && <div className="settings-sign-in-wait">
        <DotLoader pattern="spiral" size="sm" />
        <p role="status">{t("settings.chatgptWaiting")}</p>
        <button type="button" onClick={() => void waitingAction(reopenChatgptSignIn)}>{t("settings.chatgptReopen")}</button>
        <button type="button" onClick={() => void waitingAction(cancelChatgptSignIn)}>{t("settings.cancel")}</button>
      </div>}
      {connectionState === "spend_blocked" && <p>{t("settings.creditsHint")}</p>}
      <p>{t("settings.chatgptUsage")}</p>
      <div className="subscription-actions">
        {connection?.actions.some(a => a.kind === "retry") && <button type="button" disabled={busy} onClick={() => void act(async () => {
          setAccount(await chatgptResumePlan());
          setConnection(await aiProviderStatus());
        })}>{t("ai.failure.retry")}</button>}
        <button type="button" onClick={() => void waitingAction(async () => {
          await subscriptionLink(connection?.actions.find(a => a.kind === "open_provider_page")?.page ?? "chatgpt_usage");
        })}>{t("settings.usageSettings")}</button>
      </div>
    </>}
    {modelContainer ? createPortal(modelControls, modelContainer) : modelContainer === undefined ? modelControls : null}
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
