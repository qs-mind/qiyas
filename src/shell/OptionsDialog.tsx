import { uiError } from "../i18n/errors";
/** OPTIONS: how values show with no drawing open, the snap aperture and pick box, the canvas's
 *  colours and the keyboard shortcuts, kept in `settings.json` (the core checks them again). The
 *  autosave interval stays the Settings dialog's: shown here with a way there. */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { getSettings, setSettings, type Settings } from "../api";
import { useT } from "../i18n";
import { normalizeDigits } from "../tools/commandParser";
import { BUILT_IN_SHORTCUTS, keyOf, normalizeKey, shortcutClash } from "../tools/shortcuts";
import { DEFAULT_RIBBON, QUICK_ACCESS } from "./ribbonLayout";

/** The core's ranges (`settings::validate_options`). */
const PRECISIONS = [2, 3, 4];
const MIN_PX = 1;
const MAX_PX = 50;

type ColourKey = "background_color" | "crosshair_color" | "highlight_color";
const COLOURS: [ColourKey, string][] = [
  ["background_color", "options.background"],
  ["crosshair_color", "options.crosshair"],
  ["highlight_color", "options.highlight"],
];
/** What the colour picker starts from when a theme default is turned off. */
const PICKER_START: Record<ColourKey, string> = {
  background_color: "#f9f8f4",
  crosshair_color: "#0e0f12",
  highlight_color: "#6e4a9e",
};

/** The commands a key can run, each with its label's i18n key: the built-in keys' (labelled as
 *  their ribbon or quick-access button when they have one) and every tool on the ribbon, which is
 *  what the keyboard handler knows how to run. */
export const SHORTCUT_COMMANDS: [string, string][] = (() => {
  const buttons = [...QUICK_ACCESS, ...DEFAULT_RIBBON.tabs.flatMap((x) => x.groups.flatMap((g) => g.buttons))];
  const ribbonLabel = (command: string) => buttons.find((b) => b.command === command)?.label;
  const labels = new Map<string, string>();
  for (const [command] of BUILT_IN_SHORTCUTS) labels.set(command, ribbonLabel(command) ?? `shortcut.${command}`);
  for (const b of buttons) if (b.kind === "tool" && !labels.has(b.command)) labels.set(b.command, b.label);
  return [...labels];
})();
const LABELS = new Map(SHORTCUT_COMMANDS);
const builtInKeys = (command: string) => BUILT_IN_SHORTCUTS.filter(([c]) => c === command).map(([, k]) => k);

export function OptionsDialog({
  onSaved,
  onSettings,
  onClose,
}: {
  /** The settings saved, as written. */
  onSaved(settings: Settings): void;
  /** Opens the Settings dialog (the autosave interval); this one closes. */
  onSettings(): void;
  onClose(): void;
}) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  /** The pixel sizes as typed. */
  const [snapPx, setSnapPx] = useState("");
  const [pickPx, setPickPx] = useState("");
  const [shortcuts, setShortcuts] = useState<Record<string, string>>({});
  const [newCommand, setNewCommand] = useState("");
  const [newKey, setNewKey] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    getSettings()
      .then((s) => {
        setDraft(s);
        setSnapPx(String(s.snap_aperture_px ?? 10));
        setPickPx(String(s.pick_box_px ?? 6));
        setShortcuts({ ...(s.shortcuts ?? {}) });
      })
      .catch((e) => setError(uiError(e)));
    return () => opener?.focus();
  }, []);

  const change = (patch: Partial<Settings>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const label = (command: string) => t(LABELS.get(command) ?? command);

  /** `key` for `command`, unless another command has it. */
  function bind(command: string, key: string | null, next: Record<string, string> = shortcuts): boolean {
    if (!key) return false;
    const tried = { ...next, [command]: key };
    const clash = shortcutClash(tried);
    if (clash) {
      setError(t("options.keyTaken", { key: clash.key, command: label(clash.commands[0] === command ? clash.commands[1] : clash.commands[0]) }));
      return false;
    }
    setError(null);
    setShortcuts(tried);
    return true;
  }

  /** A key pressed in a key cell: the physical key with its modifiers. Tab alone moves on. */
  function pressed(e: KeyboardEvent, then: (key: string) => void) {
    if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) return;
    e.preventDefault();
    const key = keyOf(e.nativeEvent);
    if (key) then(key);
  }

  function withoutKey(command: string) {
    const next = { ...shortcuts };
    delete next[command];
    setShortcuts(next);
    setError(null);
  }

  async function save() {
    if (!draft) return;
    const px = [snapPx, pickPx].map((v) => Number(normalizeDigits(v.trim())));
    if (px.some((n) => !Number.isInteger(n) || n < MIN_PX || n > MAX_PX)) {
      setError(t("options.pxRange"));
      return;
    }
    const clash = shortcutClash(shortcuts);
    if (clash) {
      setError(t("options.keyTaken", { key: clash.key, command: label(clash.commands[0]) }));
      return;
    }
    const next: Settings = { ...draft, snap_aperture_px: px[0], pick_box_px: px[1], shortcuts };
    try {
      await setSettings(next);
      onSaved(next);
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    }
  }

  // The rows: commands with a key, built in or the user's, in the built-in order then the user's.
  const rows = [...new Set([...BUILT_IN_SHORTCUTS.map(([c]) => c), ...Object.keys(shortcuts)])];
  const free = SHORTCUT_COMMANDS.filter(([c]) => !rows.includes(c));

  return (
    <dialog
      ref={dialog}
      className="settings options"
      aria-labelledby="options-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        // The ranges are said in the window's words, not the browser's.
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 id="options-title" className="heading">
          {t("options.title")}
        </h2>
        {draft && (
          <>
            <fieldset>
              <legend className="heading">{t("options.display")}</legend>
              <label>
                <span>{t("options.precision")}</span>
                <select
                  value={draft.display_precision ?? 3}
                  onChange={(e) => change({ display_precision: Number(e.target.value) })}
                >
                  {PRECISIONS.map((n) => (
                    <option key={n} value={n}>
                      {(0).toFixed(n)}
                    </option>
                  ))}
                </select>
              </label>
              <p className="status">{t("options.precisionNote")}</p>
              <div className="field">
                <span>{t("options.autosave", { n: draft.autosave_minutes ?? 5 })}</span>
                <span className="row">
                  <button
                    type="button"
                    onClick={() => {
                      dialog.current?.close();
                      onSettings();
                    }}
                  >
                    {t("options.toSettings")}
                  </button>
                </span>
              </div>
            </fieldset>
            <fieldset>
              <legend className="heading">{t("options.cursor")}</legend>
              <label>
                <span>{t("options.snapAperture")}</span>
                <input dir="ltr" type="number" min={MIN_PX} max={MAX_PX} value={snapPx} onChange={(e) => setSnapPx(e.target.value)} />
              </label>
              <label>
                <span>{t("options.pickBox")}</span>
                <input dir="ltr" type="number" min={MIN_PX} max={MAX_PX} value={pickPx} onChange={(e) => setPickPx(e.target.value)} />
              </label>
            </fieldset>
            <fieldset>
              <legend className="heading">{t("options.colours")}</legend>
              {COLOURS.map(([key, name]) => {
                const value = draft[key] ?? null;
                return (
                  <div key={key} className="field">
                    <span>{t(name)}</span>
                    <span className="row">
                      <label className="check">
                        <input
                          type="checkbox"
                          checked={value === null}
                          aria-label={t("options.themeDefaultOf", { name: t(name) })}
                          onChange={(e) => change({ [key]: e.target.checked ? null : PICKER_START[key] })}
                        />
                        <span>{t("options.themeDefault")}</span>
                      </label>
                      <input
                        type="color"
                        aria-label={t("options.colourOf", { name: t(name) })}
                        disabled={value === null}
                        value={value ?? PICKER_START[key]}
                        onChange={(e) => change({ [key]: e.target.value.toLowerCase() })}
                      />
                    </span>
                    {key === "crosshair_color" && <span className="status">{t("options.crosshairLater")}</span>}
                  </div>
                );
              })}
            </fieldset>
            <fieldset>
              <legend className="heading">{t("options.shortcuts")}</legend>
              <p className="status">{t("options.shortcutsNote")}</p>
              <table className="manage-table">
                <thead>
                  <tr>
                    <th>{t("options.command")}</th>
                    <th>{t("options.key")}</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((command) => {
                    const builtIn = builtInKeys(command);
                    const mine = shortcuts[command];
                    return (
                      <tr key={command}>
                        <td>
                          <bdi>{label(command)}</bdi>
                        </td>
                        <td>
                          <input
                            dir="ltr"
                            readOnly
                            aria-label={t("options.keyFor", { command: label(command) })}
                            value={mine !== undefined ? (normalizeKey(mine) ?? mine) : builtIn.join(" / ")}
                            onKeyDown={(e) => pressed(e, (key) => bind(command, key))}
                          />
                        </td>
                        <td>
                          {mine !== undefined && (
                            <button
                              type="button"
                              aria-label={t(builtIn.length > 0 ? "options.builtInFor" : "options.removeFor", { command: label(command) })}
                              onClick={() => withoutKey(command)}
                            >
                              {t(builtIn.length > 0 ? "options.builtIn" : "options.remove")}
                            </button>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                  <tr>
                    <td>
                      <select aria-label={t("options.command")} value={newCommand} onChange={(e) => setNewCommand(e.target.value)}>
                        <option value="" />
                        {free.map(([command]) => (
                          <option key={command} value={command}>
                            {label(command)}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <input
                        dir="ltr"
                        readOnly
                        aria-label={t("options.newKey")}
                        placeholder={t("options.pressKey")}
                        value={newKey}
                        onKeyDown={(e) => pressed(e, setNewKey)}
                      />
                    </td>
                    <td>
                      <button
                        type="button"
                        disabled={newCommand === "" || newKey === ""}
                        onClick={() => {
                          if (!bind(newCommand, newKey)) return;
                          setNewCommand("");
                          setNewKey("");
                        }}
                      >
                        {t("options.add")}
                      </button>
                    </td>
                  </tr>
                </tbody>
              </table>
            </fieldset>
          </>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!draft}>
            {t("settings.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
