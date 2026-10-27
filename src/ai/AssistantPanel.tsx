import { chatText, contentLanguage, isolateRuns, messageFiles } from "./content";
import { slashCommand, slashMatches, slashWord, type SlashCommand } from "./slashCommands";
import { uiError, userError } from "../i18n/errors";
import { listen } from "@tauri-apps/api/event";
import { memo, useEffect, useRef, useState, type ReactNode } from "react";
import { aiAttachPdf, type PdfAttachment } from "../api";
import type { Pointing } from "../app/useAssistantWiring";
import { translate, useLanguage, useT } from "../i18n";
import { WorkspacePicker } from "./WorkspacePicker";
import { MizanMark } from "./MizanMark";
import { Markdown } from "./Markdown";
import { PlanChecklist } from "./PlanChecklist";
import { ConversationHistory, type ConversationHistoryState } from "./ConversationHistory";
import { readRuns, recoveringCard, startNewOffered, TOO_LONG, undoableCard, type Item, type ToolCardItem } from "./messages";
import { isPicture, MAX_PICTURES, readPicture, type Picture } from "./pictures";
import { QuestionList } from "./QuestionList";
import { ToolCard } from "./ToolCard";
import { Glyph } from "../shell/glyphs";
import type { OutputId } from "../shell/outputs";
import type { Assistant } from "./useAssistant";
import { useStickToBottom } from "./useStickToBottom";

type Props = {
  showTechnicalDetails?: boolean;
  assistant: Pick<
    Assistant,
    "transcript" | "busy" | "error" | "send" | "cancel" | "clear"
  > & Partial<Pick<Assistant, "workspace" | "choice" | "choose" | "phase" | "away" | "memoryLimited">>;
  onShow(card: ToolCardItem): void;
  onUndo(card: ToolCardItem): void;
  onSettings(): void;
  /** What the user points at (the selection, a marked area) as chips above the box, and the send
   *  that takes them along. Without it the message goes alone. */
  pointing?: Pointing;
  history?: ConversationHistoryState;
  /** The right column's outputs (the take-off, the BOQ...): `node` holds them all, mounted; while
   *  one is `shown` it takes the conversation's place, and the box below stays. */
  artifact?: { shown: OutputId | null; node: ReactNode; onBack(): void };
  /** The /compare command: ask for a BOQ and find its conflicts with the drawings. */
  onCompare?(): void;
  /** The /connect command and the empty chat's link: the "Connect an AI app" dialog. */
  onConnect?(): void;
  /** Reads a picture pasted, dropped or picked (tests give their own). */
  read?(file: File): Promise<Picture>;
};

const EXAMPLES = ["ai.example1", "ai.example2", "ai.example3"];

function connectionLost(error: string): boolean {
  try {
    return ["ai.error.network", "ai.error.timeout", "ai.error.stream", "ai.error.service_error", "ai.error.chatgpt_temporary"]
      .includes(JSON.parse(error).code);
  } catch {
    return false;
  }
}

/** The in-app agent, Mizan: the conversation, the tools it ran, and a box to write in; or its log.
 *  Memoised: the app re-renders on every pointer move. */
export const AssistantPanel = memo(function AssistantPanel({
  assistant,
  showTechnicalDetails = false,
  onShow,
  onUndo,
  onSettings,
  pointing,
  history,
  artifact,
  onCompare,
  onConnect,
  read = readPicture,
}: Props) {
  const t = useT();
  const { lang } = useLanguage();
  const chatLang = contentLanguage([...assistant.transcript.items].reverse().find((item) => item.kind === "user")?.text ?? "", lang);
  const { transcript, busy, error } = assistant;
  const [attaching, setAttaching] = useState(false);
  const unavailable = busy || !!history?.pending || attaching;
  type Draft = { text: string; pictures: Picture[]; documents: PdfAttachment[] };
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const key = String(assistant.workspace ?? "lobby");
  const current = drafts[key] ?? { text: "", pictures: [], documents: [] };
  const draft = current.text;
  const pictures = current.pictures;
  const documents = current.documents;
  function editDraft(change: (before: Draft) => Draft) {
    setDrafts(all => ({ ...all, [key]: change(all[key] ?? { text: "", pictures: [], documents: [] }) }));
  }
  const setDraft = (text: string) => editDraft(before => ({ ...before, text }));
  const setPictures = (next: Picture[] | ((old: Picture[]) => Picture[])) =>
    editDraft(before => ({ ...before, pictures: typeof next === "function" ? next(before.pictures) : next }));
  const setDocuments = (next: PdfAttachment[] | ((old: PdfAttachment[]) => PdfAttachment[])) =>
    editDraft(before => ({ ...before, documents: typeof next === "function" ? next(before.documents) : next }));
  useEffect(() => {
    let gone = false;
    let off: (() => void) | undefined;
    void listen<number>("workspace:closed", ({ payload }) => {
      setDrafts(all => { const kept = { ...all }; delete kept[String(payload)]; return kept; });
    }).then(unlisten => { if (gone) unlisten(); else off = unlisten; }).catch(() => {});
    return () => { gone = true; off?.(); };
  }, []);
  const box = useRef<HTMLTextAreaElement>(null);
  const filePicker = useRef<HTMLInputElement>(null);
  /** The pictures to send with the next message. */
  const [pictureError, setPictureError] = useState<string | null>(null);
  /** Slash commands: the row chosen in the list, and whether Escape closed it for this text. */
  const [slashAt, setSlashAt] = useState(0);
  const [slashOff, setSlashOff] = useState(false);

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

  /** Sends the box (or `prompt`, a slash command's words in its place) with its pictures and files. */
  async function submit(prompt?: string) {
    const text = [(prompt ?? draft).trim(), ...documents.map((document) => `[PDF attachment: ${JSON.stringify(document.name)}; ${document.pages} pages; path: ${JSON.stringify(document.path)}]`)].filter(Boolean).join("\n\n");
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

  const slashList = slashOff || unavailable ? [] : slashMatches(draft, lang);
  const slashRow = Math.min(slashAt, Math.max(0, slashList.length - 1));

  /** Runs a slash command: a prompt goes to Mizan as the user's message, an action is the window's. */
  function runSlash(command: SlashCommand) {
    setSlashAt(0);
    if (command.kind === "prompt") {
      void submit(t(`ai.slash.${command.id}.prompt`));
      return;
    }
    setDraft("");
    if (command.id === "new") assistant.clear();
    else if (command.id === "compare") onCompare?.();
    else if (command.id === "connect") onConnect?.();
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
    if (connectionLost(e)) return t("ai.connectionLost");
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

  // Group recoverable failures within their own user turn. Stored cards and the support log
  // stay intact, and a failure with no recovery still renders as an ordinary stopped card.
  const recovery = new Map<number, { indices: number[]; pending: boolean }>();
  let turn = 0;
  transcript.items.forEach((item, i) => {
    if (item.kind === "user") turn = i;
    if (item.kind !== "tool" || item.status !== "error" || !recoveringCard(transcript.items, i, busy)) return;
    const group = recovery.get(turn) ?? { indices: [], pending: false };
    group.indices.push(i);
    group.pending ||= !recoveringCard(transcript.items, i, false);
    recovery.set(turn, group);
  });
  const recoveredIndices = new Set([...recovery.values()].flatMap((group) => group.indices));
  // A run of identical reads is one line with a count.
  const runs = readRuns(transcript.items);
  const recoveryHeads = new Map([...recovery.values()].map((group) => [group.indices[0], group]));

  function show(item: Item, i: number): ReactNode {
    if (item.kind === "provider_status") return <p key={i} className="msg notice" role="status">{t(item.code)}</p>;
    if (item.kind === "retrying") return <p key={i} className="msg notice" role="status">
      {["ai.error.context_retry", "ai.error.finish_gate", "ai.error.language_retry"].includes(item.code) ? t(item.code) : t("ai.error.retrying", { seconds: item.seconds })}
    </p>;
    switch (item.kind) {
      case "cache_usage":
        return showTechnicalDetails ? <details key={i} className="technical-details">
          <summary>{t("technical.details")}</summary>
          <p>{t("ai.cacheUsage", { percent: (100 * item.cached / item.total).toFixed(1), cached: item.cached, total: item.total })}</p>
        </details> : null;
      case "plan":
        return <PlanChecklist key={i} phases={item.phases} stale={item.stale} language={chatLang} showTechnicalDetails={showTechnicalDetails}
          onApprove={item.awaitingApproval && !unavailable && !transcript.items.slice(i + 1).some((next) => next.kind === "user")
            ? () => { void assistant.send(translate(chatLang, "ai.approvedMessage")); } : undefined}
          onChange={() => box.current?.focus()} />;
      case "question":
        // Plain numbered list in Mizan's message (design B); the answer is typed, or an option
        // is put in the box. Once the user has replied it is only a record.
        return (
          <QuestionList
            key={i}
            questions={item.questions}
            onPick={unavailable || transcript.items.slice(i + 1).some((next) => next.kind === "user") ? undefined : (text) => {
              setDraft(draft.trim() ? `${draft.trim()}\n${text}` : text);
              box.current?.focus();
            }}
          />
        );
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
      case "open_findings":
        return <div key={i} className="msg notice" role="note">
          <strong className="heading">{t("ai.openFindings")}</strong>
          <ul>{[...new Set(item.findings.map(finding => finding.rule))].map(rule =>
            <li key={rule}>{t(`ai.finding.${rule}`)} ({item.findings.filter(finding => finding.rule === rule).length})</li>,
          )}</ul>
          {showTechnicalDetails && <details className="technical-details"><summary>{t("technical.details")}</summary><pre dir="ltr">{JSON.stringify(item.findings, null, 2)}</pre></details>}
        </div>;
      case "thinking":
        return null;
      case "tool": {
        if (recoveredIndices.has(i)) {
          const group = recoveryHeads.get(i);
          if (!group) return null;
          return <div key={i} className="msg notice recovery-summary" role="note">
            {t(group.pending ? "activity.fixing" : "activity.recovered")}
            {showTechnicalDetails && <details className="technical-details">
              <summary>{t("technical.details")}</summary>
              <pre dir="ltr">{group.indices.map((index) => {
                const failed = transcript.items[index] as ToolCardItem;
                return `${failed.label ?? failed.name}\n${JSON.stringify(failed.input, null, 2)}\n${failed.summary}`;
              }).join("\n\n")}</pre>
            </details>}
          </div>;
        }
        if (item.name === "qiyas_execute" && item.label === "plan" && item.status !== "error") return null;
        if (runs.hidden.has(i)) return null;
        const canShow = item.handles.length > 0 || !!item.ui?.bounds;
        const canUndo = !unavailable && item === undoable;
        return (
          <ToolCard
            key={i}
            card={item}
            repeat={runs.counts.get(i)}
            showTechnicalDetails={showTechnicalDetails}
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
            {showTechnicalDetails && item.category && <details className="technical-details"><summary>{t("technical.details")}</summary><pre>{item.category}</pre></details>}
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
  const shownOutput = artifact?.shown ?? null;
  const firstAsk = transcript.items.find((item) => item.kind === "user");
  const title = firstAsk && firstAsk.kind === "user" ? chatText(firstAsk.text).trim().replace(/\s+/g, " ").slice(0, 60) : "";
  const questions = [...transcript.items].reverse().find((item) => item.kind === "user" || item.kind === "question");
  const asked = questions?.kind === "question" ? questions.questions.length : 0;
  return (
    <section className={shownOutput ? "assistant-panel has-art" : "assistant-panel"} aria-label={t("assistant")}>
      <div className="art-slot" hidden={!shownOutput}>{artifact?.node}</div>
      <div className="chat-slot" hidden={!!shownOutput}>
      <div className="assistant-head">
        <MizanMark />
        <span className="chat-title heading">{t("assistant")}</span>
        {history && <ConversationHistory history={history} disabled={busy} />}
        <button
          type="button"
          className="ib new-chat"
          onClick={assistant.clear}
          disabled={unavailable || empty}
          aria-label={t("ai.newChat")}
          title={t("ai.newChatTip")}
        >
          <Glyph name="newChat" />
        </button>
      </div>
      {assistant.phase === "queued" && <p className="msg notice" role="status">{t("workspace.queued")}</p>}
      {assistant.memoryLimited && busy && <p className="msg notice" role="status">{t("workspace.memory")}</p>}
      {assistant.away != null && <p className="msg notice">{t("workspace.away")} {assistant.away || t(`workspace.${assistant.phase || "done"}`)}</p>}
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
              {onConnect && <button type="button" className="connect-link" onClick={onConnect}>{t("ai.connectLink")}</button>}
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
      </div>
      {shownOutput && (
        <button type="button" className="back-strip" onClick={artifact?.onBack}>
          <i className={busy ? "live" : undefined} aria-hidden="true" />
          <span><b>{t("assistant")}</b>{asked > 0 ? ` · ${t(asked === 1 ? "ai.oneQuestion" : "ai.nQuestions", { n: asked })}` : ""}</span>
          <span className="grow" />
          <span className="back-title" dir="auto">{title || t("outputs.backToChat")}</span>
          <Glyph name="next" mirror size={14} />
        </button>
      )}
      {error && (
        <div className="error" role="alert">
          {errorText(error)}
          {connectionLost(error) && <button type="button" disabled={busy}
            onClick={() => void assistant.send(t("ai.continueWork"))}>{t("ai.continueWorkButton")}</button>}
          <button type="button" className="link" onClick={onSettings}>{t(showTechnicalDetails ? "support.details" : "settings")}</button>
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
        {slashList.length > 0 && (
          <div className="slash" role="listbox" aria-label={t("ai.slash.title")}>
            {slashList.map((command, k) => (
              <button
                key={command.id}
                id={`slash-${command.id}`}
                type="button"
                role="option"
                aria-selected={k === slashRow}
                className={k === slashRow ? "sl on" : "sl"}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => runSlash(command)}
              >
                <span className="cmd" dir="auto">/{slashWord(command, lang)}</span>
                <span className="d">{t(`ai.slash.${command.id}.desc`)}</span>
              </button>
            ))}
          </div>
        )}
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
        <div className="box">
          <textarea
            ref={box}
            dir="auto"
            rows={3}
            aria-label={shownOutput ? t(`outputs.ask.${shownOutput}`) : t("ai.placeholder")}
            placeholder={shownOutput ? t(`outputs.ask.${shownOutput}`) : t("ai.placeholder")}
            role={slashList.length > 0 ? "combobox" : undefined}
            aria-expanded={slashList.length > 0 || undefined}
            aria-activedescendant={slashList.length > 0 ? `slash-${slashList[slashRow].id}` : undefined}
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setSlashOff(false);
              setSlashAt(0);
            }}
            onPaste={(e) => {
              const files = [...(e.clipboardData?.files ?? [])].filter((file) => isPicture(file) || file.type === "application/pdf");
              if (files.length === 0) return;
              e.preventDefault();
              addPictures(files);
            }}
            onKeyDown={(e) => {
              if (e.nativeEvent.isComposing) return;
              if (slashList.length > 0) {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  setSlashAt((slashRow + (e.key === "ArrowDown" ? 1 : slashList.length - 1)) % slashList.length);
                  return;
                }
                if ((e.key === "Enter" && !e.shiftKey) || e.key === "Tab") {
                  e.preventDefault();
                  runSlash(slashList[slashRow]);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  setSlashOff(true);
                  return;
                }
              }
              // Enter sends, Shift+Enter is a new line; not while an input method is composing.
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                const whole = slashCommand(draft);
                if (whole && !unavailable) runSlash(whole);
                else submit();
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
            <button
              type="button"
              className="ib"
              aria-label={t("ai.attach")}
              title={t("ai.attach")}
              disabled={pictures.length >= MAX_PICTURES}
              onClick={() => filePicker.current?.click()}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <rect x="2" y="3" width="12" height="10" rx="1.5" />
                <circle cx="6" cy="6.5" r="1.2" />
                <path d="M2.5 11.5l3.5-3 2.5 2 2-1.5 3 2.5" />
              </svg>
            </button>
            {pointing && (
              <button
                type="button"
                className="ib"
                aria-label={t("ai.markArea")}
                title={t("ai.markArea")}
                onClick={pointing.markArea}
              >
                <svg viewBox="0 0 16 16" aria-hidden="true" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M2.5 5V2.5H5M11 2.5h2.5V5M13.5 11v2.5H11M5 13.5H2.5V11" />
                  <path d="M8 6v4M6 8h4" />
                </svg>
              </button>
            )}
            {assistant.choice && assistant.choose && <WorkspacePicker choice={assistant.choice} busy={busy} onChange={assistant.choose} />}
            <span className="grow" />
            {busy ? (
              <button type="button" className="send" aria-label={t("ai.stop")} title={t("ai.stop")} onClick={assistant.cancel}>
                <Glyph name="stop" />
              </button>
            ) : (
              <button type="submit" className="send" aria-label={t("ai.send")} title={t("ai.send")} disabled={unavailable || (!draft.trim() && pictures.length === 0 && documents.length === 0)}>
                <Glyph name="arrowUp" />
              </button>
            )}
          </div>
        </div>
      </form>
    </section>
  );
});
