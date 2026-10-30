import { translate, useLanguage, useT, type Lang } from "../i18n";
import type { Activity } from "../api";
import { operationDetail, progressDetail } from "./operationActivity";
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
  /** The conversation's language, when it is not the window's: the line speaks it. */
  language?: Lang;
};

type Detail = NonNullable<Activity["detail"]>;
type Translate = ReturnType<typeof useT>;

function objectText(detail: Detail, t: Translate): string {
  const p = detail.params;
  const singular = p.n === 1 || (p.n == null && ["add", "make", "save", "open", "close", "export", "import", "measure"]
    .some((verb) => detail.key === `activity.line.${verb}`));
  const whatKey = `activity.line.object.${p.what}.${singular ? "one" : "many"}`;
  let what = t(whatKey);
  // Untranslated wire identifiers stay in technical details.
  if (what === whatKey) what = t(`activity.line.object.item.${singular ? "one" : "many"}`);
  let object = p.n == null ? what : t("activityFormat.numbered", { n: p.n, what });
  if (p.what === "layer" && p.n == null && !p.name) object = t("activity.line.layers");
  if (p.command) {
    const commandKey = [`activity.line.command.${p.command}`, `command.${p.command}`, `tool.${p.command}`, String(p.command)]
      .find((key) => t(key) !== key);
    object = commandKey ? t(commandKey) : t("activity.line.object.command.one");
    if (p.n != null) object = t("activity.line.command_count", { object, n: p.n });
  }
  if (p.name && detail.key === "activity.line.lookup") return lookedUp(String(p.name), t);
  if (p.name) {
    const namedKey = `activity.line.object.${p.what}.one`;
    const namedWhat = p.command || p.n != null ? object : t(namedKey) === namedKey ? what : t(namedKey);
    object = detail.key === "activity.line.lookup" || detail.key === "activity.line.capture"
      ? String(p.name)
      : t(p.what === "table" ? "activity.line.named_table" : "activityFormat.named", { what: namedWhat, name: p.name });
  }
  if (p.file) object = p.command ? t("activityFormat.named", { what: object, name: p.file }) : String(p.file);
  if (p.page != null) object = t("activity.line.page", { object, n: p.page });
  if (p.to) object = t("activity.line.renamed", { object, name: p.to });
  if (p.rows != null) object = t("activity.line.rows", { object, n: p.rows });
  const size = [p.paper, p.scale].filter((v) => v != null).join(t("activity.line.comma"));
  if (size) object = t("activityFormat.size", { object, size });
  return object;
}

/** The commands Mizan read how to use: their plain names when the window has them, else how many
 *  ("4 commands"): a command's own name (`drafting_knowledge`) is not for the engineer. */
function lookedUp(names: string, t: Translate): string {
  const list = names.split(",").map((name) => name.trim()).filter(Boolean);
  const plain = list.map((name) => [`activity.line.command.${name}`, `command.${name}`].find((key) => t(key) !== key));
  if (plain.every(Boolean)) return join(plain.map((key) => t(key!)), t);
  return t("activityFormat.numbered", { n: list.length, what: t(`activity.line.object.command.${list.length === 1 ? "one" : "many"}`) });
}

/** Model space by its name in the window, not its block's (`*Model_Space`). */
function sheetName(name: string | number, t: Translate): string | number {
  return typeof name === "string" && name.toLowerCase() === "*model_space" ? t("model") : name;
}

function withScope(text: string, params: Detail["params"], t: Translate): string {
  for (const field of ["layer", "sheet", "block", "collection"] as const) {
    const value = params[field];
    if (value != null) {
      const collectionKey = `activity.line.object.${value}.many`;
      const name = field === "collection" ? (t(collectionKey) === collectionKey ? t("activity.line.object.item.many") : t(collectionKey)) : field === "sheet" ? sheetName(value, t) : value;
      text = t(`activity.line.${field}`, { line: text, name });
    }
  }
  return text;
}

function join(parts: string[], t: Translate): string {
  return parts.reduce((text, part) => text ? t("activity.line.and", { first: text, next: part }) : part, "");
}

function detailText(detail: Detail, status: "running" | "done", t: Translate): string {
  const p = detail.params;
  if (detail.parts?.length || detail.key === "activity.line.batch") {
    const parts = detail.parts ?? [];
    const common: Detail["params"] = {};
    for (const field of ["layer", "sheet"] as const) {
      if (parts.length && parts[0].params[field] != null && parts.every((part) => part.params[field] === parts[0].params[field])) {
        common[field] = parts[0].params[field];
      }
    }
    const scoped = parts.map((part) => ({ ...part, params: Object.fromEntries(Object.entries(part.params).filter(([key]) => !(key in common))) }));
    const sameVerb = scoped.length > 0 && t(`${scoped[0].key}.${status}`) !== `${scoped[0].key}.${status}` && scoped.every((part) => part.key === scoped[0].key && !["activity.line.batch", "activity.line.plan"].includes(part.key) && part.params.notes == null);
    let text = sameVerb
      ? t(`${scoped[0].key}.${status}`, { object: join(scoped.map((part) => withScope(objectText(part, t), part.params, t)), t) })
      : join(scoped.map((part) => detailText(part, status, t)), t);
    if (!text) text = t(`activity.line.run.${status}`, { object: t("activity.line.object.step.many") });
    text = withScope(text, common, t);
    if (detail.key === "activity.line.unchanged") {
      text = withScope(t(`${detail.key}.${status}`, { object: join(scoped.map((part) => withScope(objectText(part, t), part.params, t)), t) }), common, t);
    }
    return p.more ? t("activity.line.more", { line: text, n: p.more }) : text;
  }
  if (detail.key === "activity.line.plan") {
    return p.total != null ? t(`${detail.key}.${status}`, p)
      : t(`activity.line.edit.${status}`, { object: t("activity.line.object.plan.one") });
  }
  const key = `${detail.key}.${status}`;
  const verb = t(key) === key ? `activity.line.run.${status}` : key;
  let text = t(verb, { object: objectText(detail, t) });
  text = withScope(text, p, t);
  if (p.notes != null) text = t("activity.line.notes", { line: text, n: p.notes });
  return text;
}

/** Both live and restored cards use the same structured data and the current language. */
export function activityLine(detail: Detail, status: "running" | "done", t: Translate): string {
  const text = detailText(detail, status, t);
  return status === "running" ? t("activityFormat.running", { line: text }) : text;
}

/** A translated activity sentence; raw tool input and output stay in support details. */
export function ToolCard({ card, onShow, onUndo, recovering = false, repeat, showTechnicalDetails = false, language }: Props) {
  const uiT = useT();
  const { lang: uiLanguage } = useLanguage();
  const t: Translate = language ? (key, vars) => translate(language, key, vars) : uiT;
  const subjectKey = `activity.subject.${card.activity?.category ?? "drawing"}`;
  const subject = t(subjectKey) === subjectKey ? t("activity.subject.drawing") : t(subjectKey);
  const effect = card.activity?.effect ?? (card.mutating ? "edit" : "read");
  const detail = operationDetail(card);
  const label = card.status === "error" ? t(recovering ? "activity.fixing" : "activity.stopped")
    : detail ? activityLine(detail, card.status, t)
      : t(`activity.${["read", "view", "edit", "file", "session"].includes(effect) ? effect : "edit"}.${card.status}`, { subject });
  return (
    <div className={`tool-card ${card.status}`} lang={language} dir={language ? (language === "ar" ? "rtl" : "ltr") : undefined}>
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
        <span className="tool-name heading">{label}{repeat && repeat > 1 && <span className="tool-repeat"> {t("activity.line.repeat", { n: repeat })}</span>}</span>
      </div>
      {card.status === "running" && card.progress && Number.isInteger(card.progress.done) &&
        Number.isInteger(card.progress.total) && card.progress.total > 0 && card.progress.done >= 0 && card.progress.done <= card.progress.total && (
        <p className="tool-summary" role="status" lang={uiLanguage} dir={uiLanguage === "ar" ? "rtl" : "ltr"}>{uiT("activity.progress", { n: card.progress.done, total: card.progress.total, label: detailText(progressDetail(card.progress.label), "done", uiT) })}</p>
      )}
      {!card.activity?.detail && card.count != null && card.status === "done" && <p className="tool-summary">{t("activity.count", { n: card.count })}</p>}
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
