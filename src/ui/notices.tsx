/** Notices: what the window says once and lets go ("Saved to …", what a modify left out, an error,
 *  the undo that follows a purge), as toasts at the end of the bottom edge, on Sonner. They are
 *  quiet (a thin card in the panel's colours, no icon), stack three at most, go by themselves after
 *  a few seconds (an error stays until it is closed or the next command works), and sit on the side
 *  the text ends on, so in Arabic they are at the bottom left. The status bar keeps to status: what
 *  is happening, the snap, the switches, the cursor. Sonner's own CSS lays the stack out; the look
 *  is ours (`theme.css`, "Notices") from the brand tokens, light or dark. */
import { useEffect, useState, type ReactNode } from "react";
import { toast, Toaster } from "sonner";
import { useLanguage, useT, translate } from "../i18n";
import { uiError } from "../i18n/errors";
import { Glyph } from "../shell/glyphs";
import { aiCancel } from "../api";
import { displayedWorkspace } from "../workspaceIpc";
import { DotLoader } from "../shell/DotLoader";

/** How long a notice stays, in milliseconds. */
export const NOTICE_MS = 5000;

type Id = string | number;

/** Where the toasts go: the one Toaster of the window. */
export function Notices({ theme }: { theme: "light" | "dark" }) {
  const { lang } = useLanguage();
  const t = useT();
  const rtl = lang === "ar";
  // Sonner keeps its toasts apart from the component: a window that opens starts with none.
  useEffect(() => {
    toast.dismiss();
  }, []);
  return (
    <Toaster
      position={rtl ? "bottom-left" : "bottom-right"}
      dir={rtl ? "rtl" : "ltr"}
      theme={theme}
      duration={NOTICE_MS}
      visibleToasts={3}
      gap={6}
      // Above the status bar and the command line.
      offset={{ bottom: 76, right: 14, left: 14 }}
      mobileOffset={{ bottom: 76, right: 14, left: 14 }}
      containerAriaLabel={t("notices.label")}
      toastOptions={{
        unstyled: true,
        classNames: { toast: "notice-slot" },
      }}
    />
  );
}

type CardProps = {
  id: Id;
  message: string;
  kind?: "plain" | "waiting" | "error" | "undo";
  children?: ReactNode;
};

/** All notice kinds share our card and the same centred, logical-end close button. */
function NoticeCard({ id, message, kind = "plain", children }: CardProps) {
  const t = useT();
  return <div className={`toast toast-${kind}`} data-notice-kind={kind}>
    {kind === "waiting" && <DotLoader pattern="pulse" />}
    <span className="toast-title">{message}</span>
    {children}
    <button type="button" className="ib toast-close" aria-label={t("about.close")} onClick={() => toast.dismiss(id)}>
      <Glyph name="close" />
    </button>
  </div>;
}

export const notify = (text: string, opts: { waiting?: boolean } = {}): Id =>
  toast.custom(id => <NoticeCard id={id} message={text} kind={opts.waiting ? "waiting" : "plain"} />,
    { duration: opts.waiting ? Infinity : NOTICE_MS });

function BusyNotice({ id, workspace }: { id: Id; workspace: number | null }) {
  const t = useT();
  const [stopping, setStopping] = useState(false);
  return <NoticeCard id={id} message={t("workspace.busy")}>
    <button type="button" className="toast-action" disabled={stopping} onClick={() => {
      setStopping(true);
      void aiCancel(workspace ?? undefined).then(() => toast.dismiss(id)).catch(error => {
        notifyError(uiError(error));
        setStopping(false);
      });
    }}>{t("ai.stop")}</button>
  </NoticeCard>;
}

/** Errors stay until closed; an edit refused while Mizan works replaces the same notice by id. */
export const notifyError = (text: string): Id => {
  if ([translate("en", "workspace.busy"), translate("ar", "workspace.busy")].some(message => text.includes(message))) {
    const workspace = displayedWorkspace();
    return toast.custom(id => <BusyNotice id={id} workspace={workspace} />, { id: "workspace-busy", duration: Infinity });
  }
  return toast.custom(id => <NoticeCard id={id} message={text} kind="error" />, { duration: Infinity });
};

export const dismissNotice = (id?: Id) => void toast.dismiss(id);

type Undo = {
  message: string;
  /** The button's name for assistive technology ("Undo"): fixed, the countdown is not read out. */
  name: string;
  /** The button's words ("Undo"), followed by the seconds left. */
  label: (seconds: number) => string;
  seconds: number;
  /** Takes the change back: the app's own undo. */
  onUndo(): void;
};

/** A toast with an Undo button that counts down and goes when the time is up; a thin line shrinks
 *  with it. Pressing the button runs `onUndo` and closes it. */
function UndoToast({ id, message, name, label, seconds, onUndo }: Undo & { id: Id }) {
  const [left, setLeft] = useState(seconds);
  useEffect(() => {
    const tick = setInterval(() => setLeft((s) => Math.max(0, s - 1)), 1000);
    return () => clearInterval(tick);
  }, []);
  return (
    <NoticeCard id={id} message={message} kind="undo">
      <button
        type="button"
        className="toast-action"
        aria-label={name}
        onClick={() => {
          toast.dismiss(id);
          onUndo();
        }}
      >
        <span aria-hidden="true">{label(left)}</span>
      </button>
      <span className="toast-timer" style={{ animationDuration: `${seconds}s` }} aria-hidden="true" />
    </NoticeCard>
  );
}

/** The undo toast on screen, so another command can take it away: its Undo would no longer be the
 *  last step. */
let undoToast: Id | null = null;

/** A notice of something done that can be taken back for `seconds` seconds. */
export function notifyUndo(undo: Undo): Id {
  if (undoToast !== null) toast.dismiss(undoToast);
  undoToast = toast.custom((id) => <UndoToast id={id} {...undo} />, { duration: undo.seconds * 1000 });
  return undoToast;
}

/** Takes the undo toast away: something else was done after it. */
export function notifyUndoGone() {
  if (undoToast === null) return;
  toast.dismiss(undoToast);
  undoToast = null;
}

/** The label of an undo toast in the window's language: "Undo (5 s)". */
export function useUndoLabel() {
  const t = useT();
  return (seconds: number) => t("toast.undoIn", { n: seconds });
}
