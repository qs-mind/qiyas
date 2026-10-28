/** Mizan's box: what the user writes, the pictures and PDFs going with it, what they point at, the
 *  slash commands, the model button and send / stop. */
import { useRef, useState, type RefObject } from "react";
import { aiAttachPdf } from "../api";
import type { Pointing } from "../app/useAssistantWiring";
import { useLanguage, useT } from "../i18n";
import { uiError, userError } from "../i18n/errors";
import { Glyph } from "../shell/glyphs";
import type { OutputId } from "../shell/outputs";
import { isPicture, MAX_PICTURES, type Picture } from "./pictures";
import { slashCommand, slashMatches, slashWord, type SlashCommand } from "./slashCommands";
import type { Drafts } from "./useDrafts";
import type { Assistant } from "./useAssistant";
import { WorkspacePicker } from "./WorkspacePicker";

type Props = {
  drafts: Drafts;
  box: RefObject<HTMLTextAreaElement | null>;
  busy: boolean;
  /** Busy, or a history read is on its way: nothing is sent. */
  blocked: boolean;
  /** Options are chosen on open questions, so send works with an empty box. */
  answering: boolean;
  pointing?: Pointing;
  /** The output on screen in place of the chat: the box asks about it. */
  shownOutput: OutputId | null;
  model?: Partial<Pick<Assistant, "choice" | "choose">>;
  read(file: File): Promise<Picture>;
  onSubmit(): void;
  onSlash(command: SlashCommand): void;
  onCancel(): void;
};

export function Composer({ drafts, box, busy, blocked, answering, pointing, shownOutput, model, read, onSubmit, onSlash, onCancel }: Props) {
  const t = useT();
  const { lang } = useLanguage();
  const { draft } = drafts;
  const filePicker = useRef<HTMLInputElement>(null);
  const [attaching, setAttaching] = useState(false);
  const [pictureError, setPictureError] = useState<string | null>(null);
  /** Slash commands: the row chosen in the list, and whether Escape closed it for this text. */
  const [slashAt, setSlashAt] = useState(0);
  const [slashOff, setSlashOff] = useState(false);
  const unavailable = blocked || attaching;
  const slashList = slashOff || unavailable ? [] : slashMatches(draft.text, lang);
  const slashRow = Math.min(slashAt, Math.max(0, slashList.length - 1));
  const empty = !draft.text.trim() && !answering && draft.pictures.length === 0 && draft.documents.length === 0;

  function runSlash(command: SlashCommand) {
    setSlashAt(0);
    onSlash(command);
  }

  function send() {
    if (unavailable) return;
    setPictureError(null);
    onSubmit();
  }

  /** Adds the pictures and PDFs among `files`, up to `MAX_PICTURES` of each. */
  async function attach(files: File[]) {
    if (unavailable) return;
    setPictureError(null);
    const pdfs = files.filter((file) => file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf"));
    if (pdfs.length) {
      setAttaching(true);
      try {
        if (draft.documents.length + pdfs.length > MAX_PICTURES) throw userError(t("ai.tooManyPictures", { n: String(MAX_PICTURES) }));
        for (const file of pdfs) {
          if (file.size > 20 * 1024 * 1024) throw userError(t("ai.pdfTooLarge"));
          const attachment = await aiAttachPdf(file.name, Array.from(new Uint8Array(await file.arrayBuffer())));
          drafts.setDocuments((now) => [...now, attachment]);
        }
      } catch (error) { setPictureError(uiError(error)); }
      finally { setAttaching(false); }
    }
    const wanted = files.filter(isPicture);
    if (wanted.length === 0) return;
    const room = MAX_PICTURES - draft.pictures.length;
    if (wanted.length > room) setPictureError(t("ai.tooManyPictures", { n: String(MAX_PICTURES) }));
    const results = await Promise.allSettled(wanted.slice(0, Math.max(0, room)).map((file) => read(file)));
    const ok = results.flatMap((r) => (r.status === "fulfilled" ? [r.value] : []));
    if (ok.length < results.length) setPictureError(t("ai.pictureFailed"));
    drafts.setPictures((now) => [...now, ...ok].slice(0, MAX_PICTURES));
  }

  const removeButton = (label: string, onClick: () => void) => (
    <button type="button" className="chip-remove" aria-label={label} title={label} onClick={onClick}>×</button>
  );
  const ask = shownOutput ? t(`outputs.ask.${shownOutput}`) : t("ai.placeholder");

  return (
    <form
      className="composer"
      onSubmit={(e) => { e.preventDefault(); send(); }}
      onDragOver={(e) => { if ([...e.dataTransfer.items].some((i) => i.kind === "file")) e.preventDefault(); }}
      onDrop={(e) => {
        const files = [...e.dataTransfer.files];
        if (files.length === 0) return;
        e.preventDefault();
        void attach(files);
      }}
    >
      {slashList.length > 0 && (
        <div className="slash" role="listbox" aria-label={t("ai.slash.title")}>
          {slashList.map((command, k) => (
            <button key={command.id} id={`slash-${command.id}`} type="button" role="option" aria-selected={k === slashRow}
              className={k === slashRow ? "sl on" : "sl"} onMouseDown={(e) => e.preventDefault()} onClick={() => runSlash(command)}>
              <span className="cmd" dir="auto">/{slashWord(command, lang)}</span>
              <span className="d">{t(`ai.slash.${command.id}.desc`)}</span>
            </button>
          ))}
        </div>
      )}
      {(pointing?.selection || pointing?.region || draft.pictures.length > 0) && (
        <div className="composer-chips">
          {pointing?.selection && (
            <span className="chip" title={pointing.selection.kinds}>
              {t("ai.chip.selection", { n: String(pointing.selection.count) })}
              {pointing.selection.kinds && <span className="chip-detail">{pointing.selection.kinds}</span>}
              {removeButton(t("ai.chip.remove"), pointing.detachSelection)}
            </span>
          )}
          {pointing?.region && (
            <span className="chip">
              {t("ai.chip.area", { w: pointing.region.width.toFixed(2), h: pointing.region.height.toFixed(2) })}
              {pointing.region.count !== null && <span className="chip-detail">{t("ai.chip.entities", { n: String(pointing.region.count) })}</span>}
              {removeButton(t("ai.chip.remove"), pointing.clearRegion)}
            </span>
          )}
          {draft.pictures.map((p, k) => (
            <span key={k} className="chip picture">
              <img src={p.url} alt={t("ai.picture")} className="thumb" />
              {removeButton(t("ai.removePicture"), () => drafts.setPictures((now) => now.filter((_, j) => j !== k)))}
            </span>
          ))}
        </div>
      )}
      {pictureError && <p className="composer-note" role="alert">{pictureError}</p>}
      <div className="box">
        <textarea
          ref={box}
          dir="auto"
          rows={3}
          aria-label={ask}
          placeholder={ask}
          role={slashList.length > 0 ? "combobox" : undefined}
          aria-expanded={slashList.length > 0 || undefined}
          aria-activedescendant={slashList.length > 0 ? `slash-${slashList[slashRow].id}` : undefined}
          value={draft.text}
          onChange={(e) => {
            drafts.setText(e.target.value);
            setSlashOff(false);
            setSlashAt(0);
          }}
          onPaste={(e) => {
            const files = [...(e.clipboardData?.files ?? [])].filter((file) => isPicture(file) || file.type === "application/pdf");
            if (files.length === 0) return;
            e.preventDefault();
            void attach(files);
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
              const whole = slashCommand(draft.text);
              if (whole && !unavailable) runSlash(whole);
              else send();
            }
          }}
        />
        {draft.documents.map((document) => (
          <div className="pointing-chip" key={document.path}>
            <span>{document.name} · {document.pages} {t("ai.pdfPages")}</span>
            <button type="button" aria-label={t("ai.removeDocument")} disabled={unavailable}
              onClick={() => drafts.setDocuments((now) => now.filter((item) => item.path !== document.path))}>×</button>
          </div>
        ))}
        <input ref={filePicker} type="file" accept="image/png,image/jpeg,image/webp,image/gif,application/pdf" multiple hidden
          onChange={(e) => { void attach([...(e.target.files ?? [])]); e.target.value = ""; }} />
        <div className="composer-buttons">
          <button type="button" className="ib" aria-label={t("ai.attach")} title={t("ai.attach")}
            disabled={unavailable || draft.pictures.length >= MAX_PICTURES} onClick={() => filePicker.current?.click()}>
            <Glyph name="attach" size={18} />
          </button>
          {pointing && (
            <button type="button" className="ib" aria-label={t("ai.markArea")} title={t("ai.markArea")} onClick={pointing.markArea}>
              <Glyph name="area" size={18} />
            </button>
          )}
          {model?.choice && model.choose && <WorkspacePicker choice={model.choice} busy={busy} onChange={model.choose} />}
          <span className="grow" />
          {busy ? (
            <button type="button" className="send" aria-label={t("ai.stop")} title={t("ai.stop")} onClick={onCancel}>
              <Glyph name="stop" />
            </button>
          ) : (
            <button type="submit" className="send" aria-label={t("ai.send")} title={t("ai.send")} disabled={unavailable || empty}>
              <Glyph name="arrowUp" />
            </button>
          )}
        </div>
      </div>
    </form>
  );
}
