import { chatText, contentLanguage, isolateRuns, messageFiles } from "./content";
import { uiError, userError } from "../i18n/errors";
import { memo, useEffect, useRef, useState, type ReactNode } from "react";
import { aiAttachPdf, type PdfAttachment } from "../api";
import type { Pointing } from "../app/useAssistantWiring";
import { translate, useLanguage, useT } from "../i18n";
import { MizanMark } from "./MizanMark";
import { Markdown } from "./Markdown";
import { PlanChecklist } from "./PlanChecklist";
import { ConversationHistory, type ConversationHistoryState } from "./ConversationHistory";
import { recoveringCard, startNewOffered, TOO_LONG, undoableCard, type Item, type ToolCardItem } from "./messages";
import { isPicture, MAX_PICTURES, readPicture, type Picture } from "./pictures";
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
  /** What the user points at (the selection, a marked area) as chips above the box, and the send
   *  that takes them along. Without it the message goes alone. */
  pointing?: Pointing;
  history?: ConversationHistoryState;
  /** Reads a picture pasted, dropped or picked (tests give their own). */
  read?(file: File): Promise<Picture>;
};

const EXAMPLES = ["ai.example1", "ai.example2", "ai.example3"];

/** The in-app agent, Mizan: the conversation, the tools it ran, and a box to write in; or its log.
 *  Memoised: the app re-renders on every pointer move. */
export const AssistantPanel = memo(function AssistantPanel({
  assistant,
  onShow,
  onUndo,
  onSettings,
  pointing,
  history,
  read = readPicture,
}: Props) {
  const t = useT();
  const { lang } = useLanguage();
  const chatLang = contentLanguage([...assistant.transcript.items].reverse().find((item) => item.kind === "user")?.text ?? "", lang);
  const { transcript, busy, error } = assistant;
  const [attaching, setAttaching] = useState(false);
  const unavailable = busy || !!history?.pending || attaching;
  const [draft, setDraft] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  const filePicker = useRef<HTMLInputElement>(null);
  /** The pictures to send with the next message. */
  const [pictures, setPictures] = useState<Picture[]>([]);
  const [documents, setDocuments] = useState<PdfAttachment[]>([]);
  const [pictureError, setPictureError] = useState<string | null>(null);

  // "Ask Mizan" on the canvas menu, or an area marked: the box takes the focus.
  const focusKey = pointing?.focusKey ?? 0;
  useEffect(() => {
    if (focusKey > 0) box.current?.focus();
  }, [focusKey]);

  /** Adds the pictures among `files`, up to `MAX_PICTURES` in all. */
  async function addPictures(files: File[]) {
    if (unavailable) return;
    const pdfs = files.filter((file) => file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf"));
    if (pdfs.length) {
      setAttaching(true);
      setPictureError(null);
      try {
        if (documents.length + pdfs.length > MAX_PICTURES) throw userError(t("ai.tooManyPictures", { n: String(MAX_PICTURES) }));
        for (const file of pdfs) {
          if (file.size > 20 * 1024 * 1024) throw userError(t("ai.pdfTooLarge"));
          const attachment = await aiAttachPdf(file.name, Array.from(new Uint8Array(await file.arrayBuffer())));
          setDocuments((now) => [...now, attachment]);
        }
      } catch (error) { setPictureError(uiError(error)); }
      finally { setAttaching(false); }
    }
    const wanted = files.filter(isPicture);
    if (wanted.length === 0) return;
    setPictureError(null);
    const room = MAX_PICTURES - pictures.length;
    if (wanted.length > room) setPictureError(t("ai.tooManyPictures", { n: String(MAX_PICTURES) }));
    const results = await Promise.allSettled(wanted.slice(0, Math.max(0, room)).map((file) => read(file)));
    const ok = results.flatMap((r) => (r.status === "fulfilled" ? [r.value] : []));
    if (ok.length < results.length) setPictureError(t("ai.pictureFailed"));
    setPictures((now) => [...now, ...ok].slice(0, MAX_PICTURES));
  }

  // The newest message stays in view while the reader is at the bottom; scrolled up, they are left
  // where they are and a button offers the way back.
  const thread = useStickToBottom([transcript, busy]);

  async function submit() {
    const text = [draft.trim(), ...documents.map((document) => `[PDF attachment: ${JSON.stringify(document.name)}; ${document.pages} pages; path: ${JSON.stringify(document.path)}]`)].filter(Boolean).join("\n\n");
    if ((!text && pictures.length === 0) || unavailable) return;
    // What the reader sends is what they want to see next to.
    thread.jump(false);
    const sent = pointing
      ? await pointing.send(text, pictures)
      : pictures.length > 0
        ? await assistant.send(text, {
            images: pictures.map(({ media_type, data }) => ({ media_type, data })),
            shown: { images: pictures.map((p) => p.url) },
          })
        : await assistant.send(text);
    if (sent) {
      setDraft("");
      setPictures([]);
      setDocuments([]);
      setPictureError(null);
    }
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
    if (reason === "repeated_ineffective_edit") return translate(chatLang, "ai.repeatedEdit");
    if (reason === "drawing_checks_unresolved") return translate(chatLang, "ai.unresolvedChecks");
    if (reason === TOO_LONG) return t("ai.tooLong");
    if (reason === "max_tokens") return t("ai.maxTokens");
    const steps = /^stopped after (\d+) steps$/.exec(reason);
    if (steps) return t("ai.steps", { n: steps[1] });
    return uiError(reason, lang);
  }

  function errorText(e: string) {
    if (e === "busy") return t("ai.busy");
    if (e === "no_api_key") return t("ai.noKey");
    if (e === "no_base_url") return t("ai.noBaseUrl");
    if (e === "no_model") return t("ai.noModel");
    const missingModel = /^(.+) has no model (.+): choose one in Settings$/.exec(e);
    if (missingModel) return t("ai.modelNotFound", { service: missingModel[1], model: missingModel[2] });
    if (e === "restarted") return t("ai.restarted");
    if (e === "crashed") return t("ai.crashed");
    return uiError(e, lang);
  }

  function show(item: Item, i: number): ReactNode {
    if (item.kind === "retrying") return <p key={i} className="msg notice" role="status">
      {item.code === "ai.error.context_retry" ? t(item.code) : t("ai.error.retrying", { seconds: item.seconds })}
    </p>;
    switch (item.kind) {
      case "plan":
        return <PlanChecklist key={i} phases={item.phases} language={chatLang}
          onApprove={item.awaitingApproval && !unavailable && !transcript.items.slice(i + 1).some((next) => next.kind === "user")
            ? () => { void assistant.send(translate(chatLang, "ai.approvedMessage")); } : undefined}
          onChange={() => box.current?.focus()} />;
      case "user":
        return (
          <div key={i} className="msg user" lang={contentLanguage(item.text, lang)} dir={contentLanguage(item.text, lang) === "ar" ? "rtl" : "ltr"}>
            {((item.pointed?.length ?? 0) > 0 || (item.images?.length ?? 0) > 0) && (
              <div className="msg-attached">
                {item.pointed?.map((label) => (
                  <span key={label} className="chip">
                    {label}
                  </span>
                ))}
                {item.images?.map((url, k) => (
                  url === "[picture removed]"
                    ? <span key={k} className="chip">{t("ai.pictureRemoved")}</span>
                    : <img key={k} src={url} alt={t("ai.picture")} className="thumb" />
                ))}
              </div>
            )}
            {messageFiles(item.text).map((file, k) => <span key={k} className="chip"><bdi>{file.name}</bdi> · {t("ai.pdfPageCount", { n: file.pages })}</span>)}
            {isolateRuns(chatText(item.text))}
          </div>
        );
      case "assistant":
        return (
          <div key={i} className="msg assistant">
            <Markdown text={item.text} />
          </div>
        );
      case "thinking":
        return null;
      case "tool": {
        if (item.name === "qiyas_execute" && item.label === "plan" && item.status !== "error") return null;
        const canShow = item.handles.length > 0 || !!item.ui?.bounds;
        const canUndo = !unavailable && item === undoable;
        return (
          <ToolCard
            key={i}
            card={item}
            recovering={recoveringCard(transcript.items, i, busy)}
            onShow={canShow ? () => onShow(item) : undefined}
            onUndo={canUndo ? () => onUndo(item) : undefined}
          />
        );
      }
      case "refused":
        return (
          <div key={i} className="msg notice refused" role="note">
            <strong className="heading">{t("ai.refused")}</strong>
            {item.text && <Markdown text={item.text} />}
            {item.category && <details className="technical-details"><summary>{t("technical.details")}</summary><pre>{item.category}</pre></details>}
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
          disabled={unavailable || empty}
          title={t("ai.newChatTip")}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M8 3.5v9M3.5 8h9" />
          </svg>
          {t("ai.newChat")}
        </button>
        {history && <ConversationHistory history={history} disabled={busy} />}
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
      {history?.error && <p className="msg notice" role="alert">{uiError(history.error, lang)}</p>}
        <div className="assistant-thread">
        <div className="assistant-log" ref={thread.ref} onScroll={thread.onScroll}>
          {empty ? (
            <div className="assistant-empty">
              <p>{t("ai.emptyTitle")}</p>
              {history?.latest && <button type="button" className="continue-chat" disabled={busy || history.pending}
                onClick={() => { void history.resume(history.latest!); }}>
                {t("ai.continue", { title: history.latest.title })}
              </button>}
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
          {!unavailable && (startNewOffered(transcript) || error === "crashed") && (
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
      {error && (
        <div className="error" role="alert">
          {errorText(error)}
          <button type="button" className="link" onClick={onSettings}>{t("support.details")}</button>
        </div>
      )}
      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        onDragOver={(e) => {
          if ([...e.dataTransfer.items].some((i) => i.kind === "file")) e.preventDefault();
        }}
        onDrop={(e) => {
          const files = [...e.dataTransfer.files];
          if (files.length === 0) return;
          e.preventDefault();
          addPictures(files);
        }}
      >
        {(pointing?.selection || pointing?.region || pictures.length > 0) && (
          <div className="composer-chips">
            {pointing?.selection && (
              <span className="chip" title={pointing.selection.kinds}>
                {t("ai.chip.selection", { n: String(pointing.selection.count) })}
                {pointing.selection.kinds && <span className="chip-detail">{pointing.selection.kinds}</span>}
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={t("ai.chip.remove")}
                  title={t("ai.chip.remove")}
                  onClick={pointing.detachSelection}
                >
                  ×
                </button>
              </span>
            )}
            {pointing?.region && (
              <span className="chip">
                {t("ai.chip.area", {
                  w: pointing.region.width.toFixed(2),
                  h: pointing.region.height.toFixed(2),
                })}
                {pointing.region.count !== null && (
                  <span className="chip-detail">
                    {t("ai.chip.entities", { n: String(pointing.region.count) })}
                  </span>
                )}
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={t("ai.chip.remove")}
                  title={t("ai.chip.remove")}
                  onClick={pointing.clearRegion}
                >
                  ×
                </button>
              </span>
            )}
            {pictures.map((p, k) => (
              <span key={k} className="chip picture">
                <img src={p.url} alt={t("ai.picture")} className="thumb" />
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={t("ai.removePicture")}
                  title={t("ai.removePicture")}
                  onClick={() => setPictures((now) => now.filter((_, j) => j !== k))}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        {pictureError && (
          <p className="composer-note" role="alert">
            {pictureError}
          </p>
        )}
        <textarea
          ref={box}
          dir="auto"
          rows={3}
          aria-label={t("ai.placeholder")}
          placeholder={t("ai.placeholder")}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onPaste={(e) => {
            const files = [...(e.clipboardData?.files ?? [])].filter((file) => isPicture(file) || file.type === "application/pdf");
            if (files.length === 0) return;
            e.preventDefault();
            addPictures(files);
          }}
          onKeyDown={(e) => {
            // Enter sends, Shift+Enter is a new line; not while an input method is composing.
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
        />
        {documents.map((document) => (
          <div className="pointing-chip" key={document.path}>
            <span>{document.name} · {document.pages} {t("ai.pdfPages")}</span>
            <button type="button" aria-label={t("ai.removeDocument")} disabled={unavailable}
              onClick={() => setDocuments((now) => now.filter((item) => item.path !== document.path))}>×</button>
          </div>
        ))}
        <input
          ref={filePicker}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif,application/pdf"
          multiple
          hidden
          onChange={(e) => {
            addPictures([...(e.target.files ?? [])]);
            e.target.value = "";
          }}
        />
        <div className="composer-buttons">
          {pointing && (
            <button
              type="button"
              className="icon"
              aria-label={t("ai.markArea")}
              title={t("ai.markArea")}
              onClick={pointing.markArea}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M2.5 5V2.5H5M11 2.5h2.5V5M13.5 11v2.5H11M5 13.5H2.5V11" />
                <path d="M8 6v4M6 8h4" />
              </svg>
            </button>
          )}
          <button
            type="button"
            className="icon"
            aria-label={t("ai.attach")}
            title={t("ai.attach")}
            disabled={pictures.length >= MAX_PICTURES}
            onClick={() => filePicker.current?.click()}
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <rect x="2" y="3" width="12" height="10" rx="1.5" />
              <circle cx="6" cy="6.5" r="1.2" />
              <path d="M2.5 11.5l3.5-3 2.5 2 2-1.5 3 2.5" />
            </svg>
          </button>
          {busy ? (
            <button type="button" className="primary" onClick={assistant.cancel}>
              {t("ai.stop")}
            </button>
          ) : (
            <button type="submit" className="primary" disabled={unavailable || (!draft.trim() && pictures.length === 0 && documents.length === 0)}>
              {t("ai.send")}
            </button>
          )}
        </div>
      </form>
    </section>
  );
});
