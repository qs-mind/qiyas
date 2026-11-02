import { SegmentedChoice } from "../ui/Segmented";
import { useT } from "../i18n";
import { EFFORTS } from "../api";

export function EffortChoices({ values, value, onChange, disabled = false, defaultChoice = false }: {
  values: readonly string[];
  value: string;
  onChange(value: string): void;
  disabled?: boolean;
  defaultChoice?: boolean;
}) {
  const t = useT();
  return <div className="field settings-effort">
    <span>{t("settings.effort")}</span>
    <SegmentedChoice label={t("settings.effort")} value={value} onChange={onChange}
      segments={(defaultChoice ? ["", ...values] : values).map(level => ({
        value: level,
        label: level === "" ? t("settings.subscriptionDefault") : (EFFORTS as readonly string[]).includes(level) ? t(`effort.${level}`) : <bdi>{level}</bdi>,
        disabled,
      }))} />
  </div>;
}
