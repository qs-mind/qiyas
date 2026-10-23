import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { XrefInfo } from "../api";
import { useT } from "../i18n";

type Props = {
  xrefs: XrefInfo[];
  /** Looks for every xref again and reopens it; a rejection is shown in the dialog. */
  onReload(): Promise<void>;
  /** Asks for the file of the xref `name` and loads it from there. */
  onLocate(name: string): Promise<void>;
  /** Forgets the file chosen for the xref `name` and looks for it again. */
  onClear(name: string): Promise<void>;
  onClose(): void;
};

/** The drawing's external references: each one's name, whether it was found, missing,
 *  unreadable or is unloaded (with what went wrong), its file and whether it is an overlay;
 *  Reload, "Locate…" for one not loaded, and "Clear chosen file" for one the user located. */
export function XrefManager({ xrefs, onReload, onLocate, onClear, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    // A modal dialog moves focus inside; give it back to the button that opened it on close.
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function act(work: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const status = (x: XrefInfo) =>
    x.unloaded
      ? t("xref.unloaded")
      : x.loaded
        ? t("xref.found")
        : x.found
          ? t("xref.unreadable")
          : t("xref.missing");

  return (
    <dialog
      ref={dialog}
      className="settings xref-manager"
      aria-labelledby="xref-title"
      onClose={onClose}
      // Keys typed here are not shortcuts for the drawing behind (Escape still closes the dialog).
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form method="dialog" onSubmit={(e) => e.preventDefault()}>
        <h2 id="xref-title" className="heading">
          {t("xrefs")}
        </h2>
        {xrefs.length === 0 ? (
          <p className="status">{t("xref.none")}</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th scope="col">{t("xref.name")}</th>
                <th scope="col">{t("xref.status")}</th>
                <th scope="col">{t("xref.path")}</th>
                <th scope="col">
                  <span className="sr-only">{t("xref.locate")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {xrefs.map((x) => (
                <tr key={x.name} className={x.loaded ? undefined : "dim"}>
                  <td dir="ltr">
                    {x.name}
                    {x.overlay && <span className="badge">{t("xref.overlay")}</span>}
                  </td>
                  <td title={x.problems.join("\n") || undefined}>{status(x)}</td>
                  <td className="path" dir="ltr" title={x.path}>
                    {x.path}
                    {x.found && !x.loaded && x.problems.length > 0 && (
                      <small className="problem">{x.problems.join(" ")}</small>
                    )}
                  </td>
                  <td className="actions">
                    {!x.loaded && !x.unloaded && (
                      <button
                        type="button"
                        disabled={busy}
                        aria-label={`${t("xref.locate")}: ${x.name}`}
                        onClick={() => act(() => onLocate(x.name))}
                      >
                        {t("xref.locate")}
                      </button>
                    )}
                    {x.chosen && (
                      <button
                        type="button"
                        disabled={busy}
                        aria-label={`${t("xref.clear")}: ${x.name}`}
                        onClick={() => act(() => onClear(x.name))}
                      >
                        {t("xref.clear")}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" disabled={busy} onClick={() => act(onReload)}>
            {t("xref.reload")}
          </button>
          <button type="button" className="primary" onClick={() => dialog.current?.close()}>
            {t("about.close")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
