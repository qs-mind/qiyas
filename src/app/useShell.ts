import { uiError } from "../i18n/errors";
/** The window's own chrome: theme, the notices and errors (toasts), the settings and About
 *  dialogs' switches, the side panel's tab, the command line aliases, and the viewport refs. */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { aliases as readAliases, commandWords, type CommandWord } from "../api";
import { blurAfterClick } from "../shell/blurAfterClick";
import { mergeAliases } from "../tools/aliases";
import { dismissNotice, notify, notifyError, notifyUndoGone } from "../ui/notices";
import type { Theme } from "../viewport/gl";
import type { ViewportHandle } from "../viewport/Viewport";
import { initialTheme, THEME_KEY } from "./shared";

/** What the one right-hand column shows (W4, design B): Mizan's conversation, or one of the
 *  outputs Mizan produced (the take-off, the BOQ, the conflicts, the take-off report) in the same
 *  column, with a way back. There is no tab strip. */
export type SideTab = "assistant" | "takeoff" | "boq" | "conflicts" | "review";

export function useShell() {
  const [theme, setTheme] = useState<Theme>(initialTheme);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  /** The "Connect an AI app" dialog. */
  const [connectOpen, setConnectOpen] = useState(false);
  /** The user's command line aliases from the settings. */
  const [userAliases, setUserAliases] = useState<Record<string, string>>({});
  const [words, setWords] = useState<CommandWord[]>([]);
  const commandAliases = useMemo(() => mergeAliases(userAliases), [userAliases]);
  /** The toast of the notice and of the error on screen, so the next one takes its place. */
  const noticeToast = useRef<string | number | null>(null);
  const errorToast = useRef<string | number | null>(null);
  /** The right column's view: the conversation, or an output. */
  const [sideTab, setSideTab] = useState<SideTab>("assistant");
  const viewport = useRef<ViewportHandle>(null);
  const viewportBox = useRef<HTMLDivElement>(null);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // Space pans the drawing; a button the mouse just clicked must not take it as a press.
  useEffect(blurAfterClick, []);

  /** A one-off message, e.g. after an export: a toast that goes by itself after a few seconds or
   *  with the next command (`run` says null). One at a time: a new one takes the place of the last. */
  const setNotice = useCallback((text: string | null, opts?: { waiting?: boolean }) => {
    if (noticeToast.current !== null) dismissNotice(noticeToast.current);
    noticeToast.current = text === null ? null : notify(text, opts);
  }, []);
  /** An error: a toast that stays until it is closed or `null` (the next success) takes it away. */
  const setError = useCallback((message: string | null) => {
    if (errorToast.current !== null) dismissNotice(errorToast.current);
    errorToast.current = message === null ? null : notifyError(message);
  }, []);

  /** Reads the user's command line aliases (at start and after the settings close). */
  const loadAliases = () =>
    readAliases()
      .then(setUserAliases)
      .catch(() => {});
  useEffect(() => {
    loadAliases();
    commandWords().then((value) => setWords(value ?? [])).catch(() => {});
  }, []);

  function chooseTheme(next: Theme) {
    setTheme(next);
    try {
      localStorage.setItem(THEME_KEY, next);
    } catch {
      // Storage blocked: the choice lasts for this session only.
    }
  }

  /** Runs an IPC step, showing its error as a toast. */
  async function run(step: () => Promise<void>) {
    setNotice(null);
    // Another command: what an undo toast offered to take back is no longer the last step.
    notifyUndoGone();
    try {
      await step();
      setError(null);
    } catch (e) {
      setError(uiError(e));
    }
  }

  // Hand the keyboard back to the drawing, so Space pans instead of pressing a button.
  const focusDrawing = () => viewportBox.current?.focus();

  return {
    theme,
    chooseTheme,
    aboutOpen,
    setAboutOpen,
    settingsOpen,
    setSettingsOpen,
    connectOpen,
    setConnectOpen,
    commandAliases,
    words,
    loadAliases,
    setError,
    setNotice,
    sideTab,
    setSideTab,
    viewport,
    viewportBox,
    run,
    focusDrawing,
  };
}

export type Shell = ReturnType<typeof useShell>;
