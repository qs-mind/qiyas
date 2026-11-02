import { readingDirection } from "../ai/content";
import { useState, type KeyboardEvent } from "react";
import { useT } from "../i18n";
import { Shell, useModal } from "./BlockDialogs";

/** Characters AutoCAD refuses in a group name (as the core checks them). */
const BAD_NAME = /[<>/\\":;?*|,=`]/;

type StatesProps = {
  /** The drawing's layer states. */
  states: string[];
  onSave(name: string): Promise<void>;
  onRestore(name: string): Promise<void>;
  onRename(from: string, to: string): Promise<void>;
  onDelete(name: string): Promise<void>;
  onClose(): void;
};

/** One state's row: restore it, rename it (Enter), delete it. */
function StateRow({ name, act, props }: { name: string; act: ReturnType<typeof useModal>["act"]; props: StatesProps }) {
  const t = useT();
  const [to, setTo] = useState(name);
  const rename = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    const next = to.trim();
    if (next !== "" && next !== name) act(() => props.onRename(name, next), false);
  };
  return (
    <li>
      <input
        {...readingDirection(to)}
        aria-label={t("layerState.renameNamed", { name })}
        value={to}
        onChange={(e) => setTo(e.target.value)}
        onKeyDown={rename}
      />
      <button type="button" aria-label={t("layerState.restoreNamed", { name })} onClick={() => act(() => props.onRestore(name))}>
        {t("layerState.restore")}
      </button>
      <button type="button" className="link" aria-label={t("layerState.deleteNamed", { name })} onClick={() => act(() => props.onDelete(name), false)}>
        {t("views.delete")}
      </button>
    </li>
  );
}

/** LAYSTATE: the drawing's layer states (as AutoCAD keeps them): save the layers as they are now
 *  under a name, restore one (the dialog closes), rename or delete one. */
export function LayerStatesDialog(props: StatesProps) {
  const t = useT();
  const modal = useModal();
  const [name, setName] = useState("");
  const save = () => {
    const n = name.trim();
    if (n === "") return;
    modal.act(async () => {
      await props.onSave(n);
      setName("");
    }, false);
  };
  return (
    <Shell modal={modal} id="layer-states-title" title={t("layerState.title")} onClose={props.onClose} onSubmit={save}>
      {props.states.length === 0 ? (
        <p className="muted">{t("layerState.none")}</p>
      ) : (
        <ul className="layer-states">
          {props.states.map((s) => (
            <StateRow key={s} name={s} act={modal.act} props={props} />
          ))}
        </ul>
      )}
      <label>
        <span>{t("layerState.new")}</span>
        <input {...readingDirection(name)} value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <div className="buttons">
        <button type="button" onClick={modal.close}>
          {t("layerState.close")}
        </button>
        <button type="submit" className="primary" disabled={name.trim() === "" || modal.busy}>
          {t("layerState.save")}
        </button>
      </div>
    </Shell>
  );
}

type GroupProps = {
  /** The drawing's group names: a new group can't take one of them. */
  names: string[];
  onSubmit(name: string): Promise<void>;
  onClose(): void;
};

/** GROUP's last step: the new group's name. */
export function GroupNameDialog({ names, onSubmit, onClose }: GroupProps) {
  const t = useT();
  const modal = useModal();
  const [name, setName] = useState("");
  const trimmed = name.trim();
  const taken = names.some((n) => n.toUpperCase() === trimmed.toUpperCase());
  const bad = BAD_NAME.test(trimmed);
  const valid = trimmed !== "" && !taken && !bad;
  const submit = () => valid && modal.act(() => onSubmit(trimmed));
  return (
    <Shell modal={modal} id="group-name-title" title={t("group.title")} onClose={onClose} onSubmit={submit}>
      <label>
        <span>{t("group.name")}</span>
        <input {...readingDirection(name)} autoFocus value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      {taken && <p className="error">{t("group.nameTaken")}</p>}
      {bad && <p className="error">{t("block.nameBad")}</p>}
      <div className="buttons">
        <button type="button" onClick={modal.close}>
          {t("text.cancel")}
        </button>
        <button type="submit" className="primary" disabled={!valid || modal.busy}>
          {t("group.create")}
        </button>
      </div>
    </Shell>
  );
}
