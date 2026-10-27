import { ModalDialog } from "../ui/ModalDialog";
import { uiError } from "../i18n/errors";
import { Popover } from "@base-ui/react/popover";
import { useEffect, useRef, useState } from "react";
import {
  aiConversations, aiDeleteConversation, aiDeleteConversations, type ConversationSummary,
} from "../api";
import { useLanguage, useT } from "../i18n";
import { Glyph } from "../shell/glyphs";
import "./history.css";

export type ConversationHistoryState = {
  chats: ConversationSummary[];
  drawing: string | null;
  latest: ConversationSummary | null;
  pending: boolean;
  error: string | null;
  resume(chat: ConversationSummary): Promise<void>;
  remove(id?: string): Promise<void>;
};

/** The index is small (at most 100 rows); filtering never reads a conversation file. */
export function useConversationHistory({ drawing, busy, empty, savedVersion = 0, prepare, resume }: {
  drawing: string | null;
  busy: boolean;
  empty: boolean;
  savedVersion?: number;
  prepare(chat: ConversationSummary): Promise<boolean>;
  resume(id: string): Promise<void>;
}): ConversationHistoryState {
  const [chats, setChats] = useState<ConversationSummary[]>([]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  async function refresh() {
    const request = ++generation.current;
    try {
      const saved = await aiConversations();
      if (request === generation.current) setChats(saved);
    } catch (e) {
      if (request === generation.current) setError(uiError(e));
    }
  }
  useEffect(() => {
    void refresh();
    return () => { generation.current += 1; };
  }, [busy, empty, savedVersion]);

  return {
    chats, drawing, pending, error,
    latest: drawing ? chats.find((chat) => chat.drawing === drawing) ?? null : null,
    async resume(chat) {
      if (busy || pending) return;
      setPending(true);
      setError(null);
      try {
        if (await prepare(chat)) await resume(chat.id);
        await refresh();
      } catch (e) {
        setError(uiError(e));
      } finally {
        setPending(false);
      }
    },
    async remove(id) {
      if (busy || pending) return;
      setPending(true);
      setError(null);
      try {
        if (id) await aiDeleteConversation(id);
        else await aiDeleteConversations();
        await refresh();
      } catch (e) {
        setError(uiError(e));
      } finally {
        setPending(false);
      }
    },
  };
}

export function ConversationHistory({ history, disabled }: { history: ConversationHistoryState; disabled: boolean }) {
  const t = useT();
  const { lang } = useLanguage();
  const [open, setOpen] = useState(false);
  const [all, setAll] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const chats = history.chats.filter((chat) => all || chat.drawing === history.drawing);
  const unavailable = disabled || history.pending;
  return <><Popover.Root open={open} onOpenChange={setOpen} modal={false}>
    <Popover.Trigger className="chat-history-trigger ib" disabled={unavailable} aria-label={t("ai.history")} title={t("ai.history")}>
      <Glyph name="history" />
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Positioner className="chat-history-positioner" side="bottom" align="start" sideOffset={6} collisionPadding={8}>
        <Popover.Popup className="chat-history-popup">
          <Popover.Title className="heading">{t("ai.history")}</Popover.Title>
          <div className="chat-history-filter" role="group" aria-label={t("ai.historyFilter")}>
            <button type="button" aria-pressed={!all} onClick={() => setAll(false)}>{t("ai.thisDrawing")}</button>
            <button type="button" aria-pressed={all} onClick={() => setAll(true)}>{t("ai.allDrawings")}</button>
          </div>
          {chats.length === 0 ? <p>{t("ai.noHistory")}</p> : <ul className="chat-history-list">
            {chats.map((chat) => <li key={chat.id}>
              <button type="button" className="chat-history-resume" disabled={unavailable} onClick={() => {
                setOpen(false);
                void history.resume(chat);
              }}>
                <strong dir="auto">{chat.title}</strong>
                <span dir="auto">{chat.drawing?.split(/[\\/]/).pop() ?? t("ai.untitledDrawing")}</span>
                <time dateTime={new Date(chat.updated).toISOString()}>{new Date(chat.updated).toLocaleString(lang === "ar" ? "ar-EG-u-nu-latn" : "en", { dateStyle: "medium", timeStyle: "short" })}</time>
              </button>
              <button type="button" className="chat-history-delete" disabled={unavailable}
                aria-label={t("ai.deleteChat", { title: chat.title })} onClick={() => { void history.remove(chat.id); }}>×</button>
            </li>)}
          </ul>}
          <button type="button" className="chat-history-delete-all" disabled={unavailable || history.chats.length === 0}
            onClick={() => { setOpen(false); setConfirmDelete(true); }}>{t("ai.deleteAllChats")}</button>
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  </Popover.Root>
  {confirmDelete && <ModalDialog labelledBy="delete-chats-title" onClose={() => setConfirmDelete(false)}>
    <h2 id="delete-chats-title">{t("ai.deleteChatsConfirm", { n: history.chats.length })}</h2>
    <p>{t("ai.deleteChatsWarning")}</p>
    <div className="buttons">
      <button type="button" onClick={() => setConfirmDelete(false)}>{t("text.cancel")}</button>
      <button type="button" disabled={unavailable} onClick={() => { setConfirmDelete(false); void history.remove(); }}>{t("ai.deleteAllChats")}</button>
    </div>
  </ModalDialog>}</>;
}
