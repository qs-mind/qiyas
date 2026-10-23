import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { AttDefInfo, BlockInfo } from "../api";
import { useT } from "../i18n";

/** Characters AutoCAD refuses in a block name (as the core checks them). */
const BAD_NAME = /[<>/\\":;?*|,=`]/;

/** A modal dialog that shows itself, gives focus back on close, keeps its keys from the drawing
 *  and runs `submit` (its error stays in the dialog). */
export function useModal() {
  const dialog = useRef<HTMLDialogElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);
  async function act(work: () => Promise<void>, close = true) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await work();
      if (close) dialog.current?.close();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }
  return { dialog, error, busy, act, close: () => dialog.current?.close() };
}

type ShellProps = {
  modal: ReturnType<typeof useModal>;
  id: string;
  title: string;
  onClose(): void;
  onSubmit?(): void;
  children: ReactNode;
};

/** The dialog and form around a block dialog's fields; Enter submits. */
export function Shell({ modal, id, title, onClose, onSubmit, children }: ShellProps) {
  return (
    <dialog
      ref={modal.dialog}
      className="settings"
      aria-labelledby={id}
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit?.();
        }}
      >
        <h2 id={id} className="heading">
          {title}
        </h2>
        {children}
        {modal.error && (
          <p className="error" role="alert">
            {modal.error}
          </p>
        )}
      </form>
    </dialog>
  );
}

type CreateProps = {
  /** The drawing's block names: a new block can't take one of them. */
  names: string[];
  /** Makes the block; `convert` puts a reference in place of the entities. */
  onSubmit(name: string, convert: boolean): Promise<void>;
  onClose(): void;
};

/** BLOCK's last step: the new block's name, and whether a reference replaces its entities. */
export function CreateBlockDialog({ names, onSubmit, onClose }: CreateProps) {
  const t = useT();
  const modal = useModal();
  const [name, setName] = useState("");
  const [convert, setConvert] = useState(true);
  const trimmed = name.trim();
  const taken = names.some((n) => n.toUpperCase() === trimmed.toUpperCase());
  const bad = BAD_NAME.test(trimmed);
  const valid = trimmed !== "" && !taken && !bad;
  const submit = () => valid && modal.act(() => onSubmit(trimmed, convert));
  return (
    <Shell modal={modal} id="create-block-title" title={t("block.createTitle")} onClose={onClose} onSubmit={submit}>
      <label>
        <span>{t("block.name")}</span>
        <input dir="auto" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      {taken && <p className="error">{t("block.nameTaken")}</p>}
      {bad && <p className="error">{t("block.nameBad")}</p>}
      <label className="check">
        <input type="checkbox" checked={convert} onChange={(e) => setConvert(e.target.checked)} />
        <span>{t("block.convert")}</span>
      </label>
      <div className="buttons">
        <button type="button" onClick={modal.close}>
          {t("text.cancel")}
        </button>
        <button type="submit" className="primary" disabled={!valid || modal.busy}>
          {t("block.create")}
        </button>
      </div>
    </Shell>
  );
}

/** One attribute to ask for: its tag, its prompt (empty: the tag is shown) and its value. */
export type AttributeField = { tag: string; prompt: string; value: string };

/** What INSERT asks for a block's attribute definitions: each but the constant ones (they draw
 *  from the block), starting from its default. */
export const promptsFor = (attdefs: AttDefInfo[]): AttributeField[] =>
  attdefs.filter((a) => !a.constant).map((a) => ({ tag: a.tag, prompt: a.prompt, value: a.default }));

type AttributesProps = {
  /** An i18n key: inserting a block, or editing a reference's attributes. */
  heading: string;
  fields: AttributeField[];
  /** Every value, by tag. */
  onSubmit(values: Record<string, string>): Promise<void>;
  onClose(): void;
};

/** INSERT's attribute prompts, and ATTEDIT: one line per attribute, by its prompt. */
export function AttributesDialog({ heading, fields, onSubmit, onClose }: AttributesProps) {
  const t = useT();
  const modal = useModal();
  const [values, setValues] = useState(fields.map((f) => f.value));
  const submit = () =>
    modal.act(() => onSubmit(Object.fromEntries(fields.map((f, i) => [f.tag, values[i]]))));
  return (
    <Shell modal={modal} id="attributes-title" title={t(heading)} onClose={onClose} onSubmit={submit}>
      {fields.map((f, i) => (
        <label key={f.tag}>
          <span>{f.prompt || f.tag}</span>
          <input
            dir="auto"
            autoFocus={i === 0}
            value={values[i]}
            onChange={(e) => setValues(values.map((v, j) => (j === i ? e.target.value : v)))}
          />
        </label>
      ))}
      <div className="buttons">
        <button type="button" onClick={modal.close}>
          {t("text.cancel")}
        </button>
        <button type="submit" className="primary" disabled={modal.busy}>
          {t("block.ok")}
        </button>
      </div>
    </Shell>
  );
}

/** An attribute definition as ATTDEF places it. */
export type AttDefDraft = { tag: string; prompt: string; default: string; height: number };

type AttDefProps = {
  /** The text height to start from. */
  height: number;
  onSubmit(draft: AttDefDraft): Promise<void>;
  onClose(): void;
};

/** ATTDEF: the tag (one word, upper-cased), prompt, default and height of the definition. */
export function AttDefDialog({ height, onSubmit, onClose }: AttDefProps) {
  const t = useT();
  const modal = useModal();
  const [tag, setTag] = useState("");
  const [prompt, setPrompt] = useState("");
  const [value, setValue] = useState("");
  const [size, setSize] = useState(String(Number(height.toPrecision(6))));
  const h = Number(size);
  const valid = /^\S+$/.test(tag.trim()) && h > 0 && Number.isFinite(h);
  const submit = () =>
    valid && modal.act(() => onSubmit({ tag: tag.trim().toUpperCase(), prompt, default: value, height: h }));
  return (
    <Shell modal={modal} id="attdef-title" title={t("block.attdefTitle")} onClose={onClose} onSubmit={submit}>
      <label>
        <span>{t("block.tag")}</span>
        <input dir="auto" autoFocus value={tag} onChange={(e) => setTag(e.target.value)} />
      </label>
      <label>
        <span>{t("block.prompt")}</span>
        <input dir="auto" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
      </label>
      <label>
        <span>{t("block.default")}</span>
        <input dir="auto" value={value} onChange={(e) => setValue(e.target.value)} />
      </label>
      <label>
        <span>{t("block.height")}</span>
        <input type="number" min={0} step="any" value={size} onChange={(e) => setSize(e.target.value)} />
      </label>
      <div className="buttons">
        <button type="button" onClick={modal.close}>
          {t("text.cancel")}
        </button>
        <button type="submit" className="primary" disabled={!valid || modal.busy}>
          {t("text.place")}
        </button>
      </div>
    </Shell>
  );
}

type ManagerProps = {
  blocks: BlockInfo[];
  /** Changes one definition's prompt and default. */
  onSave(block: string, tag: string, prompt: string, value: string): Promise<void>;
  /** Gives every reference to `block` its attributes as defined now. */
  onSync(block: string): Promise<void>;
  onClose(): void;
};

/** BATTMAN: the attribute definitions of a block, their prompts and defaults to edit, and Sync
 *  to bring every reference in line. */
export function AttributeManager({ blocks, onSave, onSync, onClose }: ManagerProps) {
  const t = useT();
  const modal = useModal();
  const tagged = blocks.filter((b) => b.attdefs.length > 0);
  const [name, setName] = useState(tagged[0]?.name ?? "");
  const block = tagged.find((b) => b.name === name);
  const [rows, setRows] = useState(block?.attdefs ?? []);
  const choose = (next: string) => {
    setName(next);
    setRows(tagged.find((b) => b.name === next)?.attdefs ?? []);
  };
  const edit = (i: number, field: "prompt" | "default", value: string) =>
    setRows(rows.map((r, j) => (j === i ? { ...r, [field]: value } : r)));
  const apply = () =>
    modal.act(async () => {
      for (const [i, row] of rows.entries()) {
        const was = block?.attdefs[i];
        if (was && (was.prompt !== row.prompt || was.default !== row.default)) {
          await onSave(name, row.tag, row.prompt, row.default);
        }
      }
    }, false);
  return (
    <Shell modal={modal} id="battman-title" title={t("block.managerTitle")} onClose={onClose}>
      {tagged.length === 0 ? (
        <p>{t("block.noAttributes")}</p>
      ) : (
        <>
          <label>
            <span>{t("block.block")}</span>
            <select value={name} onChange={(e) => choose(e.target.value)}>
              {tagged.map((b) => (
                <option key={b.name} value={b.name}>
                  {b.name}
                </option>
              ))}
            </select>
          </label>
          <table className="attdefs">
            <thead>
              <tr>
                <th>{t("block.tag")}</th>
                <th>{t("block.prompt")}</th>
                <th>{t("block.default")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={r.tag}>
                  <td dir="auto">{r.tag}</td>
                  <td>
                    <input
                      dir="auto"
                      aria-label={t("block.promptOf", { tag: r.tag })}
                      value={r.prompt}
                      onChange={(e) => edit(i, "prompt", e.target.value)}
                    />
                  </td>
                  <td>
                    <input
                      dir="auto"
                      aria-label={t("block.defaultOf", { tag: r.tag })}
                      value={r.default}
                      onChange={(e) => edit(i, "default", e.target.value)}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      <div className="buttons">
        <button type="button" onClick={modal.close}>
          {t("block.close")}
        </button>
        <button type="button" disabled={!block || modal.busy} onClick={() => modal.act(() => onSync(name), false)}>
          {t("block.sync")}
        </button>
        <button type="button" className="primary" disabled={!block || modal.busy} onClick={apply}>
          {t("block.apply")}
        </button>
      </div>
    </Shell>
  );
}

type BarProps = {
  name: string;
  onSave(): void;
  onDiscard(): void;
};

/** Shown while a block is edited in place: which block, and how to end the edit. */
export function BlockEditBar({ name, onSave, onDiscard }: BarProps) {
  const t = useT();
  return (
    <div className="block-edit-bar" role="status">
      <span>{t("block.editing", { name })}</span>
      <button type="button" className="primary" onClick={onSave}>
        {t("block.saveEdit")}
      </button>
      <button type="button" onClick={onDiscard}>
        {t("block.discardEdit")}
      </button>
    </div>
  );
}
