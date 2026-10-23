import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { Drafting, SnapKind } from "../api";
import { useT } from "../i18n";

/** The snap modes, in the order the dialog lists them. */
export const SNAP_KINDS: SnapKind[] = [
  "end",
  "mid",
  "centre",
  "node",
  "intersection",
  "perpendicular",
  "nearest",
];

const PRECISIONS = [2, 3, 4];

type Props = {
  drafting: Drafting;
  /** Saves the settings; a rejection is shown in the dialog, which stays open. */
  onSave(drafting: Drafting): Promise<void>;
  onClose(): void;
};

/** Drafting settings: which object snaps are on, ortho, polar tracking and its angle, the grid and
 *  its spacing and snap, and the decimals the take-off and the status bar show. */
export function DraftingSettings({ drafting, onSave, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState<Drafting>(drafting);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // A modal dialog moves focus inside; give it back to the button that opened it on close.
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  const change = (patch: Partial<Drafting>) => setDraft((d) => ({ ...d, ...patch }));

  async function save() {
    try {
      await onSave(draft);
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings drafting-settings"
      aria-labelledby="drafting-title"
      onClose={onClose}
      // Keys typed here are not shortcuts for the drawing behind (Escape still closes the dialog).
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 id="drafting-title" className="heading">
          {t("drafting")}
        </h2>
        <fieldset>
          <legend className="heading">{t("drafting.snaps")}</legend>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.osnap}
              onChange={(e) => change({ osnap: e.target.checked })}
            />
            <span>{t("drafting.osnapTitle")}</span>
          </label>
          <div className="modes">
            {SNAP_KINDS.map((kind) => (
              <label key={kind} className="check">
                <input
                  type="checkbox"
                  checked={draft.snap_modes[kind]}
                  onChange={(e) =>
                    change({ snap_modes: { ...draft.snap_modes, [kind]: e.target.checked } })
                  }
                />
                <span>{t(`snap.${kind}`)}</span>
              </label>
            ))}
          </div>
        </fieldset>
        <fieldset>
          <legend className="heading">{t("drafting.tracking")}</legend>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.ortho}
              onChange={(e) => change({ ortho: e.target.checked })}
            />
            <span>{t("drafting.orthoTitle")}</span>
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.polar}
              onChange={(e) => change({ polar: e.target.checked })}
            />
            <span>{t("drafting.polarTitle")}</span>
          </label>
          <label>
            <span>{t("drafting.polarStep")}</span>
            <input
              dir="ltr"
              type="number"
              min={1}
              max={180}
              step="any"
              value={draft.polar_step_deg}
              onChange={(e) => change({ polar_step_deg: Number(e.target.value) })}
            />
          </label>
        </fieldset>
        <fieldset>
          <legend className="heading">{t("drafting.gridTitle")}</legend>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.grid}
              onChange={(e) => change({ grid: e.target.checked })}
            />
            <span>{t("drafting.gridShow")}</span>
          </label>
          <label>
            <span>{t("drafting.gridSpacing")}</span>
            <input
              dir="ltr"
              type="number"
              min={0}
              step="any"
              value={draft.grid_spacing}
              onChange={(e) => change({ grid_spacing: Number(e.target.value) })}
            />
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.grid_snap}
              onChange={(e) => change({ grid_snap: e.target.checked })}
            />
            <span>{t("drafting.gridSnap")}</span>
          </label>
        </fieldset>
        <label>
          <span>{t("drafting.precision")}</span>
          <select
            value={draft.precision}
            onChange={(e) => change({ precision: Number(e.target.value) })}
          >
            {PRECISIONS.map((n) => (
              <option key={n} value={n}>
                {t("drafting.decimals", { n })}
              </option>
            ))}
          </select>
        </label>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary">
            {t("settings.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
