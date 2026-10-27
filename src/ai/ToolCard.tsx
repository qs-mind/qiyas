import { useT } from "../i18n";
import { MizanMark } from "./MizanMark";
import type { ToolCardItem } from "./messages";

type Props = {
  card: ToolCardItem;
  /** Highlights the card's entities and zooms to them; absent when there is nothing to show. */
  onShow?(): void;
  recovering?: boolean;
  /** How many identical reads this line stands for (`readRuns`). */
  repeat?: number;
  showTechnicalDetails?: boolean;
  /** Undoes the card's change; absent when it can't be undone from here. */
  onUndo?(): void;
};

/** A translated activity sentence; raw tool input and output stay in support details. */
export function ToolCard({ card, onShow, onUndo, recovering = false, repeat, showTechnicalDetails = false }: Props) {
  const t = useT();
  const subjectKey = `activity.subject.${card.activity?.category ?? "drawing"}`;
  const subject = t(subjectKey) === subjectKey ? t("activity.subject.drawing") : t(subjectKey);
  const effect = card.activity?.effect ?? (card.mutating ? "edit" : "read");
  const label = card.status === "error" ? t(recovering ? "activity.fixing" : "activity.stopped")
    : t(`activity.${["read", "view", "edit", "file", "session"].includes(effect) ? effect : "edit"}.${card.status}`, { subject });
  return (
    <div className={`tool-card ${card.status}`}>
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
        <span className="tool-name heading">{label}{repeat && repeat > 1 && <span className="tool-repeat"> ×{repeat}</span>}</span>
      </div>
      {card.count != null && card.status === "done" && <p className="tool-summary">{t("activity.count", { n: card.count })}</p>}
      {showTechnicalDetails && <details className="technical-details">
        <summary>{t("technical.details")}</summary>
        <pre dir="ltr">{card.label ?? card.name}{"\n"}{JSON.stringify(card.input, null, 2)}{"\n"}{card.summary}</pre>
      </details>}
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
