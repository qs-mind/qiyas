/** New drawing: asks its units (metres, centimetres or millimetres), starting on the last ones
 *  chosen (`qiyas.newUnits` in this computer's storage), then makes it. */
import { useState } from "react";
import type { NewUnits } from "../api";
import { useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { SegmentedChoice } from "../ui/Segmented";

const KEY = "qiyas.newUnits";
const UNITS: NewUnits[] = ["m", "cm", "mm"];

/** The units the last new drawing was made in; metres the first time. */
export function lastNewUnits(): NewUnits {
  try {
    const kept = localStorage.getItem(KEY);
    return UNITS.find((u) => u === kept) ?? "m";
  } catch {
    return "m";
  }
}

type Props = {
  onCreate(units: NewUnits): void;
  onClose(): void;
};

export function NewDrawingDialog({ onCreate, onClose }: Props) {
  const t = useT();
  const [units, setUnits] = useState<NewUnits>(lastNewUnits);
  return (
    <ModalDialog className="new-drawing" labelledBy="new-drawing-title" onClose={onClose}>
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          try {
            localStorage.setItem(KEY, units);
          } catch {
            // Kept for next time only; nothing lost without it.
          }
          onCreate(units);
        }}
      >
        <h2 id="new-drawing-title" className="heading">
          {t("newDrawing.title")}
        </h2>
        <p>{t("newDrawing.units")}</p>
        <SegmentedChoice<NewUnits>
          label={t("newDrawing.units")}
          value={units}
          onChange={setUnits}
          segments={UNITS.map((u) => ({ value: u, label: t(`newDrawing.unit.${u}`) }))}
        />
        <div className="buttons">
          <button type="button" onClick={onClose}>
            {t("newDrawing.cancel")}
          </button>
          <button type="submit" className="primary">
            {t("newDrawing.create")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
