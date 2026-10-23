import { uiError } from "../i18n/errors";
import { Command } from "cmdk";
import { useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type Ref } from "react";
import { useT } from "../i18n";
import type { CmdlineStep, CommandWord } from "../api";
import { expand, parse, type Action, type ParseState, type Suggestion } from "../tools/commandParser";
import { registryAliases, suggestWords } from "../tools/commandWords";

/** Lines of history kept for ↑ and ↓. */
const HISTORY = 50;
/** Lines shown above the input. */
const LOG_LINES = 2;

type Props = {
  /** What the line waits for, the last point and the aliases (see `parse`). */
  state: ParseState;
  /** The prompt before the input, translated, e.g. "Specify first point:". */
  prompt: string;
  onAction(action: Action): void;
  /** Registry additions are typed in the core; existing click-driven tools still use `parse`. */
  words?: CommandWord[];
  onTyped?(line: string): Promise<CmdlineStep>;
  coreActive?: boolean;
  /** The input, so the app can focus it (Ctrl+9). */
  inputRef?: Ref<HTMLInputElement>;
};

/** Actions an empty Enter never repeats: answers to a prompt, and those that would add a second
 *  count or send the question again. */
const NOT_REPEATED: Action["type"][] = ["point", "number", "finish", "cancel", "count", "ai"];

/** What the list shows for a suggestion: a command with the shortcut typed for it, or, for a
 *  shortcut whose command does not itself start with what was typed, that command and the shortcut. */
type Row = {
  /** What Tab fills in and Enter runs: the command, or the shortcut as typed. */
  name: string;
  label: string;
  /** The shortcut, shown as a small key. */
  chip?: string;
  group: "commands" | "shortcuts";
};

/** The rows of the list from the commands and aliases that match: each command with its shortest
 *  alias; an alias whose command is already a row is only that row's chip. */
function rowsOf(found: Suggestion[], aliases: Record<string, string>): Row[] {
  const commands = new Set(found.filter((f) => !f.target).map((f) => f.name));
  const shortest = (command: string) =>
    Object.keys(aliases)
      .filter((a) => aliases[a].trim().toUpperCase() === command && a.toUpperCase() !== command)
      .sort((a, b) => a.length - b.length || a.localeCompare(b))[0];
  return found.flatMap<Row>((f) => {
    if (!f.target) return [{ name: f.name, label: f.name, chip: shortest(f.name), group: "commands" }];
    if (commands.has(f.target.toUpperCase())) return [];
    return [{ name: f.name, label: f.target, chip: f.name, group: "shortcuts" }];
  });
}

/** The row cmdk starts on, which is no row: nothing is chosen until the arrows choose one, so Enter
 *  runs what was typed. It is never shown. */
const NONE = "\u0000none";

/** The command line above the status bar: type a command (or its alias), a point or a number and
 *  press Enter. An empty Enter repeats the last command (or finishes a measure at a point prompt);
 *  ↑ and ↓ go through the last 50 lines; typing shows the matching commands in a list above the
 *  line (cmdk: ↑, ↓, Tab, Enter or a click choose one, each with its shortcut as a small key, in
 *  sections when there are both commands and shortcuts to show); Escape cancels. The two lines
 *  above show what was typed and any error. */
export function CommandLine({ state, prompt, onAction, inputRef, words = [], onTyped, coreActive = false }: Props) {
  const t = useT();
  const id = useId();
  const [text, setText] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const [history, setHistory] = useState<string[]>([]);
  /** Where ↑ and ↓ are in `history`; null while typing a new line. */
  const [recalled, setRecalled] = useState<number | null>(null);
  const [lastCommand, setLastCommand] = useState<string | null>(null);
  /** The suggestions show while typing a command name; Escape, a pick or a recall hides them. */
  const [suggesting, setSuggesting] = useState(false);
  const [active, setActive] = useState(-1);
  const pending = useRef(false);
  /** A prompted command repeats its entire completed input, never its last answer alone. */
  const coreLines = useRef<string[]>([]);
  const coreStartedHere = useRef(false);
  const suggestionAliases = useMemo(() => ({ ...registryAliases(words), ...state.aliases }), [words, state.aliases]);

  const suggestions = useMemo(
    () => (suggesting && !coreActive && state.expect === "command" ? rowsOf(suggestWords(text, suggestionAliases, words), suggestionAliases) : []),
    [suggesting, text, state.expect, suggestionAliases, words, coreActive],
  );
  const open = suggestions.length > 0;
  const list = useRef<HTMLDivElement>(null);
  /** The ids the input points at: the list, and the row chosen (the combobox's `aria-*`). */
  const [ids, setIds] = useState<{ list?: string; option?: string }>({});
  useLayoutEffect(() => {
    const box = list.current;
    const name = active >= 0 ? suggestions[active]?.name.toLowerCase() : undefined;
    // The row is found by its value: cmdk marks the chosen one a moment after this runs.
    const option =
      name === undefined
        ? undefined
        : Array.from(box?.querySelectorAll<HTMLElement>("[cmdk-item]") ?? []).find(
            (el) => el.getAttribute("data-value")?.toLowerCase() === name,
          );
    const next = { list: box?.id || undefined, option: option?.id || undefined };
    setIds((was) => (was.list === next.list && was.option === next.option ? was : next));
    option?.scrollIntoView?.({ block: "nearest" });
  });

  const write = (...lines: string[]) => setLog((l) => [...l, ...lines].slice(-LOG_LINES));

  function submit(line: string) {
    if (pending.current) return;
    setText("");
    setRecalled(null);
    setSuggesting(false);
    setActive(-1);
    let action = parse(line, state);
    let typed = line.trim();
    if (!coreActive && action.type === "repeat") {
      if (!lastCommand) return;
      typed = lastCommand;
      action = parse(lastCommand, state);
    }
    if (typed) setHistory((h) => (h[h.length - 1] === typed ? h : [...h, typed].slice(-HISTORY)));
    const echo = typed ? `${prompt} ${typed}` : prompt;
    if (onTyped && (coreActive || (action.type === "error" && action.key === "cmd.unknown"))) {
      write(echo);
      const expanded = expand(typed, state.aliases);
      const input = coreActive ? typed : [expanded.name, expanded.rest].filter(Boolean).join(" ");
      if (!coreActive) {
        coreLines.current = [];
        coreStartedHere.current = true;
      }
      pending.current = true;
      onTyped(input)
        .then((result) => {
          if (result.type === "cancelled") {
            coreLines.current = [];
            coreStartedHere.current = false;
            return;
          }
          coreLines.current.push(input);
          if (result.type === "command") {
            setLastCommand(coreStartedHere.current ? coreLines.current.join(" ") : null);
            coreLines.current = [];
            coreStartedHere.current = false;
          }
        })
        .catch((error) => {
          const message = String(error);
          write(!coreActive && action.type === "error" && action.key === "cmd.unknown" &&
            /^(?:Error:\s*)?unknown command\b/i.test(message)
            ? t(action.key, action.args)
            : uiError(message));
        })
        .finally(() => { pending.current = false; });
      return;
    }
    if (action.type === "error") {
      write(echo, t(action.key, action.args));
      return;
    }
    write(echo);
    // COUNT <block> and AI <text> clear the repeat: Enter after them does nothing.
    if (action.type === "count" || action.type === "ai") setLastCommand(null);
    else if (!NOT_REPEATED.includes(action.type)) setLastCommand(typed);
    onAction(action);
  }

  function recall(step: -1 | 1) {
    if (history.length === 0) return;
    const at = recalled === null ? (step < 0 ? history.length - 1 : null) : recalled + step;
    if (at === null || at >= history.length) {
      setRecalled(null);
      setText("");
    } else {
      const i = Math.max(0, at);
      setRecalled(i);
      setText(history[i]);
    }
    setSuggesting(false);
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Home" || e.key === "End") {
      // The caret's: cmdk's root would take them (and stop the caret) for its list.
      e.stopPropagation();
      return;
    }
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const step = e.key === "ArrowUp" ? -1 : 1;
      const n = suggestions.length;
      if (open) setActive((a) => (step > 0 ? (a + 1) % n : a <= 0 ? n - 1 : a - 1));
      else recall(step);
    } else if (e.key === "Tab" && open) {
      e.preventDefault();
      setText(`${suggestions[Math.max(0, active)].name} `);
      setSuggesting(false);
      setActive(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      submit(open && active >= 0 ? suggestions[active].name : text);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (open) {
        setSuggesting(false);
        setActive(-1);
        return;
      }
      setText("");
      setRecalled(null);
      coreLines.current = [];
      coreStartedHere.current = false;
      onAction({ type: "cancel" });
    }
  }

  return (
    <section className="command-line" aria-label={t("cmd.label")}>
      <ol className="command-log" aria-live="polite">
        {log.map((line, i) => (
          <li key={i} dir="auto">
            {line}
          </li>
        ))}
      </ol>
      <Command
        className="command-input"
        shouldFilter={false}
        loop
        vimBindings={false}
        disablePointerSelection
        // The row chosen is ours (`active`): cmdk is told, and its own start on the first row is
        // taken by the hidden one.
        value={open && active >= 0 ? suggestions[active].name : NONE}
        onValueChange={() => {}}
      >
        <label htmlFor={`${id}-input`}>{prompt}</label>
        <input
          ref={inputRef}
          id={`${id}-input`}
          role="combobox"
          aria-expanded={open}
          aria-controls={open ? ids.list : undefined}
          aria-autocomplete="list"
          aria-activedescendant={open ? ids.option : undefined}
          autoComplete="off"
          spellCheck={false}
          placeholder={t("cmd.placeholder")}
          dir="auto"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setRecalled(null);
            setSuggesting(true);
            setActive(-1);
          }}
          onKeyDown={onKeyDown}
          onBlur={() => setSuggesting(false)}
        />
        {open && (
          <Command.List
            ref={list}
            label={t("cmd.suggestions")}
            // Before the input's blur, so a click on a row lands.
            onMouseDown={(e) => e.preventDefault()}
          >
            <Command.Item value={NONE} data-none="" hidden aria-hidden="true" />
            {(["commands", "shortcuts"] as const).map((group) => {
              const rows = suggestions.filter((r) => r.group === group);
              if (rows.length === 0) return null;
              const both = suggestions.some((r) => r.group !== group);
              return (
                <Command.Group key={group} heading={both ? t(`cmd.group.${group}`) : undefined}>
                  {rows.map((r) => (
                    <Command.Item key={r.name} value={r.name} className="cmd-item" onSelect={() => submit(r.name)}
                      title={words.find((word) => word.word === r.name || word.word === r.label)?.summary}>
                      <span>{r.label}</span>
                      {r.chip && <kbd>{r.chip}</kbd>}
                    </Command.Item>
                  ))}
                </Command.Group>
              );
            })}
          </Command.List>
        )}
      </Command>
    </section>
  );
}
