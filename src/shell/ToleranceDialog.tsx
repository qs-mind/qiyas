import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import { useT } from "../i18n";
import { toleranceFrame } from "../tools/dimTools";

/** GDT symbols by their letter in AutoCAD's GDT font. */
const SYMBOLS: [string, string][] = [
  ["", "dim.tol.none"],
  ["j", "dim.tol.position"],
  ["c", "dim.tol.flatness"],
  ["u", "dim.tol.straightness"],
  ["b", "dim.tol.perpendicularity"],
  ["f", "dim.tol.parallelism"],
];

type Props = {
  /** Places the frame (AutoCAD's codes); an error stays in the dialog. */
  onSubmit(frame: string): Promise<void>;
  onClose(): void;
};

/** TOLERANCE: a feature control frame's symbol, tolerance and up to three datums. */
export function ToleranceDialog({ onSubmit, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [symbol, setSymbol] = useState("j");
  const [value, setValue] = useState("");
  const [datums, setDatums] = useState(["", "", ""]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function submit() {
    if (value.trim() === "" || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(toleranceFrame(symbol, value, datums));
      dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings"
      aria-labelledby="tolerance-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <h2 id="tolerance-title" className="heading">
          {t("dim.tol.title")}
        </h2>
        <label>
          <span>{t("dim.tol.symbol")}</span>
          <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
            {SYMBOLS.map(([letter, key]) => (
              <option key={letter} value={letter}>
                {t(key)}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>{t("dim.tol.value")}</span>
          <input dir="auto" autoFocus value={value} onChange={(e) => setValue(e.target.value)} />
        </label>
        {datums.map((d, i) => (
          <label key={i}>
            <span>{t("dim.tol.datum", { n: i + 1 })}</span>
            <input
              dir="auto"
              value={d}
              onChange={(e) => setDatums(datums.map((x, j) => (j === i ? e.target.value : x)))}
            />
          </label>
        ))}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("text.cancel")}
          </button>
          <button type="submit" className="primary" disabled={value.trim() === "" || busy}>
            {t("text.place")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
