import { useEffect, useState } from "react";
import {
  chatgptAccount, chatgptSignIn, chatgptSignOut, cancelChatgptSignIn,
  claudeCodeStatus, subscriptionLink, chatgptModels, chatgptSelectAccount, chatgptResumePlan,
  type ChatgptAccount, type ClaudeCodeStatus, type Provider, type ModelInfo,
} from "../api";
import { useT } from "../i18n";
import { uiError } from "../i18n/errors";

/** Account controls never receive tokens. Claude login stays in the user's terminal. */
export function SubscriptionSettings({ provider, executable, onExecutable, model = "", onModel }: {
  provider: Provider;
  executable: string;
  onExecutable: (value: string) => void;
  model?: string;
  onModel?: (value: string) => void;
}) {
  const t = useT();
  const [account, setAccount] = useState<ChatgptAccount | null>(null);
  const [claude, setClaude] = useState<ClaudeCodeStatus | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [signingIn, setSigningIn] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let mounted = true;
    setError(null);
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
      chatgptModels().then((items) => { if (mounted) { setModels(items); if (model && !items.some((m) => m.id === model)) onModel?.(""); } })
        .catch((e) => { if (mounted) setError(uiError(e)); });
    }
    return () => { mounted = false; };
  }, [provider, account]);

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
        <input dir="ltr" value={executable} onChange={(e) => { onExecutable(e.target.value); setClaude(null); }} />
      </label>
      <p>{t("settings.claudePathHint")}</p>
      <button type="button" disabled={busy} onClick={() => void act(async () => {
        setClaude(await claudeCodeStatus(executable));
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
        <select aria-label={t("settings.chatgptModel")} value={models.some((m) => m.id === model) ? model : ""} disabled={busy} onChange={(e) => onModel?.(e.target.value)}>
          <option value="">{t("ai.error.chatgpt_model")}</option>
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
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
