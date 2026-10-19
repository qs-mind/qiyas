import { memo, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { aiLog, type AiLog } from "../api";
import { useT } from "../i18n";
import { MizanMark } from "./MizanMark";
import { Markdown } from "./Markdown";
import { startNewOffered, TOO_LONG, undoableCard, type Item, type ToolCardItem } from "./messages";
import { ToolCard } from "./ToolCard";
import type { Assistant } from "./useAssistant";
import { useStickToBottom } from "./useStickToBottom";

type Props = {
  assistant: Pick<
    Assistant,
    "transcript" | "busy" | "error" | "send" | "cancel" | "clear"
  >;
  onShow(card: ToolCardItem): void;
  onUndo(card: ToolCardItem): void;
  onSettings(): void;
};

const EXAMPLES = ["ai.example1", "ai.example2", "ai.example3"];

/** The in-app agent, Mizan: the conversation, the tools it ran, and a box to write in; or its log.
 *  Memoised: the app re-renders on every pointer move. */
export const AssistantPanel = memo(function AssistantPanel({ assistant, onShow, onUndo, onSettings }: Props) {
  const t = useT();
  const { transcript, busy, error } = assistant;
  const [draft, setDraft] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);

  // The agent log, in place of the conversation while it is shown.
  const [showLog, setShowLog] = useState(false);
  const [agentLog, setAgentLog] = useState<AiLog | null>(null);
  const logText = useRef<HTMLPreElement>(null);
  const loadLog = useCallback(() => {
    aiLog()
      .then(setAgentLog)
      .catch((e) => setAgentLog({ path: "", text: String(e) }));
  }, []);
  // Read again at each new step of a turn and when it ends.
  useEffect(() => {
    if (showLog) loadLog();
  }, [showLog, busy, transcript.items.length, loadLog]);
  useEffect(() => {
    logText.current?.scrollTo?.({ top: logText.current.scrollHeight });
  }, [agentLog]);

  // The newest message stays in view while the reader is at the bottom; scrolled up, they are left
  // where they are and a button offers the way back.
  const thread = useStickToBottom([transcript, busy]);

  async function submit() {
    const text = draft.trim();
    if (!text || busy) return;
    // What the reader sends is what they want to see next to.
    thread.jump(false);
    if (await assistant.send(text)) setDraft("");
  }

  // Undo is offered on the card whose change is the assistant's newest still in the take-off.
  const undoable = undoableCard(transcript);

  // Said once per turn to screen readers, instead of reading out the streaming log.
  const [announce, setAnnounce] = useState("");
  const wasBusy = useRef(busy);
  useEffect(() => {
    if (wasBusy.current && !busy) setAnnounce(t("ai.finished"));
    if (busy) setAnnounce("");
    wasBusy.current = busy;
  }, [busy]);

  function stopText(reason: string) {
    if (reason === "cancelled") return t("ai.cancelled");
    if (reason === TOO_LONG) return t("ai.tooLong");
    if (reason === "max_tokens") return t("ai.maxTokens");
    const steps = /^stopped after (\d+) steps$/.exec(reason);
    if (steps) return t("ai.steps", { n: steps[1] });
    return `${t("ai.stopped")}: ${reason}`;
  }

  function errorText(e: string) {
    if (e === "busy") return t("ai.busy");
    if (e === "no_api_key") return t("ai.noKey");
    if (e === "no_base_url") return t("ai.noBaseUrl");
    if (e === "restarted") return t("ai.restarted");
    if (e === "crashed") return t("ai.crashed");
    return e;
  }

  function show(item: Item, i: number): ReactNode {
    switch (item.kind) {
      case "user":
        return (
          <div key={i} className="msg user" dir="auto">
            {item.text}
          </div>
        );
      case "assistant":
        return (
          <div key={i} className="msg assistant">
            <Markdown text={item.text} />
          </div>
        );
      case "thinking":
        return (
          <details key={i} className="msg thinking">
            <summary>{t("ai.thinking")}</summary>
            <p dir="auto">{item.text}</p>
          </details>
        );
      case "tool": {
        const canShow = item.handles.length > 0 || !!item.ui?.bounds;
        const canUndo = !busy && item === undoable;
        return (
          <ToolCard
            key={i}
            card={item}
            onShow={canShow ? () => onShow(item) : undefined}
            onUndo={canUndo ? () => onUndo(item) : undefined}
          />
        );
      }
      case "refused":
        return (
          <div key={i} className="msg notice refused" role="note">
            <strong className="heading">{t("ai.refused")}</strong>
            {item.text && <p dir="auto">{item.text}</p>}
            {item.category && <span className="category">{item.category}</span>}
          </div>
        );
      case "stopped":
        return (
          <div key={i} className="msg notice stopped" role="note">
            {stopText(item.reason)}
          </div>
        );
    }
  }

  const empty = transcript.items.length === 0;
  return (
    <section className="assistant-panel" aria-label={t("assistant")}>
      <div className="assistant-head">
        <button
          type="button"
          className="new-chat"
          onClick={assistant.clear}
          disabled={busy || empty}
          title={t("ai.newChatTip")}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M8 3.5v9M3.5 8h9" />
          </svg>
          {t("ai.newChat")}
        </button>
        <button type="button" aria-pressed={showLog} onClick={() => setShowLog((on) => !on)}>
          {t("ai.log")}
        </button>
        <button
          type="button"
          className="icon gear"
          aria-haspopup="dialog"
          aria-label={t("settings")}
          title={t("settings")}
          onClick={onSettings}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="2.2" />
            <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
            <circle cx="8" cy="8" r="4.6" />
          </svg>
        </button>
      </div>
      <p className="sr-only" role="status" aria-live="polite">
        {announce}
      </p>
      {showLog ? (
        <div className="agent-log">
          <div className="agent-log-bar">
            <span className="agent-log-path" dir="ltr" title={agentLog?.path}>
              {agentLog?.path}
            </span>
            <button type="button" className="link" onClick={loadLog}>
              {t("ai.logRefresh")}
            </button>
          </div>
          <pre className="agent-log-text" dir="ltr" ref={logText} tabIndex={0} aria-label={t("ai.log")}>
            {agentLog === null ? "" : agentLog.text || t("ai.logEmpty")}
          </pre>
        </div>
      ) : (
        <div className="assistant-thread">
        <div className="assistant-log" ref={thread.ref} onScroll={thread.onScroll}>
          {empty ? (
            <div className="assistant-empty">
              <p>{t("ai.emptyTitle")}</p>
              <ul>
                {EXAMPLES.map((key) => (
                  <li key={key}>
                    <button
                      type="button"
                      onClick={() => {
                        setDraft(t(key));
                        box.current?.focus();
                      }}
                    >
                      {t(key)}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            transcript.items.map(show)
          )}
          {busy && (
            <p className="working">
              <MizanMark size="md" />
              {t("ai.working")}
            </p>
          )}
          {!busy && (startNewOffered(transcript) || error === "crashed") && (
            <button type="button" className="primary new-conversation" onClick={assistant.clear}>
              {t("ai.newConversation")}
            </button>
          )}
        </div>
        {thread.away && (
          <button type="button" className="jump-latest" onClick={() => thread.jump()}>
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M8 3v9M4.5 8.5L8 12l3.5-3.5" />
            </svg>
            {t("ai.jumpLatest")}
          </button>
        )}
        </div>
      )}
      {error && (
        <div className="error" role="alert">
          {errorText(error)}
          {!showLog && (
            <button type="button" className="link" onClick={() => setShowLog(true)}>
              {t("ai.showLog")}
            </button>
          )}
        </div>
      )}
      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <textarea
          ref={box}
          dir="auto"
          rows={3}
          aria-label={t("ai.placeholder")}
          placeholder={t("ai.placeholder")}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            // Enter sends, Shift+Enter is a new line; not while an input method is composing.
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
        />
        <div className="composer-buttons">
          {busy ? (
            <button type="button" className="primary" onClick={assistant.cancel}>
              {t("ai.stop")}
            </button>
          ) : (
            <button type="submit" className="primary" disabled={!draft.trim()}>
              {t("ai.send")}
            </button>
          )}
        </div>
      </form>
    </section>
  );
});
