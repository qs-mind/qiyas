import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { SheetSet, SheetSetView } from "../api";
import { useT } from "../i18n";

/** What the manager needs from the app: the core's sheet set commands and the file dialogs. */
export type SheetSetApi = {
  open(path: string): Promise<SheetSetView>;
  save(path: string, set: SheetSet): Promise<SheetSetView>;
  /** A new set of the open drawings in the folder of `qss`, and those outside it. */
  fromTabs(qss: string, name: string): Promise<{ set: SheetSet; skipped: string[] }>;
  /** The set's line for the drawing `drawing`, with all its layouts. */
  entry(qss: string, drawing: string): Promise<SheetSet["drawings"][number]>;
  /** Where a new `.qss` goes (a Save dialog); null when cancelled. */
  pickNew(): Promise<string | null>;
  /** A `.qss` to open; null when cancelled. */
  pickOpen(): Promise<string | null>;
  /** A drawing to add; null when cancelled. */
  pickDrawing(): Promise<string | null>;
};

type Props = {
  api: SheetSetApi;
  /** Opens these drawings, a tab each. */
  onOpenAll(paths: string[]): Promise<void>;
  onClose(): void;
};

/** The file name of `path` without its extension. */
const stem = (path: string) =>
  path
    .slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1)
    .replace(/\.[^.]*$/, "");

/** SHEETSET: a sheet set (`.qss`) of drawings and their layouts, kept beside them: make one from
 *  the open drawings or open one, add and remove drawings (each change is saved), open them all.
 *  Printing them all comes with plotting (Phase 4). */
export function SheetSetManager({ api, onOpenAll, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [path, setPath] = useState<string | null>(null);
  const [view, setView] = useState<SheetSetView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  async function act(f: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const create = () =>
    act(async () => {
      const qss = await api.pickNew();
      if (!qss) return;
      const { set, skipped } = await api.fromTabs(qss, stem(qss));
      setView(await api.save(qss, set));
      setPath(qss);
      setNotice(skipped.length > 0 ? t("sheetSet.skipped", { names: skipped.join(", ") }) : null);
    });

  const openSet = () =>
    act(async () => {
      const qss = await api.pickOpen();
      if (!qss) return;
      setView(await api.open(qss));
      setPath(qss);
      setNotice(null);
    });

  const change = (drawings: SheetSet["drawings"]) =>
    act(async () => {
      if (!path || !view) return;
      setView(await api.save(path, { ...view.set, drawings }));
    });

  const add = () =>
    act(async () => {
      if (!path || !view) return;
      const drawing = await api.pickDrawing();
      if (!drawing) return;
      const line = await api.entry(path, drawing);
      const others = view.set.drawings.filter((d) => d.path !== line.path);
      setView(await api.save(path, { ...view.set, drawings: [...others, line] }));
    });

  const openAll = () =>
    act(async () => {
      if (!view) return;
      await onOpenAll(view.entries.filter((e) => !e.missing).map((e) => e.full));
    });

  return (
    <dialog
      ref={dialog}
      className="settings sheet-set"
      aria-labelledby="sheet-set-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <h2 id="sheet-set-title" className="heading">
        {view ? t("sheetSet.titleOf", { name: view.set.name }) : t("sheetSet.title")}
      </h2>
      <div className="actions">
        <button type="button" disabled={busy} onClick={create}>
          {t("sheetSet.new")}
        </button>
        <button type="button" disabled={busy} onClick={openSet}>
          {t("sheetSet.open")}
        </button>
      </div>
      {view && (
        <>
          {path && (
            <p className="muted" dir="ltr">
              {path}
            </p>
          )}
          {view.entries.length === 0 ? (
            <p>{t("sheetSet.empty")}</p>
          ) : (
            <ul className="sheet-set-list">
              {view.entries.map((e, i) => (
                <li key={e.path}>
                  <span dir="ltr">{e.path}</span>{" "}
                  <span className="muted">
                    {e.layouts.length > 0 ? e.layouts.join(", ") : t("sheetSet.allLayouts")}
                  </span>
                  {e.missing && <span className="warn"> {t("sheetSet.missing")}</span>}
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => change(view.set.drawings.filter((_, k) => k !== i))}
                  >
                    {t("sheetSet.remove")}
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="actions">
            <button type="button" disabled={busy} onClick={add}>
              {t("sheetSet.add")}
            </button>
            <button type="button" disabled={busy || view.entries.every((e) => e.missing)} onClick={openAll}>
              {t("sheetSet.openAll")}
            </button>
            <button type="button" disabled title={t("sheetSet.printLater")}>
              {t("sheetSet.printAll")}
            </button>
          </div>
        </>
      )}
      {notice && <p role="status">{notice}</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="button" onClick={() => dialog.current?.close()}>
          {t("sheetSet.close")}
        </button>
      </div>
    </dialog>
  );
}
