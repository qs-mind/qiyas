import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { TextMatch, TextScope } from "../api";
import { useT } from "../i18n";

/** What to look for, as `find_text` and `find_replace` take it. */
export type FindQuery = { find: string; scope: TextScope; match_case: boolean; whole_word: boolean };

type Props = {
  /** The matches of `query`: how many, and the entities holding them. */
  onFind(query: FindQuery): Promise<{ count: number; matches: TextMatch[] }>;
  /** Replaces every match with `replace` (one undo step): how many were replaced, and how many
   *  texts were left as they were (a text can't be left blank or break onto two lines). */
  onReplaceAll(query: FindQuery, replace: string): Promise<{ count: number; skipped: number }>;
  /** Shows one entity holding a match: its sheet, zoomed to it. */
  onShow(match: TextMatch): void;
  onClose(): void;
};

/** FIND: find and replace over texts, MText, attributes and dimension overrides, on this sheet or
 *  the whole drawing. "Find next" shows each entity holding a match in turn; "Replace all"
 *  replaces them all in one undo step and says how many. Not modal: the drawing stays usable. */
export function FindReplaceDialog({ onFind, onReplaceAll, onShow, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [find, setFind] = useState("");
  const [replace, setReplace] = useState("");
  const [scope, setScope] = useState<TextScope>("sheet");
  const [matchCase, setMatchCase] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  /** The last find's matches, and which one is shown; dropped when the query changes. */
  const [found, setFound] = useState<{ count: number; matches: TextMatch[]; at: number } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const query: FindQuery = { find, scope, match_case: matchCase, whole_word: wholeWord };

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.show();
    return () => opener?.focus();
  }, []);

  // A changed query finds again from the start.
  useEffect(() => {
    setFound(null);
    setStatus(null);
  }, [find, scope, matchCase, wholeWord]);

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

  const findNext = () =>
    act(async () => {
      let next = found ? { ...found, at: (found.at + 1) % Math.max(found.matches.length, 1) } : null;
      if (!next) {
        const result = await onFind(query);
        next = { ...result, at: 0 };
      }
      setFound(next);
      if (next.matches.length === 0) {
        setStatus(t("find.none"));
        return;
      }
      setStatus(t("find.at", { i: next.at + 1, n: next.matches.length, count: next.count }));
      onShow(next.matches[next.at]);
    });

  const replaceAll = () =>
    act(async () => {
      const { count, skipped } = await onReplaceAll(query, replace);
      setFound(null);
      const parts = [count === 0 && skipped === 0 ? t("find.none") : t("find.replaced", { n: count })];
      if (skipped > 0) parts.push(t("find.skipped", { n: skipped }));
      setStatus(parts.join(" · "));
    });

  const ready = find !== "" && !busy;

  return (
    <dialog
      ref={dialog}
      className="settings find-replace"
      aria-labelledby="find-title"
      onClose={onClose}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") dialog.current?.close();
      }}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          if (ready) findNext();
        }}
      >
        <h2 id="find-title" className="heading">
          {t("find.title")}
        </h2>
        <label>
          <span>{t("find.find")}</span>
          <input dir="auto" autoFocus value={find} onChange={(e) => setFind(e.target.value)} />
        </label>
        <label>
          <span>{t("find.replace")}</span>
          <input dir="auto" value={replace} onChange={(e) => setReplace(e.target.value)} />
        </label>
        <label>
          <span>{t("find.scope")}</span>
          <select value={scope} onChange={(e) => setScope(e.target.value as TextScope)}>
            <option value="sheet">{t("find.sheet")}</option>
            <option value="drawing">{t("find.drawing")}</option>
          </select>
        </label>
        <label className="check">
          <input type="checkbox" checked={matchCase} onChange={(e) => setMatchCase(e.target.checked)} />
          <span>{t("find.matchCase")}</span>
        </label>
        <label className="check">
          <input type="checkbox" checked={wholeWord} onChange={(e) => setWholeWord(e.target.checked)} />
          <span>{t("find.wholeWord")}</span>
        </label>
        {status && (
          <p className="status" role="status">
            {status}
          </p>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("find.close")}
          </button>
          <button type="button" disabled={!ready} onClick={replaceAll}>
            {t("find.replaceAll")}
          </button>
          <button type="submit" className="primary" disabled={!ready}>
            {t("find.next")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
