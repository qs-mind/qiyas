/** The AI model, chosen from what the provider offers: Anthropic's Models API, or an
 *  OpenAI-compatible service's `/models` (read in the core, with the key from the keychain). A
 *  filter narrows a long list (OpenRouter offers hundreds); "Other…" takes an id the list lacks;
 *  the saved model stays listed even when the service no longer offers it. When the service
 *  can't be asked, Claude's built-in list shows, and why. */
import { useEffect, useRef, useState } from "react";
import { aiModels, type ModelList, type Provider } from "../api";
import { useT } from "../i18n";
import { DotLoader } from "./DotLoader";

/** The option that opens the custom id field. */
const OTHER = "\u0000other";
/** Lists longer than this get a filter box. */
const FILTER_FROM = 12;
/** A base URL being typed is asked about once it rests this long. */
const URL_REST_MS = 500;

type Props = {
  provider: Provider;
  baseUrl: string | null;
  value: string;
  onChange(model: string): void;
  /** Reads the list (tests give their own). */
  load?: typeof aiModels;
};

export function ModelPicker({ provider, baseUrl, value, onChange, load = aiModels }: Props) {
  const t = useT();
  const [list, setList] = useState<ModelList | null>(null);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState("");
  const [custom, setCustom] = useState(false);
  /** Answers to an older question are dropped. */
  const asked = useRef(0);

  async function refresh() {
    const n = ++asked.current;
    setLoading(true);
    try {
      const next = await load(provider, baseUrl);
      if (n === asked.current) setList(next);
    } catch (e) {
      if (n === asked.current) setList({ models: [], from_service: false, error: String(e) });
    } finally {
      if (n === asked.current) setLoading(false);
    }
  }

  // Again on another provider at once, and on another address once it rests.
  useEffect(() => {
    ++asked.current;
    setList(null);
    if (provider === "anthropic") {
      refresh();
      return () => { ++asked.current; };
    }
    const timer = window.setTimeout(refresh, URL_REST_MS);
    return () => {
      ++asked.current;
      window.clearTimeout(timer);
    };
  }, [provider, baseUrl]);

  const models = list?.models ?? [];
  const known = models.some((m) => m.id === value);
  // The model chosen stays listed whatever the filter, so the list never loses its value.
  const shown = models.filter((m) => {
    const q = filter.trim().toLowerCase();
    return m.id === value || !q || m.id.toLowerCase().includes(q) || m.label.toLowerCase().includes(q);
  });
  const typing = custom || (list !== null && !known && value !== "" && models.length === 0);

  return (
    <div className="model-picker">
      {models.length > FILTER_FROM && (
        <input
          type="search"
          dir="ltr"
          className="model-filter"
          aria-label={t("settings.modelFilter")}
          placeholder={t("settings.modelFilter")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      )}
      <div className="model-row">
        <select
          aria-label={t("settings.model")}
          dir="ltr"
          value={typing ? OTHER : value}
          onChange={(e) => {
            if (e.target.value === OTHER) {
              setCustom(true);
              return;
            }
            setCustom(false);
            onChange(e.target.value);
          }}
        >
          {!value && <option value="">{t("settings.modelChoose")}</option>}
          {!known && value && (
            <option value={value}>
              {value} {t("settings.modelSaved")}
            </option>
          )}
          {shown.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label === m.id ? m.id : `${m.label} — ${m.id}`}
            </option>
          ))}
          <option value={OTHER}>{t("settings.modelOther")}</option>
        </select>
        <button
          type="button"
          className="icon"
          aria-label={t("settings.modelRefresh")}
          title={t("settings.modelRefresh")}
          onClick={refresh}
          disabled={loading}
        >
          {loading ? (
            <DotLoader pattern="spiral" size="sm" />
          ) : (
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M13 8a5 5 0 1 1-1.5-3.5M13 3v3h-3" />
            </svg>
          )}
        </button>
      </div>
      {typing && (
        <input
          dir="ltr"
          aria-label={t("settings.modelCustom")}
          placeholder={t("settings.modelCustom")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      )}
      {list && !list.from_service && (
        <p className="note" role="status">
          {list.error === "no_base_url"
            ? t("settings.modelNoBaseUrl")
            : models.length > 0
              ? t("settings.modelBuiltin", { why: list.error ?? "" })
              : t("settings.modelNone", { why: list.error ?? "" })}
        </p>
      )}
      {list?.from_service && value && !known && (
        <p className="note" role="status">
          {t("settings.modelMissing", { model: value })}
        </p>
      )}
    </div>
  );
}
