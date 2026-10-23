import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { ParameterInfo } from "../api";
import { useT } from "../i18n";

type Props = {
  parameters: ParameterInfo[];
  /** Digits after the point the values show with. */
  precision: number;
  /** The parameter `name` takes `expression`; the drawing is solved again. An error (a circle,
   *  an unknown name, a conflict) stays in the dialog. */
  onSet(name: string, expression: string): Promise<void>;
  /** Deletes the parameter's constraint (and its Dimension). */
  onDelete(id: number): void;
  onClose(): void;
};

/** PARAMETERS: every dimensional constraint's parameter, with its expression (a number or a
 *  formula of others, `2*d2 + 1.5`) and its value now. Enter or leaving a changed expression
 *  sets it; the geometry follows. Lengths are in drawing units, angles in degrees. */
export function ParametersManager({ parameters, precision, onSet, onDelete, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  /** Expressions being typed, by name. */
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function commit(p: ParameterInfo) {
    const typed = drafts[p.name];
    if (typed === undefined || busy) return;
    if (typed.trim() === p.expression || typed.trim() === "") {
      setDrafts(({ [p.name]: _, ...rest }) => rest);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSet(p.name, typed.trim());
      setDrafts(({ [p.name]: _, ...rest }) => rest);
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings parameters"
      aria-labelledby="parameters-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <h2 id="parameters-title" className="heading">
        {t("param.manager")}
      </h2>
      {parameters.length === 0 ? (
        <p>{t("param.none")}</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th scope="col">{t("param.name")}</th>
              <th scope="col">{t("param.expression")}</th>
              <th scope="col">{t("param.value")}</th>
              <th scope="col">
                <span className="sr-only">{t("param.delete")}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {parameters.map((p) => (
              <tr key={p.id}>
                <th scope="row">
                  <bdi>{p.name}</bdi>
                </th>
                <td>
                  <input
                    dir="ltr"
                    aria-label={t("param.expressionOf", { name: p.name })}
                    value={drafts[p.name] ?? p.expression}
                    disabled={busy}
                    onChange={(e) => {
                      const value = e.target.value;
                      setDrafts((d) => ({ ...d, [p.name]: value }));
                    }}
                    onBlur={() => commit(p)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        commit(p);
                      }
                    }}
                  />
                </td>
                <td>
                  <bdi>
                    {p.value === null
                      ? "—"
                      : `${p.value.toFixed(precision)}${p.kind === "angular" ? "°" : ""}`}
                  </bdi>
                  {p.error && (
                    <p className="error" role="note">
                      {t("param.broken", { why: p.error })}
                    </p>
                  )}
                </td>
                <td>
                  <button
                    type="button"
                    aria-label={t("param.deleteOf", { name: p.name })}
                    title={t("param.deleteOf", { name: p.name })}
                    disabled={busy}
                    onClick={() => onDelete(p.id)}
                  >
                    ×
                  </button>
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
        <button type="button" className="primary" onClick={() => dialog.current?.close()}>
          {t("param.close")}
        </button>
      </div>
    </dialog>
  );
}
