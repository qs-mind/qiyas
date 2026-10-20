/** ALIASEDIT: the user's command line aliases, alias to command, as a table (`settings.json`
 *  `aliases`, over the built-in ones). An alias is one word; a command is a command's name with
 *  words after it if wanted (`LAYOFF A-WALL`). */
import { useEffect, useRef, useState } from "react";
import { commandWords, getSettings, setSettings, type Settings } from "../api";
import { useT } from "../i18n";
import { COMMANDS } from "../tools/aliases";
import { registryNames } from "../tools/commandWords";

type Row = { alias: string; command: string };

/** What is wrong with the rows, as an i18n key and its values; null when nothing is. */
export function aliasProblem(rows: Row[], words: string[] = []): { key: string; args?: Record<string, string> } | null {
  const known = new Set<string>([...COMMANDS, ...words]);
  const isCommand = (word: string) => known.has(word.toUpperCase());
  const seen = new Set<string>();
  for (const { alias, command } of rows) {
    const a = alias.trim().toUpperCase();
    const head = command.trim().split(/\s+/, 1)[0] ?? "";
    if (a === "" || /\s/.test(a) || head === "") return { key: "aliasEdit.oneWord" };
    if (isCommand(a)) return { key: "aliasEdit.builtIn", args: { alias: a } };
    if (seen.has(a)) return { key: "aliasEdit.twice", args: { alias: a } };
    seen.add(a);
    if (!isCommand(head)) return { key: "aliasEdit.unknown", args: { command: head.toUpperCase() } };
  }
  return null;
}

export function AliasEditor({ onSaved, onClose }: { onSaved(aliases: Record<string, string>): void; onClose(): void }) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [words, setWords] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    getSettings()
      .then((s) => {
        setDraft(s);
        setRows(Object.entries(s.aliases ?? {}).map(([alias, command]) => ({ alias, command })));
      })
      .catch((e) => setError(String(e)));
    commandWords().then((items) => setWords(registryNames(items ?? []))).catch(() => {});
    return () => opener?.focus();
  }, []);

  const edit = (i: number, patch: Partial<Row>) => setRows((r) => r.map((row, j) => (j === i ? { ...row, ...patch } : row)));

  async function save() {
    if (!draft) return;
    const problem = aliasProblem(rows, words);
    if (problem) {
      setError(t(problem.key, problem.args));
      return;
    }
    const aliases = Object.fromEntries(rows.map((r) => [r.alias.trim().toUpperCase(), r.command.trim()]));
    try {
      await setSettings({ ...draft, aliases });
      onSaved(aliases);
      dialog.current?.close();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <dialog
      ref={dialog}
      className="settings alias-editor"
      aria-labelledby="alias-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 id="alias-title" className="heading">
          {t("aliasEdit.title")}
        </h2>
        <p className="status">{t("aliasEdit.note")}</p>
        {draft && (
          <table className="manage-table">
            <thead>
              <tr>
                <th>{t("aliasEdit.alias")}</th>
                <th>{t("aliasEdit.command")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((row, i) => (
                <tr key={i}>
                  <td>
                    <input
                      dir="ltr"
                      aria-label={t("aliasEdit.aliasN", { n: i + 1 })}
                      value={row.alias}
                      onChange={(e) => edit(i, { alias: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      dir="auto"
                      aria-label={t("aliasEdit.commandN", { n: i + 1 })}
                      value={row.command}
                      onChange={(e) => edit(i, { command: e.target.value })}
                    />
                  </td>
                  <td>
                    <button
                      type="button"
                      aria-label={t("aliasEdit.removeN", { n: i + 1 })}
                      onClick={() => setRows((r) => r.filter((_, j) => j !== i))}
                    >
                      {t("options.remove")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <span className="row">
          <button type="button" disabled={!draft} onClick={() => setRows((r) => [...r, { alias: "", command: "" }])}>
            {t("aliasEdit.add")}
          </button>
        </span>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!draft}>
            {t("settings.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
