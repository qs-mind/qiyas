import { uiError } from "../i18n/errors";
/** PURGE: a dry run lists, by kind, what nothing in the drawing uses; the kinds ticked are then
 *  taken out as one undo step, and the dialog says what went. The window does not open it while a
 *  block is edited in place (`useOptions`); an error from the core is shown. */
import { useEffect, useState } from "react";
import { PURGE_KINDS, type Command, type Outcome, type PurgeKind, type PurgeReport } from "../api";
import { useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { notifyUndo, useUndoLabel } from "../ui/notices";

type Props = {
  /** Runs a command that changes nothing (the dry run). */
  look(cmd: Command): Promise<Outcome>;
  /** Runs a command that changes the drawing (the purge itself), redrawing after it. */
  apply(cmd: Command): Promise<Outcome>;
  /** Takes the purge back: the app's undo, offered in a toast for a few seconds once the dialog is
   *  closed (the dialog itself is modal, and the toast could not be pressed under it). */
  onUndo?(): void;
  onClose(): void;
};

const reportOf = (o: Outcome) => (o.type === "purged" ? o.report : null);

/** How long the undo toast stays, in seconds. */
const UNDO_SECONDS = 8;

export function PurgeDialog({ look, apply, onUndo, onClose }: Props) {
  const t = useT();
  const undoLabel = useUndoLabel();
  const [found, setFound] = useState<PurgeReport | null>(null);
  const [gone, setGone] = useState<PurgeReport | null>(null);
  const [ticked, setTicked] = useState<Set<PurgeKind>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    look({ type: "purge", kinds: [...PURGE_KINDS], dry_run: true })
      .then((o) => {
        const r = reportOf(o);
        setFound(r);
        if (r) setTicked(new Set(PURGE_KINDS.filter((k) => r[k].length > 0 || (k === "groups" && r.group_members > 0))));
      })
      .catch((e) => setError(uiError(e)));
  }, []);

  async function purge() {
    setBusy(true);
    try {
      setGone(reportOf(await apply({ type: "purge", kinds: PURGE_KINDS.filter((k) => ticked.has(k)), dry_run: false })));
      setError(null);
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  /** Closes it; what it purged can be taken back for a few seconds from a toast. */
  function close() {
    if (gone && gone.total > 0 && onUndo) {
      notifyUndo({ message: t("purge.done", { n: gone.total }), name: t("undo"), label: undoLabel, seconds: UNDO_SECONDS, onUndo });
    }
    onClose();
  }

  const shown = gone ?? found;
  const count = (r: PurgeReport, k: PurgeKind) => r[k].length;

  return (
    <ModalDialog className="purge" labelledBy="purge-title" onClose={close}>
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          purge();
        }}
      >
        <h2 id="purge-title" className="heading">
          {t("purge.title")}
        </h2>
        {found?.limited_reason && (
          <p className="status" dir="auto">
            {found.limited_reason}
          </p>
        )}
        {gone && <p role="status">{t("purge.done", { n: gone.total })}</p>}
        {!gone && found && found.total === 0 && <p className="status">{t("purge.nothing")}</p>}
        {shown &&
          PURGE_KINDS.filter((k) => !gone || count(shown, k) > 0 || (k === "groups" && shown.group_members > 0)).map((k) => {
            const name = `${t(`purge.kind.${k}`)} (${count(shown, k)})`;
            const empty = count(shown, k) === 0 && !(k === "groups" && shown.group_members > 0);
            return (
              <fieldset key={k}>
                <legend>
                  {gone ? (
                    name
                  ) : (
                    <label className="check">
                      <input
                        type="checkbox"
                        disabled={empty}
                        checked={ticked.has(k)}
                        onChange={(e) =>
                          setTicked((s) => {
                            const next = new Set(s);
                            if (e.target.checked) next.add(k);
                            else next.delete(k);
                            return next;
                          })
                        }
                      />
                      <span>{name}</span>
                    </label>
                  )}
                </legend>
                {shown[k].length > 0 && (
                  <ul className="purge-names">
                    {shown[k].map((n) => (
                      <li key={n}>
                        <bdi>{n}</bdi>
                      </li>
                    ))}
                  </ul>
                )}
                {k === "groups" && shown.group_members > 0 && (
                  <p className="status">{t("purge.members", { n: shown.group_members })}</p>
                )}
              </fieldset>
            );
          })}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={close}>
            {t(gone ? "purge.close" : "settings.cancel")}
          </button>
          {!gone && (
            <button type="submit" className="primary" disabled={!found || busy || ticked.size === 0}>
              {t("purge.run")}
            </button>
          )}
        </div>
      </form>
    </ModalDialog>
  );
}
