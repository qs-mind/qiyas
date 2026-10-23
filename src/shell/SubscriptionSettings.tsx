import { useEffect, useState } from "react";
import {
  chatgptAccount, chatgptSignIn, chatgptSignOut, cancelChatgptSignIn,
  claudeCodeStatus, subscriptionLink,
  type ChatgptAccount, type ClaudeCodeStatus, type Provider,
} from "../api";
import { useT } from "../i18n";
import { uiError } from "../i18n/errors";

/** Account controls never receive tokens. Claude login stays in the user's terminal. */
export function SubscriptionSettings({ provider, executable, onExecutable }: {
  provider: Provider;
  executable: string;
  onExecutable: (value: string) => void;
}) {
  const t = useT();
  const [account, setAccount] = useState<ChatgptAccount | null>(null);
  const [claude, setClaude] = useState<ClaudeCodeStatus | null>(null);
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
      {account?.available === false && <p role="status">{t("ai.error.chatgpt_setup")}</p>}
      {account?.signed_in ? <>
        <p><bdi>{account.display_name}</bdi></p>
        {!account.plan_granted && <p role="alert">{t("ai.error.chatgpt_scope")}</p>}
        <button type="button" disabled={busy} onClick={() => void act(async () => {
          await chatgptSignOut(); setAccount(await chatgptAccount());
        })}>{t("settings.chatgptSignOut")}</button>
      </> : <button className="chatgpt-sign-in" type="button" disabled={busy || account?.available === false} onClick={() => void act(async () => {
        setAccount(await chatgptSignIn());
      })}>{t("settings.chatgptSignIn")}</button>}
      {busy && <>
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
