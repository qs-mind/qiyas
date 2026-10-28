import { Popover } from "@base-ui/react/popover";
import { useEffect, useRef, useState } from "react";
import { getSettings, claudeCodeStatus, chatgptModels, type ClaudeCodeStatus, type PlanModel, type Settings, type WorkspaceChoice } from "../api";
import { useT } from "../i18n";
import { uiError } from "../i18n/errors";
import { Glyph } from "../shell/glyphs";
import { ModelPicker } from "../shell/ModelPicker";
import { DotLoader } from "../shell/DotLoader";

/** What the button says when no model is chosen yet: the service. */
const PROVIDER_NAMES: Record<WorkspaceChoice["provider"], string> = {
  anthropic: "Claude",
  openai_compatible: "API",
  claude_code: "Claude Code",
  chatgpt_plan: "ChatGPT",
};

/** The model button in Mizan's box (W4): the model's name, which opens a popover with this drawing's provider, model and
 *  effort. Credentials and provider addresses stay in Settings; these choices belong to this drawing. */
export function WorkspacePicker({ choice, busy, onChange }: {
  choice: WorkspaceChoice; busy: boolean; onChange(choice: WorkspaceChoice): Promise<void>;
}) {
  const t = useT();
  const [defaults, setDefaults] = useState<Settings | null>(null);
  const [changing, setChanging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { getSettings().then(setDefaults).catch(() => {}); }, []);
  const change = (next: WorkspaceChoice) => {
    setError(null); setChanging(true);
    void onChange(next).catch(error => setError(uiError(error))).finally(() => setChanging(false));
  };
  const name = choice.model || choice.provider;
  return <Popover.Root modal={false}>
    <Popover.Trigger className="model-button" aria-label={`${t("workspace.model")}: ${name}`} title={`${t("workspace.provider")} · ${name}`}>
      <Glyph name="spark" size={15} />
      <bdi className="model-name">{choice.model || PROVIDER_NAMES[choice.provider]}</bdi>
      <Glyph name="down" size={12} />
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Positioner className="model-positioner" side="top" align="start" sideOffset={8} collisionPadding={8}>
        <Popover.Popup className="workspace-picker" aria-label={t("workspace.provider")}>
    <p className="workspace-now"><bdi>{name}</bdi></p>
    <fieldset disabled={busy || changing || !defaults}>
      <label>{t("settings.provider")}
        <select value={choice.provider} onChange={event => {
          const provider = event.target.value as WorkspaceChoice["provider"];
          const profile = defaults?.providers[provider];
          change({ provider, model: profile?.model ?? "", effort: profile?.effort ?? null, base_url: profile?.base_url });
        }}>
          <option value="anthropic">Anthropic</option>
          <option value="openai_compatible">{t("settings.providerOpenai")}</option>
          <option value="claude_code">Claude Code</option>
          <option value="chatgpt_plan">ChatGPT</option>
        </select>
      </label>
      {choice.provider === "claude_code" || choice.provider === "chatgpt_plan" ?
        <SubscriptionChoice key={choice.provider} choice={choice} ready={!!defaults} executable={defaults?.claude_executable} onChange={change} /> : <>
      <label>{t("settings.model")}</label>
      <ModelPicker provider={choice.provider} baseUrl={choice.base_url ?? defaults?.providers[choice.provider]?.base_url ?? null}
        value={choice.model} onChange={model => change({ ...choice, model })} />
      <label>{t("settings.effort")}
        <select value={choice.effort ?? ""} onChange={event => change({ ...choice, effort: event.target.value || null })}>
          <option value="">{t("settings.subscriptionDefault")}</option>
          {["none", "low", "medium", "high", "xhigh", "max"].filter(effort => choice.provider === "claude_code" ? ["low", "medium", "high", "max"].includes(effort) : choice.provider !== "openai_compatible" || !["xhigh", "max"].includes(effort)).map(effort => <option key={effort} value={effort}>{t(`effort.${effort}`)}</option>)}
        </select>
      </label>
      </>}
    </fieldset>
    {error && <p role="alert">{error}</p>}
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  </Popover.Root>;
}


/** Subscription choices come from documented CLI help or the account's model metadata, read when they open. */
function SubscriptionChoice({ choice, ready, executable, onChange }: {
  choice: WorkspaceChoice; ready: boolean; executable?: string | null; onChange(choice: WorkspaceChoice): void;
}) {
  const t = useT();
  const [claude, setClaude] = useState<ClaudeCodeStatus | null>(null);
  const [models, setModels] = useState<PlanModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const isClaude = choice.provider === "claude_code";
  const efforts = isClaude ? (claude?.supports_effort ? claude.effort_levels ?? [] : [])
    : models.find(model => model.id === choice.model)?.reasoning_efforts ?? [];
  async function readChoices() {
    setLoading(true); setError(null);
    try {
      if (isClaude) {
        const status = await claudeCodeStatus(executable);
        if (!mounted.current) return;
        setClaude(status);
        if (status.code) setError(t(`ai.error.${status.code}`));
        if (status.compatible) {
          const model = status.supports_model ? choice.model : "";
          const effort = status.supports_effort && status.effort_levels?.includes(choice.effort ?? "") ? choice.effort : null;
          if (model !== choice.model || effort !== choice.effort) onChange({ ...choice, model, effort });
        }
      } else {
        const found = await chatgptModels();
        if (mounted.current) setModels(found);
      }
    } catch (error) { if (mounted.current) setError(uiError(error)); }
    finally { if (mounted.current) setLoading(false); }
  }
  // Read once when the choices open (the founder: no refresh button, the list is just there).
  useEffect(() => { if (ready) void readChoices(); }, [ready, executable]);
  return <div aria-busy={loading}>
    {loading && <DotLoader pattern="spiral" size="sm" label={t("settings.modelsLoading")} />}
    {isClaude && claude?.supports_model && <label>{t("settings.model")}
      <input aria-label={t("settings.model")} dir="ltr" value={choice.model} placeholder={t("settings.subscriptionDefault")}
        onChange={event => onChange({ ...choice, model: event.target.value })} />
    </label>}
    {!isClaude && models.length > 0 && <label>{t("settings.model")}
      <select value={choice.model} onChange={event => {
        const model = event.target.value;
        const allowed = models.find(item => item.id === model)?.reasoning_efforts ?? [];
        onChange({ ...choice, model, effort: allowed.includes(choice.effort ?? "") ? choice.effort : null });
      }}>
        <option value="">{t("settings.subscriptionDefault")}</option>
        {models.map(model => <option key={model.id} value={model.id}>{model.label}</option>)}
      </select>
    </label>}
    {efforts.length > 0 && <label>{t("settings.effort")}
      <select value={choice.effort ?? ""} onChange={event => onChange({ ...choice, effort: event.target.value || null })}>
        <option value="">{t("settings.subscriptionDefault")}</option>
        {efforts.map(effort => <option key={effort} value={effort}>{t(`effort.${effort}`)}</option>)}
      </select>
    </label>}
    {error && <p role="alert">{error}</p>}
  </div>;
}
