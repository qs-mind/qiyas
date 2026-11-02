import { useEffect, useState } from "react";
import { aiPictureSupport, type Provider } from "../api";
import { useT } from "../i18n";

/** Polls the local result; the core schedules and deduplicates background checks. */
export function PictureSupport({ provider, model, baseUrl = null, typedKey, executable, metadata, ready = true, account }: {
  provider: Provider;
  model: string;
  baseUrl?: string | null;
  typedKey?: string;
  executable?: string | null;
  metadata?: boolean | null;
  ready?: boolean;
  account?: string | null;
}) {
  const t = useT();
  const identity = JSON.stringify([provider, model, baseUrl, executable, account]);
  const [answer, setAnswer] = useState<{ identity: string; supported: boolean | null } | null>(null);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    setAnswer(null);
    async function check() {
      let supported: boolean | null = null;
      try {
        supported = await aiPictureSupport(provider, model, baseUrl, typedKey, executable);
        if (active) setAnswer({ identity, supported });
      } catch { /* Account and connection errors are shown by the provider controls. */ }
      if (active) timer = setTimeout(check, supported == null ? 2000 : 30000);
    }
    if (ready) timer = setTimeout(check, 500);
    return () => { active = false; clearTimeout(timer); };
  }, [identity, typedKey, ready]);
  const supported = answer?.identity === identity ? answer.supported : metadata;
  return <p className="muted picture-support" role="note">{t(supported === true ? "ai.visionAvailable" : supported === false ? "ai.visionUnavailable" : "ai.visionUnknown")}</p>;
}
