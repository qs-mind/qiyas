/** The assistant's state in the window: the transcript from `ai:event`s, whether a turn runs, the
 *  last error, and the provider's status. */
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import {
  aiCancel,
  aiConversations,
  drawingInfo,
  aiClear,
  aiResume,
  aiSend,
  aiSetLanguage,
  aiStatus,
  workspaceAttach,
  workspaceProvider,
  type WorkspaceChoice,
  type WorkspaceEvent,
  type AiContext,
  type AiImage,
  type AiStatus,
  type AiUi,
} from "../api";
import type { Lang } from "../i18n";
import { displayedWorkspace } from "../workspaceIpc";
import { EMPTY, reduce, type Attached, type Transcript } from "./messages";

/** What goes with a message besides its words: what the user points at, the pictures they
 *  attached, and how the transcript shows them. */
export type SendExtras = { context?: AiContext; images?: AiImage[]; shown?: Attached };

type Options = {
  workspace?: number | null;
  lang: Lang;
  /** A tool changed the drawing or take-off: refresh it and redraw. */
  onMutated(label: string): void;
  /** A tool asks the window to show something (a sheet, a zoom, other layers). */
  onUi(ui: AiUi): void;
  /** Sending needs a key, service address or model: open the settings. */
  onNeedSettings(): void;
};

export type Assistant = {
  workspace?: number | null;
  phase: string;
  away: string | null;
  memoryLimited: boolean;
  choice: WorkspaceChoice | null;
  choose(choice: WorkspaceChoice): Promise<void>;
  transcript: Transcript;
  busy: boolean;
  /** "busy", "no_api_key", "no_base_url", "no_model", or a message. */
  error: string | null;
  status: AiStatus | null;
  /** A finished turn or reset has updated the saved conversation index. */
  savedVersion: number;
  /** Resolves false when the message was not sent (the composer keeps it). */
  send(text: string, extras?: SendExtras): Promise<boolean>;
  cancel(): void;
  clear(): void;
  resume(id: string): Promise<void>;
  markUndone(id: string): void;
  markUndoStale(id: string): void;
  refreshStatus(): void;
};

export function useAssistant({ workspace, lang, onMutated, onUi, onNeedSettings }: Options): Assistant {
  const [phase, setPhase] = useState("idle");
  const [away, setAway] = useState<string | null>(null);
  const [memoryLimited, setMemoryLimited] = useState(false);
  const [choice, setChoice] = useState<WorkspaceChoice | null>(null);
  const [transcript, dispatch] = useReducer(reduce, EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [savedVersion, setSavedVersion] = useState(0);
  // The listeners are set up once; they call the latest callbacks through this.
  const callbacks = useRef({ onMutated, onUi, onNeedSettings });
  callbacks.current = { onMutated, onUi, onNeedSettings };
  /** The turn's tool calls that change the drawing or take-off: their labels, by id. */
  const visible = useRef(workspace);
  visible.current = workspace;
  const cursor = useRef(0);
  const attaching = useRef(true);
  const pending = useRef<WorkspaceEvent[]>([]);
  const mutating = useRef(new Map<string, string>());
  // A deliberate New chat stays empty when returning to this drawing.
  const visited = useRef(new Set<number>());
  const resumeGeneration = useRef(0);
  const restoring = useRef<Promise<unknown>>(Promise.resolve());
  // History can be clicked while opening a drawing restores its latest conversation.
  // Serialize the two resumes so the explicitly selected conversation wins.
  const loadSaved = useCallback((id: string, target: number | null | undefined) => {
    const next = restoring.current.catch(() => {}).then(() => aiResume(id, target));
    restoring.current = next;
    return next;
  }, []);

  const refreshStatus = useCallback(() => {
    aiStatus(workspace)
      .then(value => { if (visible.current === workspace) setStatus(value); })
      .catch(() => { if (visible.current === workspace) setStatus(null); });
  }, [workspace]);

  useEffect(() => {
    let gone = false;
    let bound = workspace;
    let off: (() => void) | undefined;
    cursor.current = 0;
    attaching.current = true;
    pending.current = [];
    mutating.current.clear();
    const apply = ({ workspace: source, cursor: next, event }: WorkspaceEvent) => {
      if (bound != null && source !== bound) return;
      if (next <= cursor.current) return;
      cursor.current = next;
      if (event.type === "queued") {
        setBusy(true); setPhase("queued"); setMemoryLimited(event.memory_limited ?? false);
        // The sender already displays its message. Other/reloaded windows read it on attach.
        return;
      }
      if (event.type === "working") { setBusy(true); setPhase("working"); return; }
      if (event.type === "end") {
        dispatch({ type: "end" }); setBusy(false); setPhase(event.status);
        setError(event.error); mutating.current.clear(); refreshStatus();
        setSavedVersion(value => value + 1);
        callbacks.current.onMutated("completed");
        return;
      }
      dispatch(event);
      if (event.type === "clear") { setBusy(false); setError(null); setPhase("idle"); return; }
      if (event.type === "tool_call" && event.mutating) mutating.current.set(event.id, event.label ?? event.name);
      if (event.type === "tool_result" && !event.is_error) {
        if (event.ui) callbacks.current.onUi(event.ui);
        const tool = mutating.current.get(event.id);
        if (tool) callbacks.current.onMutated(tool);
      }
      if (event.type === "awaiting_approval" || event.type === "question") setPhase("waiting");
    };
    void listen<WorkspaceEvent>("workspace:event", ({ payload }) => {
      if (gone || (workspace != null && payload.workspace !== workspace)) return;
      if (attaching.current) pending.current.push(payload);
      else apply(payload);
    }).then(async unlisten => {
      if (gone) { unlisten(); return; }
      off = unlisten;
      try {
        const attachGeneration = resumeGeneration.current;
        let historyError: string | null = null;
        let snapshot = await workspaceAttach(workspace);
        if (gone || !snapshot) return;
        const firstVisit = workspace != null && !visited.current.has(snapshot.workspace);
        if (workspace != null) visited.current.add(snapshot.workspace);
        if (firstVisit && snapshot.transcript.length === 0 && !["working", "queued", "waiting"].includes(snapshot.status)) {
          const generation = attachGeneration;
          try {
            const drawing = await drawingInfo(snapshot.workspace);
            if (gone) return;
            const chats = drawing?.path ? await aiConversations(drawing.path) : [];
            const latest = chats?.filter((chat) => chat.drawing === drawing.path).sort((a, b) => b.updated - a.updated)[0];
            if (gone) return;
            if (latest && generation === resumeGeneration.current && pending.current.length === 0) {
              await loadSaved(latest.id, snapshot.workspace);
              if (gone) return;
              snapshot = await workspaceAttach(snapshot.workspace);
              if (gone || !snapshot) return;
            }
          } catch (error) {
            // A history read failure must not leave the previous drawing's thread on screen.
            historyError = String(error);
          }
        }
        if (gone) return;
        bound = snapshot.workspace;
        cursor.current = snapshot.cursor;
        if (attachGeneration === resumeGeneration.current) {
          dispatch({ type: "reattach", entries: snapshot.transcript });
          setBusy(["working", "queued"].includes(snapshot.status));
          setPhase(snapshot.status); setError(snapshot.error ?? historyError); setAway(snapshot.away);
        }
        setChoice(snapshot.choice); setMemoryLimited(snapshot.memory_limited);
        for (const entry of snapshot.transcript) {
          if (entry.type === "event" && entry.event.type === "tool_call" && entry.event.mutating)
            mutating.current.set(entry.event.id, entry.event.label ?? entry.event.name);
        }
      } catch (error) { if (!gone) setError(String(error)); }
      finally {
        if (!gone) { attaching.current = false; pending.current.forEach(apply); pending.current = []; }
      }
    });
    refreshStatus();
    return () => { gone = true; off?.(); };
  }, [workspace, refreshStatus, loadSaved]);

  const choose = useCallback(async (next: WorkspaceChoice) => {
    await workspaceProvider(next, workspace);
    const saved = await workspaceAttach(workspace);
    if (visible.current === workspace) { setChoice(saved.choice); refreshStatus(); }
  }, [workspace, refreshStatus]);

  // The prompt follows the window's language; a change starts a new conversation.
  useEffect(() => {
    aiSetLanguage(lang, workspace).catch((e) => { if (visible.current === workspace) setError(String(e)); });
  }, [lang, workspace]);

  const send = useCallback(async (text: string, extras: SendExtras = {}) => {
    resumeGeneration.current += 1;
    setError(null);
    // Before the call: the turn's first events may arrive before it returns.
    dispatch({ type: "user", text, ...extras.shown });
    setBusy(true);
    try {
      await restoring.current.catch(() => {});
      await aiSend(text, extras.context, extras.images, extras.shown?.pointed, workspace);
      return true;
    } catch (e) {
      if (visible.current !== workspace) return false;
      const why = String(e);
      dispatch({ type: "unsend" });
      // "busy" means a turn is still running; keep showing it as busy.
      if (why !== "busy") setBusy(false);
      setError(why);
      if (why === "no_api_key" || why === "no_base_url" || why === "no_model") callbacks.current.onNeedSettings();
      return false;
    }
  }, [workspace]);

  const cancel = useCallback(() => {
    aiCancel(workspace ?? undefined).catch((e) => { if (visible.current === workspace) setError(String(e)); });
  }, [workspace]);

  const clear = useCallback(() => {
    resumeGeneration.current += 1;
    if (workspace != null) visited.current.add(workspace);
    dispatch({ type: "clear" });
    setError(null);
    void restoring.current.catch(() => {}).then(() => aiClear(workspace)).catch((e) => { if (visible.current === workspace) setError(String(e)); });
  }, [workspace]);

  const markUndone = useCallback((id: string) => dispatch({ type: "undone", id }), []);
  const markUndoStale = useCallback((id: string) => dispatch({ type: "undo_stale", id }), []);

  const resume = useCallback(async (id: string) => {
    resumeGeneration.current += 1;
    // History opens its drawing first. Capture that destination even before React rerenders.
    const target = displayedWorkspace() ?? workspace;
    const saved = await loadSaved(id, target);
    if (visible.current !== target) return;
    dispatch({ type: "restore", entries: saved.transcript });
    setBusy(false);
    setPhase("done");
    setAway(null);
    setError(null);
    mutating.current.clear();
    void aiStatus(target).then(value => { if (visible.current === target) setStatus(value); })
      .catch(() => { if (visible.current === target) setStatus(null); });
  }, [workspace, loadSaved]);

  // One object per change, so memoised panels skip the app's pointer-move renders.
  return useMemo(
    () => ({ workspace, phase, away, memoryLimited, choice, choose, transcript, busy, error, status, savedVersion, send, cancel, clear, resume, markUndone, markUndoStale, refreshStatus }),
    [workspace, phase, away, memoryLimited, choice, choose, transcript, busy, error, status, savedVersion, send, cancel, clear, resume, markUndone, markUndoStale, refreshStatus],
  );
}
