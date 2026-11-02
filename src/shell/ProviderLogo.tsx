import type { CSSProperties } from "react";
import anthropic from "./provider-marks/anthropic.svg";
import openai from "./provider-marks/openai.svg";
import claude from "./provider-marks/claude.svg";
import type { Provider } from "../api";

export function ProviderLogo({ provider }: { provider: Provider }) {
  const mark = provider === "anthropic" ? anthropic : provider === "claude_code" ? claude : openai;
  return <span className="settings-provider-logo" aria-hidden="true" style={{ "--provider-mark": `url("${mark}")` } as CSSProperties} />;
}
