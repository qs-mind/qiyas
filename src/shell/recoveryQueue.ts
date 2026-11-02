import { locale } from "../i18n/locale";
/** The recovery questions waiting their turn: one per drawing about to open (`then`), or the
 *  launch one (`then` null), each shown alone in `RecoveryDialog`, first in, first asked; inside
 *  one, the drawings Qiyas crashed with come first. Pure. */
import type { Recoverable } from "../api";
import type { Lang } from "../i18n";

export type RecoveryAsk = { id: number; items: Recoverable[]; then: string | null };

let next = 1;

/** `q` with a question for `items` (opening `then` after), crashed ones first, each kind in the
 *  order given; `first` puts it ahead of the others. A drawing is asked about once: one already
 *  in `q` is left out (a drawing double-clicked at launch is found by the launch question and by
 *  its open), and when nothing is left `then` opens after the launch question holding it. */
export function enqueue(q: RecoveryAsk[], items: Recoverable[], then: string | null, first = false): RecoveryAsk[] {
  const asked = new Set(q.flatMap((a) => a.items.map((i) => i.drawing)));
  items = items.filter((i) => !asked.has(i.drawing));
  if (items.length === 0) {
    const k = q.findIndex((a) => a.then === null && a.items.some((i) => i.drawing === then));
    return then !== null && k >= 0 ? q.map((a, j) => (j === k ? { ...a, then } : a)) : q;
  }
  const ask = { id: next++, items: [...items.filter((i) => i.crashed), ...items.filter((i) => !i.crashed)], then };
  return first ? [ask, ...q] : [...q, ask];
}

/** An autosave worth recovering: a crash's (whatever its age), or one newer than its drawing (the
 *  same second counts: Rust compares finer) or whose drawing is gone. */
const toRecover = (i: Recoverable) =>
  i.autosave != null && (i.crashed || i.drawing_at == null || (i.saved_at ?? 0) >= i.drawing_at);

/** What to ask about before an open: an autosave to recover, a drawing open in another Qiyas (a
 *  warning, even with nothing to recover) and older copies a crash left. Not an older autosave
 *  alone, nor a crash that left nothing (Rust clears its lock when the drawing opens). */
export const offered = (items: Recoverable[]) =>
  items.filter((i) => toRecover(i) || i.locked_elsewhere != null || (i.older_copies ?? []).length > 0);

/** What to ask about at launch: an autosave to recover only, never a drawing open in another
 *  Qiyas (its open warns) or crash copies alone (listed before the drawing's open). */
export const atLaunch = (items: Recoverable[]) => items.filter((i) => toRecover(i) && i.locked_elsewhere == null);

/** `q` with question `id` no longer opening `drawing` afterwards (it was recovered or opened). */
export const forgetThen = (q: RecoveryAsk[], id: number, drawing: string) =>
  q.map((ask) => (ask.id === id && ask.then === drawing ? { ...ask, then: null } : ask));

/** What Later opens: the drawing the question was asked before, unless it is open in another
 *  Qiyas (Open anyway is the way there); the launch question opens nothing. */
export function laterOpens(ask: RecoveryAsk): string | null {
  const held = ask.items.some((i) => i.drawing === ask.then && i.locked_elsewhere != null);
  return held ? null : ask.then;
}

/** `secs` (seconds since 1970) as a date and time in the app's language. */
export const localTime = (secs: number, lang: Lang) =>
  new Date(secs * 1000).toLocaleString(locale(lang), { dateStyle: "medium", timeStyle: "short" });

/** Rust's refusal of a Discard while another Qiyas holds the drawing ("… is open in another Qiyas
 *  (process N on HOST): its autosave stays"): who holds it, to say in the window's language. */
export function refusedBy(message: string): { pid: string; host: string } | null {
  const m = /is open in another Qiyas \(process (\d+) on (.+)\): its autosave stays/.exec(message);
  return m ? { pid: m[1], host: m[2] } : null;
}

/** `q` with `drawing`'s autosave (or only its older `copy`) dealt with in question `id`, the one
 *  the click came from: the row stays while older copies are left (as copies only), the question
 *  goes once it is empty, and a drawing gone from it is no longer what Later opens. */
export function drop(q: RecoveryAsk[], id: number, drawing: string, copy?: string): RecoveryAsk[] {
  return q.flatMap((ask) => {
    if (ask.id !== id) return [ask];
    const items = ask.items.flatMap((i) => {
      if (i.drawing !== drawing) return [i];
      const copies = (i.older_copies ?? []).filter((c) => c.path !== copy);
      const left =
        copy === undefined
          ? { ...i, autosave: null, saved_at: null, crashed: false, locked_elsewhere: null, older_copies: copies }
          : { ...i, older_copies: copies };
      return left.autosave != null || left.locked_elsewhere != null || left.older_copies.length > 0 ? [left] : [];
    });
    if (items.length === 0) return [];
    const gone = ask.then === drawing && !items.some((i) => i.drawing === drawing);
    return [{ ...ask, items, then: gone ? null : ask.then }];
  });
}
