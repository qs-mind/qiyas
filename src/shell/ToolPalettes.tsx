import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { PaletteItem } from "../api";
import { useT } from "../i18n";

type Props = {
  items: PaletteItem[];
  /** A button was clicked: insert its block, or run its command line. */
  onRun(item: PaletteItem): void;
  /** Keeps the palette as it is now (in the settings). */
  onSave(items: PaletteItem[]): Promise<void>;
  onClose(): void;
};

/** The user's own palette of blocks and commands: click one to use it, add or remove buttons. */
export function ToolPalettes({ items, onRun, onSave, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [list, setList] = useState(items);
  const [kind, setKind] = useState<PaletteItem["kind"]>("block");
  const [label, setLabel] = useState("");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function keep(next: PaletteItem[]) {
    setError(null);
    try {
      await onSave(next);
      setList(next);
    } catch (e) {
      setError(uiError(e));
    }
  }

  const valid = label.trim() !== "" && value.trim() !== "";

  return (
    <dialog
      ref={dialog}
      className="settings tool-palettes"
      aria-labelledby="tool-palettes-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <h2 id="tool-palettes-title" className="heading">
        {t("palette.title")}
      </h2>
      {list.length === 0 && <p>{t("palette.empty")}</p>}
      <ul className="palette-items">
        {list.map((item, i) => (
          <li key={`${i}-${item.label}`}>
            <button
              type="button"
              dir="auto"
              onClick={() => {
                dialog.current?.close();
                onRun(item);
              }}
            >
              {item.label}
            </button>
            <button
              type="button"
              aria-label={t("palette.remove", { label: item.label })}
              onClick={() => keep(list.filter((_, j) => j !== i))}
            >
              ×
            </button>
          </li>
        ))}
      </ul>
      <form
        className="palette-add"
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid) return;
          keep([...list, { kind, label: label.trim(), value: value.trim() }]).then(() => {
            setLabel("");
            setValue("");
          });
        }}
      >
        <label>
          <span>{t("palette.kind")}</span>
          <select value={kind} onChange={(e) => setKind(e.target.value as PaletteItem["kind"])}>
            <option value="block">{t("palette.block")}</option>
            <option value="command">{t("palette.command")}</option>
          </select>
        </label>
        <label>
          <span>{t("palette.label")}</span>
          <input dir="auto" value={label} onChange={(e) => setLabel(e.target.value)} />
        </label>
        <label>
          <span>{t("palette.value")}</span>
          <input dir="auto" value={value} onChange={(e) => setValue(e.target.value)} />
        </label>
        <button type="submit" disabled={!valid}>
          {t("palette.add")}
        </button>
      </form>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="buttons">
        <button type="button" onClick={() => dialog.current?.close()}>
          {t("block.close")}
        </button>
      </div>
    </dialog>
  );
}
