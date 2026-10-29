import { chatText } from "./content";
import { errorText, connectionLost } from "./chatErrors";
import { uiError } from "../i18n/errors";
import { memo, useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { Pointing } from "../app/useAssistantWiring";
import { translate, useLanguage, useT } from "../i18n";
import { MizanMark } from "./MizanMark";
import { PackCheckCard } from "./PackCheckCard";
import { ConversationHistory, type ConversationHistoryState } from "./ConversationHistory";
import { readRuns, startNewOffered, undoableCard, type ToolCardItem } from "./messages";
import { readPicture, type Picture } from "./pictures";
import { answerText } from "./QuestionList";
import type { SlashCommand } from "./slashCommands";
import { Composer } from "./Composer";
import { ThreadItem, planActions, type ThreadContext } from "./ThreadItem";
import { PlanDock, latestDockPlan } from "./PlanDock";
import { chatLanguage, openQuestions, recoveryGroups } from "./thread";
import { useDrafts } from "./useDrafts";
import { Glyph } from "../shell/glyphs";
import type { ShownOutput } from "../shell/outputs";
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
  artifact?: { shown: ShownOutput | null; node: ReactNode; onBack(): void };
  /** The drawing on screen and the same revision used by the take-off report. */
  drawing?: { path: string; revision: unknown; hasTakeoff: boolean };
  /** The /compare command: ask for a BOQ and find its conflicts with the drawings. */
  onCompare?(): void;
  /** The /connect command and the empty chat's link: the "Connect an AI app" dialog. */
  onConnect?(): void;
  /** A key asked for the box (`/`, Ctrl+K): it takes the focus, and `text` goes in it when empty. */
  ask?: { n: number; text: string };
  /** The Artifacts button, after History in the head. */
  artifacts?: ReactNode;
  /** Reads a picture pasted, dropped or picked (tests give their own). */
  read?(file: File): Promise<Picture>;
};

const EXAMPLES = ["ai.example1", "ai.example2", "ai.example3", "ai.example4", "ai.example5", "ai.example6", "ai.example7", "ai.example8"];
/** Three different examples each time the empty chat shows. */
function pickExamples(): string[] {
  return [...EXAMPLES].sort(() => Math.random() - 0.5).slice(0, 3);
}

/** The in-app agent, Mizan: the conversation (`ThreadItem`), or an output in its place, and the box
 *  (`Composer`). Memoised: the app re-renders on every pointer move. */
export const AssistantPanel = memo(function AssistantPanel({
  assistant,
  showTechnicalDetails = false,
  onShow,
  onUndo,
  onSettings,
  pointing,
  history,
  artifact,
  drawing,
  onCompare,
  onConnect,
  ask,
  artifacts,
  read = readPicture,
}: Props) {
  const t = useT();
  const { lang } = useLanguage();
  const { transcript, busy, error } = assistant;
  const items = transcript.items;
  const chatLang = chatLanguage(items, lang);
  const blocked = busy || !!history?.pending;
  const drafts = useDrafts(assistant.workspace);
  const { draft } = drafts;
  const [examples] = useState(pickExamples);
  const box = useRef<HTMLTextAreaElement>(null);
  const focusBox = () => box.current?.focus();

  // `/` or Ctrl+K: the box takes the focus (a slash typed in it lists the commands).
  useEffect(() => {
    if (!ask || ask.n === 0) return;
    focusBox();
    if (ask.text && !draft.text) drafts.setText(ask.text);
  }, [ask?.n]);

  // "Ask Mizan" on the canvas menu, or an area marked: the box takes the focus.
  const focusKey = pointing?.focusKey ?? 0;
  useEffect(() => {
    if (focusKey > 0) focusBox();
  }, [focusKey]);

  // The newest message stays in view while the reader is at the bottom; scrolled up, they are left
  // where they are and a button offers the way back.
  const thread = useStickToBottom([transcript, busy]);
  const planId = useId();
  const dock = artifact?.shown ? null : latestDockPlan(items);
  const planItemId = (index: number) => `${planId}-plan-${index}`;

  const open = openQuestions(items);
  const picks = open ? draft.picks : {};
  const answering = !!open && Object.keys(picks).length > 0;

  /** Sends `text` (the box's words by default) with the box's pictures and PDFs. */
  async function submit(prompt?: string) {
    const typed = prompt ?? (open && answering ? answerText(open.questions, picks, draft.text) : draft.text);
    const files = draft.documents.map((document) => `[PDF attachment: ${JSON.stringify(document.name)}; ${document.pages} pages; path: ${JSON.stringify(document.path)}]`);
    const text = [typed.trim(), ...files].filter(Boolean).join("\n\n");
    if ((!text && draft.pictures.length === 0) || blocked) return;
    // What the reader sends is what they want to see next to.
    thread.jump(false);
    const pictures = draft.pictures;
    const sent = pointing
      ? await pointing.send(text, pictures)
      : pictures.length > 0
        ? await assistant.send(text, {
            images: pictures.map(({ media_type, data }) => ({ media_type, data })),
            shown: { images: pictures.map((p) => p.url) },
          })
        : await assistant.send(text);
    if (sent) drafts.clear();
  }

  /** Chooses an option on the open questions (the same again takes it off); when every question has
   *  one, the answers go at once with whatever is in the box. */
  function pick(questionId: string, label: string) {
    if (!open) return;
    const next = { ...picks };
    if (next[questionId] === label) delete next[questionId];
    else next[questionId] = label;
    drafts.setPicks(next);
    if (open.questions.every((q) => next[q.id])) void submit(answerText(open.questions, next, draft.text));
  }

  /** A prompt command goes to Mizan as the user's message; an action is the window's. */
  function runSlash(command: SlashCommand) {
    if (command.kind === "prompt") {
      void submit(t(`ai.slash.${command.id}.prompt`));
      return;
    }
    drafts.setText("");
    if (command.id === "new") assistant.clear();
    else if (command.id === "compare") onCompare?.();
    else if (command.id === "connect") onConnect?.();
  }

  // Said once per turn to screen readers, instead of reading out the streaming log.
  const [announce, setAnnounce] = useState("");
  const wasBusy = useRef(busy);
  useEffect(() => {
    if (wasBusy.current && !busy) setAnnounce(t("ai.finished"));
    if (busy) setAnnounce("");
    wasBusy.current = busy;
  }, [busy]);

  const cx: ThreadContext = {
    items, busy, unavailable: blocked, chatLang, showTechnicalDetails,
    dockedPlanIndex: dock?.index,
    openIndex: open?.index ?? null, picks, onPick: pick,
    runs: readRuns(items), recovery: recoveryGroups(items, busy), undoable: undoableCard(transcript),
    onShow, onUndo, onSettings, send: (text) => { void assistant.send(text); }, focusBox,
  };

  const empty = items.length === 0;
  const shownOutput = artifact?.shown ?? null;
  const firstAsk = items.find((item) => item.kind === "user");
  const title = firstAsk && firstAsk.kind === "user" ? chatText(firstAsk.text).trim().replace(/\s+/g, " ").slice(0, 60) : "";
  const asked = open?.questions.length ?? 0;
  return (
    <section className={shownOutput ? "assistant-panel has-art" : "assistant-panel"} aria-label={t("assistant")}>
      <div className="art-slot" hidden={!shownOutput}>{artifact?.node}</div>
      <div className="chat-slot" hidden={!!shownOutput}>
        <div className="assistant-head">
          <MizanMark still={!busy} />
          <span className="chat-title heading">{t("assistant")}</span>
          {artifacts}
          {history && <ConversationHistory history={history} disabled={busy} />}
          <button type="button" className="ib new-chat" onClick={assistant.clear} disabled={blocked || empty}
            aria-label={t("ai.newChat")} title={t("ai.newChatTip")}>
            <Glyph name="newChat" />
          </button>
        </div>
        {assistant.phase === "queued" && <p className="msg notice" role="status">{t("workspace.queued")}</p>}
        {assistant.memoryLimited && busy && <p className="msg notice" role="status">{t("workspace.memory")}</p>}
        {assistant.away != null && <p className="msg notice">{t("workspace.away")} {assistant.away || t(`workspace.${assistant.phase || "done"}`)}</p>}
        <p className="sr-only" role="status" aria-live="polite">{announce}</p>
        {history?.error && <p className="msg notice" role="alert">{uiError(history.error, lang)}</p>}
        <div className="assistant-thread">
          <div className="assistant-log" ref={thread.ref} onScroll={thread.onScroll}>
            {drawing && !drawing.hasTakeoff && !shownOutput && <PackCheckCard
              key={`${assistant.workspace ?? "drawing"}:${drawing.path}`}
              drawing={drawing.path}
              workspace={assistant.workspace}
              revision={drawing.revision}
              busy={busy}
            />}
            {empty ? (
              <div className="assistant-empty">
                <p className="sr-only">{t("ai.emptyTitle")}</p>
                {history?.latest && <button type="button" className="continue-chat" disabled={busy || history.pending}
                  onClick={() => { void history.resume(history.latest!); }}>
                  {t("ai.continue", { title: history.latest.title })}
                </button>}
                <ul>
                  {examples.map((key) => (
                    <li key={key}>
                      <button type="button" onClick={() => { drafts.setText(t(key)); focusBox(); }}>{t(key)}</button>
                    </li>
                  ))}
                </ul>
              </div>
            ) : (
              items.map((item, i) => <ThreadItem key={i} item={item} i={i} cx={cx} id={item.kind === "plan" ? planItemId(i) : undefined} />)
            )}
            {busy && (
              <p className="working" lang={chatLang} dir={chatLang === "ar" ? "rtl" : "ltr"}>
                <MizanMark size="md" />
                {translate(chatLang, "ai.working")}
              </p>
            )}
            {!blocked && (startNewOffered(transcript) || error === "crashed") && (
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
      {dock && <PlanDock key={`${assistant.workspace ?? "chat"}:${dock.index}`} plan={dock.plan} busy={busy} language={chatLang}
        threadItemId={planItemId(dock.index)} awaitingApproval={dock.awaitingApproval} showTechnicalDetails={showTechnicalDetails}
        {...planActions(dock.plan, dock.index, cx)} onShowInChat={thread.onScroll} />}
      {error && (
        <div className="error" role="alert">
          {errorText(error, lang)}
          {connectionLost(error)
            ? <button type="button" disabled={busy} onClick={() => void assistant.send(t("ai.continueWork"))}>{t("ai.continueWorkButton")}</button>
            : <button type="button" className="link" onClick={onSettings}>{t("settings")}</button>}
        </div>
      )}
      <Composer
        drafts={drafts}
        box={box}
        busy={busy}
        blocked={blocked}
        answering={answering}
        pointing={pointing}
        shownOutput={shownOutput}
        model={assistant}
        read={read}
        onSubmit={() => void submit()}
        onSlash={runSlash}
        onCancel={assistant.cancel}
      />
    </section>
  );
});
