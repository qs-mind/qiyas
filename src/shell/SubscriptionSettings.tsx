import { useEffect, useRef, useState } from "react";
import {
  chatgptAccount, chatgptSignIn, chatgptSignOut, cancelChatgptSignIn,
  claudeCodeStatus, subscriptionLink, chatgptModels, chatgptSelectAccount, chatgptResumePlan,
  EFFORTS, type ChatgptAccount, type ClaudeCodeStatus, type Provider, type PlanModel,
} from "../api";
import { useT } from "../i18n";
import { uiError } from "../i18n/errors";

/** Account controls never receive tokens. Claude login stays in the user's terminal. */
const OTHER_MODEL = "\u0000other";

export function SubscriptionSettings({ provider, executable, onExecutable, model = "", onModel, effort, onEffort }: {
  provider: Provider;
  executable: string;
  onExecutable: (value: string) => void;
  model?: string;
  onModel?: (value: string) => void;
  effort?: string | null;
  onEffort?: (value: string | null) => void;
}) {
  const t = useT();
  const [account, setAccount] = useState<ChatgptAccount | null>(null);
  const [claude, setClaude] = useState<ClaudeCodeStatus | null>(null);
  const [models, setModels] = useState<PlanModel[]>([]);
  const [otherModel, setOtherModel] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const statusGeneration = useRef(0);
  useEffect(() => () => { statusGeneration.current += 1; }, [provider, executable]);
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

  const selectedModel = models.find((item) => item.id === model) ?? models[0];
  const modelAliases = claude?.model_aliases ?? [];
  const customModel = otherModel || !!(model && !modelAliases.includes(model));
  const efforts = provider === "chatgpt_plan" ? selectedModel?.reasoning_efforts ?? []
    : claude?.supports_effort ? claude.effort_levels ?? [] : [];
  const effortLabel = (value: string) => (EFFORTS as readonly string[]).includes(value) ? t(`effort.${value}`) : value;

  async function act(operation: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try { await operation(); } catch (e) { setError(uiError(e)); }
    finally { setBusy(false); }
  }

  return <section className="subscription-settings">
    {provider === "claude_code" && <>
      <p>{t("settings.claudeLogin")}</p>
      <label>
        <span>{t("settings.claudeExecutable")}</span>
        <input dir="ltr" value={executable} disabled={busy} onChange={(e) => { onExecutable(e.target.value); setClaude(null); setOtherModel(false); }} />
      </label>
      <p>{t("settings.claudePathHint")}</p>
      <button type="button" disabled={busy} onClick={() => void act(async () => {
        const generation = statusGeneration.current;
        const status = await claudeCodeStatus(executable);
        if (generation !== statusGeneration.current) return;
        setClaude(status);
        if (status.compatible) {
          if (!status.supports_model && model) onModel?.("");
          if (effort && (!status.supports_effort || !status.effort_levels?.includes(effort))) onEffort?.(null);
        }
      })}>{t("settings.claudeCheck")}</button>
      {claude && <p role="status">
        {claude.code ? t("ai.error." + claude.code) : claude.signed_in === true
          ? t("settings.claudeReady") : claude.signed_in === false
            ? t("ai.error.claude_auth") : t("settings.claudeUnknownLogin")}
        {claude.version && <> <bdi>{claude.version}</bdi></>}
      </p>}
      {claude?.installed === false && <button type="button" onClick={() => void act(async () => {
        await subscriptionLink("claude_install");
      })}>{t("settings.claudeInstall")}</button>}
      {claude?.supports_model && <>
        <label>
          <span>{t("settings.model")}</span>
          <select value={customModel ? OTHER_MODEL : model} disabled={busy} onChange={(e) => {
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
    </>}
    {provider === "chatgpt_plan" && <>
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
      {account?.display_name && <p><bdi>{account.display_name}</bdi></p>}
      {account?.signed_in && !account.plan_granted && <p role="alert">{t("ai.error.chatgpt_scope")}</p>}
      <div className="subscription-actions">
        <button className="chatgpt-sign-in" type="button" disabled={busy || !account} onClick={() => void act(async () => {
          setSigningIn(true);
          try { setModels([]); onModel?.(""); setAccount(await chatgptSignIn()); }
          finally { setSigningIn(false); }
        })}>{t("settings.chatgptSignIn")}</button>
        {account?.signed_in && <button type="button" disabled={busy} onClick={() => void act(async () => {
          setModels([]); onModel?.(""); setAccount(await chatgptSignOut());
        })}>{t("settings.chatgptSignOut")}</button>}
      </div>
      {account?.notice && <p role="status">{t("ai.error." + account.notice)}</p>}
      {account?.paused && <>
        <p role="alert">{t("ai.error.chatgpt_limit")}</p>
        <button type="button" disabled={busy} onClick={() => void act(async () => {
          setAccount(await chatgptResumePlan());
        })}>{t("settings.chatgptResume")}</button>
      </>}
      {account?.signed_in && account.plan_granted && !account.paused && <label>
        <span>{t("settings.chatgptModel")}</span>
        <select aria-label={t("settings.chatgptModel")} value={selectedModel?.id ?? ""} disabled={busy || !models.length} onChange={(e) => {
          onModel?.(e.target.value);
          if (effort && !models.find((m) => m.id === e.target.value)?.reasoning_efforts?.includes(effort)) onEffort?.(null);
        }}>
          {!models.length && <option value="">{t("settings.chatgptNoModels")}</option>}
          {models.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
        </select>
      </label>}
      {signingIn && <>
        <p role="status">{t("settings.chatgptWaiting")}</p>
        <button type="button" onClick={() => void cancelChatgptSignIn()}>{t("settings.cancel")}</button>
      </>}
      <p>{t("settings.chatgptUsage")}</p>
      <button type="button" onClick={() => void act(async () => {
        await subscriptionLink("chatgpt_limits");
      })}>{t("settings.chatgptLimits")}</button>
    </>}
    {efforts.length > 0 && <label>
      <span>{t("settings.effort")}</span>
      <select value={effort && efforts.includes(effort) ? effort : ""} disabled={busy} onChange={(e) => onEffort?.(e.target.value || null)}>
        <option value="">{t("settings.subscriptionDefault")}</option>
        {efforts.map((level) => <option key={level} value={level}>{effortLabel(level)}</option>)}
      </select>
    </label>}
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
