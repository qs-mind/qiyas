/** The assistant's state in the window: the transcript from `ai:event`s, whether a turn runs, the
 *  last error, and the provider's status. */
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import {
  aiCancel,
  aiClear,
  aiSend,
  aiSetLanguage,
  aiStatus,
  type AgentEvent,
  type AiContext,
  type AiDone,
  type AiImage,
  type AiStatus,
  type AiUi,
} from "../api";
import type { Lang } from "../i18n";
import { EMPTY, reduce, type Attached, type Transcript } from "./messages";

/** What goes with a message besides its words: what the user points at, the pictures they
 *  attached, and how the transcript shows them. */
export type SendExtras = { context?: AiContext; images?: AiImage[]; shown?: Attached };

type Options = {
  lang: Lang;
  /** A tool changed the drawing or take-off: refresh it and redraw. */
  onMutated(label: string): void;
  /** A tool asks the window to show something (a sheet, a zoom, other layers). */
  onUi(ui: AiUi): void;
  /** Sending failed for want of a key or an address: open the settings. */
  onNeedSettings(): void;
};

export type Assistant = {
  transcript: Transcript;
  busy: boolean;
  /** "busy", "no_api_key", "no_base_url", or a message. */
  error: string | null;
  status: AiStatus | null;
  /** Resolves false when the message was not sent (the composer keeps it). */
  send(text: string, extras?: SendExtras): Promise<boolean>;
  cancel(): void;
  clear(): void;
  markUndone(id: string): void;
  markUndoStale(id: string): void;
  refreshStatus(): void;
};

export function useAssistant({ lang, onMutated, onUi, onNeedSettings }: Options): Assistant {
  const [transcript, dispatch] = useReducer(reduce, EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<AiStatus | null>(null);
  // The listeners are set up once; they call the latest callbacks through this.
  const callbacks = useRef({ onMutated, onUi, onNeedSettings });
  callbacks.current = { onMutated, onUi, onNeedSettings };
  /** The turn's tool calls that change the drawing or take-off: their labels, by id. */
  const mutating = useRef(new Map<string, string>());

  const refreshStatus = useCallback(() => {
    aiStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(() => {
    let gone = false;
    const unlisten: (() => void)[] = [];
    const on = <T>(name: string, handler: (payload: T) => void) =>
      listen<T>(name, (e) => handler(e.payload)).then((off) => {
        if (gone) off();
        else unlisten.push(off);
      });
    on<AgentEvent>("ai:event", (event) => {
      dispatch(event);
      if (event.type === "tool_call" && event.mutating) {
        mutating.current.set(event.id, event.label ?? event.name);
      }
      if (event.type === "tool_result" && !event.is_error) {
        const tool = mutating.current.get(event.id);
        // Reserve a reveal before the mutation's redraw can fetch the changed drawing.
        if (event.ui) callbacks.current.onUi(event.ui);
        if (tool) callbacks.current.onMutated(tool);
      }
    });
    on<AiDone>("ai:done", ({ error }) => {
      dispatch({ type: "end" });
      setBusy(false);
      if (error) setError(error);
      mutating.current.clear();
      refreshStatus();
    });
    on<null>("ai:cleared", () => {
      dispatch({ type: "clear" });
      setError(null);
    });
    refreshStatus();
    return () => {
      gone = true;
      unlisten.forEach((off) => off());
    };
  }, [refreshStatus]);

  // The prompt follows the window's language; a change starts a new conversation.
  useEffect(() => {
    aiSetLanguage(lang).catch((e) => setError(String(e)));
  }, [lang]);

  const send = useCallback(async (text: string, extras: SendExtras = {}) => {
    setError(null);
    // Before the call: the turn's first events may arrive before it returns.
    dispatch({ type: "user", text, ...extras.shown });
    setBusy(true);
    try {
      await aiSend(text, extras.context, extras.images);
      return true;
    } catch (e) {
      const why = String(e);
      dispatch({ type: "unsend" });
      // "busy" means a turn is still running; keep showing it as busy.
      if (why !== "busy") setBusy(false);
      setError(why);
      if (why === "no_api_key" || why === "no_base_url") callbacks.current.onNeedSettings();
      return false;
    }
  }, []);

  const cancel = useCallback(() => {
    aiCancel().catch((e) => setError(String(e)));
  }, []);

  const clear = useCallback(() => {
    dispatch({ type: "clear" });
    setError(null);
    aiClear().catch((e) => setError(String(e)));
  }, []);

  const markUndone = useCallback((id: string) => dispatch({ type: "undone", id }), []);
  const markUndoStale = useCallback((id: string) => dispatch({ type: "undo_stale", id }), []);

  // One object per change, so memoised panels skip the app's pointer-move renders.
  return useMemo(
    () => ({ transcript, busy, error, status, send, cancel, clear, markUndone, markUndoStale, refreshStatus }),
    [transcript, busy, error, status, send, cancel, clear, markUndone, markUndoStale, refreshStatus],
  );
}
