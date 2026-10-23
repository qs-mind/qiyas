import { entityKind } from "../i18n/entityKind";
import { uiError } from "../i18n/errors";
import { useState, type ReactNode } from "react";
import type { ReadReport, Recoverable } from "../api";
import { useLanguage, useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { localTime, refusedBy } from "./recoveryQueue";

/** The file name and the folder of `path`. */
function split(path: string): [string, string] {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at < 0 ? [path, ""] : [path.slice(at + 1), path.slice(0, at)];
}

/** `text` (a translation with `{name}` holes) with each hole filled from `vars` in its own
 *  `<bdi>`, so a date, a PID or a computer's name keeps its own order inside Arabic. */
function fill(text: string, vars: Record<string, string | number>): ReactNode[] {
  return text
    .split(/\{(\w+)\}/)
    .map((part, i) => (i % 2 === 1 ? <bdi key={i}>{vars[part] ?? `{${part}}`}</bdi> : part));
}

type Props = {
  /** The autosaves to offer back (`recoverableDrawings`), crashed ones first (`enqueue`). */
  items: Recoverable[];
  /** Opens the autosave of the drawing (or its older `copy`) as a new drawing; an error stays in
   *  the dialog. */
  onRecover(drawing: string, copy?: string): Promise<void>;
  /** Deletes the autosave of the drawing (with nothing autosaved: the crash's dead lock); a
   *  refusal (another Qiyas took the drawing meanwhile) stays in the dialog. */
  onDiscard(drawing: string, copy?: string): Promise<void>;
  /** Opens the drawing itself, open in another Qiyas: its autosave is that Qiyas's, left alone. */
  onOpen(drawing: string): Promise<void>;
  onClose(): void;
};

/** On launch and on open: each autosave newer than its drawing, or left when Qiyas closed
 *  unexpectedly (then whatever its age: the row says when it is older than the drawing), to
 *  recover (opened as a new drawing beside it, then Save As) or discard (deleted), and under it
 *  the older copies earlier crashes left, each to recover or discard. A drawing open in another
 *  Qiyas is warned about and only opens anyway (or Later): Discard would delete that Qiyas's
 *  autosave. Times are in the app's language. */
export function RecoveryDialog({ items, onRecover, onDiscard, onOpen, onClose }: Props) {
  const t = useT();
  const { lang } = useLanguage();
  const crashedRow = (i: Recoverable) => i.crashed && i.autosave != null;
  const crashed = items.some(crashedRow);
  /** Something to recover or discard beside older copies. */
  const saved = items.some((i) => i.autosave != null);
  /** Crashed and newer drawings together: the heading speaks of the crash, rows mark their own. */
  const mixed = crashed && items.some((i) => !crashedRow(i));
  const heading = crashed
    ? "recovery.crashedTitle"
    : saved
      ? "recovery.title"
      : items.some((i) => i.locked_elsewhere)
        ? "recovery.lockedTitle"
        : "recovery.copiesTitle";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const time = (secs: number) => localTime(secs, lang);

  async function act(f: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      const held = refusedBy(String(e));
      setError(held ? t("recovery.refused", held) : uiError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    // Escape waits, as Later does, while Recover or Discard is working.
    <ModalDialog className="recovery" labelledBy="recovery-title" dismissible={!busy} onClose={onClose}>
      <h2 id="recovery-title" className="heading">
        {t(heading)}
      </h2>
      {saved && <p>{t("recovery.intro")}</p>}
      <ul className="recovery-list">
        {items.map((item, n) => {
          const [name, folder] = split(item.drawing);
          const copies = item.older_copies ?? [];
          return (
            <li key={item.drawing}>
              <span className="recovery-file" dir="auto">
                <strong>{name}</strong> <span className="muted">{folder}</span>
              </span>
              {mixed && crashedRow(item) && <span className="warn">{t("recovery.crashedItem")}</span>}
              {item.saved_at != null && (
                <span className="muted">{fill(t("recovery.savedAt"), { time: time(item.saved_at) })}</span>
              )}
              <span className="muted">
                {item.drawing_at == null
                  ? t("recovery.drawingGone")
                  : fill(t("recovery.drawingAt"), { time: time(item.drawing_at) })}
              </span>
              {item.drawing_at != null && item.saved_at != null && item.saved_at < item.drawing_at && (
                <span className="warn">{t("recovery.older")}</span>
              )}
              <span className="actions">
                {item.locked_elsewhere ? (
                  <button type="button" disabled={busy} onClick={() => act(() => onOpen(item.drawing))}>
                    {t("recovery.openAnyway")}
                  </button>
                ) : (
                  item.autosave != null && (
                    <>
                      <button type="button" disabled={busy} onClick={() => act(() => onRecover(item.drawing))}>
                        {t("recovery.recover")}
                      </button>
                      <button type="button" disabled={busy} onClick={() => act(() => onDiscard(item.drawing))}>
                        {t("recovery.discard")}
                      </button>
                    </>
                  )
                )}
              </span>
              {item.locked_elsewhere && (
                <p className="recovery-warning" role="note">
                  {fill(t("recovery.lockedElsewhere"), {
                    pid: item.locked_elsewhere.pid,
                    host: item.locked_elsewhere.host,
                  })}
                </p>
              )}
              {copies.length > 0 && (
                <div className="recovery-copies">
                  <span id={`recovery-copies-${n}`} className="muted">
                    {t("recovery.olderCopies")}
                  </span>
                  <ul aria-labelledby={`recovery-copies-${n}`}>
                    {copies.map((c) => (
                      <li key={c.path}>
                        <span className="muted">
                          {fill(t(c.kind === "instance" ? "recovery.copyInstance" : "recovery.copyCrash"), {
                            time: time(c.saved_at),
                          })}
                        </span>
                        <span className="actions">
                          <button type="button" disabled={busy} onClick={() => act(() => onRecover(item.drawing, c.path))}>
                            {t("recovery.recover")}
                          </button>
                          <button type="button" disabled={busy} onClick={() => act(() => onDiscard(item.drawing, c.path))}>
                            {t("recovery.discard")}
                          </button>
                        </span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="button" disabled={busy} onClick={onClose}>
          {t("recovery.later")}
        </button>
      </div>
    </ModalDialog>
  );
}

/** RECOVER: what the reader made of a damaged drawing, and what it dropped by kind. */
export function RecoverReportDialog({
  path,
  report,
  onClose,
}: {
  path: string;
  report: ReadReport;
  onClose(): void;
}) {
  const t = useT();
  const [name] = split(path);
  return (
    <ModalDialog className="recovery" labelledBy="recover-report-title" onClose={onClose}>
      <h2 id="recover-report-title" className="heading">
        {t("recover.title", { name })}
      </h2>
      <p>
        {t("recover.summary", {
          entities: report.entities,
          decoded: report.decoded,
          records: report.records,
          skipped: report.skipped,
        })}
      </p>
      {report.dropped.length === 0 ? (
        <p>{t("recover.nothingDropped")}</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>{t("recover.kind")}</th>
              <th>{t("recover.count")}</th>
            </tr>
          </thead>
          <tbody>
            {report.dropped.map((d) => (
              <tr key={d.kind}>
                <td>{entityKind(d.kind, t)}</td>
                <td>{d.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="actions">
        <button type="button" onClick={onClose}>
          {t("recover.close")}
        </button>
      </div>
    </ModalDialog>
  );
}
