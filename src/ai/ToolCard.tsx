import { useT } from "../i18n";
import { MizanMark } from "./MizanMark";
import type { ToolCardItem } from "./messages";

type Props = {
  card: ToolCardItem;
  /** Highlights the card's entities and zooms to them; absent when there is nothing to show. */
  onShow?(): void;
  /** Undoes the card's change; absent when it can't be undone from here. */
  onUndo?(): void;
};

/** One tool the assistant ran: its name, dots moving while it runs, then its one-line result. */
export function ToolCard({ card, onShow, onUndo }: Props) {
  const t = useT();
  const key = card.label ? `command.${card.label}` : `tool.${card.name}`;
  const readable = (card.label ?? card.name).replaceAll("_", " ").replaceAll(".", " · ");
  const label = t(key) === key ? readable.charAt(0).toUpperCase() + readable.slice(1) : t(key);
  return (
    <div className={`tool-card ${card.status}`} title={JSON.stringify(card.input)}>
      <div className="tool-head">
        <span className="tool-state" aria-hidden="true">
          {card.status === "running" ? (
            <MizanMark />
          ) : (
            <svg viewBox="0 0 16 16">
              <path d={card.status === "done" ? "M3.5 8.5l3 3 6-7" : "M4 4l8 8M12 4l-8 8"} />
            </svg>
          )}
        </span>
        <span className="tool-name heading">{label}</span>
        <span className="tool-status">
          {card.status === "running" ? t("ai.running") : card.status === "error" ? t("ai.failed") : ""}
        </span>
      </div>
      {card.summary && (
        <p className="tool-summary" dir="auto">
          {card.summary}
        </p>
      )}
      {(onShow || onUndo || card.undone) && (
        <div className="tool-actions">
          {onShow && (
            <button type="button" onClick={onShow}>
              {t("ai.show")}
            </button>
          )}
          {card.undone ? (
            <span className="tool-undone">{t("ai.undone")}</span>
          ) : (
            onUndo && (
              <>
                <button type="button" onClick={onUndo} disabled={card.stale}>
                  {t("ai.undo")}
                </button>
                {card.stale && <span className="tool-undone">{t("ai.undoStale")}</span>}
              </>
            )
          )}
        </div>
      )}
    </div>
  );
}
